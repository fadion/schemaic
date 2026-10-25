# History and snippets

**Query history** is a record of what you've run. The **Snippet library** is the SQL you've chosen
to keep. Both open as panels from the buttons at the top right of the window, and both are searched
by **Find anywhere** (<kbd>Ctrl</kbd>+<kbd>P</kbd>) too.

## Query history

Every statement you run is recorded with the connection and database it ran on, how long it took,
and whether it failed. The panel groups them under **TODAY**, **THIS WEEK** and **EARLIER**, newest
first, with a search box above.

- *Click* a row to open it in a new tab. A row from another connection switches to that connection
  first.
- *Right-click* a row for **Open in new tab** and **Delete**.
- **Current** shows this connection's history; **All** shows every connection's, each row headed
  with where it ran.
- The bin button clears the history of the *current* connection only, after asking — even while
  **All** is showing.

Schemaic keeps the newest 200 statements per connection. A statement that carries a password —
`CREATE USER … IDENTIFIED BY`, `ALTER LOGIN … PASSWORD` and the like — is never recorded.

## Snippets

A snippet is a named piece of SQL. Click one in the library to insert it at the caret, replacing the
selection if there is one; **Open in new tab** on its right-click menu puts it in a tab of its own.

*Saving one.* Write the SQL in the editor and press the library's save button. The new snippet
appears at the top of the list with its name ready to type.

*Where it shows.* A new snippet belongs to the connection you saved it on. Right-click it ›
**Show in** to widen it to **This connection**, every connection of the same engine (for example
**Every PostgreSQL connection**), or **All connections**. The library groups snippets under the same
three headings, narrowest first.

*Editing one.* Right-click › **Edit** opens a dialog for the **Name**, the **Query**, and an
**Expansion shortcut**: a short word that, typed in the editor, offers the snippet in the completion
list. Accept it and the word is replaced by the snippet's SQL.

*Placeholders.* Write `:name` in a snippet where a value belongs. After inserting it, the first
`:name` is selected so you can type over it, and <kbd>Tab</kbd> moves to the next. A `:name` left in
when you run becomes a parameter (see [Query editor](README.md#running-sql)).

*Built-in snippets* ship with Schemaic for each engine — common checks like running queries or
table sizes — marked **Built-in**. They can't be edited or deleted; **Duplicate** one to get a copy
you can change.

Snippets are saved on this machine. Settings › **General** › **Settings file** exports them with
your preferences, except snippets that belong to a single connection.
