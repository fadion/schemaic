# Schemaic guide

Schemaic is a native SQL editor for MySQL, MariaDB, PostgreSQL, SQLite and SQL Server. This guide
covers the parts where people most often need a hand: finding your way round a database, changing
its structure, and the tools built on top of that. It is not a reference to every control — the
interface is meant to explain itself, and the guide picks up where it doesn't.

## How to read it

- Bold text is always an interface label, spelled exactly as it appears in the app — a menu
  entry, a button, a tab — such as **Edit table**. If you can't find one, you may be on an engine
  that doesn't offer it; each page says where that's the case.
- Keys are written as <kbd>Ctrl</kbd>+<kbd>P</kbd>.
- In the schema tree, *right-click* also means the keyboard's menu key, or
  <kbd>Shift</kbd>+<kbd>F10</kbd>, on the selected row.

## Engines are not all equal

MySQL/MariaDB, PostgreSQL and SQL Server are fully supported. SQLite reads, writes, imports and
edits tables, views and triggers, but has no manual-transaction mode and no users to manage. Where
an engine can't do something, Schemaic leaves the entry out rather than showing it greyed out — a
greyed-out entry usually means the connection is marked read-only.

Found something wrong or missing? Every page has an edit link at the top that opens its source on
GitHub.
