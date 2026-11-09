# `db/duckdb.rs`

**`duckdb.rs` is the fifth engine** (the `duckdb` crate, the engine compiled in — `bundled`,
with the `parquet` and `json` extensions linked statically), phase 1 and the first part of phase 2
of the roadmap's *DuckDB as a fifth engine*: it connects, reads, introspects, validates, runs
batches, scripts and DDL plans, plans a query, lists its settings, backs a file up, writes grid
edits back, imports a file into a table, dumps to a `.sql` file and holds a Manual tab's
transaction. It is **embedded, as SQLite is**, so most
of `sqlite.rs`'s first paragraph holds here unchanged — there is no server, a connection is a
file (`Db::file`, `connection::is_file_backed`), host/port/user/password/SSH are inert, the one
database is the file's own catalog (`current_database()`, named after the file), and the driver
is blocking, so every call runs on Tokio's blocking pool. What makes it more than a second SQLite
is the things below, and the module doc names the same ones.
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
overlap (`overlapping_operations_share_one_instance`). A Manual tab's pinned connection (below) is
not this exception but the invariant's existing `TxMode::Manual` one, and it holds the instance —
and the file's lock — for as long as the tab stays in Manual mode. **It is why the init SQL refuses `SET
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
**A string inside a list, struct or map, and a struct's field name, is escaped with backslashes**
(`nested_string`: `['it\'s', 'a\\b']`), not by SQL's doubling of the quote, because that text is
what a grid edit of the cell sends back for DuckDB to cast, and the cast does not read doubling —
`['it''s']` reads back as `its` (measured on 2.0). The doubled form was what this rendered first;
`a_nested_string_is_escaped_the_way_duckdb_reads_it_back` casts each rendering back and compares
it with the value it came from.
**Two decode hazards are walled off, both in the driver** (duckdb 1.20000.1): a `NULL`-typed
column (`SELECT NULL`) is never asked of it, since decoding one panics, and every other cell is
read under `catch_unwind`, so a value it cannot decode costs that cell — a `<TYPE>` placeholder —
rather than the worker and the whole result (`values_read_as_duckdb_writes_them`). Both walls
are in `decode_cell`, the one cell decoder, which `run_query` and `refetch_rows` share so that a
re-fetched row reads exactly as the result it is spliced into. **A write's
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
rather than a reconstruction — **with what a table's text leaves out put back**: `duckdb_tables().sql`
carries neither the table's `CREATE INDEX`es nor any `COMMENT ON`, so *Copy DDL* and a dump both
restored a table with no index and no comment. `TableInfo::create_ddl`'s DuckDB arm
(`duckdb_create_parts`) now appends `COMMENT ON TABLE`/`COMMENT ON COLUMN` and each
non-constraint index's `IndexInfo::create_sql` (`duckdb_indexes().sql`), and
`create_ddl_holding` hands the indexes back as its held half, for the dump to build after the
rows; a key or unique constraint is in the text already. A view's definition is the text after the stored statement's first
top-level `AS` (`view_body`, over `sql::skip_noncode`). The primary key marks its columns and is
the `PRIMARY` index the model reads keys from, a `UNIQUE` constraint is a unique index of its
name, checks and foreign keys are themselves, and a column defaulting to `nextval(…)` is read as
auto-numbered. **A generated column is told apart by `information_schema.columns.is_generated`**,
because `duckdb_columns()` reports its expression as the column's *default* (measured:
`CAST((a * 2) AS INTEGER)`) and says nothing else; read as given, it was an ordinary column with a
default. It arrives as `generated: Some(expr)` with `default: None`, which is what has `edit.rs`
keep it read-only, as on every engine — `table_facts`, provenance's own column read, makes the same
join for the same reason. **Macros are DuckDB's user functions and arrive as `RoutineInfo`** — one per name,
an overload being the same macro to `DROP MACRO`, a table macro's `returns` reading `TABLE`.
`body` is `duckdb_functions().macro_definition`, the expression or query alone, and a later
overload is folded into it as `…, (a, b) AS expr` — the form one `CREATE MACRO` declares them all
in — so `RoutineInfo::create_sql`'s DuckDB arm (`duckdb_create_sql`) writes the whole macro back
as `CREATE MACRO s.m(args) AS [TABLE] …` (`a_macros_ddl_replays_as_the_same_macro`, which replays
each one into a fresh database and calls every overload). Until that arm existed DuckDB fell on
MySQL's emitter, and *Copy DDL* on a macro wrote `CREATE FUNCTION … NOT DETERMINISTIC`. **A
parameter's default is not carried**: the catalogue lists `b := 2` as `b` (measured on 2.0), so
the text declares the parameter without it.
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
DDL being transactional (`a_ddl_plan_is_all_or_nothing`), and Stop reaches a statement already
running through the connection's interrupt handle, as `sqlite::run_ddl`'s does. It once read its
token only before it started, which would have had a plan's one long statement hold the preview
modal open for its whole length with the button reading *Stop*. An interrupt surfaces as an
ordinary error, so the token is what maps it to `DdlCause::Cancelled`
(`stop_interrupts_a_running_ddl_statement`, a `CREATE TABLE … AS` counting 10^11 rows). No plan
from the editors reaches it yet, since `ddl::duckdb_supports` refuses every change. `run_script`
is one connection for the whole file and wraps nothing, with SQLite's runtime-flavour choice between
`block_in_place` and `spawn_blocking`. **A backup is `ATTACH` + `COPY FROM DATABASE` + `DETACH`**
(`backup_to`, behind `Db::backup_to_file`): every schema, table, view, sequence and macro with its
rows, into a **new** file. A target that already exists is refused before anything runs, as
SQLite's `VACUUM INTO` refuses one, because `ATTACH` would open it and copy *into* what it holds
(`a_backup_copies_every_table_with_its_rows`, into `:memory:`). `Db::explain` builds `EXPLAIN` or
`EXPLAIN ANALYZE` in the dispatcher, on a read-only transaction where it is asked `read_only`, and
`fetch_table` — the Live Monitor — is a generated `SELECT * … LIMIT` there too.
**Grid write-back reads its provenance from the statement, because the driver reports none.**
`run_query` calls `origins_of` on the connection that ran the statement, after it ran and before
its rows are read, so the session state the result was read under — a temporary table,
`search_path`, a `USE` — is the state that decides it; `fetch_query` and `run_batch` capture the
file's own catalog (`home`) when they connect, for it to compare against. `attribute` takes
`intel::projection_of`'s placement — positional, one base table, refusing a join, a set operation,
a CTE, a derived table, a wildcard with options and a qualifier that may be a struct column (see
`core::intel`) — and proves it against the catalogue, and **any doubt about the relation
attributes the whole result to nothing**, since a wrong origin is an `UPDATE` of another row or
another table while a missing one is only a read-only grid. The doubts: the connection is no
longer on `home` (a `USE` of an attached database would have a bare name mean another file); for a
bare name, `search_path` is set or a temporary table or view has that name, either of which has
DuckDB resolve it elsewhere; a qualifier is also a catalog's name (`duckdb_databases()`), which
DuckDB could read either way; the relation is not a base table of that schema — a view's rows
belong to whatever it reads; and a wildcard's result columns are not the table's, in order and by
name. **The catalog refusal has a known casualty**: a file named `main.duckdb` is the catalog
`main`, so `main.t` could name the schema or the catalog and its results get no edits. Within a
proven relation a placed name that is not one of its columns — `rowid`, an earlier select-list
alias — is that one column attributed to nothing; names match case-insensitively, as DuckDB binds,
and come back in the catalogue's spelling. That match is sound because a table column wins over an
earlier alias of the same name (measured: `SELECT x + 10 AS a, a` returns the column `a`). The
flags are `table_facts`': the primary key (its columns `not_null` too), a single-column `UNIQUE`
as a unique key, `NOT NULL`, a `nextval(…)` default as auto-increment, and `no_default` for a
`NOT NULL` column with neither a default nor a generation expression
(`a_single_table_read_is_attributed_by_position`,
`what_the_statement_cannot_vouch_for_is_attributed_to_nothing`).
**`rowid` is never an origin, and so never the implicit key it is on SQLite: a keyless table is
read-only here.** A checkpoint after a delete that empties most of a row group compacts it and
renumbers what is left — the row holding 63 comes back as rowid 3, a number another row held
before — so a grid that read rowid 3 and then committed would write that other row, and the 1-row
net would see exactly the one row it wants. Measured on 2.0 on a file and across a reopen, and in
memory by `a_checkpoint_renumbers_rowids_so_none_is_a_key`, which fails if rowids ever survive the
compaction — the cue to re-measure the decision, not to start keying on them. (An `UPDATE` alone
keeps the rowid.) **The seam is tested, not only the halves**:
`the_grid_edits_a_keyed_table_and_not_a_keyless_one` puts this engine's results and its schema
read through `edit::analyze_edit` — a keyed table editable by its key with its generated column
read-only, and `SELECT rowid, * FROM k` over a keyless one not editable at all.
**The executors** — `commit_writes`, `refetch_rows`, `fetch_blob` — share `with_conn_stoppable`:
an operation's own connection on the blocking pool, with Stop reaching a running statement through
its interrupt handle, the shape of `sqlite::commit_writes`. As there, a cancel answers
`DbError::Cancelled` at once rather than awaiting the worker, whose interrupted statement then
fails into the closure's `ROLLBACK`. A write names `"schema"."table"` (`table_sql`), never the
catalog, which is safe for the reason SQLite's bare name is: the statement runs on a fresh
connection on the file's own catalog, and `origins_of` attributes nothing outside it. Key values
bind as typed `DValue`s, a text key cast by DuckDB to the column's type at bind (measured on 2.0
for integers, decimals, zoned timestamps, intervals, booleans, UUIDs and `HUGEINT`s past `i64` —
the forms `cell_of` writes), and a `NULL` key compares `IS NULL`. A staged `CellEdit::Default` is
the keyword `DEFAULT` in `SET` and in `VALUES` alike (measured), and an insert that stages no
column at all is `DEFAULT VALUES`. The commit is one transaction, each statement held to `one_row_verdict`, and its
rollback is always `Rollback::Complete`, DuckDB having no non-transactional table — a Manual
tab's batch, below, is the one that reports more
(`a_grid_commit_updates_inserts_and_deletes_in_one_transaction`,
`a_write_that_misses_its_row_rolls_the_whole_commit_back`). `refetch_rows` looks a row up by its
key and confirming columns, in `edit::refetch_key`'s order, and skips one no longer there.
`fetch_blob` reads the **whole** value and cuts it to `blob::FETCH_CAP` on this side, DuckDB having
no byte-slicing of a `BLOB` in SQL — so showing a large blob's first bytes still carries all of it
across (`rows_are_refetched_and_blobs_read_by_key`). The two reads' bodies are `refetch_on` and
`blob_on`, over any connection, so a Manual tab's pinned one runs the same lookup (below).
**An import is `sqlite::import_rows`' shape** — one connection, one `BEGIN TRANSACTION`, the
rows in batches of `import::INSERT_BATCH_ROWS` as the multi-row `INSERT`s every engine's load runs
(`ImportTarget::insert_sql`, so `build_insert_as`, the table named `"schema"."table"`, never the catalog,
through `export::qualified_table`), each batch held to exactly the rows it carried (`WriteCause::Batch`,
`Rollback::Complete`), and `ROLLBACK` on any failure, a failed statement having already aborted
DuckDB's transaction. It runs **off the executor rather than on the blocking pool** every other
operation here uses (`off_executor`: `block_in_place` on a multi-thread runtime, inline on a
current-thread one, where `block_in_place` panics), because `RowSource` is a borrowed iterator that
cannot move into a `'static` task and the transaction needs the one connection for every batch.
A file's cells arrive as text and DuckDB casts each literal to its column's type on insert —
booleans, timestamps and lists included (`an_import_loads_every_batch_in_one_transaction`,
`a_failed_import_leaves_the_table_as_it_was`, a duplicate key in the second batch leaving nothing
of the first, and `an_import_runs_on_a_current_thread_runtime`).
**A `.sql` dump restores into an empty database, and replays onto its source**
(`dump::supports_dump`; the file is `core::dump`'s plan, whose entry has DuckDB's half of each
rule). The engine has neither a session switch for foreign keys nor `DROP … CASCADE`, so the file
drops everything up front as SQL Server's does — but with no key drops, DuckDB having no `ALTER
TABLE … DROP CONSTRAINT`: the referencing table is dropped before the one it references, and its
keys go with it. Each table's captured text, comments and keys inline, is created and filled in
turn, its indexes are built after every table's rows, a schema other than `main` is created if
missing, and a sequence restarts at the value the source hands out next, DuckDB having no
`setval` to move it on with. The round trip is tested for real on in-memory instances, through
`plan`, `render_rows`, the script splitter and `run_script` as *Export* and *Run SQL file* run
them: a fixture of two schemas, two sequences behind key defaults, a check, a unique constraint,
a foreign key, two indexes, a view, a macro, table and column comments and `DATE`, `JSON`, list
and struct values restores into an empty database with every table's statement and comment, every
column comment, every index and every table's and view's rows equal, the next keys continuing
from the source's (9 and 101), the macro callable and a default key clearing the restored rows
(`a_dump_restores_into_an_empty_database_as_it_was`); replayed onto its source it leaves the same
fingerprint and the counters unmoved (`a_dump_replays_onto_its_source`); and without *One
transaction* that replay stops at `DROP TABLE … "parent"`, the first table a key points at,
leaving the source as it was (`a_replay_without_a_transaction_stops_at_a_referenced_table`).
**What the file does not carry**, measured by the change's author: a user type — `CREATE TYPE
mood AS ENUM …`, a `STRUCT` alias — is inlined into each column in `duckdb_tables().sql` (`m
ENUM('ok', 'sad')`), so the table restores without the named type, and the type itself is not
carried, none being read into the model; a view or macro naming the type would fail its restore.
A view's or a macro's comment is not carried either — only a table's and its columns' are — nor a
macro parameter's default, for the reason above.
**A Manual tab pins one connection, and with it the file** (`Pinned`, opened by `pin`, behind
`session.rs`'s `Backend::DuckDb`). It is the connection an operation would take — `connect`'s, so
the init SQL runs on it — held with its interrupt handle, the file's own catalog read at open
(`home`, for provenance as above) and the `Arc<Instance>` it came off, so the instance does not
close between the tab's statements and **no other program can open the file until the tab goes
back to Auto**. That is the one thing the roadmap said this mode had to tell the user, and the
connection form's hint says it (`connections-duckdb-hint`). The driver is blocking, so `on_pinned`
runs each call on the blocking pool with the connection behind a `std::sync::Mutex` of its own —
the session's `tokio` mutex above it is what orders the tab's operations — and a watcher task
interrupts it on Stop. **Unlike `with_conn_stoppable`, it awaits the worker after a Stop**, because
this connection outlives the statement: the tab's next operation never starts while the
interrupted one still holds it. An error after a Stop reads as
`DbError::Cancelled` (`stop_interrupts_a_manual_tabs_statement`, which then runs a statement on the
same connection). `pinned_query`, `pinned_exec`, `pinned_refetch` and `pinned_blob` are the
session's calls over it, the last two on the pinned connection because only it sees what the open
transaction wrote (`a_manual_grid_commit_is_refetched_on_the_pinned_connection`). The session
spells its transaction `BEGIN TRANSACTION`, `BEGIN TRANSACTION READ ONLY`, `COMMIT` and `ROLLBACK`,
and **a read-only connection needs nothing set at open**: every transaction `ensure_tx` opens on it
is `READ ONLY`, which refuses a write by its effect, and a Manual tab runs nothing outside one
(`a_read_only_manual_tab_refuses_a_write`). A failed statement aborts the transaction, as on
PostgreSQL — `session::tx_engine_of` maps DuckDB to that model, measured on 2.0 — so
`Session::commit` probes `SELECT 1` first, as it does there, and reports the abort rather than a
commit of nothing (`commit_after_a_failed_statement_reports_the_abort`); the tab's statements are
one transaction no other connection sees until Commit (`a_manual_tab_holds_its_writes_until_commit`).
`Session::alive` answers yes without asking: there is no connection in this process to lose.
**DuckDB has no `SAVEPOINT`** (2.0: a parser error), and two things follow from it. **A failed grid
commit takes the whole transaction with it.** The other engines nest the batch in a savepoint so a
failure undoes it alone; here `pinned_write` runs it bare inside the tab's transaction, where a
statement the engine refuses has aborted the transaction and a one-row miss has written into it,
so on **any** failure — those two, or a Stop — it issues `ROLLBACK` and fails with
`Rollback::Transaction`, whose note says every statement the tab ran since its transaction began
is undone, not only the edits. `Session::commit_writes`' DuckDB arm then clears `in_tx` and reports
`StmtOutcome::FailedAndRolledBack`, folding the pill to `Idle`, never an isolated outcome. That is
the roadmap's decision: the grid commit is not refused inside a transaction, and its failure is
reported as the transaction's loss rather than claimed as a batch undone alone
(`a_failed_grid_commit_rolls_the_manual_transaction_back_and_says_so` — a one-row miss after an
edit that landed, the tab's earlier `INSERT` gone too, the next statement opening a fresh
transaction that commits; red with the `ROLLBACK` removed — and
`a_refused_grid_statement_rolls_the_manual_transaction_back_too`). **A Stop before the batch is
sent costs nothing**: `pinned_write` answers a plain `Cancelled` without a `ROLLBACK`, and the
session reports it `NotSent` (`a_grid_commit_stopped_before_it_is_sent_keeps_the_transaction`, red
without that check). And `Rollback::Transaction` is claimed only where the server took the
`ROLLBACK`; one it refused is `Rollback::Unknown`. **And the reads are unfenced.**
`run_scope_sql` answers `false` for DuckDB, so `fence_read` sets no savepoint around
`refetch_rows` or `fetch_blob`, and a read stopped in a Manual tab folds as a plain `Cancelled` —
an aborted transaction in PostgreSQL's model, **which is the truth here**: an interrupt aborts
DuckDB's transaction as PostgreSQL's cancel does, the next statement answering *Current
transaction is aborted* (measured on 2.0; `stop_interrupts_a_manual_tabs_statement` holds Commit to
reporting the abort).
**What is not written yet is refused, never faked**: `ddl::duckdb_supports` keeps the designer and
the view, trigger and routine editors off.
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
**`duckdb.rs` answers every name in `ENGINE_ENTRY_POINTS`**, and the dispatcher calls it for each — and all three convention tests hold it to that:
it is in the module list of `every_engine_module_answers_the_whole_interface` and the loop of
`the_dispatcher_calls_every_engine_module_for_every_entry_point`, and
`the_entry_point_list_is_what_the_dispatcher_actually_dispatches` intersects all five modules.
