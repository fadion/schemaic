# `db/duckdb.rs`

**`duckdb.rs` is the fifth engine** (the `duckdb` crate, the engine compiled in — `bundled`,
with the `parquet` and `json` extensions linked statically), phase 1 of the roadmap's *DuckDB as
a fifth engine*: it connects, reads, introspects, validates, runs batches, scripts and DDL plans,
plans a query, lists its settings and backs a file up. It is **embedded, as SQLite is**, so most
of `sqlite.rs`'s first paragraph holds here unchanged — there is no server, a connection is a
file (`Db::file`, `connection::is_file_backed`), host/port/user/password/SSH are inert, the one
database is the file's own catalog (`current_database()`, named after the file), and the driver
is blocking, so every call runs on Tokio's blocking pool. What makes it more than a second SQLite
is four things, and the module doc names the same four.
**One database instance per file, per process.** DuckDB locks a file per *instance*, and a
second instance on the same file in the same process is refused outright — measured on 2.0, *The
process cannot access the file because it is being used by another process*, naming this
process. So SQLite's "open the file per operation" would have two overlapping operations — a
running query and the health check, a stream and a cell's blob — refuse each other. Instead
every operation takes **its own connection** (`duckdb::Connection::try_clone`) off a **shared
instance** (`instance`), and the registry, `INSTANCES`, holds a `Weak<Instance>` keyed by the
file's **canonical** path — so two spellings of one file share the instance rather than the
second being refused the lock — and the instance closes, releasing the file to the DuckDB CLI or
any other program, the moment the last operation holding it ends. That is this engine's
exception to the one-connection-per-operation invariant, and it is narrower than it sounds:
connections are still per operation, and only the database underneath them is shared while they
overlap (`overlapping_operations_share_one_instance`). **It is why the init SQL refuses `SET
GLOBAL` here** (`init_sql`'s `DUCKDB_SETTINGS` and its `duckdb` arm): a `GLOBAL` setting is the
instance's, and the instance belongs to whatever else is open on the file at that moment. The init
SQL itself runs on each operation's cloned connection, in `connect`.
**Sealed instances, for the paths nobody watches.** A DuckDB query can read any file on this
machine — `read_csv`, `read_parquet`, and `FROM 'secrets.csv'`, a string in table position that
no text gate can name. `Db::sealed()` sets `Db::sealed`, and a sealed handle's instance opens
with external access off and the configuration locked (`config`), and on a file read-only, so
neither a statement nor its functions reach a file the database does not hold, and no `SET` turns
that back on (`a_sealed_instance_reads_no_file_and_refuses_the_other_kind`: `read_csv` of a real
path fails *disabled by configuration*, and `SET enable_external_access = true` fails). The text
gate (`sql::read_only_reason`) names the readers first; the seal is what holds when a spelling
slips past it. **An instance is sealed or not for its whole life**, so a handle asking for the
other kind while one is open is refused — `DbRefusal::DuckDbSealedElsewhere`, which does not
`stand`, since the file is free again once the other run ends — rather than
quietly given the wrong one. The test's in-memory instance is sealed without the read-only open,
there being no file to open read-only, so that half of the seal has no test. **Who seals:**
`cli::query`'s `read_only_query`, `read_only_export` and `read_only_explain` each call
`db.clone().sealed()` before they run (`schemaic query`/`export`/`explain`), and the MCP server
seals every handle it serves in `mcp::serve`/`serve_many` — none of its tools writes, and
`sample_rows` and `describe_table`'s samples read through views a body may have aimed at a file.
The Live Monitor's `fetch_table` is the editor's, attended, and unsealed. No test reaches a
DuckDB file through `cli` (the `memory:` targets are `schemaic-db`'s `cfg(test)` alone), and no
source gate reads the `.sealed()` calls.
**Nothing is fetched from the network.** Every instance — sealed or not — opens with extension
auto-install and auto-load off, so `parquet` and `json` are there and any other extension is
unreachable rather than downloaded behind a query (`no_extension_is_fetched_behind_a_query`).
`INSTALL`/`LOAD` are on the read gate's deny lists besides.
**Values are typed by column, and rendered as DuckDB writes them** (`cell_of`, `text_of`,
`type_name`): `true`/`false`, integers as numbers (a `HUGEINT` past `i64` as its digits), a
`FLOAT` through its own shortest text so `0.1::FLOAT` reads `0.1`, a `DECIMAL` exact as text, a
timestamp in ISO form with `TIMESTAMP WITH TIME ZONE` in UTC (`+00`, this build linking no ICU),
an interval as `1 year 2 months 3 days 04:05:06`, a list `[1, 2]`, a struct `{'a': 1}`, a blob
as its size and its column marked binary. A column's type name keeps a declared alias (`JSON`).
**Two decode hazards are walled off, both in the driver** (duckdb 1.20000.1): a `NULL`-typed
column (`SELECT NULL`) is never asked of it, since decoding one panics, and every other cell is
read under `catch_unwind`, so a value it cannot decode costs that cell — a `<TYPE>` placeholder —
rather than the worker and the whole result (`values_read_as_duckdb_writes_them`). **A write's
row count arrives as a result**: DuckDB answers `INSERT`/`UPDATE`/`DELETE` without `RETURNING`,
and `CREATE TABLE … AS`, with a one-row `Count` column, which `counts_rows` recognises and turns
into `ResultSet::affected` so the grid does not show it as data
(`a_write_reports_its_count_not_a_count_column`).
**`Enforce` is a read-only transaction**: anything that `refuses_writes` — `ReadOnly` and
`Generated` alike — runs inside `BEGIN TRANSACTION READ ONLY`, where DuckDB refuses a write by its
effect (*Cannot write to database … transaction is launched in read-only mode*), and is rolled
back after, on the same cloned connection so the next operation is not left inside it
(`a_read_only_run_refuses_a_write_by_its_effect`). That holds the session half of the read-only
invariant; it does not stop a file read, which is the seal's job. `run_batch` and `fetch_query`
take the connection's interrupt handle before any work, so Stop reaches a running statement, as
on SQLite. `prepare_check` — the live validation, run as the user types — does not prepare a
statement whose head acts at prepare time or changes the session or the instance (`SET`, `PRAGMA`,
`ATTACH`, `INSTALL`/`LOAD`, `USE`, `CHECKPOINT`, `COPY`, the transaction words and the rest of its
list), the reason SQLite's skips a `PRAGMA`; those go unchecked and their run reports them.
**Introspection is DuckDB's own catalog functions** — `duckdb_schemas/tables/views/columns/
constraints/indexes/sequences/functions()`, each scoped to `current_database()` and, where the
function has the column, `NOT internal` — so an `ATTACH`ed database, which lives only as long as
the connection that ran it, is never part of the tree. **Each object's stored `CREATE` text is
kept** (`TableInfo::create_sql`, `IndexInfo::create_sql`), so *Copy DDL* is the engine's own words
rather than a reconstruction; a view's definition is the text after the stored statement's first
top-level `AS` (`view_body`, over `sql::skip_noncode`). The primary key marks its columns and is
the `PRIMARY` index the model reads keys from, a `UNIQUE` constraint is a unique index of its
name, checks and foreign keys are themselves, and a column defaulting to `nextval(…)` is read as
auto-numbered. **Macros are DuckDB's user functions and arrive as `RoutineInfo`** — one per name,
an overload being the same macro to `DROP MACRO`, a table macro's `returns` reading `TABLE`.
`main` is always among the namespaces (`schema::DUCKDB_DEFAULT_SCHEMA`, `default_namespace`), and
names compare ignoring case (`names_ignore_case`). Table sizes are `duckdb_tables()`'
`estimated_size` rows and nothing on disk, DuckDB keeping no per-table figure; the Server
variables list is `version()` and then `duckdb_settings()` with its descriptions.
(`the_schema_reads_tables_views_keys_and_macros`.) Sessions, locks and accounts are refused in
the dispatcher's DuckDB arms (`NO_SESSIONS_MSG`, `NO_USERS_MSG`) and statement statistics are
answered *unavailable*; `activity::supports_activity` answers no for DuckDB, as for SQLite, so
nothing asks. (It was `!matches!(dialect, Sqlite)` until DuckDB arrived and fell onto the "has
sessions" side — the shape of every bug the capability rule exists for.)
**Writes, phase 1.** `run_ddl` is one connection and one transaction, all or nothing, DuckDB's
DDL being transactional (`a_ddl_plan_is_all_or_nothing`) — but it reads its cancel token only
before it starts, where `sqlite::run_ddl` was fixed to interrupt a running statement; nothing
reaches it yet, since `ddl::duckdb_supports` refuses every change. `run_script` is one connection
for the whole file and wraps nothing, with SQLite's runtime-flavour choice between
`block_in_place` and `spawn_blocking`. **A backup is `ATTACH` + `COPY FROM DATABASE` + `DETACH`**
(`backup_to`, behind `Db::backup_to_file`): every schema, table, view, sequence and macro with its
rows, into a **new** file. A target that already exists is refused before anything runs, as
SQLite's `VACUUM INTO` refuses one, because `ATTACH` would open it and copy *into* what it holds
(`a_backup_copies_every_table_with_its_rows`, into `:memory:`). `Db::explain` builds `EXPLAIN` or
`EXPLAIN ANALYZE` in the dispatcher, on a read-only transaction where it is asked `read_only`, and
`fetch_table` — the Live Monitor — is a generated `SELECT * … LIMIT` there too.
**What phase 1 does not do is refused, never faked**: `commit_writes`, `refetch_rows`,
`fetch_blob` and `import_rows` answer `DbRefusal::DuckDbNotYet`; no result column carries
provenance (`origin: None`), so every result is read-only and the grid offers no edit to commit;
`Session::open` refuses a pinned session (`DuckDbNoManual`) and `tx::supports_manual_mode` keeps
the segment off; `import::supports_import` and `dump::supports_dump` say no, and
`ddl::duckdb_supports` keeps the designer and the view, trigger and routine editors off. For
when the session lands, `session::tx_engine_of` already maps DuckDB to PostgreSQL's model — it
aborts a transaction on a failed statement and rolls DDL back, measured on 2.0.
**A missing file is refused, never created** (`open_target`, `DbRefusal::DuckDbMissing`). DuckDB's
open creates one, which for a database *client* is the wrong default by some distance: a mistyped
path would open an empty database that looked like it worked. An empty path is `DuckDbNoFile`, and
`:memory:` and a MotherDuck `md:`/`motherduck:` name are `DuckDbNotAFile` — the first a database
that dies with each operation's instance, the second needing an extension this build does not
fetch (`a_missing_file_is_refused_not_created`, `names_that_are_not_a_file_are_refused`, whose
filesystem question is a parameter so no file is touched). **The suite runs on in-memory DuckDB**,
the sanctioned exception in-memory SQLite already is: a `memory:<name>` target, accepted only under
`cfg(test)`, is a shared in-memory instance the tests seed, held for the process in `KEEP`, since
an in-memory instance *is* its data and letting it close between two operations of one test would
empty it.
**`duckdb.rs` answers every name in `ENGINE_ENTRY_POINTS`**, the unwritten ones with the refusal,
and the dispatcher calls it for each — but neither convention test lists it yet: the module list
of `every_engine_module_answers_the_whole_interface` and the loop of
`the_dispatcher_calls_every_engine_module_for_every_entry_point` stop at `mssql`, and
`the_entry_point_list_is_what_the_dispatcher_actually_dispatches` intersects four modules. A name
this module drops is a compile error at its dispatch arm and nothing at the census.
