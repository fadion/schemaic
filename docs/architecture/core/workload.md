# `core/workload.rs`

`workload.rs` — the server-wide half of the performance picture, beside `stats.rs`'s per-table
one (`core/ui-state-models.md`): which statements a server has spent most on, which indexes
nothing reads or another index already covers, which tables and indexes are largest, and — on
SQL Server — which indexes the optimiser recorded as missing. It is the backend of the roadmap's
*Workload and performance insight*, and it has three front ends: the MCP server's four
performance tools (`cli/mcp.rs`, rendered by `cli/perf.rs`), the headless `schemaic statements`,
`indexes`, `sizes` and `locks`, which print those same tables (`cli/run.md`), and the app's
*Performance* view (`ui/perf_view.rs`, fetched by the app's `perf_fetch`). **All three rank with
these functions rather than a second time**, so an agent, a script and a person looking at one
server are told the same thing.
**Statements.** `StatementStat` is one normalised statement — `query`, `database` where the
source records one, `calls`, `total_ms`, `mean_ms`, and `rows` where the source counts them — and
`StatementOrder` (`Total`, the default; `Mean`; `Calls`) is the ranking, `name`/`parse` being the
stable spelling the MCP `order` argument and `schemaic statements --order` take
(`an_order_round_trips_through_its_name`).
`statement_limit` clamps a requested count into `1..=MAX_STATEMENT_LIMIT` (500), `None` being
`DEFAULT_STATEMENT_LIMIT` (50), so an agent asking for "all" cannot make a fetch carry every digest
a busy server has seen. The text is data to show and never to run — and it is *data* in the gating
sense too: PostgreSQL and MySQL replace literals with placeholders, but Query Store keeps a
statement as it was sent, which is why `top_statements` is a row-data tool (`cli/mcp.md`).
**An absent source is said, never drawn as an empty list**, which is the roadmap's trap and the
reason `StatementReport` is `Ready { statements, since }` or `Unavailable(Unavailable)` rather
than a `Vec`: an empty table reads as "nothing is slow". `since` is when the counters started, as
the server renders it, where the source records it — every figure is "since then". `Unavailable`
names which source is missing: `NotSupported` (SQLite, which keeps no statement history),
`ExtensionNotInstalled` and `ExtensionNotLoaded` (PostgreSQL's `pg_stat_statements`, absent, or
present but not in `shared_preload_libraries` and so collecting nothing), `PerformanceSchemaOff`
and `DigestConsumerOff` (MySQL and MariaDB, where it is off by default on MariaDB), `QueryStoreOff`
and `QueryStoreError` (SQL Server, opt-in per database), `QueryStoreMissing` (SQL Server 2014 and
earlier, which predate Query Store), and `NoPermission` carrying the engine's
own words. `reason()` is the English; `enable_sql()` is the statement that turns the source on
**only where one statement does** — `CREATE EXTENSION pg_stat_statements;`, the
`setup_consumers` `UPDATE`, `ALTER DATABASE CURRENT SET QUERY_STORE = ON;` — and `None` where it
takes a configuration change and a restart, or a grant only an administrator can make
(`only_a_one_statement_fix_offers_its_statement`). It is shown for the user to run; nothing in
Schemaic runs it. Four classifiers keep each engine's read from deciding this inline.
`classify_pg_statements_error` turns an error naming `shared_preload_libraries` into
`ExtensionNotLoaded` and a permission denial into `NoPermission`, and leaves anything else an
error (`pg_errors_are_classified_only_when_they_mean_the_source_is_off`).
`mysql_digest_availability` reads `@@performance_schema` and the `statements_digest` consumer's
`ENABLED`, and **an unreadable consumer row is not a disabled one** — `None` there means try the
read (`mysql_digests_need_the_schema_and_the_consumer`). `query_store_availability` reads
`actual_state_desc`: `READ_ONLY` is readable, since it still holds the history it gathered, and
no row at all is a database where Query Store was never configured, so off
(`query_store_is_readable_read_only_and_off_without_a_row`). `classify_query_store_error` reads
the error that state read gives a server older than Query Store, which came with 2016 and so has
no `sys.database_query_store_options`: *Invalid object name* naming that view is
`QueryStoreMissing`, with no `enable_sql`, since no statement adds Query Store to a server that
lacks it; an invalid object name naming anything else, or any other error, stays one
(`a_server_older_than_query_store_says_so_rather_than_failing`). Before it, such a server answered
an error — exit 1 from `schemaic statements`, `isError` from the MCP tool, a red error in the view
— where every other absent source is said. **One more decision is the extension's, not the
server's**: `pg_statements_has_exec_columns(extversion)` says whether `pg_stat_statements` names
its timings `total_exec_time`/`mean_exec_time` — from the extension's 1.8 on — or 1.7's
`total_time`/`mean_time`, and a version it cannot read is taken as a current one. The rename
shipped with PostgreSQL 13, which is why the server's `server_version_num` was asked first and
why that was wrong: a cluster brought up by `pg_upgrade` keeps the extension objects it had until
someone runs `ALTER EXTENSION … UPDATE`, so a 16 server with extension 1.7 failed with *column
s.total_exec_time does not exist* rather than listing
(`the_statement_columns_follow_the_extensions_version_not_the_servers`). The capabilities are
`supports_statement_stats`, false on SQLite alone, and `supports_missing_index_hints`, true on SQL
Server alone — PostgreSQL and MySQL record nothing of the kind, and inferring one from the
statement list would be guessing.
**Index health adds no catalogue read of its own.** `index_health(tables, stats)` sweeps the
`SchemaStats` that `Db::fetch_table_stats` already returns and the index definitions
`fetch_schema` already holds, and reports each `IndexFinding` with one `IndexProblem`. `Unused` is
`IndexStats::is_unused` — a *counted* zero, never a primary key or a unique index — with four
exclusions. An unused index **backing a foreign key** is left out, because MySQL requires one and
refuses to drop it while the key stands, so naming it is advice the server will refuse
(`an_unused_index_backing_a_foreign_key_is_left_alone`). **A clustered index**
(`IndexInfo::clustered == Some(true)`, SQL Server's `CLUSTERED`, through `is_clustered`) is never
reported at all — not unused, not a duplicate, not redundant — because it is the table itself in
key order: dropping it leaves a heap and rebuilds every other index, and SQL Server counts its
seeks like any other index's, so a table that is only ever written had its clustered index
reported unused, with the Performance view's *Drop index* on the row
(`a_clustered_index_is_never_reported_unused`). An index already reported as a
duplicate or a prefix is **reported once, as the overlap**: that finding holds whatever the
counters say, and two rows for one index read as two problems
(`an_unread_duplicate_is_reported_once_as_the_duplicate`). And **the index a finding names as its
`of` or `by` is never reported unused**: the two rows were each sound alone and, both followed,
dropped every index on the column
(`the_index_a_finding_names_as_its_stand_in_is_not_reported_unused`). **Usage decides which of a
pair is named**, which is what keeps that last exclusion from hiding the right advice:
`table_overlaps` takes an `idle` predicate — would this index be reported unused on its own count,
`is_unused` and neither backing a foreign key nor clustered — and asks it below wherever it chooses
between two indexes. `Duplicate { of }` is the same key
compared `IndexColumn` by `IndexColumn` — so a prefix length, a direction, an expression or a
collation is a difference — with the same predicate, the same method (`btree` named and unnamed
being one, since PostgreSQL names the default MySQL leaves unset), the same `INCLUDE` set in any
order and the same `using`. Of a group, the one kept is ranked by `keeper_rank`: the primary key,
then the clustered index, then a unique or constraint-backed index (dropping either changes what
the table accepts), then one backing a foreign key, then **one something reads over an idle one**,
then the first by name. The name alone decided it before the clustered and read ranks were there,
and both went wrong the same way: `IX_audit_created` sorts before `cx_audit` byte-wise, so a
nonclustered twin was kept and the clustered index reported as its duplicate
(`of_a_clustered_and_a_nonclustered_twin_the_clustered_one_is_kept`), and of twins `ix_a` (unread)
and `ix_a2` (read) the read one was reported and the unread one then reported unused
(`of_twins_the_read_one_is_kept_whatever_their_names`). **A unique index is never reported against
a plain twin**, since dropping it would loosen the table
(`a_unique_index_is_never_reported_against_a_plain_twin`). `Redundant { by }` is a strict
leading prefix of a wider index, both plain B-trees over every row — no predicate, the default
method — and the narrow one neither unique, a primary key, clustered nor carrying `INCLUDE`
columns: the wider index enforces no uniqueness on fewer columns and may not carry what was
included, and a nonclustered index does not serve every lookup the table's own row order does
(`a_clustered_index_is_not_covered_by_a_wider_nonclustered_one`). It names a **read** wider index
over an idle one, then the **shortest**, the closest stand-in
(`a_leading_prefix_is_covered_by_the_shortest_wider_index`) — and **a read prefix is not called
redundant when the only stand-in is idle**: the advice there is to drop the wide index nothing
reads, which the unused finding gives, where `ix_a` (read) beside `ix_ab` (unread) used to come out
as `ix_ab` unused *and* `ix_a` redundant by `ix_ab`
(`a_read_index_is_not_called_redundant_beside_an_unread_wider_one`). With neither read, the prefix
is still the redundant one and its stand-in stays. Three kinds of index are never
compared at all (`comparable`): one read only in part (`IndexInfo::lossy` — an operator class or
a `NULLS` ordering it could not see may be the whole difference, `a_lossy_index_is_never_compared`),
SQL Server's XML and spatial kinds, and a partition's inherited copy of its parent's index, which
goes when the parent's does. Views are skipped: a PostgreSQL materialised view's index costs its
refresh, a different question. Findings come unused, then duplicate, then redundant, largest
first within each, because size is what a drop buys back.
**What "unused" can mean is the caller's to say, and `usage_counted` is what lets it.** It is
true when any index in the stats carries a scan count at all. With none counted —
`performance_schema` off or ungranted on MySQL, the collector off on PostgreSQL, and always on
SQLite, which counts nothing — an empty unused list means nobody could say, not that every index
is read (`usage_is_counted_only_when_some_index_has_a_figure`). `largest(stats, n)` ranks tables
by `TableStats::total_bytes`, a table with no figure left out, and indexes by their own bytes
**only where the engine reports a size per index**: MySQL reports one figure for all of a table's
indexes, so its index list is empty rather than guessed
(`largest_ranks_tables_by_total_and_skips_unreported_index_sizes`).
**Missing indexes are SQL Server's alone.** `MissingIndex` is one row of the
`sys.dm_db_missing_index_*` views since the last restart: the equality columns and then the
inequality columns as the key — the order the DMV's documentation gives — the included columns,
the uses, the average cost and the estimated impact. `score` is uses × cost × impact/100, a
relative figure for ordering and not a time, and `rank_missing` sorts by it and keeps `n`.
`parse_bracketed_list` reads the DMV's `[a], [b]]c]` lists, undoubling a `]` inside a name, a
NULL list being no columns. `suggested_name` is `IX_<table>_<key columns>`, cut to SQL Server's
128-character limit on a character boundary (`a_long_suggested_name_is_cut_to_128_characters`).
**A suggestion becomes DDL by the path every index edit takes, never a hand-written `CREATE
INDEX`.** `index_info` states it as an `IndexInfo` — non-clustered, so the differ writes T-SQL's
plain form — and `suggestion_ddl(table, m, target)` pushes it onto `TableDraft::from_table(table)`
as an `IndexDraft::new` and returns `ddl::diff`'s plan as `ChangeSet::editor_script`, so
*generated DDL is never emitted from a second differ* holds here too (`invariants.md`). It is the
**reader's script, not the raw statements**: an `emit()` joined by the caller would drop the
*INCOMPLETE* header of a plan the emitter could only partly express, and the batches a client
needs, which is the `client_script` rule. It is text to propose; nothing in this module or
its callers runs it (`a_suggestion_becomes_the_differs_create_index`, which asserts `CREATE INDEX
[IX_Orders_CustomerID] ON [dbo].[Orders] ([CustomerID])` and its `INCLUDE ([Total])`). **The
edit itself is `MissingIndex::add_to`**, which pushes `index_info()` onto a draft as an
`IndexDraft::new`: `suggestion_ddl` calls it, and so does the app's *Create index*, which wants
the draft edit for the preview rather than a script — one decision of how a suggestion becomes an
index, two front ends (`ui/perf_view.md`). `index_sizes_per_table_only` is the other question a
size list needs answered: an empty index ranking means "this engine sizes indexes per table"
only when some table has index bytes and no index has a figure of its own, and otherwise that
there are no indexes (`only_a_table_wide_index_figure_means_sizes_are_per_table`). Verified
live by the change's author: duplicate and redundant detection on MariaDB 10.11, and that same
statement from a real SQL Server's DMVs through `index_health`.
