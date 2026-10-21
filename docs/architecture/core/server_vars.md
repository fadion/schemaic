# `core/server_vars.rs`

`server_vars.rs` — the **Server variables** viewer's model: a server's settings as the engine
reports them, over an already-fetched list. No DB, no UI — `activity.rs`'s and `users.rs`'s
shape again, the fetch being one query per engine in `schemaic-db` (`Db::fetch_server_vars`,
below) because each engine publishes its settings somewhere else, while what every answer has
in common is a `ServerVar` — `name`, `value` (PostgreSQL's with its unit appended) and an
optional `detail` for the engines that describe a setting (PostgreSQL's `short_desc`, SQL
Server's `description`; MySQL publishes none). `VarKind` is `Variables` — the default, what the
server was configured to do — or `Status`, its counters since start.
**Both capabilities are exhaustive `match`es, and the first answers `true` for all four
engines.** `supports_server_vars` includes SQLite, though it is a library and not a server:
what it has instead of server variables is its PRAGMAs, and those are the settings a user of a
SQLite file asks after. It is asked anyway — by both menus, by the app's fetch and by
`Db::fetch_server_vars` — so a fifth engine has to answer it rather than inherit a menu row. `supports_status` is MySQL's
alone — PostgreSQL spreads its counters over a dozen `pg_stat_*` views with no single
name/value shape, SQL Server over its DMVs, and SQLite has no server to count — so the viewer's
*Variables | Status* switch is offered there and nowhere else, rather than opening onto an
empty list. **`SQLITE_PRAGMAS` is a fixed list, and every entry is read without an
assignment** — `PRAGMA journal_mode`, never `PRAGMA journal_mode = …`, since a PRAGMA given a
value *changes* the setting. They are named here rather than discovered because `pragma_list`
names every PRAGMA, the ones that act (`optimize`, `integrity_check`, `wal_checkpoint`)
included, so a viewer reading that list would be a way to run them;
`the_sqlite_pragmas_are_bare_names_and_unique` pins that each is a bare lower-case name, with
no `=` or `(` to be an assignment. `var_matches` is the search: an ASCII case-insensitive
substring over name, value *and* detail, so *utf8* finds every setting whose value is a UTF-8 character
set and *parallel* finds SQL Server's settings by their description; an empty query matches
everything. `render_slice` is `activity::render_slice`'s rule — the first `RENDER_CAP` (300)
matches as references, plus a *count* of the rest rather than a silent cut — because a MariaDB
server reports some six hundred variables and the list is rebuilt on every keystroke of the
search.
