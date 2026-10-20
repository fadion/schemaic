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
# The Agent CLI dropdown's off choice (the others are product names). English
# must read as schemaic-ai's Agent::label.
settings-ai-disabled = Disabled
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
# The active connection's level and what it lets leave (ai-data-* below);
# $name is the connection's name.
settings-ai-data-line = { $level } — { $hint }
settings-ai-data-line-named = { $name }: { $level } — { $hint }
# A connection's AI data access level and its consequence (core's
# connection::AiData; English must read exactly as its label and hint).
ai-data-schema-only = Schema only
ai-data-on-request = Only what I attach
ai-data-full = Let it read data
ai-data-schema-only-hint = Structure only — names, types and definitions. No row ever leaves this machine, and attaching is refused.
ai-data-on-request-hint = The assistant reads no data on its own. Rows you attach from a result leave this machine with that question.
ai-data-full-hint = The assistant may run read-only queries and read sample rows by itself. Whatever it reads leaves this machine.
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
# The keyword, function and type case dropdowns' choices (core's KeywordCase;
# English must read exactly as its label).
settings-format-case-keep = As typed
settings-format-case-upper = Upper case
settings-format-case-lower = Lower case
# The Commas and Between statements dropdowns' choices (core's CommaStyle and
# StatementGap); the latter's "As typed" is settings-format-case-keep.
settings-format-commas-trailing = End of line
settings-format-commas-leading = Start of line
settings-format-gap-one = One blank line
settings-format-gap-none = No blank line
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
# A statement timeout's value, in its largest whole unit (core's TimeoutSpan;
# English must read exactly as its Display). Also quoted by the message a
# timed-out statement leaves (timeout-cancelled-*), so the two agree.
timeout-off = No timeout
timeout-hours = { $n ->
    [one] { $n } hour
   *[other] { $n } hours
}
timeout-minutes = { $n ->
    [one] { $n } minute
   *[other] { $n } minutes
}
timeout-seconds = { $n ->
    [one] { $n } second
   *[other] { $n } seconds
}
settings-query-confirm-writes = Confirm before running writes
settings-query-confirm-writes-hint = Ask before executing any statement that modifies data or schema.
settings-query-validate = Live database validation
settings-query-validate-hint = Check the statement under the cursor against the database as you type (a non-executing PREPARE) to surface exact errors. Adds a DB round-trip on each pause.
settings-query-notify = Notify when a run finishes in the background
settings-query-notify-hint = Show a desktop notification when a query, script, export or dump that took a second or more ends while Schemaic is not the active window. The taskbar button is highlighted either way. The notification never includes the SQL.
# That notification's title, by what ended and how (core's history::finished_note;
# English must read exactly as it does), and its body: $duration as the app
# writes one (850ms, 12.3s, 12m 34s).
notify-query-finished = Query finished
notify-query-failed = Query failed
notify-script-finished = Script finished
notify-script-failed = Script failed
notify-export-finished = Export finished
notify-export-failed = Export failed
notify-dump-finished = Dump finished
notify-dump-failed = Dump failed
notify-took = Took { $duration }.
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
# The Truncate confirmation (core's stats::truncate_prompt). $table is the
# table's shown name; $rows a figure worth naming, formatted (4,213,551 or ~4.2m).
tree-truncate-rows = Delete all { $rows } rows in { $table }? This can't be undone.
tree-truncate-every-row = Delete every row in { $table }? This can't be undone.
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

## A database error's own words, around the server's (schemaic-db's DbError).
## $error is the server's or the driver's message, which stays as it came.

# A statement the server refused; for a write path's failure, $error also
# carries what the rollback achieved.
db-query-failed = query failed: { $error }
db-connection-failed = connection failed: { $error }
db-query-cancelled = query cancelled

## A DDL plan that failed, in the DDL preview (schemaic-db's DdlError). English
## must read exactly as DdlError displays (a test holds it).

# $n is the failed statement's place in the plan, from 1; $error why.
ddl-statement-failed = statement { $n } failed: { $error }
# $failed is ddl-statement-failed; $n how many earlier statements stay in effect.
ddl-already-applied = { $n ->
    [one] { $failed } — { $n } earlier statement already applied and cannot be rolled back
   *[other] { $failed } — { $n } earlier statements already applied and cannot be rolled back
}
ddl-cancelled = the plan was cancelled
# SQLite's own check after the plan; $row is the check's description of the row.
ddl-dangling-foreign-key = the plan leaves a foreign key pointing at nothing: { $row }
# $why is SQLite's own words.
ddl-breaks-view = the plan would break the view { $view }, which reads a column it renames or drops ({ $why }). Nothing was changed. Edit or drop the view first.
ddl-no-databases = SQLite has no databases to create or drop — a database there is a file, which Schemaic does not create or delete for you.
# What the refresh after a SQL Server plan's commit could not do, under the
# applied plan (core's ddl::ElsewhereNote). sp_refreshview is a procedure name.
ddl-elsewhere-failed = The views in other databases that select * from { $table } were not refreshed ({ $error }), and read and write the wrong columns until they are (sp_refreshview).
# $views is ddl-elsewhere-list over "db.view (why)" items.
ddl-elsewhere-unrefreshed = { $count ->
        [one] { $count } view in another database that selects * from { $table } could not be refreshed, and reads and writes the wrong columns until it is (sp_refreshview): { $views }.
       *[other] { $count } views in another database that select * from { $table } could not be refreshed, and read and write the wrong columns until they are (sp_refreshview): { $views }.
    }
ddl-elsewhere-unsearched = { $count ->
        [one] { $count } database was not searched for a view that selects * from { $table }: { $databases }.
       *[other] { $count } databases were not searched for a view that selects * from { $table }: { $databases }.
    }
# $items is the first eight, joined; $more how many beyond them.
ddl-elsewhere-list = { $items }{ $more ->
        [0] {""}
       *[other] , and { $more } more
    }
# $error is why the plan failed; $rollback SQLite's words.
ddl-rollback-failed = { $error } — and the rollback failed too: { $rollback }

## A write path's failure — the grid's Commit, an import, a copy (core's
## model::WriteFailure). The server's words in $error stay the server's.

# The 1-row safety net: a statement that changed $affected rows rather than
# one. $table is db.table or db.schema.table, as written.
write-one-row-delete = delete on { $table } affected { $affected } rows (expected exactly 1)
write-one-row-update = update on { $table } affected { $affected } rows (expected exactly 1)
write-one-row-insert = insert into { $table } affected { $affected } rows (expected exactly 1)
# A batch of $rows rows that wrote $wrote.
write-batch-inserted = a batch of { $rows } rows inserted { $wrote }
write-batch-affected = a batch of { $rows } rows affected { $wrote }
# A record the check passed that no longer reads — the file changed under the
# import. $problem is one of the import-issue-* messages; $column the target
# column; $text the field as the file has it.
write-row-issue = line { $line }, column { $column }: { $problem } ({ $text })
write-row-issue-empty = line { $line }, column { $column }: { $problem } (empty)
# A copied value the copy must not write; $column is the column's name.
write-copy-too-long = { $column } holds more text than one read can carry, so the copy stopped before writing it
write-copy-too-long-unnamed = a column holds more text than one read can carry, so the copy stopped before writing it
write-copy-bytes = { $column } holds bytes the copy cannot read as they are, so it stopped before writing them
write-copy-not-its-form = the value read for { $column } is not the form the copy asked for, so nothing was written
write-copy-not-its-form-unnamed = the value read for a column is not the form the copy asked for, so nothing was written
write-commit-cancelled = Commit cancelled
write-import-cancelled = Import cancelled
# What the rollback achieved, appended to the failure, so each starts with its
# own separator.
write-rollback-complete = {" "}— rolled back all changes
write-rollback-incomplete = {" "}— the rollback did NOT undo them: this table's storage engine is not transactional, so the rows already written remain. Check the table before retrying.
write-rollback-unknown = {" "}— and the rollback could not be confirmed: the connection failed before the server answered. Rows may or may not remain. Check the table before retrying.

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
# A session's state on its row — also what the search box matches (core's
# SessionState; English must read exactly as its label).
activity-state-blocked = Blocked
activity-state-idle-in-tx = Idle in txn
activity-state-running = Running
activity-state-idle = Idle
# The counts line above the list (English must read as core's
# ActivitySummary). $figure is the count as core prints it — "500+" on a
# capped list — and $n the number the plural is chosen by.
activity-count-sessions = { $n ->
    [one] { $figure } session
   *[other] { $figure } sessions
}
activity-count-running = { $figure } running
activity-count-blocked = { $figure } blocked
activity-count-idle-in-tx = { $figure } idle in txn
activity-lock-wait = Lock wait
# The card's sentence (core's activity::LockWait; English must read exactly as
# its Display). $waiter and $holder are session ids, $waiter_who and
# $holder_who who they are (user@client). $waited and $held are ages such as
# "4m 12s", or "none" where the server withheld one. $doing is the holder's
# state, $others how many other sessions also hold a lock the waiter needs.
activity-lock-wait-sentence = Session { $waiter } ({ $waiter_who }) { $waited ->
        [none] is waiting for
       *[other] has been waiting { $waited } for
    } a lock held by session { $holder } ({ $holder_who }), { $doing ->
        [idle-in-tx] idle in transaction
        [blocked] itself blocked
        [running] running
       *[idle] idle
    }{ $held ->
        [none] {""}
       *[other] {" "}for { $held }
    }.{ $others ->
        [0] {""}
        [one] {" "}{ $others } other session also holds a lock it needs.
       *[other] {" "}{ $others } other sessions also hold a lock it needs.
    }
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
# The line under a session's state (core's activity::SessionNote). $ids are
# session ids, joined by commas.
activity-waiting-on = waiting on a lock held by { $ids }
activity-blocks = { $n ->
        [one] blocks { $n } session
       *[other] blocks { $n } sessions
    }
# A kill refused on a read-only connection (core's activity::read_only_refusal).
activity-read-only-cancel = This connection is marked read-only, so Schemaic won't cancel queries on it. Clear read-only in the connection's settings first.
activity-read-only-terminate = This connection is marked read-only, so Schemaic won't terminate sessions on it. Clear read-only in the connection's settings first.

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
chart-too-large = These values are too large to draw.
# The line under the canvas, its parts joined with " · ". $n is the result's
# row count; $skipped the values not drawn; $max the most a series draws.
chart-caption-rows = { $n ->
    [one] { $n } row
   *[other] { $n } rows
}
chart-caption-skipped-null = { $skipped } NULL, not drawn
chart-caption-skipped = { $skipped } not drawn (NULL, or past { $max })
chart-caption-truncated = the rows read, not the whole result

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
# What stops a view being emitted (core's ddl::ViewProblem), in the footer.
view-problem-no-name = The view needs a name.
view-problem-no-select = A view needs a SELECT to define it.
view-problem-not-a-query = A view's body has to be a query — it starts with SELECT, WITH, VALUES or TABLE.
view-problem-materialized = Schemaic can't edit a materialized view — PostgreSQL has no CREATE OR REPLACE for one.
view-problem-hidden = This view's definition is encrypted or hidden from this login, so it can be removed here but not edited.
view-problem-verbatim = This view's header says something Schemaic can't restate, so it can be removed here but not edited.
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
# Under the row figure: whether the server estimated it or Schemaic counted it
# (core's TableStats::row_caption).
props-rows-estimated = rows (estimated)
props-rows-counted = rows (counted)
props-rows-not-reported = rows — not reported
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
# An index's facts after its name, joined by " · " (core's stats::IndexFact).
# $cardinality and $shown arrive formatted (~1.2k, 12,345).
props-index-cardinality = cardinality { $cardinality }
props-index-scans = { $n ->
        [one] { $shown } scan
       *[other] { $shown } scans
    }
props-index-not-counted = usage not counted
# Under an index the server has counted no use of (core's stats::unused_note).
props-index-unused = never used since the server's counters were reset
# Why the figures may be out of date (core's stats::Freshness). ANALYZE and
# information_schema_stats_expiry are a statement and a server setting, never
# translated. $when is the server's own timestamp; $age arrives formatted
# (30s, 24h, 2d) and $secs is the setting's value.
props-never-analysed = This table has never been analysed, so the server has no row estimate for it. Run ANALYZE, or count the rows.
props-analysed-at = Estimated as of the last ANALYZE, { $when }.
props-read-live = Read live — this server has information_schema_stats_expiry = 0.
props-cached-up-to = Server-cached: these figures may be up to { $age } old (information_schema_stats_expiry = { $secs }).
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
# Under the form when Copy is refused (core's copy_table::CopyRefusal).
copy-table-unnamed = Name the copy.
copy-table-onto-itself = A table can't be copied onto itself — give the copy another name.
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
# What stops a database or schema being created (core's ddl::DatabaseProblem),
# in the form's status line.
database-problem-no-name = The name can't be empty.
database-problem-bad-collation = A collation name holds only letters, digits and underscores.
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
# What no plan from this comparison carries, above the tree and in the preview's
# Not included list (core's compare::PlanOmission). English must read exactly as
# core's omission_note and notes (a test holds it). $kind is compare-kind-*.
compare-described = { $kind } { $name }
# $what is compare-described; $why one of the reasons below.
compare-omitted = { $what } — { $why }
compare-omitted-needs-source = its body must be re-read from the server before it can be applied
compare-omitted-unread = this comparison has no statement for it — its definition could not be read
compare-omitted-no-statement = this comparison has no statement for the difference it found
# $parent is the partitioned table held back; $sub yes for a deeper level.
compare-held-partition = it is a { $sub ->
        [yes] sub-partition
       *[no] partition
    } of { $parent }, which is held back — recreate them together, by hand
compare-held-partitioning-differs = its partitioning differs, and this comparison writes no statement that repartitions a table — recreate it by hand
# SHOW CREATE SEQUENCE is a statement: never translated.
compare-held-sequence-unwritten = this comparison writes no statement for a sequence — copy it from `SHOW CREATE SEQUENCE`, which its pane shows
compare-held-partitioned-unrestated = it is partitioned, and no CREATE TABLE here restates its partition scheme — create the partition function and scheme, then the table, by hand
# $at is where the target's counter is; $min and $max the new range.
compare-held-sequence-range = the target's counter has reached { $at }, outside the new range { $min } to { $max }, and the server refuses to narrow a sequence past a value it has handed out. Restart it on the target (ALTER SEQUENCE … RESTART WITH) at a value no table holds, then compare again
# Two objects on one side whose names fold to one on a case-insensitive target;
# $side is target or source.
compare-note-set-aside = { $aside } — the { $side ->
        [target] target
       *[source] source
    } also holds { $kept }, and the target's names ignore case, so it can hold only one of the two; { $kept } is the one compared
# An alias type's bound rule or default; $bound is rule, default or both.
compare-note-binding = Alias type { $alias }: { $bound ->
        [rule] the rule { $rule }
        [default] the default { $default }
       *[both] the rule { $rule } and the default { $default }
    } bound to it on the source is not compared or carried — bind it by hand (sp_bindrule / sp_bindefault).
compare-hint-blocked = body must be re-read before this can be applied
compare-hint-uncertain = an index this model reads only in part — a match here isn't certain
compare-preview-migration = Preview migration
# Beside Preview migration: how many objects the plan holds (core's
# compare::selection_note). Only the empty one ends in a full stop.
compare-selected = { $n ->
        [0] Nothing selected.
        [one] { $n } object selected
       *[other] { $n } objects selected
    }
# Two engines' schemas cannot be compared (core's compare::comparable).
# $left and $right are engine names (MySQL, PostgreSQL…).
compare-incomparable = { $left } and { $right } can't be compared. Type names, defaults and index shapes don't carry across engines, so any migration generated from the difference would be wrong.
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
# What stops an event being emitted (core's ddl::EventProblem), in the footer.
event-problem-no-name = The event needs a name.
event-problem-no-time = A one-off event needs a time to run at.
event-problem-no-interval = A repeating event needs an interval.
event-problem-no-unit = A repeating event needs an interval unit (DAY, HOUR…).
event-problem-no-body = The event needs a body.
# The draft's name is one an event in the same database already has.
event-name-clash = An event called { $name } is already here.

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
# What stops a function or procedure being emitted (core's
# ddl::RoutineProblem), in the footer; a message per kind where the sentence
# names it.
routine-problem-function-no-name = The function needs a name.
routine-problem-procedure-no-name = The procedure needs a name.
routine-problem-function-no-language = The function needs a language (plpgsql, sql).
routine-problem-procedure-no-language = The procedure needs a language (plpgsql, sql).
routine-problem-no-return-type = The function needs a return type.
routine-problem-procedure-returns = A procedure returns nothing — clear the return type.
routine-problem-function-no-body = The function needs a body.
routine-problem-procedure-no-body = The procedure needs a body.
routine-problem-bare-parameter = SQL Server names every parameter with @ — @sku varchar(20), @n int OUTPUT.
# SQL Server: a routine of this shape refusing WITH $option (SQL); -native when
# it refuses it only once the routine is natively compiled.
routine-problem-procedure-refuses = A procedure can't take WITH { $option }.
routine-problem-scalar-refuses = A scalar function can't take WITH { $option }.
routine-problem-inline-refuses = An inline table-valued function can't take WITH { $option }.
routine-problem-multi-refuses = A multi-statement table-valued function can't take WITH { $option }.
routine-problem-procedure-refuses-native = A procedure can't take WITH { $option } once it is natively compiled.
routine-problem-scalar-refuses-native = A scalar function can't take WITH { $option } once it is natively compiled.
routine-problem-inline-refuses-native = An inline table-valued function can't take WITH { $option } once it is natively compiled.
routine-problem-multi-refuses-native = A multi-statement table-valued function can't take WITH { $option } once it is natively compiled.
routine-problem-native-needs-schemabinding = A natively compiled routine needs WITH SCHEMABINDING — SQL Server requires the two together.
routine-problem-inline-with-execute-as = INLINE = ON can't be combined with EXECUTE AS — a function that runs as someone else is never inlined.
# $was is the procedure's name; $group its numbered members (orders;2, orders;3).
routine-problem-numbered-rename = { $was } heads a numbered procedure group ({ $group }), and renaming it would drop them all. Keep the name to edit it.
routine-problem-trigger-function-args = A trigger function takes no declared arguments — they arrive in TG_ARGV.
# The draft's $name is one another routine already has. -refused where the
# server refuses the whole plan; -destroys where applying it drops $original
# first; -destroys-new for a routine not yet created.
routine-clash-function-refused = A function called { $name } is already here. On this engine a rename is a drop and a create, so the server refuses this one and nothing changes — pick another name, or drop { $name } first.
routine-clash-procedure-refused = A procedure called { $name } is already here. On this engine a rename is a drop and a create, so the server refuses this one and nothing changes — pick another name, or drop { $name } first.
routine-clash-function-destroys = A function called { $name } is already here. On this engine a rename is a drop and a create, so applying this would destroy { $original } and leave { $name } as it was.
routine-clash-procedure-destroys = A procedure called { $name } is already here. On this engine a rename is a drop and a create, so applying this would destroy { $original } and leave { $name } as it was.
routine-clash-function-destroys-new = A function called { $name } is already here. On this engine a rename is a drop and a create, so applying this would destroy the original and leave { $name } as it was.
routine-clash-procedure-destroys-new = A procedure called { $name } is already here. On this engine a rename is a drop and a create, so applying this would destroy the original and leave { $name } as it was.

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
# What the account list does not cover (core's users::PrincipalsNote), under it.
# SQL Server's sentences can appear together, one after the other.
users-note-my-own-account = This connection cannot read mysql.user, so this is not the server's account list — it is the one account this connection can see, its own. Connect as an account with SELECT on the mysql database to browse the rest.
users-note-ms-logins-elsewhere = Azure SQL Database keeps its logins in master, and they are not managed here: listed are the database's users and roles.
users-note-ms-logins-only = Only the server's logins are listed: pick a database to see its users and roles.
users-note-ms-short = This login lacks VIEW ANY DEFINITION, so the server shows it only the principals it may see — the list may be short.
# What a privilege list does not cover (core's users::GrantsNote), under it.
users-note-my-scope = Privileges held through a granted role are not expanded: SHOW GRANTS lists what is granted directly to the account, and the roles it is a member of, but not what those roles themselves hold. Open the role's own privileges to see those.
# PostgreSQL's, over $database; or, with none selected, -no-database. What the
# role holds implicitly (-superuser, -predefined, -owns) goes before it.
users-note-pg-scope = Schema, table and sequence privileges are those in { $database } — PostgreSQL keeps them in each database's own catalogue, so privileges in other databases are not listed. EXECUTE on functions and procedures, and privileges granted on a single column, are not listed either. Privileges held through role membership or granted to PUBLIC are not expanded, and neither are the privileges a role holds by owning an object or by being a superuser.
users-note-pg-no-database = Schema, table and sequence privileges are not listed: PostgreSQL keeps them in each database's own catalogue, and this connection has no database selected. Open a query tab on a database to see them. EXECUTE on functions and procedures, and privileges granted on a single column, are not listed either. Privileges held through role membership or granted to PUBLIC are not expanded, and neither are the privileges a role holds by owning an object or by being a superuser.
users-note-pg-superuser = This role is a SUPERUSER: it bypasses every permission check, so it holds every privilege on every object whether or not anything below says so.
# $role is one of PostgreSQL's pg_* roles.
users-note-pg-predefined = { $role } is one of PostgreSQL's predefined roles: what it can do is built into the server rather than granted, so it is recorded in no catalogue this reads.
# $n objects the role owns in $database; -cluster with no database selected.
users-note-pg-owns = { $n ->
    [one] This role owns { $n } object in { $database } (schemas, tables, sequences or the database itself). An owner holds every privilege on what it owns, and PostgreSQL records no grant for that, so none of it is listed below.
   *[other] This role owns { $n } objects in { $database } (schemas, tables, sequences or the database itself). An owner holds every privilege on what it owns, and PostgreSQL records no grant for that, so none of it is listed below.
}
users-note-pg-owns-cluster = { $n ->
    [one] This role owns { $n } object in this cluster (schemas, tables, sequences or the database itself). An owner holds every privilege on what it owns, and PostgreSQL records no grant for that, so none of it is listed below.
   *[other] This role owns { $n } objects in this cluster (schemas, tables, sequences or the database itself). An owner holds every privilege on what it owns, and PostgreSQL records no grant for that, so none of it is listed below.
}
# SQL Server: $n permissions the list could not render, on $classes — the
# server's own securable-class names, joined with ", ".
users-note-ms-unshown = { $n ->
    [one] { $n } permission is not shown here, on { $classes }: the server reports it on a securable this list cannot name.
   *[other] { $n } permissions are not shown here, on { $classes }: the server reports them on securables this list cannot name.
}
users-note-ms-pick-database = A database user's permissions are that database's — pick one to see them.
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
# What stops a trigger being emitted (core's ddl::TriggerProblem), in the
# footer. $engine is a product name (SQLite, SQL Server, MySQL); $event an
# event's SQL (INSERT, UPDATE, DELETE); the rest of the SQL stays as written.
trigger-problem-no-name = The trigger needs a name.
trigger-problem-no-table = A trigger needs a table to fire on.
trigger-problem-no-event = A trigger needs at least one event to fire on.
trigger-problem-pg-no-function = A PostgreSQL trigger needs a function to execute.
trigger-problem-pg-body = A PostgreSQL trigger runs a function, not a body — pick or write one to execute.
trigger-problem-truncate-row-level = A TRUNCATE trigger has to be FOR EACH STATEMENT.
trigger-problem-instead-of-row-only = An INSTEAD OF trigger has to be FOR EACH ROW.
trigger-problem-instead-of-view-only = Only a view can have an INSTEAD OF trigger — a table takes BEFORE or AFTER.
trigger-problem-pg-view-row-timing = A view's BEFORE or AFTER trigger has to be FOR EACH STATEMENT — use INSTEAD OF to act on rows.
trigger-problem-update-of-without-update = UPDATE OF names columns on an UPDATE trigger.
trigger-problem-pg-instead-of-columns = An INSTEAD OF trigger takes no column list — PostgreSQL fires it on any UPDATE of the view.
trigger-problem-one-event = { $engine } fires a trigger on one event — make a separate trigger per event.
trigger-problem-no-truncate = { $engine } has no TRUNCATE trigger.
trigger-problem-sqlite-view-timing = A view's trigger has to be INSTEAD OF — SQLite has no BEFORE or AFTER trigger on a view.
trigger-problem-sqlite-row-only = SQLite has only FOR EACH ROW triggers — there is no statement-level one.
trigger-problem-no-body = A { $engine } trigger needs a body.
trigger-problem-sqlite-not-block = A SQLite trigger's body has to be a BEGIN … END block holding at least one statement.
trigger-problem-runs-body = A { $engine } trigger runs a body, not a function — write the statements to run.
trigger-problem-ms-no-before = SQL Server has no BEFORE trigger — use INSTEAD OF to act before the write.
trigger-problem-ms-view-timing = A view's trigger has to be INSTEAD OF on SQL Server.
trigger-problem-ms-statement-only = SQL Server fires a trigger once per statement — read the rows from inserted and deleted.
trigger-problem-ms-instead-of-rank = An INSTEAD OF trigger can't have a firing order — set each event back to any order.
# SQL Server: a firing order set on an event the trigger doesn't fire on.
trigger-problem-first-without-event = It fires first on { $event }, which it isn't a trigger for.
trigger-problem-last-without-event = It fires last on { $event }, which it isn't a trigger for.
trigger-problem-no-when = { $engine } has no WHEN condition — put the test inside the body with IF.
trigger-problem-ms-no-update-of = SQL Server has no UPDATE OF — test UPDATE(column) inside the body.
trigger-problem-my-no-view = MySQL takes no trigger on a view.
trigger-problem-my-no-instead-of = MySQL has no INSTEAD OF trigger — those are PostgreSQL's.
trigger-problem-my-no-update-of = MySQL has no UPDATE OF — a trigger sees every column.
# $name is a trigger's name.
trigger-problem-constraint-trigger = Schemaic can't edit the constraint trigger { $name } — it doesn't model the deferral settings one carries.
trigger-problem-not-editable = Schemaic can't edit the trigger { $name } — its definition is encrypted, hidden from this login, or uses an option Schemaic doesn't model. Change it in SQL instead.
trigger-problem-twice = Two triggers are both called { $name }.
# SQL Server: $n triggers set to fire first (or last) on $event; $names are all
# but the last, joined with ", ".
trigger-problem-first-clash = { $n ->
    [2] Only one trigger can fire first on { $event } — { $names } and { $last } are both set to.
   *[other] Only one trigger can fire first on { $event } — { $names } and { $last } are all set to.
}
trigger-problem-last-clash = { $n ->
    [2] Only one trigger can fire last on { $event } — { $names } and { $last } are both set to.
   *[other] Only one trigger can fire last on { $event } — { $names } and { $last } are all set to.
}
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
# What stops a type, domain or sequence being emitted (core's
# ddl::ObjectProblem), in the footer. Figures are as printed; $value is an enum
# label in double quotes; $name a constraint's name; $data_type a SQL type.
object-problem-type-no-name = The type needs a name.
object-problem-duplicate-value = The value { $value } is listed more than once.
object-problem-empty-value = A value can't be empty — PostgreSQL can never remove it.
object-problem-domain-no-name = The domain needs a name.
object-problem-domain-no-base = A domain needs a type to be based on.
object-problem-constraint-no-name = Every constraint needs a name.
object-problem-constraint-no-predicate = Constraint { $name } has no predicate.
object-problem-constraint-twice = Constraint { $name } is named twice.
object-problem-sequence-no-name = The sequence needs a name.
object-problem-zero-increment = A sequence can't increment by 0.
object-problem-min-above-max = The minimum ({ $min }) is above the maximum ({ $max }).
object-problem-start-outside = The start value ({ $start }) is outside { $min }..{ $max }.
object-problem-cache-below-one = The cache has to be at least 1.
object-problem-does-not-fit = { $min }..{ $max } doesn't fit in { $data_type }.
object-problem-restart-outside = Restarting at { $restart } is outside { $min }..{ $max }.
# $last is where the sequence's counter is now.
object-problem-counter-outside = The sequence is at { $last }, outside the new { $min }..{ $max }. Give it a restart value inside the range.
# SQL Server's objects.
object-problem-no-name = The name can't be empty.
object-problem-tsql-no-type = A sequence needs a type.
object-problem-start-not-whole = The start must be a whole number.
object-problem-increment-not-whole = The increment must be a whole number.
object-problem-minimum-not-whole = The minimum must be a whole number.
object-problem-maximum-not-whole = The maximum must be a whole number.
object-problem-tsql-zero-increment = The increment can't be 0.
object-problem-min-not-below-max = The minimum must be below the maximum.
# An existing sequence that started at $start (as typed), outside the new range.
object-problem-started-outside = It started at { $start }, outside the new range: Restart at a value between the minimum and the maximum.
object-problem-tsql-start-outside = The start must lie between the minimum and the maximum.
object-problem-cache-not-positive = The cache size must be a whole number above 0.
object-problem-restart-not-whole = The restart value must be a whole number.
object-problem-tsql-restart-outside = The restart value must lie between the minimum and the maximum.
object-problem-synonym-no-target = A synonym needs the object it stands for.
object-problem-alias-no-base = An alias type needs a base type.
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
# Why Preview is held back, under the form (core's users::AccountBlocker).
# Microsoft Entra is a product name.
account-editor-password-required = A password is required.
account-editor-name-required = A name is required.
account-editor-entra-only-user = Only a database user is made from Microsoft Entra.
account-editor-entra-no-password = An Entra user has no login or password here — Entra signs it in.
account-editor-login-needs-password = A login needs a password.
account-editor-user-for-login-password = A user for a login signs in with the login's password — leave this blank.
account-editor-confirm-password = Type the password again to confirm it.
account-editor-passwords-differ = The two passwords differ.
# Under the password field: what of the password the previewed SQL shows
# (core's users::PasswordHint) — a hash of it on PostgreSQL, the password
# itself elsewhere.
account-editor-password-hashed = The previewed SQL, which is the statement that runs, carries a hash of the password in the form this server stores, not the password itself — unless it has characters the login would normalise (full-width letters, ligatures, no-break spaces), or the server runs a password-check extension, which are sent as typed.
account-editor-password-as-typed = The password appears in the previewed SQL, which is the statement that runs.

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
# The designer's index advice, under the change count (core's ddl::IndexHint).
# $index, $fk and $cover are names; $cover may be the SQL PRIMARY KEY.
designer-hint-redundant = Index { $index } is covered by { $cover }: every lookup it serves, that one serves too.
# What stops a table design being emitted (core's ddl::TableProblem), in the
# footer. $name, $index, $column, $fk and $parent are the draft's own names;
# the SQL in them stays as written.
table-problem-no-name = The table needs a name.
table-problem-no-columns = A table needs at least one column.
table-problem-column-no-name = Every column needs a name.
table-problem-column-twice = Two columns are both called { $name }.
table-problem-column-no-type = Column { $name } needs a type.
table-problem-key-not-column = The primary key names { $column }, which isn't a column.
table-problem-index-no-name = Every index needs a name.
table-problem-index-twice = Two indexes are both called { $name }.
table-problem-index-no-columns = Index { $index } has no columns.
table-problem-xml-one-column = Index { $index } must name exactly one column.
table-problem-xml-no-options = Index { $index } can't be unique, include columns or be filtered: an XML or spatial index takes none of those.
table-problem-xml-no-using = Index { $index } needs its USING XML INDEX … FOR … clause: a secondary XML index is built on a primary or selective one.
table-problem-selective-no-paths = Index { $index } needs its FOR (…) paths: a selective XML index is the paths it promotes.
table-problem-xml-parent-gone = Index { $index } is built on { $parent }, which isn't a primary or selective XML index of this table: it goes when its parent does.
table-problem-not-orderable = Index { $index } can't be ordered: only a columnstore index has an ORDER.
table-problem-order-not-column = Index { $index } orders by { $column }, which isn't a column.
table-problem-order-not-stored = Index { $index } orders by { $column }, which it doesn't store.
table-problem-columnstore-unique = Index { $index } can't be unique: a columnstore index stores columns, not keys.
table-problem-columnstore-include = Index { $index } can't include columns: a columnstore index lists every column it stores.
table-problem-columnstore-sorted = Index { $index } can't sort its columns: a columnstore column has no direction.
table-problem-clustered-columnstore-filtered = Index { $index } can't be filtered: a clustered columnstore index stores every row.
table-problem-clustered-include = Index { $index } can't include columns: a clustered index holds every column already.
table-problem-unique-include = Index { $index } can't include columns: a unique constraint has no INCLUDE here.
table-problem-clustered-filtered = Index { $index } can't be filtered: a clustered index covers every row.
table-problem-constraint-filtered = Index { $index } can't be filtered: it backs a UNIQUE constraint, which takes no condition here. Drop it and add a unique index with the condition.
table-problem-not-partial = Index { $index } can't be filtered: this index can't be partial here.
table-problem-index-not-column = Index { $index } names { $column }, which isn't a column.
table-problem-include-not-column = Index { $index } includes { $column }, which isn't a column.
# More than one clustered structure. $clustered lists them — the -key and
# -index phrases below, joined with -and between each.
table-problem-cluster-clash = A table holds only one clustered index, and { $clustered } each cluster it - make all but one nonclustered.
table-problem-cluster-key = the primary key
table-problem-cluster-index = index { $index }
table-problem-cluster-and = and
table-problem-fk-no-name = Every foreign key needs a name.
table-problem-fk-twice = Two foreign keys are both called { $name }.
table-problem-fk-no-table = Foreign key { $fk } references no table.
table-problem-fk-unpaired = Foreign key { $fk } must pair each column with one it references.
table-problem-fk-not-column = Foreign key { $fk } names { $column }, which isn't a column.
table-problem-check-no-name = Every check constraint needs a name.
table-problem-check-twice = Two check constraints are both called { $name }.
table-problem-check-no-predicate = Check { $name } has no predicate.
table-problem-without-rowid-no-key = A WITHOUT ROWID table needs a primary key.
# SQLite: $column of a STRICT table has $data_type, which it doesn't take.
table-problem-strict-type = STRICT tables take only INT, INTEGER, REAL, TEXT, BLOB or ANY — { $column } is { $data_type }.
designer-hint-unindexed-fk = Foreign key { $fk } has no index starting with its columns, so a delete or key update on the table it references scans this one.
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
# Under it on SQL Server (core's connection::read_only_caveat).
connections-read-only-mssql = SQL Server has no read-only session, so reads run in a transaction Schemaic rolls back. A procedure's effect outside the database survives that; for a guarantee, connect with a login that can only read.
# "schemaic" and "schemaic mcp" are a command and its subcommand: never translated.
connections-cli-access = CLI access
connections-cli-access-hint = Let the schemaic command line, and AI agents through schemaic mcp, use this connection.
connections-ai-data-access = AI data access
connections-statement-timeout = Statement timeout
connections-statement-timeout-hint = Cancels a statement on this connection that runs longer.
# The connection's timeout choice that follows the app's (core's
# statement_timeout_choice_label); the other choices are timeout-*.
connections-statement-timeout-same = Same as the app
connections-environment = Environment
# Its choices, also the top bar's badge (core's connection::Environment).
env-none = None
env-local = Local
env-development = Development
env-testing = Testing
env-staging = Staging
env-production = Production
connections-type = Type
connections-init-sql = Init SQL
# PRAGMA and SET are SQL keywords.
connections-init-sql-pragma-hint = Optional. PRAGMA settings run on every connection Schemaic opens.
connections-init-sql-set-hint = Optional. SET statements run on every connection Schemaic opens — a search path, a time zone, a role.
connections-host = Host
connections-port = Port
# Under Host and Port for a SQL Server named instance (core's
# connection::named_instance_port_note). SQL Server Browser is a product name;
# Port is the field above.
connections-named-instance-port = A named instance: SQL Server Browser gives its port, so Port is not used. To skip the Browser, set the host without the instance and the port it listens on.
connections-database = Database
connections-database-hint = Optional, and the database the connection opens in. Some hosted providers allow only their own and refuse a connection that does not name it.
connections-authentication = Authentication
# The SSH tunnel's authentication choices (core's connection::SshAuth).
ssh-auth-password = Password
ssh-auth-key-pair = Key pair
ssh-auth-agent = SSH agent
# The sign-in method's choices (core's connection::AuthMode). Microsoft Entra,
# AWS IAM, Google Cloud IAM and the CLIs in brackets are product names.
auth-password = User and password
auth-windows = Windows (this account)
auth-azure-cli = Microsoft Entra (Azure CLI)
auth-aws-iam = AWS IAM (AWS CLI)
auth-gcp-iam = Google Cloud IAM (gcloud)
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
# Its modes and what each protects (core's connection::SslMode). CA is a
# certificate authority.
tls-disable = Disable
tls-prefer = Prefer
tls-require = Require
tls-verify-ca = Verify CA
tls-verify-full = Verify full
tls-disable-description = Connect in plain text.
tls-prefer-description = Encrypt if the server offers it. No verification.
tls-require-description = Refuse to connect without encryption. No verification.
tls-verify-ca-description = Encrypt, and check the certificate against the CA.
tls-verify-full-description = Encrypt, check the certificate, and check the host name.
# Under Verify CA on MySQL and MariaDB (core's SslMode::caveat).
tls-verify-ca-checks-host = On MySQL and MariaDB this also checks the host name, so it behaves as Verify full — a driver limitation, not a setting.
# When the sign-in method connects at a stronger mode than the one picked
# (core's AuthMode::tls_note). Root CA is the field below; RDS, Azure SQL and
# Cloud SQL are product names.
tls-floor-azure-cli = Microsoft Entra sign-in connects at Verify full whatever is picked here: its token opens every Azure SQL database the account can reach.
tls-floor-aws-iam = AWS IAM sign-in connects at Require at least: its token must not cross the network in the clear. Verify full needs the RDS CA bundle as Root CA.
tls-floor-gcp-iam = Google Cloud IAM sign-in connects at Verify CA at least — set Root CA to the instance's server CA: its token opens every Cloud SQL instance the account can reach.
tls-floor-other = This sign-in connects at a stronger mode than the one picked here.
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
# The problem list's tail, under the lines it shows (core's import::IssueTail).
# $shown is how many lines are listed, $total how many the check found; the
# check stops counting at a cap, which the last two say.
import-tail-cut = Showing { $shown } of { $total }.
import-tail-uncounted = Showing all { $total } — the check stopped counting there, so the file holds more.
import-tail-cut-and-uncounted = Showing { $shown } of the first { $total } — the check stopped counting there, so the file holds more.
# Before a large load (core's import::MemoryWarning). $size is the file's size
# and $estimate the memory the load needs, both as printed (600 MiB).
import-memory-json = This JSON file is { $size }, and a JSON import is held in memory while it loads — expect it to need about { $estimate }. A CSV of the same data loads in constant memory.
import-memory-xlsx = This workbook is { $size }, and an Excel import is held in memory while it loads — expect it to need roughly { $estimate }, since the file is compressed. A CSV of the same sheet loads in constant memory.
# Why a file could not be read or planned (core's import::ImportError). $error
# is either a library's own text, as written, or one of the import-refuse-*
# sentences below.
import-read-failed = Couldn't read the file: { $error }
import-no-columns-mapped = No file columns are mapped to table columns
# $mib is the preview's cap in MiB.
import-preview-record-too-large = its first JSON record is larger than the { $mib } MiB the preview reads, so there is nothing to show
# Schemaic's own reasons (core's import::ReadRefusal). Sizes are as printed
# (512 MiB); $max is the largest workbook this opens.
import-refuse-not-objects = expected JSON objects (an array of them, or one per line)
import-refuse-oversize = This workbook is { $size }, and an Excel file has to be read whole before any of it can be shown — { $max } is the most this can open. Export the sheet as CSV and import that instead; a CSV loads in constant memory.
import-refuse-oversize-unsized = This workbook is over { $max }, and an Excel file has to be read whole before any of it can be shown — { $max } is the most this can open. Export the sheet as CSV and import that instead; a CSV loads in constant memory.
# $unpacked is what the workbook's parts declare they unpack to, at least;
# $ceiling the most unpacked content this opens.
import-refuse-inflated = This workbook is { $size } on disk but unpacks to at least { $unpacked }, and an Excel file has to be read whole before any of it can be shown — { $ceiling } of unpacked content is the most this will open, and it needs more memory than that to hold it. Export the sheet as CSV and import that instead; a CSV loads in constant memory.
# $sheet is the sheet the import was set up from; $sheets the ones the
# workbook holds, joined with ", ".
import-refuse-no-such-sheet = this workbook has no sheet called "{ $sheet }" — it has { $sheets }
import-refuse-no-sheets = this workbook has no sheets
# $width is the sheet's declared column count; $max what a worksheet holds.
import-refuse-too-wide = this sheet claims { $width } columns; an Excel worksheet holds { $max }. Export it as CSV and import that instead.
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
# Its hint, which differs by engine (core's DropHint; English must read exactly
# as its text). CREATE and DROP TABLE IF EXISTS are SQL; One transaction is the
# toggle below (dump-one-transaction); mysql.procs_priv is a table name.
dump-drop-hint-up-front = Drop each table and view before its CREATE, so the file loads onto a database that already holds them. With One transaction it also replaces every routine in their schemas, and stops before dropping anything it cannot put back as it is. Without One transaction it leaves routines as they are, so a replay onto a database that still has them stops at the first one. Each table's owner and permissions are put back at the end.
dump-drop-hint-routines-in-place = DROP TABLE IF EXISTS before each CREATE, so the file loads onto a database that already holds these tables. It also replaces every routine in their schemas. Each table's owner, grants and row security are put back at the end.
dump-drop-hint-routines-dropped-first = DROP TABLE IF EXISTS before each CREATE, so the file loads onto a database that already holds these tables. It also replaces every routine and event in their schemas: on MySQL each is dropped first, which revokes every grant on it, and the file grants them back at its end where your account can read mysql.procs_priv.
dump-drop-hint-tables-only = DROP TABLE IF EXISTS before each CREATE, so the file loads onto a database that already holds these tables.
dump-one-transaction = One transaction
dump-one-transaction-hint = Wrap the load, so a failure leaves nothing behind.
dump-disable-fk-checks = Disable foreign-key checks
# Its hint (core's FkHint; English must read exactly as its text). The second
# is for an engine where dropping a table runs its ON DELETE actions (SQL).
dump-fk-hint = Off for the duration of the load, so the order rows arrive in can't refuse them.
dump-fk-hint-drops = Off for the duration of the load, so the order rows arrive in can't refuse them. A file that drops tables turns them off anyway: a drop with them on deletes the rows of tables outside the file through their ON DELETE actions.
dump-no-fk-guard-hint = This engine has no session switch for foreign-key checks; the file adds the constraints after the data instead.
dump-export = Export
# The progress line; $rows is an abbreviated count (9.4k).
dump-progress = Writing { $table } — { $index } of { $total }, { $rows } rows so far
# Animated with trailing dots while it lasts.
dump-reading-schema = Reading the schema
# The title of the prompt asking whether to overwrite files already in the folder.
dump-replace-files = Replace files
# Its question (core's dump::folder_replace_prompt; English must read exactly
# as it does). $n counts the files, $names lists up to eight, $more the rest;
# $folder is the folder.
dump-replace-prompt = { $n ->
    [one] { $n } file in { $folder } will be replaced, and it cannot be recovered: { $names }.
   *[other] { $n } files in { $folder } will be replaced, and they cannot be recovered: { $names }.
}
dump-replace-prompt-more = { $n } files in { $folder } will be replaced, and they cannot be recovered: { $names }, and { $more } more.
# What a finished dump says (core's dump::done_note): each an own sentence,
# joined by a space. The export's own note (export-note-*) follows the first.
# $n counts, $names lists. A graph edge table is SQL Server's.
# $note is the export's note (export-note-*), made a sentence of its own.
dump-done-export = { $note }.
dump-done-tables = { $n ->
    [one] Wrote { $n } table.
   *[other] Wrote { $n } tables.
}
dump-done-missing = { $n ->
    [one] { $n } ticked table not found and is not in the file: { $names }.
   *[other] { $n } ticked tables not found and are not in the file: { $names }.
}
dump-done-left-out = { $n ->
    [one] { $n } object the export asked for is not in the file: { $names }.
   *[other] { $n } objects the export asked for are not in the file: { $names }.
}
dump-done-rows-left-out = { $n ->
    [one] The rows of graph edge table { $names } are not in the file; it is created empty.
   *[other] The rows of graph edge tables { $names } are not in the file; they are created empty.
}
dump-done-refused = { $n ->
    [one] A replay onto a database that already holds this stops before it drops anything: the file cannot put it back as it is: { $names }.
   *[other] A replay onto a database that already holds any of these stops before it drops anything: the file cannot put them back as they are: { $names }.
}
# A cancelled dump that had begun writing. $part is the .part file.
dump-cancelled-partial = Export cancelled — { $name } was not changed; what had been written is in { $part }
# A folder export's outcome (core's export::files_*). $n counts files, $folder
# is the folder; replaced and missing files follow as sentences of their own.
files-done = { $n ->
    [one] Wrote { $n } file.
   *[other] Wrote { $n } files.
}
# $head is files-done, $note the export's note (export-note-*).
files-done-with-note = { $head } { $note }.
files-cancelled-none = Export cancelled — no file was finished, so nothing was written to { $folder }.
files-cancelled = { $n ->
    [one] Export cancelled — { $n } file was finished in { $folder }; the table in progress was left unwritten.
   *[other] Export cancelled — { $n } files were finished in { $folder }; the table in progress was left unwritten.
}
# $message already says the export failed.
files-failed-kept = { $n ->
    [one] { $message } — { $n } file already written to { $folder } is kept.
   *[other] { $message } — { $n } files already written to { $folder } are kept.
}
files-replaced = { $n ->
    [one] { $n } existing file was replaced: { $names }.
   *[other] { $n } existing files were replaced: { $names }.
}
files-missing = { $n ->
    [one] { $n } ticked table not found and was no file: { $names }.
   *[other] { $n } ticked tables not found and were no file: { $names }.
}

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
# Why the chosen key cannot match rows (core's result_diff::DiffError);
# $column is a column name.
diff-missing-key = both results need a column named { $column } to match rows by
diff-binary-key = { $column } holds binary values, which are not compared, so it cannot match rows
# The footer's headline (core's ResultDiff::summary), its parts joined by a
# middle dot: each kind with rows, then the unchanged count, then any binary
# column left out ($columns, comma-separated names).
diff-summary-identical = { $n ->
        [one] No differences · { $n } row unchanged
       *[other] No differences · { $n } rows unchanged
    }
diff-summary-changed = { $n } changed
diff-summary-added = { $n } added
diff-summary-removed = { $n } removed
diff-summary-unchanged = { $n } unchanged
diff-summary-not-compared = { $columns } not compared
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
## The risk lines, and the Used by, withheld and omitted lines are core's words
## and stay as they are for now.

# The heading over what the plan destroys: every consequence listed can be
# reversed, or not. English must read exactly as core's ddl::risk_heading_for.
preview-risk-reversible = Before you apply
preview-risk-irreversible = This can't be undone

## The change list: one line per change (core's ddl::ChangeLine). English must
## read exactly as core's Change::summary (a test holds it). Names, types, SQL
## expressions and clause keywords (default, generated as, on update, collate,
## NOT NULL, RESTART WITH …) are never translated. $what is an object kind as
## kind-* words it; $columns, $indexes and similar are lists already joined.

ddl-line-create-table = { $columns ->
    [one] Create table { $name } with { $columns } column
   *[other] Create table { $name } with { $columns } columns
}
ddl-line-rebuild-table = Rebuild { $name } — this can't be altered in place, so the rows are copied into a new table and it takes the old one's place
ddl-line-drop-table = Drop the table
ddl-line-drop-foreign-table = Drop the foreign table
ddl-line-truncate-table = Delete every row
ddl-line-rename-table = Rename the table to { $to }
# $clauses is the column's clauses, each a ddl-clause-added, joined by commas;
# $has_clauses is yes or no.
ddl-line-add-column = Add column { $name } { $datatype }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-drop-column = Drop column { $name }
ddl-line-rebuild-computed-column = Rebuild computed column { $name } as ({ $expression }) around the column change
# $retyped is yes when the rename also changes the type from $old_type to
# $new_type; $clauses is each ddl-clause-changed, joined by commas.
ddl-line-rename-column = Rename column { $from } to { $to }{ $retyped ->
        [yes] , and its type from { $old_type } to { $new_type }
       *[no] {""}
    }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-move-column-first = Move column { $name } first
ddl-line-move-column-after = Move column { $name } after { $after }
ddl-line-move-column = Move column { $name }
ddl-line-retype-column = Change column { $name } from { $from } to { $to }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
ddl-line-change-column = Change column { $name }{ $has_clauses ->
        [yes] , { $clauses }
       *[no] {""}
    }
# $clause is the clause's keyword (default, collate…), $value its SQL.
ddl-clause-added = { $clause } { $value }
ddl-clause-changed = { $clause } { $from } → { $to }
# A clause the column does not have, on one side of ddl-clause-changed.
ddl-clause-none = (none)
ddl-line-add-primary-key = Add primary key ({ $columns })
ddl-line-drop-primary-key = Drop the primary key ({ $columns })
ddl-line-rebuild-primary-key = Rebuild the primary key ({ $columns }) around the column change
ddl-line-change-primary-key = Change the primary key from ({ $from }) to ({ $to })
# Each optional part has a yes/no selector beside its value: $kind (one of
# ddl-index-kind-*), $method (an access method: gist, gin…), $columns (the
# key), $include, $order (lists) and $predicate (the filter's SQL).
ddl-line-add-index = Add { $unique ->
        [yes] unique{" "}
       *[no] {""}
    }{ $has_kind ->
        [yes] { $kind }{" "}
       *[no] {""}
    }index { $name }{ $has_method ->
        [yes] {" "}using { $method }
       *[no] {""}
    }{ $has_columns ->
        [yes] {" "}on ({ $columns })
       *[no] {""}
    }{ $has_include ->
        [yes] {" "}including ({ $include })
       *[no] {""}
    }{ $has_order ->
        [yes] {" "}ordered by ({ $order })
       *[no] {""}
    }{ $has_predicate ->
        [yes] {" "}where { $predicate }
       *[no] {""}
    }
ddl-index-kind-columnstore = columnstore
ddl-index-kind-clustered-columnstore = clustered columnstore
ddl-index-kind-primary-xml = primary XML
ddl-index-kind-selective-xml = selective XML
ddl-index-kind-secondary-xml = secondary XML
ddl-index-kind-spatial = spatial
ddl-line-drop-index = Drop index { $name }
ddl-line-rename-index = Rename index { $from } to { $to }
ddl-line-keep-lossy-index = Leave index { $name } unchanged — Schemaic can't read all of it
ddl-line-add-foreign-key = Add foreign key { $name } → { $table }({ $columns })
ddl-line-drop-foreign-key = Drop foreign key { $name }
ddl-line-add-check = Add check { $name } ({ $expression })
ddl-line-drop-check = Drop check { $name }
# $parts is ddl-table-option-* joined by commas.
ddl-line-table-options = Set the table's { $parts }
ddl-table-option-engine = engine to { $engine }
ddl-table-option-collation = collation to { $collation }
ddl-table-option-comment = comment
ddl-table-option-comment-cleared = comment (cleared)
ddl-line-create-view = Create view { $name }
ddl-line-create-view-index = Create view { $name } and its index { $index }
ddl-line-create-view-indexes = Create view { $name } and its indexes { $indexes }
ddl-line-redefine-view = Redefine view { $name }
ddl-line-recreate-view = Drop and re-create view { $name }
ddl-line-rename-view = Rename the view to { $to }
ddl-line-drop-view = Drop the view
ddl-line-drop-materialized-view = Drop the materialized view
ddl-line-refresh-view = Rebuild the materialized view's rows — readers are blocked until it finishes
ddl-line-refresh-view-concurrently = Rebuild the materialized view's rows without blocking readers
ddl-line-create-trigger = Create trigger { $name }
ddl-line-redefine-trigger = Redefine trigger { $name }
ddl-line-recreate-trigger = Re-create trigger { $name }
# $from is the trigger's name on the server, $name the one it takes.
ddl-line-recreate-trigger-as = Re-create trigger { $from } as { $name }
ddl-line-drop-trigger = Drop trigger { $name }
# $what is a function or a procedure.
ddl-line-create-routine = Create { $what } { $name }
ddl-line-redefine-routine = Redefine { $what } { $name }
ddl-line-redefine-routine-as = Redefine { $what } { $from } as { $name }
ddl-line-recreate-routine = Re-create { $what } { $name }
ddl-line-recreate-routine-as = Re-create { $what } { $from } as { $name }
ddl-line-rename-routine = Rename { $what } { $from } to { $to }
ddl-line-drop-routine = Drop { $what } { $name }
ddl-line-create-event = Create event { $name }
ddl-line-alter-event = Alter event { $name }
# $edits is ddl-event-edit-* joined by commas.
ddl-line-alter-event-edits = Alter event { $name }: { $edits }
ddl-line-drop-event = Drop event { $name }
ddl-event-edit-rename = rename to { $to }
# $schedule is the schedule's SQL.
ddl-event-edit-schedule = schedule to { $schedule }
ddl-event-edit-status-enabled = status to enabled
ddl-event-edit-status-disabled = status to disabled
ddl-event-edit-status-replica = status to disabled on replica
ddl-event-edit-preserve = keep it after it completes
ddl-event-edit-no-preserve = delete it after it completes
ddl-event-edit-definer = definer to { $definer }
ddl-event-edit-comment = comment
ddl-event-edit-body = body
ddl-line-create-enum = { $values ->
    [one] Create type { $name } with { $values } value
   *[other] Create type { $name } with { $values } values
}
ddl-line-add-enum-value = Add value { $value }
ddl-line-add-enum-value-after = Add value { $value } after { $after }
ddl-line-add-enum-value-before = Add value { $value } before { $before }
ddl-line-rename-enum-value = Rename value { $from } to { $to }
# $values and $columns are counts.
ddl-line-recreate-enum = Re-create type { $name } ({ $values ->
    [one] { $values } value
   *[other] { $values } values
}, re-casting { $columns ->
    [one] { $columns } column
   *[other] { $columns } columns
})
ddl-line-create-domain = Create domain { $name } as { $datatype }
ddl-line-set-domain-default = Set the domain's default to { $default }
ddl-line-drop-domain-default = Drop the domain's default
ddl-line-domain-not-null = Make the domain NOT NULL
ddl-line-domain-nullable = Make the domain nullable
# $was is the type it had.
ddl-line-recreate-domain = Re-create domain { $name } as { $datatype } (was { $was }; re-casting { $columns ->
    [one] { $columns } column
   *[other] { $columns } columns
})
ddl-line-create-sequence = Create sequence { $name }
# $edits is ddl-sequence-edit-* joined by commas.
ddl-line-alter-sequence = Set the sequence's { $edits }
ddl-sequence-edit-type = type to { $datatype }
ddl-sequence-edit-increment = increment to { $value }
ddl-sequence-edit-minimum = minimum to { $value }
ddl-sequence-edit-maximum = maximum to { $value }
ddl-sequence-edit-start = start to { $value }
ddl-sequence-edit-cache = cache to { $value }
ddl-sequence-edit-cycling-on = cycling on
ddl-sequence-edit-cycling-off = cycling off
# $owner is table.column.
ddl-sequence-edit-owner = owner to { $owner }
ddl-sequence-edit-no-owner = owner to none
ddl-line-restart-sequence = Restart the sequence at { $value }
ddl-line-create-object = Create { $what } { $name }
ddl-line-replace-object = Drop the { $what } { $name } and create it again
ddl-line-replace-object-as = Drop the { $what } { $name } and create it again as { $to }
# $clauses is SQL Server's ALTER SEQUENCE clauses, as SQL.
ddl-line-alter-tsql-sequence = Alter the sequence: { $clauses }
ddl-line-rename-object = Rename the { $what } to { $to }
ddl-line-drop-object = Drop the { $what }
ddl-line-set-object-comment = Set the { $what }'s comment
ddl-line-clear-object-comment = Clear the { $what }'s comment
ddl-line-create-database = Create database { $name }
ddl-line-drop-database = Drop database { $name }
ddl-line-create-schema = Create schema { $name }
ddl-line-drop-schema = Drop schema { $name }
ddl-line-create-extension = Create extension { $name }
# $who is the account (app, app@%).
ddl-line-create-user = Create user { $who }
ddl-line-create-role = Create role { $who }
ddl-line-create-login = Create login { $who }
ddl-line-drop-user = Drop user { $who }
ddl-line-drop-role = Drop role { $who }
ddl-line-drop-login = Drop login { $who }
ddl-line-set-password = Set password for { $who }
# $privileges is up to three privilege names (SELECT, INSERT), or ddl-privileges-*.
ddl-line-grant-privileges = Grant { $privileges } to { $who }
ddl-line-revoke-privileges = Revoke { $privileges } from { $who }
ddl-privileges-none = no privileges
ddl-privileges-count = { $count } privileges
ddl-line-grant-role = Grant { $role } to { $member }
ddl-line-revoke-role = Revoke { $role } from { $member }
# A comparison's line: $subject is the object the change is about.
ddl-line-of = { $subject } — { $line }

## The risk block: what the plan destroys or changes for good (core's
## ddl::Risk). English must read exactly as core's Change::risks (a test holds
## it). Names, types, SQL keywords (NOT NULL, DENY, CASCADE, ALTER USER …) are
## never translated. $what is an object kind as kind-* words it.

# $engine is sqlserver where SQL Server's own guarded rebuild runs it.
ddl-risk-rebuild-table = { $engine ->
        [sqlserver] Drops { $name } and recreates it, copying every row across, and puts back its keys, indexes, checks, defaults and triggers, its own and its columns' comments, and the foreign keys other tables have on it. It stops before it starts if the table has anything else Schemaic doesn't restate — permissions, an owner of its own, a schema-bound dependent, system versioning, a ledger, replication, compression, index options, a disabled index, a bound rule or default, a sensitivity classification, a description on a key, index, default, check or trigger. Statistics created by hand are not carried over.
       *[other] Drops { $name } and recreates it, copying every row across. Anything about the table Schemaic didn't read is not carried over.
    }
ddl-risk-drop-table = Drops the table and every row in it.
ddl-risk-drop-foreign-table = Drops the foreign table: its definition goes, and the rows it reads stay where they are, on the foreign server or in the file; a view or routine that names it fails from then on.
ddl-risk-truncate-table = Deletes every row in the table. This can't be undone.
ddl-risk-drop-column = Drops column { $name } and all the data in it.
ddl-risk-rebuild-computed-column = Drops computed column { $name } and adds it back, so it moves to the end of the table.
ddl-risk-column-not-null = Column { $name } becomes NOT NULL — the statement fails if any row holds NULL.
ddl-risk-column-stops-generated = Column { $name } stops being generated. Its current values are kept, but nothing recomputes them from now on.
# $expression is SQL.
ddl-risk-column-becomes-generated = Column { $name } becomes generated from { $expression }. Every value in it is replaced by what the expression computes.
ddl-risk-column-expression-changes = Column { $name }'s expression changes from { $from } to { $to }. On PostgreSQL this drops and re-adds the column, so every value is recomputed and the column moves to the end of the table.
# $subject is a column's name, or ddl-risk-domain-subject; $loss is truncates,
# rounds or both.
ddl-risk-type-narrows = Narrowing { $subject } from { $from } to { $to } { $loss ->
        [truncates] truncates values that no longer fit
        [rounds] rounds every value in the column
       *[both] truncates values that no longer fit and rounds every value in the column
    }.
ddl-risk-type-rewrites = Changing { $subject } from { $from } to { $to } rewrites every value; it can fail or lose precision.
ddl-risk-domain-subject = domain { $name }
ddl-risk-primary-key-dropped = Leaves the table without a primary key — rows can no longer be edited from the grid.
ddl-risk-primary-key-rebuilt = Drops and re-adds the primary key around the column change. When it is the clustered index this rewrites the whole table, and a foreign key in another table that references it stops the plan.
# $columns is the old key's columns, joined; $count how many.
ddl-risk-primary-key-replaced = Replaces the primary key: { $count ->
        [one] { $columns } is
       *[other] { $columns } are
    } no longer unique, and duplicates are accepted from now on. On MySQL this rebuilds the clustered index, which rewrites the whole table.
ddl-risk-drop-foreign-key = Drops foreign key { $name }. Rows that no longer have a matching parent are accepted from now on, and existing data is not re-examined if it's added back.
ddl-risk-drop-unique-index = Drops unique index { $name }. Duplicate values are accepted from now on, and the grid may lose the key it was editing rows by.
# $cost is one of ddl-risk-view-cost-*.
ddl-risk-drop-view = { $materialized ->
        [yes] Drops the materialized view. { $cost }
       *[no] Drops the view. { $cost }
    }
# What goes with a dropped view, per engine; -restored where the plan puts its
# INSTEAD OF triggers back.
ddl-risk-view-cost-sqlite = Views that select from it stop resolving until it is back, and its INSTEAD OF triggers are dropped with it and aren't restored.
ddl-risk-view-cost-sqlite-restored = Views that select from it stop resolving until it is back, and its INSTEAD OF triggers are dropped with it.
ddl-risk-view-cost-sqlserver = Grants on it, its description and its INSTEAD OF triggers are dropped with it and aren't restored.
ddl-risk-view-cost-sqlserver-restored = Grants on it and its description are dropped with it and aren't restored; its INSTEAD OF triggers are dropped too.
ddl-risk-view-cost-other = Dependent views, rules, grants and its INSTEAD OF triggers are dropped with it and aren't restored.
ddl-risk-view-cost-other-restored = Dependent views, rules and grants are dropped with it and aren't restored; its INSTEAD OF triggers are dropped too.
# $why is sqlite, sqlserver or postgres: the engine's reason it can't replace in place.
ddl-risk-recreate-view = Re-creating { $name } drops it first. { $cost } { $why ->
        [sqlite] SQLite has no way to replace a view in place, so every edit to one — a rename included — takes this route.
        [sqlserver] SQL Server's sp_rename would leave the view's stored definition naming the old one, so a rename takes this route.
       *[postgres] PostgreSQL can't replace a view whose columns changed name, type or order, so this is the only way to apply the edit.
    }
ddl-risk-view-statements-replayed = { $count ->
        [one] { $count } statement dropped with it is put back afterwards, in the same transaction.
       *[other] { $count } statements dropped with it are put back afterwards, in the same transaction.
    }
# An indexed view's edit. $dropped is its indexes, joined, and $dropped_count
# how many; $case says which are built again: all, none, or some ($again those
# that are, $gone those that are not, with $again_count and $gone_count).
ddl-risk-view-indexes-dropped = Redefining view { $view } drops its { $dropped_count ->
        [one] index
       *[other] indexes
    } ({ $dropped }) — SQL Server's ALTER VIEW always does — { $case ->
        [all] and this plan builds { $again_count ->
            [one] it
           *[other] them
        } again after it, in the same transaction. On a large view that takes as long as building { $again_count ->
            [one] it
           *[other] them
        } did. An index with a fill factor, compression, padding, lock options, a filegroup or a description stops the plan before anything changes — Schemaic can't build those again. Statistics created by hand on the view are not carried over.
        [none] and this plan does not build { $gone_count ->
            [one] it
           *[other] them
        } again: the view it is made to match has no such index. Statistics created by hand on the view are not carried over.
       *[some] and this plan builds { $again } again after it, in the same transaction, but not { $gone }, which the view it is made to match does not have. An index with a fill factor, compression, padding, lock options, a filegroup or a description stops the plan before anything changes — Schemaic can't build those again. Statistics created by hand on the view are not carried over.
    }
ddl-risk-add-not-null-column = Column { $name } is NOT NULL with no default. If the table already has rows there is no value to give them, and the engine will refuse the whole plan.
ddl-risk-drop-check = Drops check { $name }. Rows the constraint refused are accepted from now on, and existing data is not re-examined if it's added back.
ddl-risk-keep-lossy-index = Your edit to index { $name } is not included. It uses something Schemaic can't read back — an expression key column, an operator class, or a NULLS ordering — and applying the edit would mean re-creating the index without it. Edit this index in SQL instead.
ddl-risk-drop-trigger = Drops trigger { $name }. Whatever it did on each write stops happening, and rows written from now on won't have it applied.
ddl-risk-redefine-trigger-in-place = Redefines trigger { $name } in place — it is not dropped, so its permissions stay — and what it does on each write changes from now on.
ddl-risk-recreate-trigger = Re-creating trigger { $name } drops it first. Where DDL isn't transactional (MySQL), a new definition the server rejects leaves the table with no trigger.
# $kind is trigger, function or procedure.
ddl-risk-signature-lost = { $kind ->
        [trigger] Trigger
        [function] Function
       *[procedure] Procedure
    } { $name } is signed (ADD SIGNATURE), and any change to it removes the signature — Schemaic can't sign it again without the certificate's key. Whatever the certificate let it do stops working until you run ADD SIGNATURE again.
ddl-risk-redefines-routine = Redefines { $name }. Everything that calls it runs the new body from now on, including triggers on other tables this edit never mentioned.
ddl-risk-routine-parameters-change = Changes { $name }'s parameter list. Callers that pass the old arguments no longer resolve to it.
ddl-risk-recreate-routine = Re-creating { $held } drops it first. Where DDL isn't transactional (MySQL), a new definition the server rejects leaves no { $what } at all.
ddl-risk-routine-grants-lost = { $comment ->
        [yes] Privileges granted on { $held } and its comment do not survive being dropped, and this plan does not restore them — re-apply any GRANT and COMMENT afterwards.
       *[no] Privileges granted on { $held } do not survive being dropped, and this plan does not restore them — re-apply any GRANT afterwards.
    }
# $members is the group's members (grp;2, grp;3), joined.
ddl-risk-drop-numbered-group = Drops { $what } { $name } and every member of its numbered group with it: { $members }.
ddl-risk-drop-routine = Drops { $what } { $name }. PostgreSQL refuses while a trigger still uses it, so any that do have to be dropped first.
ddl-risk-drop-event = Drops event { $name }. Its schedule and its body are gone with it — nothing else in the database holds a copy.
ddl-risk-one-shot-event = This event runs once and the server then deletes it — that's ON COMPLETION NOT PRESERVE. Turn Preserve on to keep it.
ddl-risk-one-shot-event-altered = Event { $name } will run once and the server will then delete it — that's ON COMPLETION NOT PRESERVE. Turn Preserve on to keep it.
ddl-risk-add-enum-value = Adding { $value } can't be undone in place — PostgreSQL can't remove an enum value, so taking it back means re-creating the type. The value also can't be used until this plan is applied.
ddl-risk-rename-enum-value = Every row holding { $from } reads { $to } from now on. If you meant to remove { $from } rather than rename it, delete it and apply that on its own.
# $kind is type or domain; $columns the columns re-cast, joined, $has_columns
# yes or no, and $more how many beyond those named.
ddl-risk-recreate = Re-creating { $kind ->
        [domain] domain
       *[type] type
    } { $name } drops and rebuilds it. Every column below is re-cast through text; a value the new definition doesn't accept fails the whole plan, and nothing is applied{ $has_columns ->
        [yes] : { $columns }{ $more ->
            [0] {""}
           *[other] {" "}and { $more } more
        }. A view or function built on it can't be listed here and would make the server refuse.
       *[no] . Nothing uses it today, but a view or function built on it can't be listed here and would make the server refuse.
    }
ddl-risk-drop-domain-check = Drops check { $name }. Every column of this domain stops being checked, and existing data is not re-examined if it's added back.
ddl-risk-domain-nullable = Every column of this domain starts accepting NULL, including ones in tables this edit never mentioned.
ddl-risk-domain-not-null = The statement fails if any column of this domain already holds NULL.
ddl-risk-drop-sequence = Drops the sequence and the position it had reached. The server refuses while a column's default still draws from it, so those defaults have to go first.
ddl-risk-drop-synonym = Drops the synonym. Nothing refuses it: a view, routine or query that names it fails from then on, when it next runs.
ddl-risk-drop-extension = Drops the extension and the functions, types and operators it installed. The server refuses while anything of yours still depends on them — a column of its type, an index or a server on its wrapper — and names what does.
ddl-risk-drop-used-object = Drops the { $what }. The server refuses while a column still uses it, so those columns have to change type first.
ddl-risk-restart-sequence = Restarts the counter at { $value }. Values already handed out are not changed, so a key it reaches again collides.
ddl-risk-replace-synonym = Drops the synonym and creates it again. Nothing refuses the drop: the two statements run in one transaction.
# $start is the value it starts over at; ddl-risk-start-value where the draft names none.
ddl-risk-replace-sequence = Drops the sequence and creates it again: the position it had reached is lost, and it starts over at { $start }. The server refuses the drop while a column's default still draws from it.
ddl-risk-start-value = its start value
ddl-risk-replace-object = Drops the { $what } and creates it again. The server refuses the drop while a column still uses it, so those columns have to change type first.
ddl-risk-replace-refused = { $alias_type ->
        [yes] Refused while it has permissions on it, an owner of its own, extended properties or a bound rule or default: the re-create would not restore them.
       *[no] Refused while it has permissions on it, an owner of its own or extended properties: the re-create would not restore them.
    }
ddl-risk-drop-database = Drops the database { $name } and everything in it — every table and all its rows, every view, index, routine and trigger. Nothing is kept and there is no undo: the only way back is a backup taken before this runs.
ddl-risk-drop-schema = Drops the schema { $name }. The server refuses while it still holds tables, views or routines, so this either takes an empty schema or fails{ $cascade ->
        [yes] {" "}— Schemaic never sends CASCADE.
       *[no] .
    }
ddl-risk-drop-login = Drops the login { $who }, and with it every server permission and server-role membership it holds — recorded nowhere else, so a login made again under this name starts without them. The server refuses while anything is connected as it, so its sessions have to end first. Its users in each database are not dropped: they stay, keeping their permissions, mapped to no login, and a new login of this name does not re-attach to them without ALTER USER … WITH LOGIN.
ddl-risk-drop-account = Drops { $who } and every privilege it holds. The grants are not recorded anywhere else, so putting the account back means granting them all again from memory. { $cuts_off ->
        [yes] Anything connected as it fails at its next statement in this database.
       *[no] Anything still connected as it keeps running until it disconnects.
    }
ddl-risk-set-password = Changes { $who }'s password. The old one cannot be recovered, so anything still configured with it — an application, a job, another developer's client — fails to connect until it is updated by hand. Sessions already open keep running.
# $privileges is up to three names or ddl-privileges-*; $cleared likewise, the
# column permissions a table revoke deletes, where $has_cleared is yes.
ddl-risk-revoke-privileges = Takes { $privileges } away from { $who }. Anything relying on it — an application, a job, a view someone else owns — starts failing at its next statement.{ $cascades ->
        [yes] {" "}The revoke carries CASCADE, so it is also taken from every account { $who } granted it on to, and what runs as those accounts fails the same way.
       *[no] {""}
    }{ $deny ->
        [yes] {" "}Where { $who } is denied it instead (DENY), the revoke lifts the denial — which gives access back if a role it belongs to grants it.
       *[no] {""}
    }{ $has_cleared ->
        [yes] {" "}On a table it also deletes every column-level grant and DENY of { $cleared } { $who } has on the table's columns: a column { $who } was denied is no longer denied, and one granted on its own is no longer granted.
       *[no] {""}
    }
ddl-risk-revoke-role = Takes the role { $role } away from { $member }, and with it every privilege the role carries.
ddl-risk-blank-password = { $who } will have no password: anyone who can reach the server can log in as it. Set one here, or with ALTER USER afterwards.
ddl-risk-host-is-everywhere = Its host is % — every machine on the network, not just this one.
ddl-risk-grant-server-wide = Grants { $privileges } to { $who } on the whole server — the instance itself, not the database being browsed.
ddl-risk-grant-every-database = Grants { $privileges } to { $who } on every database on this server — including databases that do not exist yet, not just the one being browsed.
ddl-risk-grant-keeps-column-denies = A table-level grant of { $privileges } deletes any DENY of it { $who } has on one of the table's columns, so the statement denies those columns to { $who } again straight after granting: a column { $who } was denied stays denied. Where { $who } also holds the grant option on the table, the server refuses to deny it again without CASCADE, and nothing is applied.
ddl-risk-star-refresh = Refreshes every view and inline function in this database that selects * from { $table }, directly or through a synonym: SQL Server binds those to the table's columns by position, and would otherwise read and write the wrong columns. The plan fails if one of them no longer compiles against the new columns. One in another database that selects * from { $table } is refreshed after the plan commits, outside its transaction: until then it reads the old positions, and if its refresh fails the plan stays applied and the view is named here.
ddl-risk-piecewise = SQL Server will not create, alter or drop a natively compiled module inside a transaction, so this plan runs one statement at a time and cannot be stopped: if a statement fails, the ones before it stay applied.
ddl-risk-drop-sequence-table = Drops the sequence: the value it has reached is lost, and anything that still draws from it fails.
# $rows is the figure as the properties panel prints it (~4.2m, 12).
ddl-risk-drop-scale = By the schema tree's statistics, that is { $rows } rows.
## An AI proposal the preview cannot open, on the proposal's card (core's
## propose::ProposeError). English must read exactly as its Display (a test
## holds it). Backticked names are the table's and the model's own.
propose-wrong-table = the proposal is for table `{ $proposed }`, but `{ $actual }` is open
propose-no-ops = the proposal asks for no changes
propose-no-such-column = `{ $table }` has no column named `{ $name }`
propose-column-exists = `{ $table }` already has a column named `{ $name }`
propose-no-such-index = `{ $table }` has no index named `{ $name }`
propose-index-exists = `{ $table }` already has an index named `{ $name }`
propose-no-such-foreign-key = `{ $table }` has no foreign key named `{ $name }`
propose-no-such-check = `{ $table }` has no check constraint named `{ $name }`
propose-key-width-mismatch = a foreign key needs as many referenced columns as referencing ones
propose-empty-index = an index needs at least one column
# $problem is what stops the table design (table-problem-*).
propose-invalid = the proposed table wouldn't be valid: { $problem }
propose-not-a-table = `{ $name }` is a view — a proposal can only change a table
# list_schema is a tool name: never translated.
propose-no-such-table = no table called `{ $name }` — call list_schema to see what exists
# $field is default, generated or check; $text is the model's SQL.
propose-not-an-expression = { $field ->
        [default] the default `{ $text }` isn't a single SQL expression
        [generated] the generated expression `{ $text }` isn't a single SQL expression
       *[check] the check expression `{ $text }` isn't a single SQL expression
    }
propose-not-a-type = `{ $text }` isn't a column type — give the type alone, with no default, no NOT NULL, and nothing after it
propose-unknown-action = `{ $text }` isn't a foreign-key action
# The card's own refusals, before core is asked.
propose-not-loaded = { $table } isn't loaded in { $database } right now — open the database in the schema tree, or wait for a refresh to finish, and try again.
propose-nothing-to-change = { $subject } already looks like that — there is nothing to change.
propose-no-database = This tab has no database selected, so a schema change applied from here would have nowhere to go. Pick one for this connection first — if every database is hidden, the SCHEMA eye is where to bring one back.
propose-other-connection = This change is about a tab on a different connection — switch to it, or select its connection in the tree, before applying a schema change.
# A code block's Run, refused for the same two reasons.
ai-run-no-database = This tab has no database selected, so a statement run from here would have nowhere to go. Pick one for this connection first — if every database is hidden, the SCHEMA eye is where to bring one back.
ai-run-other-connection = This chat is about a tab on a different connection. A new tab would open on the connection selected in the tree, so the statement would run somewhere else — switch to that tab, or select its connection in the tree, first.

## The withheld lines: what the plan asks for that this engine can't express,
## which keeps Apply off (core's ddl::Withheld). English must read exactly as
## core's ChangeSet::unsupported (a test holds it). A change it has no
## statement for is said as its change line (ddl-line-*).

# A column a plan renames or drops, inside a refusal.
ddl-moved-renamed = { $from }, which this plan renames to { $to }
ddl-moved-dropped = { $name }, which this plan drops
ddl-withheld-unreplayable-rename = { $count ->
        [one] Renaming { $was } to { $to } can't carry its { $count } trigger across: their SQL names { $was }, and re-writing it here would change what you wrote. Rename the view on its own after re-creating the triggers against the new name.
       *[other] Renaming { $was } to { $to } can't carry its { $count } triggers across: their SQL names { $was }, and re-writing it here would change what you wrote. Rename the view on its own after re-creating the triggers against the new name.
    }
# $members is the group's members (grp;2, grp;3), joined.
ddl-withheld-numbered-group = { $name } heads a numbered procedure group ({ $members }), and this plan drops it first, which drops every member of the group with it. Edit it under its own name and kind, which alters it in place and keeps them.
# $indexes is the view's indexes, joined; $count how many.
ddl-withheld-view-index-create = { $count ->
        [one] Creating view { $view } can't create its index { $indexes } — of a kind Schemaic can't restate. Create this view in SQL instead.
       *[other] Creating view { $view } can't create its indexes { $indexes } — of a kind Schemaic can't restate. Create this view in SQL instead.
    }
ddl-withheld-view-index-edit = { $count ->
        [one] Editing view { $view } drops its index { $indexes }, which Schemaic can't create again — of a kind Schemaic can't restate. Edit this view in SQL instead.
       *[other] Editing view { $view } drops its indexes { $indexes }, which Schemaic can't create again — of a kind Schemaic can't restate. Edit this view in SQL instead.
    }
ddl-withheld-tsql-lossy-index = Rebuilding { $table } would drop the index { $index }: it is of a kind Schemaic doesn't read whole
ddl-withheld-tsql-encrypted-trigger = Rebuilding { $table } would drop the trigger { $trigger }: its text is encrypted
ddl-withheld-tsql-signed-trigger = Rebuilding { $table } would drop the signature on the trigger { $trigger }, which can't be put back without the certificate's private key
ddl-withheld-tsql-renamed-trigger = Rebuilding { $table } under a new name would leave the trigger { $trigger } naming the old one
# An index SQLite's pragmas only partly read; $why is may-name (with $moved a
# ddl-moved-*), not-found, edited or no-text.
ddl-withheld-partly-read = Index { $index } was only partly read — SQLite keeps a partial index's predicate, and an expression key, only in the index's own CREATE text{ $why ->
        [may-name] , and that text may name { $moved }. Drop the index to go on without it.
        [not-found] , so it can't be created from what was read. Drop the index to go on without it.
        [edited] , so this edit to it can't be written — what was read is not the whole index. Undo the change to keep the index as it is, or drop it.
       *[no-text] {" "}— and there is no such text for this one, so recreating it would change what it indexes. Drop the index to go on without it.
    }
ddl-withheld-condition-names-dropped = Index { $index }'s condition names { $column }, which this plan drops. Edit the condition or drop the index first.
ddl-withheld-strands-triggers = This table has { $count ->
        [one] { $count } trigger
       *[other] { $count } triggers
    } on it, and their SQL is replayed exactly as SQLite stored it — which would still name { $moved }. SQLite accepts such a trigger and then refuses every write to the table. Drop or edit the trigger first, or rename the column with no other change so SQLite's own ALTER TABLE can rewrite it.
# $owner is ddl-withheld-trigger-on or ddl-withheld-some-trigger.
ddl-withheld-strands-other-trigger = { $owner } names { $moved }. A rebuild renames this table underneath that trigger and leaves its SQL exactly as it was, so the trigger would go on naming a column that is gone — SQLite accepts such a trigger and then refuses every write to *that* table. Drop or edit the trigger first, or rename the column with no other change so SQLite's own ALTER TABLE can rewrite it.
ddl-withheld-trigger-on = Trigger { $name } on { $table }
ddl-withheld-some-trigger = A trigger on another table
ddl-withheld-strands-generated-column = Generated column { $column } computes from { $moved }. A rebuild writes the table from the model, and nothing rewrites that expression — SQLite refuses the new table with "no such column" and the whole plan rolls back. Edit or remove { $column } first.
# $clauses is the ddl-sqlite-clause-* the declaration carries, the last joined by
# ddl-sqlite-clauses.
ddl-withheld-cannot-restate = This table's declaration carries { $clauses }, which Schemaic's model has no field for. A rebuild writes the table back from that model, so the clause would be gone from a table that still looked right — and every write depending on it would behave differently from then on. Change this table with a script instead (Copy DDL is the start of one), or take the clause off first.
ddl-sqlite-clauses = { $first } and { $last }
ddl-sqlite-clause-deferrable = a foreign key's DEFERRABLE clause
ddl-sqlite-clause-on-conflict = an ON CONFLICT clause
ddl-sqlite-clause-desc-key = a DESC primary-key column
ddl-sqlite-clause-collate-key = a COLLATE on a primary-key column
ddl-withheld-virtual-table = `{ $name }` is a virtual table. Its columns are a module's interface rather than a storage declaration, and its real data lives in shadow tables a rebuild would drop — an FTS5 index would be gone from a table that still held the text. Change it with a script instead (DROP and CREATE VIRTUAL TABLE), or edit the tables it indexes.
# A comparison's withheld line: $subject is the object it is about.
ddl-withheld-of = { $subject } — { $line }

## The Used by lines: what in the loaded schema still names something the plan
## renames, drops or retypes (core's used_by::UsedByNote and Listing). English
## must read exactly as core's change_warning and drop_warning (a test holds it).

# A count of one kind of dependent, in a list ("2 views and 1 routine").
ddl-used-by-count-view = { $count ->
    [one] { $count } view
   *[other] { $count } views
}
ddl-used-by-count-routine = { $count ->
    [one] { $count } routine
   *[other] { $count } routines
}
ddl-used-by-count-trigger = { $count ->
    [one] { $count } trigger
   *[other] { $count } triggers
}
ddl-used-by-count-event = { $count ->
    [one] { $count } event
   *[other] { $count } events
}
ddl-used-by-count-foreign-key = { $count ->
    [one] { $count } foreign key
   *[other] { $count } foreign keys
}
ddl-used-by-count-expression = { $count ->
    [one] { $count } table expression
   *[other] { $count } table expressions
}
# $first is every count but the last, joined by commas; $last the last.
ddl-used-by-counts = { $first } and { $last }
# $counts is the counts above; $names the first few dependents, joined; $more
# how many more there are.
ddl-used-by-listing = { $counts }: { $names }{ $more ->
        [0] {""}
       *[other] {" "}and { $more } more
    }
# A drop's line: $listing is ddl-used-by-listing.
ddl-used-by-drop = Used by { $listing }.
# A change's line: $subject is ddl-used-by-column or the object's name; $then
# is renamed (with $name the old name), dropped or retyped.
ddl-used-by-change = { $subject } is used by { $listing }. { $then ->
        [renamed] They still name { $name } after the rename.
        [dropped] They break when it is dropped.
       *[retyped] Check them against its new type.
    }
ddl-used-by-column = Column { $name }

# A comparison's risk: $subject is the object it happens to.
ddl-risk-of = { $subject } — { $risk }
ddl-risk-cycle = The foreign keys between these tables form a cycle, so no creation order satisfies all of them. One statement will be refused for referencing a table that does not exist yet, and neither MySQL nor MariaDB rolls DDL back.
# A name a table being dropped still holds and one being created needs: $names
# joined; $why says why the drop can't come first, $cost what the engine does.
ddl-risk-clash = { $holder } still holds { $names }, which creating { $claimer } needs — { $why ->
        [not-dropped] and this plan does not drop it
        [default] and it cannot be dropped first because a column default draws from it
       *[referenced] and it cannot be dropped first because another table references it
    }. { $cost ->
        [no-rollback] One statement will be refused, and neither MySQL nor MariaDB rolls DDL back.
        [rolls-back] One statement will be refused, which rolls the whole migration back.
       *[refused] One statement will be refused.
    }

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
# Why an entry was not imported: the "why" in "name (why)" above, so lower
# case with no full stop (core's conn_import::SkipReason). $driver is a
# driver or provider name as the source wrote it; $error a parser's text,
# or one of the conn-import-url-* sentences below.
conn-import-reason-unsupported-engine = unsupported engine ({ $driver })
conn-import-reason-no-server = no server to connect to
conn-import-reason-not-published = its port is not published to this machine
conn-import-reason-unreadable = could not be read ({ $error })
conn-import-reason-not-an-export = could not be read (not a Schemaic connections export: { $error })
# $version is the export's format version, a whole number.
conn-import-reason-newer-export = could not be read (exported by a newer Schemaic (version { $version }) — update to import it)
# Under the paste field when a pasted line is not a connection URL (core's
# conn_import::UrlError). The schemes (mysql://, sqlite:…), Server= and
# tcp:host are syntax, never translated.
conn-import-url-empty = Paste a connection URL.
conn-import-url-no-scheme = No scheme — a URL starts with mysql://, postgresql://, sqlserver:// or sqlite:, a SQL Server connection string with Server=.
# $scheme is the scheme as pasted, often a misspelling.
conn-import-url-unknown-scheme = Unknown scheme "{ $scheme }" — expected mysql, mariadb, postgresql, sqlserver or sqlite.
conn-import-url-no-host = No host in the URL.
conn-import-url-bad-port = "{ $port }" is not a port number.
conn-import-url-no-path = No database file after sqlite:.
# $server is the server as written; named pipes and LocalDB are SQL Server's transports.
conn-import-url-transport = "{ $server }" is reached over named pipes or LocalDB — Schemaic connects over TCP only.
conn-import-url-userinfo = The server name holds an @ — a user name or password goes in its own field, not in the server.
conn-import-url-not-sql-server = This connection string doesn't say it is SQL Server's — MySQL's and PostgreSQL's .NET drivers write the same keywords. Paste the server's URL instead (mysql://, postgresql://, sqlserver://), or, for SQL Server, write the server as tcp:host.
# Under a review list longer than it shows (core's conn_import::row_overflow_sentence).
conn-import-row-overflow = Showing the first { $shown } of { $total }. The other { $hidden } are still imported if they are ticked — this list stops here so the window stays responsive.
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
# One warning per line under it (core's plan::PlanNote), by the engine whose
# plan raised it. $table, $op and $wait are names from the plan itself; the
# Extra: and type = parts quote MySQL's plan columns and stay as written. A
# message ending -result or -unnamed is for a row the plan named no table on.
plan-detail-no-index = no index used
plan-detail-key-unused = index not used for lookup
plan-my-full-scan = Full table scan on `{ $table }` ({ $detail }, type = ALL)
plan-my-full-scan-result = Full table scan on the result ({ $detail }, type = ALL)
plan-my-filesort = Filesort on `{ $table }` (Extra: Using filesort)
plan-my-filesort-result = Filesort on the result (Extra: Using filesort)
plan-my-temporary = Temporary table for `{ $table }` (Extra: Using temporary)
plan-my-temporary-result = Temporary table for the result (Extra: Using temporary)
plan-pg-seq-scan = Sequential scan on `{ $table }` (no index used)
plan-pg-seq-scan-unnamed = Sequential scan on a table (no index used)
plan-pg-sort = Sort node — the query sorts rows on the fly
plan-ms-full-scan = Full scan of `{ $table }` ({ $op })
plan-ms-full-scan-unnamed = Full scan of a table ({ $op })
plan-ms-sort = Sort — the query sorts rows on the fly
plan-ms-spool = { $op } — an intermediate worktable in tempdb
# A warning SQL Server wrote on one operator; $warning is a plan-server-* line.
plan-ms-operator = { $op }: { $warning }
plan-server-no-join-predicate = no join predicate — every row pairs with every row
plan-server-no-statistics = a column it reads has no statistics
plan-server-spill = spilled to tempdb — its memory grant was too small
# $issue and $expression are the server's own text.
plan-server-convert = a type conversion affects the plan ({ $issue }): { $expression }
plan-server-unmatched-indexes = a filtered index could not be used
plan-server-memory-grant = its memory grant was misjudged
plan-server-wait = waited on { $wait }
# One this does not know, $name as SQL Server named it.
plan-server-other = the server warns: { $name }
# An index SQL Server says it would have used. $keys and $include are column
# lists joined with ", "; $impact is a whole percentage.
plan-missing-index = Missing index on `{ $table }` ({ $keys })
plan-missing-index-include = Missing index on `{ $table }` ({ $keys }; include { $include })
plan-missing-index-impact = Missing index on `{ $table }` ({ $keys }) — the server expected a { $impact }% improvement
plan-missing-index-include-impact = Missing index on `{ $table }` ({ $keys }; include { $include }) — the server expected a { $impact }% improvement
plan-ask-ai = Ask AI
# A SQL Server plan's table, which Schemaic lays out from the plan's XML (core's
# plan::PlanWording): its headers, then a heading row per planned statement
# where a batch plans more than one. $kind is SQL Server's statement type
# (SELECT, COND WITH QUERY); $name the function's or procedure's name.
plan-ms-operation = Operation
plan-ms-object = Object
plan-ms-estimated-rows = Estimated rows
plan-ms-estimated-cost = Estimated cost
plan-ms-actual-rows = Actual rows
plan-ms-executions = Executions
plan-ms-statement = Statement { $n }: { $kind }
plan-ms-statement-in-procedure = Statement { $n }: { $kind } in procedure { $name }
plan-ms-statement-in-function = Statement { $n }: { $kind } in function { $name }
plan-ms-statement-in-a-procedure = Statement { $n }: { $kind } in a procedure
plan-ms-statement-in-a-function = Statement { $n }: { $kind } in a function
# The one header of a table that holds no plan.
plan-ms-plan = Plan
plan-ms-no-plan = No statement in this batch has a plan.
# A plan document that could not be read; $reason is one of the
# plan-ms-unread-* messages, or the XML reader's own error in English.
plan-ms-unread = Could not read the plan: { $reason }
plan-ms-unread-not-xml = not an XML document
# $found is the document's root element.
plan-ms-unread-not-showplan = expected ShowPlanXML, found { $found }
plan-ms-unread-empty = an empty document

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
# $scope is what would be sent: one of the grid-attach-scope-* messages below.
grid-attach-to-chat = Attach { $scope } to chat
# What an attachment covers (core's AttachScope; English must read exactly as
# its Display). Whole rows, some columns of one row, or both counted.
grid-attach-scope-rows = { $rows ->
    [one] { $rows } row
   *[other] { $rows } rows
}
grid-attach-scope-columns = { $cols ->
    [one] { $cols } column
   *[other] { $cols } columns
}
grid-attach-scope-rows-and-columns = { $rows ->
    [one] { $rows } row
   *[other] { $rows } rows
} and { $cols ->
    [one] { $cols } column
   *[other] { $cols } columns
}
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

# The status line. $rows is one of the grid-rows-read-* messages below; $ms is
# the run time.
grid-stats = { $rows } · { $cols ->
    [one] { $cols } col
   *[other] { $cols } cols
} · { $ms } ms
# The same, led by the database the result came from.
grid-stats-in-database = { $database } · { $rows } · { $cols ->
    [one] { $cols } col
   *[other] { $cols } cols
} · { $ms } ms
# What was read (core's RowsRead; English must read exactly as its Display).
# $read and $total are figures as printed (200k, ~292.02k, 4,213,551); $n is
# the number the noun follows — the total's, where one is named. "capped": the
# read stopped at the row limit.
grid-rows-read = { $n ->
    [one] { $read } row
   *[other] { $read } rows
}
grid-rows-read-capped = { $n ->
    [one] { $read } row
   *[other] { $read } rows
} (capped)
grid-rows-read-of-total = { $n ->
    [one] { $read } of { $total } row
   *[other] { $read } of { $total } rows
}
# Beside the status line for a result kept (pinned) on its panel.
grid-kept-note = · kept — read-only
# $columns lists the columns whose values ran past what the grid holds.
grid-arena-note = · { $columns } too large to hold in full — later rows show blank
grid-sorted-subset = · sorted subset (capped) — not the full order
# The link that re-reads a capped result: the whole of it when the total is
# within one step, otherwise the next step. $rows is the step's figure,
# formatted (1m, 200k).
grid-read-all = read all rows
grid-read-more = read { $rows } rows
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
# The selection readout's figures. $n is the count, which selects the plural;
# $more is "+" when the scan stopped at its budget, else empty. $value is a
# figure, formatted (1,234.50). Length counts characters; $lo and $hi bound it.
grid-figure-rows = { $n ->
    [one] { $n }{ $more } row
   *[other] { $n }{ $more } rows
}
grid-figure-nulls = { $n }{ $more } null
grid-figure-distinct = { $n } distinct
grid-figure-sum = Sum { $value }
grid-figure-avg = Avg { $value }
grid-figure-median = Median { $value }
grid-figure-min = Min { $value }
grid-figure-max = Max { $value }
grid-figure-length = { $n ->
    [one] Length { $n } char
   *[other] Length { $n } chars
}
grid-figure-length-range = Length { $lo }–{ $hi } chars
grid-goto-label = Go to row:

## The results grid's clipboard, fill, attach and export (grid.rs).

# Where an export went, when it has no file name of its own to say.
export-dest-clipboard = the clipboard
export-dest-file = the file
# A finished export (core's ExportNote; English must read exactly as its
# Display). $rows is the count as written (16k), $n the number; $name the file,
# folder or export-dest-* it went to.
export-note-rows = { $n ->
    [one] Exported { $rows } row to { $name }
   *[other] Exported { $rows } rows to { $name }
}
# The rows line followed by what could not be carried: $caveats is one or more
# export-note-withheld/-blanked/-cut, joined by "; ".
export-note-with-caveats = { $head } — { $caveats }
# $cols lists the columns, joined by ", "; $n counts them.
export-note-withheld = { $n ->
    [one] { $cols } holds bytes this format cannot carry: those cells are empty
   *[other] { $cols } hold bytes this format cannot carry: those cells are empty
}
export-note-blanked = { $cols } too large to hold in full: later rows are blank
# $limit is Excel's cell limit in characters (32767).
export-note-cut = { $cols } cut to Excel's { $limit }-character cell limit
# A failed or cancelled export. $message already says it failed; $name is the
# file, untouched, and $part its .part sibling holding what was written.
export-failure-partial = { $message } — { $name } was not changed; the rows that were written are in { $part }
export-cancelled-partial = Export cancelled — { $name } was not changed; the rows that were written are in { $part }
export-cancelled-nothing = Export cancelled — { $name } was not changed.
# Copy as SQL with binary columns (core's SqlCopyNote). $cols lists them, $n
# counts them; NULL is SQL.
copy-sql-as-null = { $n ->
    [one] Copied to the clipboard — { $cols } holds bytes SQL text cannot carry: written as NULL
   *[other] Copied to the clipboard — { $cols } hold bytes SQL text cannot carry: written as NULL
}
copy-sql-nothing = { $n ->
    [one] Nothing copied — { $cols } holds bytes SQL text cannot carry
   *[other] Nothing copied — { $cols } hold bytes SQL text cannot carry
}
copy-sql-left-out = { $n ->
    [one] Copied to the clipboard — { $cols } holds bytes SQL text cannot carry: those values are left out
   *[other] Copied to the clipboard — { $cols } hold bytes SQL text cannot carry: those values are left out
}

grid-fill-nothing = Nothing to fill: the selected columns can't be written, or the cell to copy is a new row's unset value or a staged DEFAULT.
# What a paste skipped (core's edit::paste_report; a test holds the English
# equal). Each grid-paste-* part is one clause; $notes is them joined with
# ", ". $n, $cap and $cells are figures as the app writes them, $count the
# raw number of cells pasted.
grid-paste-outside = { $n } outside the grid
grid-paste-read-only = { $n } in read-only columns
grid-paste-capped = { $n } over the { $cap }-cell paste limit
grid-paste-deleted = { $n } in rows marked for deletion
grid-paste-nothing = Nothing pasted: { $notes }.
grid-paste-partial = { $count ->
        [one] Pasted { $cells } cell, skipping { $notes }.
       *[other] Pasted { $cells } cells, skipping { $notes }.
    }
# The row panel's refusals (core's rowjson::FieldRefusal); $column is a name.
grid-row-not-editable = Column `{ $column }` is not editable.
grid-row-not-nullable = Column `{ $column }` cannot be NULL.
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
# Copy as SQL UPDATE / WHERE IN's other refusals (core's edit::CopySqlRefusal).
# English must read exactly as core's (a test holds it).
grid-copy-sql-no-keyed-table = Nothing to copy as SQL: the rows need to come from one table with a key.
grid-copy-sql-key-too-large = Nothing to copy as SQL: the key was too large for this result to hold whole, so some rows cannot be named by it.
grid-copy-sql-pinned = Nothing to copy as SQL: this result is pinned, so its rows are a snapshot.
grid-copy-sql-read-only = Nothing to copy as SQL: this connection is read-only.
grid-copy-update-too-large = Nothing to copy as UPDATE: the selected columns were too large for this result to hold whole, and some of their cells show blank.
grid-copy-update-nothing-writable = Nothing to copy as UPDATE: none of the selected columns can be written.
grid-copy-where-in-no-key = Nothing to copy as WHERE IN: this table has no primary or unique key to name its rows by.
grid-copy-new-row-no-key = Nothing to copy: a new row has no key until it is saved.
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
# Under the filter field when the typed condition can't be used (core's
# filter::FilterError); a condition that doesn't parse shows the parser's own
# text instead. The ] is SQL Server's bracket.
grid-filter-bracket-in-name = a name containing `]` cannot be filtered here
grid-filter-trailing-input = unexpected trailing input after condition
# A filter, sort or page change asked for over staged edits (core's
# filter::STAGED_REQUERY_MSG). No full stop, as core's has none.
grid-staged-requery = Commit or discard your changes first — re-reading the rows would throw them away
# When a re-read lands over edits staged while it ran (filter::STAGED_LANDING_MSG).
grid-staged-landing = Edits were staged while the table was being re-read, so they were kept and the table was not refreshed — commit or discard them, then refresh.
# After a commit, when the filter calls a function Schemaic does not re-run
# on its own (filter::REREAD_WITHOUT_FILTER_MSG).
grid-reread-without-filter = The filter calls a function Schemaic does not re-run unasked, so after the commit the table was re-read without the filter and sort.
grid-hidden-columns-tip = { $count ->
    [one] { $count } hidden column — click to show all, or right-click a header to choose
   *[other] { $count } hidden columns — click to show all, or right-click a header to choose
}
grid-copied-staged-sql = Copied the staged changes as SQL.
# After a copy whose cells hold a tab or a line break, which the clipboard's
# tab-separated text cannot carry (core's edit::copy_split_note).
grid-copy-split = { $n ->
        [one] { $n } copied cell contains a tab or a line break — a paste will read it as more than one cell.
       *[other] { $n } copied cells contain a tab or a line break — a paste will read each of them as more than one cell.
    }
# The confirmation a large duplicate asks first: its title and question (core's
# edit::duplicate_prompt). $rows is the count as printed (25,431); $n the number
# the noun follows.
grid-duplicate-title = Duplicate rows
grid-duplicate-question = { $n ->
    [one] Duplicate { $rows } row? It is staged as a pending row and nothing is written until you commit.
   *[other] Duplicate { $rows } rows? They are staged as pending rows and nothing is written until you commit.
}
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
# The diff bar's summary (core's diff::inline_plan_summary; a test holds the
# English equal). $hunks selects the plural; $n, $removed and $added are
# figures. − and + are the removed and added line counts.
cmdk-no-changes = No changes suggested
cmdk-diff-summary = { $hunks ->
        [one] { $n } hunk · { $removed } − / { $added } +
       *[other] { $n } hunks · { $removed } − / { $added } +
    }

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
# What the run guard says (core's sql::GuardReason), on the guard bar and in
# the script panel's error line. SQL keywords stay as the statement spelt them.
guard-read-only = Read-only connection.
# $reason is one of the read-only-* messages below.
guard-read-only-refused = Read-only connection: { $reason }
# Why the read-only gate refused a statement (core's sql::ReadOnlyReason),
# after "Read-only connection:", so each starts in lower case. $heads is the
# statements the engine reads with, joined by "/" (SELECT/SHOW/…); $word,
# $name and $clause are SQL as the statement spelt it, in backticks.
read-only-several = only a single statement is allowed
read-only-not-a-read = only read-only queries ({ $heads }) are allowed
read-only-denied = `{ $word }` is not permitted in a read-only query
read-only-advances-sequence = `NEXT VALUE FOR` advances a sequence, which a read-only query does not
read-only-unlisted-call = `{ $name }` is not a function known to be read-only, so it is not permitted in a read-only query; functions from extensions or defined in the database are refused too
read-only-locks = `{ $clause }` locks the rows it reads, which a read-only query does not; drop the locking clause
guard-no-database = No database selected.
guard-writes-one = This statement modifies data.
guard-writes-several = These statements modify data.
# $file is the script's file name.
guard-script = Run every statement in { $file }? A script can create, alter and drop objects, and what it changes cannot be undone from here.
# Before a run that would replace a result holding $n staged edits; it follows
# the guard's other question when there is one.
guard-staged = { $n ->
    [one] This run replaces the result holding { $n } staged change — running would discard it. Commit or discard it first, or run anyway.
   *[other] This run replaces the result holding { $n } staged changes — running would discard them. Commit or discard them first, or run anyway.
}
# A statement that destroys stored rows (core's sql::Unsafe). $kind is DELETE
# or UPDATE; $obj is TYPE, DOMAIN or EXTENSION.
guard-truncate-partition-all = TRUNCATE PARTITION ALL removes every row in the table.
guard-truncate = TRUNCATE removes every row in the table.
guard-no-where = { $kind } statement without WHERE clause detected.
guard-drop-table = DROP TABLE deletes the table and every row in it.
guard-drop-database = DROP DATABASE deletes the database and every table in it.
guard-drop-schema = DROP SCHEMA deletes the schema and every table in it.
guard-drop-owned = DROP OWNED deletes everything the role owns, every table and its rows with it.
guard-drop-cascade = DROP { $obj } … CASCADE deletes every column that depends on it, with its values.
guard-replace-table = CREATE OR REPLACE TABLE deletes the existing table and every row in it.
guard-replace-database = CREATE OR REPLACE DATABASE deletes the existing database and every table in it.
guard-replace-schema = CREATE OR REPLACE SCHEMA deletes the existing schema and every table in it.
guard-truncate-partition = TRUNCATE PARTITION removes every row in the partitions it names.
guard-drop-partition = DROP PARTITION deletes the partition and every row in it.
# The hover's quick info on a name (core's quickinfo::QuickInfo; a test holds
# the English equal). $name, $database, $value, $expression, $arguments and
# $returns are the catalogue's own text; $count selects the plural and $n is
# it as a figure. A column's traits are joined by a middle dot.
quickinfo-table = table { $name } · { $database }
quickinfo-view = view { $name } · { $database }
quickinfo-columns = { $count ->
        [one] { $n } column
       *[other] { $n } columns
    }
quickinfo-primary-key = primary key
quickinfo-null = null
quickinfo-not-null = not null
quickinfo-auto-increment = auto increment
quickinfo-default = default { $value }
quickinfo-generated = generated as { $expression }
quickinfo-function = function { $name }({ $arguments })
quickinfo-function-returns = function { $name }({ $arguments }) returns { $returns }
quickinfo-procedure = procedure { $name }({ $arguments })
quickinfo-procedure-returns = procedure { $name }({ $arguments }) returns { $returns }
# The autocomplete popup's own words (core's rank::RowWords; a test holds the
# English equal): a foreign-key join row's detail, and the row that expands
# SELECT * into its columns. $count selects the plural; $n is it as a figure.
completion-foreign-key = foreign key
completion-expand-star = expand *
completion-columns = { $count ->
        [one] { $n } column
       *[other] { $n } columns
    }
# A squiggle's tooltip, under its message: the fix a key would make. $keys is
# the key (Alt+Enter); $fix is what it does.
editor-diag-fix = { $keys }: { $fix }
# What a squiggle says (core's intel::DiagMessage). Names in backticks and SQL
# keywords stay as the statement spelt them.
diag-syntax = Syntax error
# $detail is the SQL parser's own message, which stays English.
diag-syntax-detail = Syntax error: { $detail }
# A reserved word used as an alias, and how this engine quotes a name.
diag-reserved-alias-backticks = `{ $word }` is a reserved keyword and can't be used as an alias (quote it with backticks)
diag-reserved-alias-double-quotes = `{ $word }` is a reserved keyword and can't be used as an alias (quote it with double quotes)
diag-reserved-alias-brackets = `{ $word }` is a reserved keyword and can't be used as an alias (quote it with brackets)
# $within is "some" when the statement named the database ($database), and
# $suggested when there is a near name to offer ($suggestion).
diag-table-not-found = Table `{ $table }` not found{ $within ->
        [some] {" "}in `{ $database }`
       *[none] {""}
    }{ $suggested ->
        [some] {" "}— did you mean `{ $suggestion }`?
       *[none] {""}
    }
# $within is "some" when there is one table to name ($table).
diag-column-not-found = Column `{ $column }` not found{ $within ->
        [some] {" "}in `{ $table }`
       *[none] {""}
    }{ $suggested ->
        [some] {" "}— did you mean `{ $suggestion }`?
       *[none] {""}
    }
diag-aliased-as = `{ $name }` is aliased as `{ $alias }` here — qualify with `{ $alias }`
diag-ambiguous-column = Column `{ $column }` is ambiguous
diag-join-without-condition = JOIN has no ON/USING condition — this is a cross join; use CROSS JOIN if that's intended
diag-comma-join-without-where = Comma join with no WHERE — this is a cross join; add a WHERE, or use CROSS JOIN if that's intended
diag-not-grouped = Column `{ $column }` must appear in GROUP BY or be used in an aggregate
diag-insert-row-width = INSERT names { $columns } { $columns ->
        [one] column
       *[other] columns
    } but this row has { $values } { $values ->
        [one] value
       *[other] values
    }
# A row for a table without a column list, where the table takes $needed
# values (fewer than it has).
diag-row-width-without-list = `{ $table }` takes { $needed } { $needed ->
        [one] value
       *[other] values
    } without a column list but this row has { $values }
diag-row-width-of-table = `{ $table }` has { $columns } { $columns ->
        [one] column
       *[other] columns
    } but this row has { $values } { $values ->
        [one] value
       *[other] values
    }
diag-nullable-not-in = `{ $column }` can be NULL, and `NOT IN` matches no row once the subquery returns one — add `WHERE { $column } IS NOT NULL` to it, or use NOT EXISTS
diag-update-without-where = UPDATE without WHERE changes every row in the table
diag-delete-without-where = DELETE without WHERE removes every row in the table
# $fix is IS NULL or IS NOT NULL.
diag-null-comparison = A comparison with NULL is never true — use { $fix }
diag-misspelled-keyword = `{ $word }` looks like a misspelled keyword
diag-misspelled-function = `{ $word }` looks like a misspelled function
# A T-SQL batch's control flow (core's intel::FlowError).
diag-flow-merge-needs-semicolon = `MERGE` must end with `;`
diag-flow-if-without-statement = `IF` has no statement to run
diag-flow-else-without-statement = `ELSE` has no statement to run
diag-flow-while-without-statement = `WHILE` has no statement to run
diag-flow-end-try-without-begin-try = `END TRY` without a matching `BEGIN TRY`
diag-flow-end-catch-without-begin-catch = `END CATCH` without a matching `BEGIN CATCH`
diag-flow-end-without-begin = `END` without a matching `BEGIN`
diag-flow-throw-needs-semicolon = The statement before `THROW` must end with `;`
diag-flow-end-try-without-catch = `END TRY` must be followed by `BEGIN CATCH`
diag-flow-if-without-statement-before-else = `IF` has no statement before its `ELSE`
diag-flow-else-without-if = `ELSE` without an `IF` before it
diag-flow-begin-catch-without-end-try = `BEGIN CATCH` must follow an `END TRY`
diag-flow-begin-try-without-end-try = `BEGIN TRY` has no matching `END TRY`
diag-flow-begin-catch-without-end-catch = `BEGIN CATCH` has no matching `END CATCH`
diag-flow-begin-without-end = `BEGIN` has no matching `END`

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
# Why a regular expression does not compile (core's text_ops::PatternError);
# $why is the regex library's own one-line reason.
find-bad-pattern = Not a valid pattern
find-bad-pattern-why = Not a valid pattern: { $why }
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

## Why a client or tool is not handed what it was about to be (core's
## launch::Refusal). Program names (psql, sqlcmd, sqlite3, mysql, docker ps,
## cmd.exe), the quoted characters and 127.0.0.1 are never translated.

# The connection import's Docker search and the database CLI both say it.
cli-refused-shim = The client found on your PATH is a .cmd/.bat shim, which Windows runs through cmd.exe — where a database name carrying '&' or '|' would be a second command. Point Settings at the real executable, or open a query tab instead.
cli-refused-name-equals = This database's name contains '=', which psql would read as a connection string and follow to another server. Open it in a query tab instead.
cli-refused-name-uri = This database's name begins like a connection URI, which psql would follow to another server. Open it in a query tab instead.
# ./name.db is an example path.
cli-refused-file-dash = This database file's name begins with '-', which the sqlite3 client would read as an option rather than as the file. Rename it, or give it a path (./name.db), and open it again.
cli-refused-sqlcmd-entra = sqlcmd cannot be handed the Azure CLI's sign-in token, so this connection cannot open in it. Open it in a query tab instead.
cli-refused-sqlcmd-host = This connection's host contains a character sqlcmd reads as part of the address (',', ';', a second '\' or a ':' prefix). Set the port in its own field, and open it again.
cli-refused-sqlcmd-ca = sqlcmd cannot verify against a CA file. Add the CA to the system's trust store and clear the connection's CA field, or open the connection in a query tab.
cli-refused-sqlcmd-client-cert = SQL Server does not sign in with a client certificate, so sqlcmd has nowhere to send this connection's. Clear the client certificate and key, and open it again.
cli-refused-wsl-paths = This connection's TLS certificate paths are Windows paths, which the WSL client cannot open. Install a native client, or move the certificates inside WSL.
cli-refused-tunnelled-verify = This connection verifies the server's certificate and reaches it through an SSH tunnel, so the client would be told to dial 127.0.0.1 and check the certificate against that — which the server's own certificate cannot satisfy. Open a query tab instead, or lower the TLS mode for the CLI session.
# The connection import's Docker search.
cli-refused-not-container-id = docker ps printed something that is not a container id.
# Back up and Restore from file.
cli-refused-file-line-break = That file name contains a line break, which the client would read as two arguments. Choose another name.
cli-refused-role = The role in this connection's init SQL cannot be handed to the tool as it is.
cli-refused-mysql-source-path = The mysql client would read that file's path as commands of its own (a ';' or a '\' in it, or a space at an end). Move or rename it, then restore it.
cli-refused-cloud-token = This connection signs in through a cloud CLI's token, which an external client is not handed. Open it in a query tab instead.

## Settings › Install's failures (install_cli.rs). $error is the operating
## system's own text; $path a file path.

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

## Settings › Install and Remove: why not, what was done, and the row's hint
## (core's cli_install Refusal, Report and RowHint; English must read exactly
## as their Display, which a test holds). schemaic is the command typed, PATH
## the variable's name, schemaic.com a file name: none is translated. $dir is a
## folder, $exe the running program, $link the link's path, $to and $target
## what a link points at, $path the command's own path.

install-refuse-no-folder = Can't tell which folder { $exe } is in.
install-refuse-no-shim = There is no schemaic.com beside this copy of Schemaic, in { $dir }, so there is no console command to put on PATH. The installed app has one; a development build doesn't.
install-refuse-semicolon = This copy of Schemaic is in { $dir }, whose name holds a ';' — PATH would read it as two different folders. Move it somewhere without one and try again.
install-refuse-exe-replaced = { $exe } was replaced while Schemaic was running, so a link to it would point at nothing. Restart Schemaic, then Install again.
install-refuse-no-home = There is no home directory to hold the link, so there is nowhere to look.
install-refuse-path-not-text = Your user PATH isn't stored as text in the registry, so it was left alone.
install-refuse-path-not-unicode = Your user PATH contains characters that can't be read back safely, so it was left alone.
install-refuse-portable = This is a portable copy, so Remove can't tell whether Install put { $dir } on your PATH or it was there already with other programs in it. Take it out under Environment Variables if Install added it.
install-refuse-spot-taken = { $link } already exists and isn't a link, so Install left it alone. Move it aside and try again.
install-refuse-points-elsewhere = { $link } already points at { $to }. Remove it if that's an old copy of Schemaic, then try again.
install-refuse-unlink-elsewhere = { $link } points at { $to }, not at this copy of Schemaic, so Remove left it alone.
install-refuse-unlink-not-ours = { $link } isn't a link Schemaic made, so Remove left it alone.
install-report-already = The schemaic command is already on your PATH, in { $dir }.
install-report-added = Added { $dir } to your user PATH. Open a new terminal to use schemaic — one that is already open keeps the old PATH.
install-report-linked = Linked { $link } to { $target }.
# macOS, where the app cannot tell whether the folder is on a terminal's PATH.
install-report-linked-if-not-found = Linked { $link } to { $target }. If a new terminal can't find schemaic, add { $dir } to your PATH in your shell profile.
install-report-linked-not-on-path = Linked { $link } to { $target }. { $dir } isn't on your PATH yet — add it in your shell profile, then open a new terminal.
install-report-removed = Removed { $dir } from your user PATH. A terminal that is already open keeps the old PATH.
install-report-path-absent = { $dir } isn't on your user PATH, so there was nothing to remove.
install-report-unlinked = Removed { $link }.
install-report-link-absent = There is no { $link }, so there was nothing to remove.
install-report-package = The schemaic command in { $dir } came with the installed package, not from Install — remove the package to remove it.
install-hint-adds-folder = Run Schemaic from a terminal by name. Adds its folder to your user PATH.
install-hint-links = Run Schemaic from a terminal by name. Links it into ~/.local/bin.
install-hint-folder-on-path = Run Schemaic from a terminal by name. Its folder, { $dir }, is already on your user PATH.
install-hint-comes-with-app = Run Schemaic from a terminal by name. The schemaic command comes with the app, at { $path }.

## The connection gate's refusals (main.rs) — what a run, a query plan or a
## grid commit says when the check that the server is reachable stops it.

# An operation on an SSH connection that finds no tunnel (core's
# TunnelFailures::refusal). $error is the SSH library's or the server's own
# text, with no closing full stop.
tunnel-still-opening = SSH tunnel is not established yet
tunnel-could-not-open = The SSH tunnel could not be opened: { $error }. Refresh the schema to try again.
tunnel-server-failed = Could not connect through the SSH tunnel: { $error }. Refresh the schema to try again.

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
# Why a restore was refused (core's RestoreRefusal; English must read exactly
# as its Display). $switches lists up to three restore-switch(-unnamed)
# entries, joined by ", "; $more counts the rest. $path is the file, $database
# the database the restore was pointed at.
restore-refused = { $path } moves to another database as it runs — { $switches } — so restored into { $database } it would write there instead. Nothing was run. Restore it into the database it names, or remove those lines from a copy of the file.
restore-refused-more = { $path } moves to another database as it runs — { $switches }, and { $more } more — so restored into { $database } it would write there instead. Nothing was run. Restore it into the database it names, or remove those lines from a copy of the file.
# One line that moves the restore. $how is what does it, as SQL writes it
# (USE, \connect, CREATE DATABASE): never translated. $name is the database.
restore-switch = { $how } `{ $name }` on line { $line }
restore-switch-unnamed = a { $how } on line { $line } that names no database Schemaic can read

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
## (main.rs), and the dump writers' (core's ExportFailure, worded in
## dump_view.rs; English must read exactly as its Display). $error is the
## operating system's, the engine's or the writer's own text.

export-already-running = An export is already running. Cancel it or wait for it to finish.
export-failed = Export failed: { $error }
# The background task writing the file stopped without an answer.
export-worker-died = Export failed: worker died: { $error }
# A folder export's table file could not be renamed into place; $path is it.
export-not-published = Export failed: { $path } could not be published: { $error }
# A dump's finished .part file ($part) could not be renamed over $path.
export-not-renamed = The export wrote { $part } but could not rename it to { $path }: { $error }
export-nothing-matched = Nothing to export — no table matched the selection.
# Stopped by the user just before the file was put in place.
export-stopped = Export cancelled.
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

## Startup notices (core's persist::Notice), shown in the error modal when the
## app starts or a save answers. Each message is one line; a notice shows its
## lines one under the other. ** marks bold, * italic and ` code.

# A config file that did not parse. $name is the file's name; $error is the
# parser's own text and stays as written.
notice-unreadable = { $name } could not be read ({ $error }).
notice-unreadable-kept = The unreadable file was kept as { $name }.corrupt; Schemaic fell back to its backup, or to defaults if the backup was unreadable too.
# Added for connections.json alone.
notice-unreadable-swept = That copy can hold passwords in the clear, so it is removed the next time you delete a connection — recover anything you need from it before then.
# A config file that was gone and came back off a sibling copy; $from is that
# copy's suffix (.bak, .tmp), so { $name }{ $from } is its file name.
notice-missing = { $name } was missing.
notice-missing-restored = Schemaic recovered it from { $name }{ $from }, which was still there. It will be written back the next time this setting changes — until then that sibling is the only copy.
# The keyring would not answer at startup; $n secrets are unreadable, and
# $error is the keyring's own message.
notice-keyring-unread = { $n ->
    [one] Schemaic could not read { $n } saved secret from the OS keyring.
   *[other] Schemaic could not read { $n } saved secrets from the OS keyring.
}
notice-keyring-unread-why = { $n ->
    [one] Schemaic could not read { $n } saved secret from the OS keyring ({ $error }).
   *[other] Schemaic could not read { $n } saved secrets from the OS keyring ({ $error }).
}
notice-keyring-unread-kept = Those passwords are unavailable this session and were **not** deleted — leave the fields blank and they will come back once the keyring is reachable. Typing a new one saves over the stored secret.
# A save the keyring would not take: $n passwords written to the file instead.
notice-in-the-clear = { $n ->
    [one] Schemaic could not reach the OS keyring, so { $n } password was saved **in plain text** in `connections.json`.
   *[other] Schemaic could not reach the OS keyring, so { $n } passwords were saved **in plain text** in `connections.json`.
}
notice-in-the-clear-where = That file is in the folder Settings → General's *Open folder* button opens, beside the log — check before sharing it. Saving again once the keyring is reachable moves them back in.
# A save that could not delete $n passwords the user cleared.
notice-undeleted = { $n ->
    [one] { $n } cleared password is still in the OS keyring — Schemaic could not delete it.
   *[other] { $n } cleared passwords are still in the OS keyring — Schemaic could not delete them.
}
notice-undeleted-after = The field is blank on disk, but the next launch will fill it back in. Save again once the keyring is reachable.

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

## What a failed or cancelled run cost its transaction (core's tx::TxNote),
## shown after the error and a blank line. MySQL, SQL Server, COMMIT, DDL,
## MyISAM and MEMORY are names and keywords, never translated; Rollback is
## the button. $stmts counts the statements run in the transaction.

# In place of an error, for a cancelled run (tx::CANCELLED).
tx-cancelled = The statement was cancelled.
tx-committed-before-ddl = The transaction this ran in is gone: MySQL commits before it runs a DDL statement, so the statements already in it are permanent and Rollback will not undo them.
tx-rolled-back-by-server = The transaction this ran in is gone: SQL Server rolled it back, so every statement in it since it began is undone.
tx-doomed = The transaction this ran in can no longer commit: SQL Server marked it uncommittable, so Rollback is the only way on, and it undoes every statement in it.
tx-after-own-commit = The transaction this ran in has ended, and this ran a COMMIT of its own before it failed, so what came before that COMMIT is likely committed rather than undone. Check the data before running any of it again.
tx-lost-on-commit = The connection was lost with a transaction open. It was lost on a COMMIT, so whether { $stmts ->
        [0] the transaction was
        [one] the { $stmts } statement run in it since it began was
       *[other] the { $stmts } statements run in it since it began were
    } committed or rolled back is unknown: the server may have committed before the connection dropped. Check the data before running any of it again.
tx-lost-on-implicit-commit = The connection was lost with a transaction open. MySQL commits an open transaction before it runs this statement, so { $stmts ->
        [0] nothing had run in it yet, so there was nothing to commit
        [one] the { $stmts } statement run in it since it began was most likely committed then, and is permanent
       *[other] the { $stmts } statements run in it since it began were most likely committed then, and are permanent
    }. Check the data before running any of it again.
tx-lost-rolled-back = The connection was lost with a transaction open. The server rolls an unfinished transaction back when its connection ends, so { $stmts ->
        [0] nothing had run in it yet
        [one] the { $stmts } statement run in it since it began is undone
       *[other] the { $stmts } statements run in it since it began are undone
    }.
# As above on MySQL, where a write to a non-transactional table stays.
tx-lost-rolled-back-mysql = The connection was lost with a transaction open. The server rolls an unfinished transaction back when its connection ends, so { $stmts ->
        [0] nothing had run in it yet
        [one] the { $stmts } statement run in it since it began is undone — except a write to a table that does not support transactions (MyISAM, MEMORY), which no rollback undoes
       *[other] the { $stmts } statements run in it since it began are undone — except a write to a table that does not support transactions (MyISAM, MEMORY), which no rollback undoes
    }.
# A run whose statement never left the client (tx::not_sent_message): the
# statement timeout fired, or the user pressed Stop, while it waited.
tx-not-sent-timeout = The statement timeout fired while this run was still waiting for the tab's connection. An earlier operation on it — a commit, or the re-fetch after one — had not finished, so the statement was never sent to the server. Nothing ran, and the transaction is exactly where it was.
tx-not-sent-cancelled = This run was cancelled while it was still waiting for the tab's connection. An earlier operation on it — a commit, or the re-fetch after one — had not finished, so the statement was never sent to the server. Nothing ran, and the transaction is exactly where it was.
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
# $timeout is the setting's value as its dropdown shows it (15 minutes) — one
# of timeout-hours/-minutes/-seconds, never timeout-off.
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

## Why an agent CLI was not started or failed (schemaic-ai's SpawnRefusal and
## CliFailure), and what its own tools are kept to (Constraint::notice).
## English must read exactly as schemaic-ai's (a test holds it). $total and
## $limit are figures; $unit is one of ai-unit-*; $levers one of
## ai-levers-*; $code an exit status; $mcp is "isolated" when the CLI keeps
## the user's own MCP servers out of the session, else "shared".

ai-oversize = The context sent to { $cli } is too large for one command line ({ $total } { $unit }; this platform allows about { $limit }). { $levers }
ai-unit-characters = characters
ai-unit-bytes = bytes
ai-levers-schema-and-query = Narrow Settings → AI → Schema context to the active database (or None), or shorten the query in the editor.
ai-levers-sample = Lower this connection's AI data access below "Let it read data", which stops the row sample being sent, or narrow Settings → AI → Schema context.
ai-batch-shim = { $cli } resolved to a batch shim ({ $bin }), which cannot be given a multi-line prompt — Windows runs a .cmd through cmd.exe, and it refuses the argument before { $cli } is started. Set Settings → AI → CLI path to the real executable (an npm install usually has one under node_modules, beside the shim).
ai-cli-exited = the { $cli } CLI exited with status { $code }
ai-cli-signalled = the { $cli } CLI was terminated by a signal
ai-constraint-sandboxed = { $cli } runs sandboxed: its own built-in tools cannot write files or run commands, though they can still read this machine.{ $mcp ->
        [isolated] {""}
       *[shared] {" "}Any MCP servers you have registered with it are also available to this session, and Schemaic cannot restrict them.
    }
ai-constraint-workspace-rules = { $cli } is kept to answering by a permissions file Schemaic writes for this session, not by a sandbox: its own tools are refused writing files and running commands, but they can still read files on this machine.{ $mcp ->
        [isolated] {""}
       *[shared] {" "}MCP servers you have registered with it also start with this session; their tools are refused unless your own settings allow them.
    }
ai-constraint-denylist = This { $cli } build does not accept the flag that empties its built-in tools, so they are held back by a denylist instead — weaker, and not something Schemaic can guarantee. Updating the CLI restores the full seal.
ai-constraint-unrestricted = Schemaic could not fully restrict { $cli }'s built-in tools for this session, so it may be able to do more than answer questions about this database.
ai-constraint-unknown = Schemaic could not confirm that this { $cli } binary accepts the flags that restrict it, so the assistant is disabled. Check the path in Settings → AI.
# Seed rows' reply it could not use (core's seed::SeedError); $error is the
# JSON parser's own text.
ai-seed-empty = the model returned no content
ai-seed-not-json = the reply was not valid JSON: { $error }
ai-seed-not-rows = the reply was not a JSON array of row objects

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
