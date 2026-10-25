# SQL files

A query tab can be tied to a `.sql` file on disk, so you can keep queries and migrations with the
rest of your project. A file far too big to edit — a dump, say — is run as a script instead.

## Opening and saving

| You want to…                  | Do this                                                     |
|-------------------------------|-------------------------------------------------------------|
| Open a file in a tab          | <kbd>Ctrl</kbd>+<kbd>O</kbd>, or right-click a tab › **Open file** |
| Save the tab                  | <kbd>Ctrl</kbd>+<kbd>S</kbd>, or **Save**                   |
| Save under another name       | <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>S</kbd>, or **Save as** |
| Throw away your edits         | Right-click the tab › **Reload from disk**                  |

A file opens in a tab of its own, on the active connection. If that file is already open there,
Schemaic switches to its tab rather than opening it twice. Saving a tab that has no file asks for a
name, as **Save as** does.

A file tab shows a small file icon before its title, and its title turns *italic* when it has edits
you haven't saved. Hover the tab to see the file's full path. Closing a tab with unsaved edits
asks first; the file on disk is left as it was.

*When the file changes elsewhere.* If another program changes the file while it's open, a bar
above the editor says so when you come back to Schemaic: **Reload** takes the version on disk,
**Keep mine** keeps what's in the tab. Schemaic won't save over a file that has changed since it
was opened — reload it, or **Save as** under another name.

*Large files.* Schemaic checks the whole tab as you type, so a big file makes the editor slow.
A file over 8 MB asks before it opens; one over 64 MB won't open in a tab at all. Run that one as a
script instead.

## Running a whole script

To run a `.sql` file — a dump, a migration, a seed script — without opening it, right-click a
database in the schema tree and choose **Import**. Schemaic summarises what the file does before
anything runs, then runs it with progress and a way to stop. See
[Running a SQL script](../import-export/run-script.md).
