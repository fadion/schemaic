# `db/duckdb.rs`

**`duckdb.rs` is the fifth engine** (the `duckdb` crate, the engine compiled in — `bundled`,
with the `parquet` and `json` extensions linked statically): it connects, reads, introspects, validates, runs
batches, scripts and DDL plans, plans a query, lists its settings, backs a file up, writes grid
edits back, imports a file into a table, dumps to a `.sql` file, holds a Manual tab's
transaction, designs tables and edits views and macros, creates and drops schemas, compares two
files' schemas (*Schema compare*, below), and opens a CSV, Parquet or JSON file into a
throwaway in-memory session (*Open data file*, below). It is **embedded, as SQLite is**, so most
of `sqlite.rs`'s first paragraph holds here unchanged — there is no server, a connection is a
file (`Db::file`, `connection::is_file_backed`), host/port/user/password/SSH are inert, the one
database is the file's own catalog (`current_database()`, named after the file — up to its
first dot, which `connection::duckdb_catalog_name` reads off the path without an open), and the
driver is blocking, so every call runs on Tokio's blocking pool. What makes it more than a second SQLite
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
any other program, the moment the last operation holding it ends. **The registry's lock is never
held across an open** (`instance_opened_by`): it was, so a file on a share that had gone away, or a
long WAL replay, stalled every DuckDB operation in the process — other files, data sessions, and
*Open data file*, which takes the lock on the UI thread, froze the window. A lookup takes it
briefly; a miss takes the file's own lock in `OPENING`, looks again, opens with neither held and
inserts, so two opens of one file queue on that file alone
(`a_slow_open_does_not_stall_another_files`). That is this engine's
exception to the one-connection-per-operation invariant, and it is narrower than it sounds:
connections are still per operation, and only the database underneath them is shared while they
overlap (`overlapping_operations_share_one_instance`). A Manual tab's pinned connection (below) is
not this exception but the invariant's existing `TxMode::Manual` one, and it holds the instance —
and the file's lock — for as long as the tab stays in Manual mode. **It is why the init SQL refuses `SET
GLOBAL` here** (`init_sql`'s `DUCKDB_SETTINGS` and its `duckdb` arm): a `GLOBAL` setting is the
instance's, and the instance belongs to whatever else is open on the file at that moment — and why
the list is only `schema` and `search_path`. Most other settings are instance-wide however they
are spelled (`threads` and `memory_limit` have only a global scope), and a sealed instance's
locked configuration refuses everything but those two (`DBConfig::CheckLock`), so the form's
own former placeholder, `SET threads = 4`, failed every unattended read of the connection at
connect (`a_sealed_handle_runs_the_init_the_form_offers`). The init SQL itself runs on each
operation's cloned connection, in `connect`.
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
quietly given the wrong one. That test's in-memory instance is sealed without the read-only open,
there being no file to open read-only, so the other half has a test of its own on a real file —
one of the workspace's sanctioned filesystem exceptions, with the user's say-so
(`a_sealed_file_is_opened_read_only`: a file made under the system temp directory, opened
sealed, reads, and refuses `INSERT`, `CREATE` and `DROP` as *read-only*; reopened unsealed, it
holds what it held, and the directory is removed however the test ends). **Who seals:**
`cli::query`'s `read_only_query`, `read_only_export` and `read_only_explain` each call
`db.clone().sealed()` before they run (`schemaic query`/`export`/`explain`), and the MCP server
seals every handle it serves in `mcp::serve`/`serve_many` — none of its tools writes, and
`sample_rows` and `describe_table`'s samples read through views a body may have aimed at a file.
The Live Monitor's `fetch_table` is the editor's, attended, and unsealed. No test reaches a
DuckDB file through `cli` (the suites' `memory:` targets live in `schemaic-db`'s own tests, and an
*Open data file* session in the app's process alone), and no source gate reads the `.sealed()`
calls.
**Nothing is fetched from the network.** Every instance — sealed or not — opens with extension
auto-install and auto-load off, so `parquet` and `json` are there and any other extension is
unreachable rather than downloaded behind a query (`no_extension_is_fetched_behind_a_query`).
`INSTALL`/`LOAD` are on the read gate's deny lists besides.
**Values are typed by column, and rendered as DuckDB writes them** (`cell_of`, `text_of`,
`type_name`): `true`/`false`, integers as numbers (a `HUGEINT` past `i64` as its digits), a
`FLOAT` through its own shortest text so `0.1::FLOAT` reads `0.1`, a `DECIMAL` exact as text, a
timestamp in ISO form with `TIMESTAMP WITH TIME ZONE` in UTC (`+00`, this build linking no ICU),
an interval as `1 year 2 months 3 days 04:05:06`, a list `[1, 2]`, a struct `{'a': 1}`, a blob
as its size and its column marked binary. **A `BIT` and a `BIGNUM` are not blobs**, though the
driver hands both over as their storage bytes: `bit_text` reads the padding byte and the bits,
`bignum_text` the three-byte header (sign and length) and the magnitude, inverted for a negative,
and each is written as DuckDB writes it, the column not marked binary
(`a_bit_and_a_bignum_read_as_their_text`, checked against DuckDB's own cast) — they showed
`<n bytes>` and a dump withheld them as NULL under a note calling them binary. A column's type name keeps a declared alias (`JSON`).
**A string inside a list, struct or map, and a struct's field name, is escaped with backslashes**
(`nested_string`: `['it\'s', 'a\\b']`), not by SQL's doubling of the quote, because that text is
what a grid edit of the cell sends back for DuckDB to cast, and the cast does not read doubling —
`['it''s']` reads back as `its` (measured on 2.0). The doubled form was what this rendered first;
`a_nested_string_is_escaped_the_way_duckdb_reads_it_back` casts each rendering back and compares
it with the value it came from.
**Two decode hazards are walled off, both in the driver** (duckdb 1.20000.1): a `NULL`-typed
column (`SELECT NULL`) is never asked of it, since decoding one panics, and every other cell is
read under `catch_unwind`, so a value it cannot decode costs that cell — a `<TYPE>` placeholder —
rather than the worker and the whole result (`values_read_as_duckdb_writes_them`). **The
caught panic does not run the process's panic hook** (`quietly`): every `TIME_NS` value panics
in the driver, and the app's hook forces a backtrace and logs about 5 KB a time — a gigabyte of
log and a minute for a 200k-row result. The first decode wraps whatever hook is installed (the
app's, set at start-up) in one that skips a panic raised while a thread-local flag is set, and
`quietly` sets it around the driver's call alone, so every other panic still reaches the hook
(`an_undecodable_cell_does_not_run_the_panic_hook`). Both walls
are in `decode_cell`, the one cell decoder, which `run_query` and `refetch_rows` share so that a
re-fetched row reads exactly as the result it is spliced into.
**What the driver bends is put back where it can be, and said where it cannot**
(`values_the_driver_bends_read_as_duckdb_writes_them`): a timestamp is split into seconds and
nanoseconds by flooring (`seconds_and_nanos` — truncation put a pre-1970 `TIMESTAMP_NS` in the
wrong second) and a `TIMESTAMP_NS`/`TIME_NS` keeps its nanoseconds; `TIME '24:00:00'` is written
as itself rather than wrapped to midnight; DuckDB's ±infinity (`i64::MAX` and its negation for a
timestamp, `i32::MAX` and its negation for a date) is `infinity`/`-infinity` rather than the raw
integer; a NULL union is NULL rather than the text `NULL`; and each column's type travels down
into a list, struct or map as a `Shape`, because nested the driver carries an unsigned `HUGEINT`
as a signed one and `[UHUGEINT max]` read `[-1]` — the text a grid edit of the struct would send
back. **A `TIME WITH TIME ZONE` reaches the decoder without its offset** (duckdb 1.20000.1 hands
it over as a plain `Time64`), so it shows the `<TIME WITH TIME ZONE>` placeholder rather than
another instant; a union's member type is not known either, the driver not saying which member
is set. The dump does not read either from the decoder (`dump::literal_select`, below). **A write's
row count arrives as a result**: DuckDB answers `INSERT`/`UPDATE`/`DELETE` without `RETURNING`,
and `CREATE TABLE … AS`, with a one-row `Count` column, which `counts_rows` recognises and turns
into `ResultSet::affected` so the grid does not show it as data
(`a_write_reports_its_count_not_a_count_column`). **So does a statement's mere success**: DuckDB
answers `CREATE` with an empty `Count` result and `DROP`, `ALTER`, `SET`, `COMMENT`, transaction
control and the like with an empty `Success` one (2.0.0, measured), and both reached the window
as a zero-row grid headed `Success BOOLEAN`, while `schemaic exec` printed an empty table.
`reports_status` recognises a lone `Count` or `Success` column under a statement whose head is on
its list (`CREATE`, `DROP`, `ALTER`, `SET`/`RESET`, `COMMENT`, the transaction words,
`ATTACH`/`DETACH`, `USE`, `CHECKPOINT`, `VACUUM`, `LOAD`/`INSTALL`, …), and `run_query` turns it
into `affected = Some(0)` with no columns — what SQLite reports for any statement without
columns — so both say *0 rows affected*. **The head decides as well as the column**, so `SELECT
true AS "Success"` stays a query; don't reduce it to the column name. It is asked after
`counts_rows`, so `CREATE TABLE … AS` keeps its real count
(`a_status_only_statement_reports_no_rows`).
**`Enforce` is a read-only transaction**: anything that `refuses_writes` — `ReadOnly` and
`Generated` alike — runs inside `BEGIN TRANSACTION READ ONLY`, where DuckDB refuses a write by its
effect (*Cannot write to database … transaction is launched in read-only mode*), and is rolled
back after, on the same cloned connection so the next operation is not left inside it
(`a_read_only_run_refuses_a_write_by_its_effect`). That holds the session half of the read-only
invariant; it does not stop a file read, which is the seal's job. `run_batch` and `fetch_query`
take the connection's interrupt handle before any work, so Stop reaches a running statement, as
on SQLite. **The workers ask the token themselves too**, right before each statement
(`fetch_on`, and `run_batch`'s loop): an interrupt that lands before a statement starts is
cleared when it starts, so a Stop during a slow connect answered *Cancelled* at once while the
query then ran to its end with the instance and the file's lock held — and the CLI's `--timeout`
waited on it at exit (`a_cancel_before_the_statement_starts_stops_it`). `prepare_check` — the live validation, run as the user types — does not prepare a
statement whose head acts at prepare time or changes the session or the instance (`SET`, `PRAGMA`,
`ATTACH`, `INSTALL`/`LOAD`, `USE`, `CHECKPOINT`, `COPY`, the transaction words and the rest of its
list), the reason SQLite's skips a `PRAGMA`; those go unchecked and their run reports them.
**Every prepare of statement text is one statement by DuckDB's own count** (`prepare_one`):
the driver's `prepare` (duckdb 1.20000.1, `InnerConnection::prepare`) *executes* every statement
but the last, and DuckDB's scanner and core's lexer need not agree on where one ends — a lone
`\r` ends a `--` comment for DuckDB and did not for core. A text core took for one `SELECT`
therefore ran a hidden `DELETE` from live validation, with nobody pressing Run, and a hidden
`COMMIT` ended a read-only run's `READ ONLY` transaction so the `CREATE` after it ran in
autocommit. `run_query` (so `fetch_query`, `run_batch`, a Manual tab's `pinned_query` and the
plan's `EXPLAIN`), `prepare_check`, `refetch_on` and `blob_on` all go through `prepare_one`, which
asks `duckdb_extract_statements` on a parse-only in-memory database of the process's own
(`statement_count`, `PARSER` — the extraction only parses) and refuses any count but one before
anything is prepared (`validation_runs_no_statement_hidden_behind_a_comment`,
`a_read_only_run_runs_no_hidden_commit`). It is the boundary check, not the lexer, that closes
this: a lexer fix closes one disagreement, and this refuses the next.
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
auto-numbered. **A column's collation is read from the table's stored statement**
(`intel::column_collations` over `duckdb_tables().sql`), the one place DuckDB keeps it:
`duckdb_columns()` has no column for it and `information_schema.columns.collation_name` is always
null (measured), so a collation the designer set read as an edit again on every open
(`a_columns_collation_is_read_back_and_survives_a_rebuild`). **A generated column is told apart by `information_schema.columns.is_generated`**,
because `duckdb_columns()` reports its expression as the column's *default* (measured:
`CAST((a * 2) AS INTEGER)`) and says nothing else; read as given, it was an ordinary column with a
default. It arrives as `generated: Some(expr)` with `default: None`, which is what has `edit.rs`
keep it read-only, as on every engine — `table_facts`, provenance's own column read, makes the same
join for the same reason. **The cast round it comes off** (`generated_expression`): DuckDB wraps
the expression in a cast to the column's type each time it stores one, so restating the text it
hands back wraps it again — a designer rebuild turned `CAST((a * 2) AS INTEGER)` into
`CAST(CAST((a * 2) AS INTEGER) AS INTEGER)`, which the designer then read as an edit nobody made.
Only a cast that spans the whole text and names exactly the column's type is dropped, the scan
for its closing bracket over `sql::skip_noncode`; anything else is kept as read
(`a_generated_expression_loses_only_the_cast_duckdb_added`). **Which tables other tables' keys
point at is linked after the keys are read** (`schema::link_inbound_foreign_keys`, so
`TableInfo::referenced_by` is filled here as on SQL Server), since the designer's rebuild cannot
drop such a table and says so. **Macros are DuckDB's user functions and arrive as `RoutineInfo`** — one per name
and kind, an overload being the same macro to `DROP MACRO`, a table macro's `returns` reading
`TABLE`. **A scalar and a table macro of one name are two macros**, not overloads of one: they
are separate catalogue entries (measured on 2.0.0 — `CREATE OR REPLACE MACRO t(n) AS n + 1` over
a table macro `t` adds a second rather than replacing it), so only entries of the same kind fold
(`a_scalar_and_a_table_macro_of_one_name_read_as_two`).
`body` is `duckdb_functions().macro_definition`, the expression or query alone, and a later
overload is folded into it as `…, (a, b) AS expr` — the form one `CREATE MACRO` declares them all
in (`fold_overloads`). **A table macro with more than one overload has each query in
parentheses**: folded bare, `SELECT a, 1, (a, b) AS TABLE SELECT a, b` is a parse error (measured
on 2.0.0), the `SELECT`'s own commas running on into the next overload, and *Copy DDL*, the dump
and every edit of such a macro failed (`an_overloaded_table_macro_replays`,
`overloads_fold_into_one_declaration`); a scalar body is one expression, with no top-level comma
to confuse, and a lone table query is left bare — so `RoutineInfo::create_sql`'s DuckDB arm
(`duckdb_create_sql`) writes the whole macro back as `CREATE MACRO s.m(args) AS [TABLE] …`
(`a_macros_ddl_replays_as_the_same_macro`, which replays each one into a fresh database and
calls every overload). Until that arm existed DuckDB fell on
MySQL's emitter, and *Copy DDL* on a macro wrote `CREATE FUNCTION … NOT DETERMINISTIC`. The
argument list is `macro_parameters` over `parameters` and `parameter_types`, the second read
position by position (`positional_strings`, an untyped parameter's type being `NULL`), so a typed
macro reads as `a INTEGER, b INTEGER`; before, it was read by name alone and *Copy DDL* and the
dump restated it untyped. Each name goes through `ident_if_needed`, so `"select"` or `"my b"` is
restated quoted — joined bare, *Copy DDL*, the dump and every edit of the macro failed to parse
(`a_macro_parameter_that_needs_quoting_is_quoted`). **A parameter's default is not in the catalogue**: it lists `b := 2`
as `b` and holds the value in no column (measured on 2.0.0), so a text built from it declares the
parameter without it. **Whether a macro has one is found by binding** (`macro_hides_defaults`):
a default is the one thing that lets a call pass fewer arguments than an overload declares, so for each `k`
below the largest overload's arity a call with `k` `NULL`s is *prepared* — bound, never run — and
how many overloads accepted it, read off the error (*Macro m() does not support the supplied
arguments*: none; *has multiple overloads*: two or more; anything else, success included: at
least one), is compared with how many declare `k`. More acceptors than declarers is a default.
**It fails closed**: an error in a form it does not recognise counts as accepted, and two
overloads of one arity below the largest — typed, where the count cannot tell them apart —
count as a default outright; either way the cost is an export asked for needlessly, or a macro
left uneditable, never a default dropped. **What the default is, is read from an export**
(`read_macro_defaults`), the one place DuckDB 2.0.0 prints it — asked once per schema read,
however many macros want it, and only when one does. An `EXPORT DATABASE` of the file itself
would write every table's rows, so it never runs there: an in-memory database is `ATTACH`ed
under a name unique to the process and the call, `COPY FROM DATABASE <catalog> TO <scratch>
(SCHEMA)` copies the catalog into it without rows — a macro whole, defaults and all (measured on
2.0.0) — and that empty copy is exported to a fresh directory under the system temp directory,
whose `schema.sql` is read before the scratch database is `DETACH`ed and the directory removed.
Nothing is written to the user's file. **The directory is not a choice**: DuckDB 2.0.0 has no
in-memory filesystem to export to (`memory://` is read as a relative path), which makes this one
of the workspace's sanctioned filesystem exceptions — in the app at every such read, and in every
test here that reads a macro with a default. `exported_macros`/`exported_macro` read the script's
`CREATE MACRO` statements, whose form is regular (measured on 2.0.0):
`CREATE MACRO s."n" (a, b := 2) AS ((a + b)), (a) AS (a);`, every body inside one pair of the
export's own parentheses — which is what lets a top-level comma end an overload even after a
table macro's `SELECT x, y`. That pair is taken off, so a body reads as `macro_definition` gives
it, and a statement out of that form is skipped, never guessed at
(`the_exports_macros_are_read_with_their_defaults`, `an_export_statement_out_of_form_is_skipped`).
A detected macro whose export entry agrees on schema, name, kind and number of overloads takes
its parameter lists and bodies from there, defaults and all, and is editable like any other
(`a_macros_hidden_defaults_are_read_from_the_export`, over a defaulted macro, a defaulted
overload, a defaulted table macro and their undefaulted twins, a typed one and a nullary one,
and that no scratch database is left attached; `a_macro_read_with_its_defaults_replays_with_them`,
whose replayed `m(1)` still answers `3`). **Where the export cannot be read, the macro is
marked instead** — a sealed handle, its external access off, fails at the export, and so does any
failed step, or an export with no entry that agrees. Such a macro is
`RoutineInfo::defaults_unread`: listed and droppable, not editable (`is_editable`), its `CREATE`
headed by a `-- DuckDB does not report this macro's parameter defaults …` line so *Copy DDL* and
the dump say what they leave out (`a_hidden_default_the_export_cannot_reach_is_marked`, the same
macros on a sealed handle).
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
(`stop_interrupts_a_running_ddl_statement`, a `CREATE TABLE … AS` counting 10^11 rows). The
table designer's plans reach it now — `ALTER TABLE` one action at a time where the table allows,
the rebuild where it does not (below) — and the view editor's and the macro editor's.
`run_script`
is one connection for the whole file and wraps nothing, with SQLite's runtime-flavour choice between
`block_in_place` and `spawn_blocking`. **A backup is `ATTACH` + `COPY FROM DATABASE` + `DETACH`**
(`backup_to`, behind `Db::backup_to_file`): every schema, table, view, sequence and macro with its
rows, into a **new** file. A target that already exists is refused before anything runs, as
SQLite's `VACUUM INTO` refuses one, because `ATTACH` would open it and copy *into* what it holds
(`a_backup_copies_every_table_with_its_rows`, into `:memory:`). That refusal no longer reaches a
user who chose to replace a file: `Db::backup_to_file` hands it the `.part` sibling and renames
the whole copy over the target (`staged_backup`, *db/overview.md*). `Db::explain` builds `EXPLAIN` or
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
longer on `home` (a `USE` of an attached database would have a bare name mean another file); a
temporary table or view has that name, **qualified or not** — the temporary catalog has a schema
`main` too, which DuckDB searches first, so `SELECT * FROM main.p` read a temporary `p`'s rows
while its columns were attributed to the file's `p`, and a commit rewrote the real row (the check
was once the bare name's alone); for a bare name, `search_path` is set, which has DuckDB resolve it
elsewhere; a qualifier is also a catalog's name (`duckdb_databases()`), which
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
fails into the closure's `ROLLBACK`. **A write, a re-read and a blob read name
`"catalog"."schema"."table"`** (`table_sql`), the catalog being the origin's own — the one
`origins_of` proved the read against. Two-part names were safe on a fresh connection, whose
catalog is the file's, and they were not on a Manual tab's pinned one, which carries whatever the
tab did since the read: after a `USE` of an attached database holding a table of the same name,
or a temporary table of that name (which `main.p` resolves to first), the write landed in the
other table, reported `Ok(1)` and was re-read from it
(`a_manual_grid_write_lands_in_the_table_it_was_read_from`). A three-part name reaches neither —
a temporary table's catalog is `temp`. Key values
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
missing, and a sequence is created starting at the value the source hands out next — and, after
the rows, moved on to it with `setval` where a replay found it already standing behind them, only
ever forward (`dump::sequence_advance_sql`; this said DuckDB had no `setval`, and it does, though
not transactionally). The round trip is tested for real on in-memory instances, through
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
**A user type is carried, ahead of everything** (`DbSchema::user_types`, read by `user_types`
for the dump alone — not shown in the tree, not compared). A table does not need one: the type is
inlined into each column in `duckdb_tables().sql` (`m ENUM('ok', 'sad')`), and `DROP TYPE` does
not refuse a type a table is typed with (measured) — but a view or a macro naming it failed its
restore without it. `duckdb_types()` lists each with an enum's labels and only a `STRUCT` or
`DECIMAL` alias's logical type, so the definition is DuckDB's own printing of the type,
`typeof(CAST(NULL AS t))`; `UserType::create_sql` writes `CREATE TYPE … AS …` and its `COMMENT ON
TYPE`. The file drops it up front where it drops, and otherwise creates it `IF NOT EXISTS`, as a
sequence; a type in a namespace the export does not carry is left out, as a sequence there is.
**So is a view's and a macro's comment**, a `COMMENT ON VIEW` / `COMMENT ON MACRO [TABLE]` after
its `CREATE` (`ddl::script_restates_object_comments`, `view_comment_sql`, `routine_comment_sql`,
in *Copy DDL* too) — the dump dropped both, keeping only a table's and its columns'
(`a_dump_carries_user_types_and_view_and_macro_comments`, into an empty database and replayed
onto its source). **A macro parameter's default is carried where the reader could read it** from
the export, above; where it could not, the file does not carry it, and the macro says so in the
line above its `CREATE`. A macro is dropped up front as what it is, `DROP MACRO [TABLE] IF EXISTS`
(`RoutineInfo::object_keyword`, under `schema.rs`).
**A Manual tab pins one connection, and with it the file** (`Pinned`, opened by `pin`, behind
`session.rs`'s `Backend::DuckDb`). It is the connection an operation would take — `connect`'s, so
the init SQL runs on it — held with its interrupt handle, the file's own catalog read at open
(`home`, for provenance as above) and the `Arc<Instance>` it came off, so the instance does not
close between the tab's statements and **no other program can open the file until the tab goes
back to Auto**. That is the one thing this mode has to tell the user, and the
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
commit of nothing (`commit_after_a_failed_statement_reports_the_abort`). **Not every failure
aborts, though**: a parser error is raised before the transaction is touched (and `prepare_one`
refuses text DuckDB cannot parse before the connection sees it), and folding it to the aborted
state hid Commit over a healthy transaction and left only Rollback, which discarded the tab's
work. So after a failed statement inside a transaction the session asks the same `SELECT 1`
probe, and a live transaction reports the failure as `StmtOutcome::FailedIsolated`, which leaves
it open and committable; a catalog or constraint error still aborts and still says so
(`a_parser_error_leaves_a_manual_transaction_committable`). The tab's statements are
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
a decision, not a gap: the grid commit is not refused inside a transaction, and its failure is
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
**The table designer is on, through `core::ddl`'s DuckDB half** — `duckdb_supports`,
`alters_in_place`, `emit_duckdb` and `duckdb_rebuild_sql`, whose entry under `ddl.rs` has the
measured refusals behind each. What decides most plans is not the change but the table:
DuckDB's `ALTER` refuses to rename, retype or tighten a column, drop one or rename the table once
an index, a key, a check, a generated column or another table's key depends on it, and inside a
transaction a `DROP INDEX` does not release that, so such a table is rebuilt — made under a
working name, filled, the original dropped, renamed into place, its indexes and comments after.
A table another table's key references is refused rather than rebuilt (`DROP TABLE` refuses it,
and there is no `ADD FOREIGN KEY` to put the key back). The reader's half is pinned by
`a_read_table_round_trips_to_no_change` — every kind of column and constraint it models diffs
against its own draft to nothing — and each plan shape is run end to end here, through `diff` (or
`create`) → `emit` → `run_ddl`, the table read back and diffed against the draft until nothing is
left between them: `a_plain_table_alters_in_place` (no copy, one action per `ALTER TABLE`),
`a_keyed_table_is_rebuilt_with_its_rows_indexes_and_comments`,
`constraints_are_added_and_dropped_by_a_rebuild`, `a_referenced_table_is_not_rebuilt`,
`a_rebuild_keeps_a_generated_column`, `a_sequence_default_survives_a_rebuild` and
`a_new_table_is_created_as_drawn`.
**The view editor is on too** — `CREATE [OR REPLACE] VIEW`, `ALTER VIEW … RENAME TO` and `DROP
VIEW`, whose entry under `ddl.rs` has the measurements — and `read_schema` fills a view's
`ViewOptions` with what a replace would lose: the explicit column list, out of
`duckdb_views().sql` through `ddl::view_column_list` as SQLite's reader takes it, and the comment,
which DuckDB's `CREATE OR REPLACE VIEW` clears and the plan restates after it
(`ddl::replace_clears_comment`) — and the column comments, which the replace clears as well, taken
in the `duckdb_columns()` loop for a view's columns into `ViewOptions::column_comments`
(`ddl::view_comments_restated` decides which the plan can put back).
`a_view_edit_keeps_its_column_list_and_comment` runs a body change
and a rename through `diff_view` → `emit` → `run_ddl` and reads back the list, the comment and the
body in DuckDB's own rendering — it re-prints what it stores, so the draft's text is not what reads
back — and `a_view_drops` drops one. `a_view_edit_keeps_its_column_comments` edits a body, drops a
column and renames the view, and reads back the surviving column's comment with the dropped one's
gone, and an explicit list's names carrying theirs;
`a_star_view_edit_says_which_column_comments_it_clears` pins that a `SELECT *` restates none, names the cleared one as a risk, and still applies; and
`a_compare_carries_a_views_column_comments` plans a source's column comment onto the target's view
and finds nothing left to compare after.
**So is the macro editor** — the routine editor over a macro, `CREATE [OR REPLACE] MACRO` and
`DROP MACRO [TABLE]`, whose entry under `ddl.rs` has the measurements. A macro is a *function* to
the model and DuckDB has no procedure, so `duckdb_supports` admits a function's create and
replace alone and the Create menus offer *Function* and not *Procedure*. There is no `ALTER
MACRO`, so a rename is a drop and a create in `run_ddl`'s one transaction; a change between a
scalar and a table macro is too, its drop naming the kind the server holds — a replace across the
two kinds would leave both standing (above). The comment, which a replace clears as a view's does,
is set again after every create and replace. `a_macro_is_created_edited_renamed_and_dropped` runs
the editor's plans end to end through `diff_routine` → `emit` → `run_ddl`: a body replaced with
its comment kept, a table macro turned scalar leaving one `t` with its comment, a rename leaving
no old name, a new table macro, and a drop of each kind. The body reads back in DuckDB's own
rendering (`a * b` as `(a * b)`), as a view's does.
**Schema compare between two files needed no gate of its own.** `compare::comparable` is dialect
equality, so two DuckDB connections always passed it, and the picker's second step lists the one
database a DuckDB connection holds (`fetch_databases`, the file's catalog) — two files are two
connections. What it needed was for the plans a comparison writes to be ones `duckdb_supports`
admits, and two kinds of statement were missing: **schemas** — `CREATE SCHEMA` with no owner
(`AUTHORIZATION` is a syntax error) and `DROP SCHEMA`, never `CASCADE`; a schema takes no comment
here (*not implemented*) — and **sequences**, created, commented and dropped and never altered,
every fact measured on 2.0.0 under `ddl.rs`. Admitting the schema pair turned
`supports_namespace_editing` true with no edit of its own, so the tree's Create menu offers
*Schema* and a namespace row offers **Drop** (`overlays::duckdb_is_offered_what_it_emits`), and
*Sequence* opens PostgreSQL's sequence form less the type, cache and restart DuckDB lacks — a
made one showing its numbers read-only and leaving its comment to edit (`ObjectDraft::blank` and
the `sequence_*` capabilities, under `ddl.rs`). A sequence a column
default uses can be neither dropped nor replaced — DuckDB says so itself (*Dependency Error*),
inside the plan's one transaction — and a sequence whose options differ between the files is
disclosed with the generic *no statement for the difference it found*, its `AlterSequence`
emitting nothing. A source macro whose parameter defaults the catalogue hides is planned with
them, read from the export (above); only where the export could not be read (`defaults_unread`,
above — a sealed handle) is it held back rather than created without them
(`HeldBack::DefaultsUnread`, under `compare.rs`). The plans are run end to end on in-memory
instances — compare, plan, `run_ddl`, compare again: `two_files_compare_the_same_once_the_plan_is_applied`
covers every kind DuckDB holds — a schema created before the table in it, a sequence before the
table whose default draws on it, tables created, altered in place and dropped, views and macros
of both shapes, a sequence comment — and finds nothing left, the target's rows kept and the new
sequence handing out its source's start. `a_compare_rebuilds_what_it_must_and_holds_back_what_it_cannot_restate`
holds the rest: a keyed table rebuilt by the plan with its rows (the widened column cast, the
dropped one gone), a dropped table that used a sequence dropped before the sequence, a view's
comment restated after its replace, the defaulted macro planned with its `b := 2` and nothing in
`omitted()`, and a referenced table's refused rebuild (`Withheld::DuckDbReferenced`) in the
whole plan's `unsupported()` — which keeps Apply off until its tick is cleared, the same gate as
on every engine — while the plan without it applies; a second comparison then finds only that
table left. Before the export was read, the defaulted macro was the second thing left; the
held-back case is now `compare.rs`'s own tests'.
**What DuckDB does not have is refused, never faked**: a procedure, as above, and the trigger
editor, DuckDB having no triggers, through `ddl::duckdb_supports`.
**A missing file is refused, never created** (`open_target`, `DbRefusal::DuckDbMissing`). DuckDB's
open creates one, which for a database *client* is the wrong default by some distance: a mistyped
path would open an empty database that looked like it worked. **Making one is a separate,
deliberate act** — the connection form's *New file*, `schemaic_db::create_duckdb_file` over
`create_file`, as SQLite's is `create_sqlite_file` (*db/sqlite.md*) — and until it existed Schemaic could not start a DuckDB database at all: an empty
file is no way round the refusal, DuckDB calling it "not a valid DuckDB database file".
`create_file` refuses a path that exists (it only ever sees the `.part` sibling `staged_backup` has
cleared, *db/overview.md*), opens it with the app's own `config(false, false)`, so no extension
is loaded, and runs `CHECKPOINT` on a blocking thread, so the file holds a whole, empty catalogue
before the rename puts it in place. No committed test makes one, since that needs a file; it was
checked once on real files by a throwaway test — a new path and a path holding junk each became
a writable database, with no `.part` left — and the staging it rests on is
`a_backup_replaces_its_target_only_with_a_whole_copy`'s. An empty path is `DuckDbNoFile`, and
`:memory:` and a MotherDuck `md:`/`motherduck:` name are `DuckDbNotAFile` — the first a database
that dies with each operation's instance, the second needing an extension this build does not
fetch (`a_missing_file_is_refused_not_created`, `names_that_are_not_a_file_are_refused`, whose
filesystem question is a parameter so no file is touched). **The suite runs on in-memory DuckDB**,
the sanctioned exception in-memory SQLite already is. A `memory:<name>` target resolves in every
build (`open_target`'s `Target::Memory`); what `cfg(test)` decides is only whether one nobody holds
may be *made* — `instance` is `instance_with(db, cfg!(test))`. The suites make theirs and hold each
for the process in `KEEP` (itself `cfg(test)`), since an in-memory instance *is* its data and
letting it close between two operations of one test would empty it. **A release build refuses one
nobody holds** — `DbRefusal::DuckDbSessionClosed(name)` — rather than creating it empty, which is
the missing file's refusal over again: a connection naming a session that has closed would
otherwise open an empty database that looked like it worked
(`a_memory_target_with_no_session_is_refused_outside_the_suites`, which asks `instance_with(…,
false)` from inside the suite). The one thing that makes such a target outside the suites is a
`DataSession`.
**Open data file is a view in a throwaway in-memory session** — `core/data_file.rs` decides the
words, this module holds the database. `DataSession::open` makes an in-memory instance, unsealed
and with the configuration every instance takes, under a fresh `memory:session-<n>` target — a
process-wide counter, so no two sessions of one run share one (`two_sessions_are_two_databases`) —
registers it in `INSTANCES` as any instance is, and keeps the **strong** `Arc` itself. The registry
stays `Weak`, so the file rule — close with the last operation — is untouched; the session is what
an in-memory instance needs instead, since that rule would empty it between two queries. Dropping
the `DataSession` closes the database once any operation still running on it ends, as a file's
instance closes, and the target is refused `DuckDbSessionClosed` from then on
(`a_data_session_holds_its_database_until_it_is_dropped`, a seeded row read back across two
operations and the target refused after the drop). **It is opened unsealed**, as the editor's
handles are, because reading the file is the point; a sealed handle on the target is
`DuckDbSealedElsewhere`, as for any file open the other way, and the unattended paths could not
reach it regardless — the session exists in this process alone, never in `connections.json`
(`Connection::to_save`, under `core/connection.rs`), and `schemaic query` and the AI panel's MCP
server are other processes reading that file. **So the AI panel cannot see a session**: its MCP
server is such a subprocess, and could not share the in-process database if it did.
`open_data_file(db, path)` refuses `DuckDbNotASession` unless `db.file` is a `memory:` target —
checked before anything opens, so a database file the user owns never takes an unpreviewed view —
and `DuckDbNotADataFile(path)` for a path `data_file::kind_of` has no reader for
(`a_data_file_opens_only_into_a_session_and_only_with_a_reader`). It then takes a name free in the
session's `main` (`free_view_name`: `information_schema.tables` for `current_database()` and
`main`, tables and views alike, compared without case —
`a_data_file_view_takes_a_name_free_in_the_session`, where another schema's `events` is no
collision), runs `data_file::create_view_sql` on that operation's connection, and answers
`current_database()` — `memory` for an in-memory database — and the view's name, which together
are the tab's `TableSource`. That `CREATE VIEW` is generated DDL run without the preview, the one
place that is, on the terms stated under *Generated DDL is never run silently* in the invariants.
The view reads the file through DuckDB's reader each time it is queried: nothing is copied in, and
nothing written to the session reaches the file. Measured by the change's author: a CSV, a JSONL
and a Parquet file each read through the view, `parquet` and `json` being linked in, and the views
list in the tree through `fetch_table_list`. No test reads a real data file, the suite touching
none. The app's half — when a file joins the active session rather than opening a new one, and
what closing one takes with it — is under `app/main.rs`.
**`duckdb.rs` answers every name in `ENGINE_ENTRY_POINTS`**, and the dispatcher calls it for each — and all three convention tests hold it to that:
it is in the module list of `every_engine_module_answers_the_whole_interface` and the loop of
`the_dispatcher_calls_every_engine_module_for_every_entry_point`, and
`the_entry_point_list_is_what_the_dispatcher_actually_dispatches` intersects all five modules.
