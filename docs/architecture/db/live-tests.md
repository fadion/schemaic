# `tests/live/` — the DB layer against real servers

**`tests/live/` is the DB layer against real servers**, and it exists because the pure suite can
only reach the *decisions*: SQLite is testable directly (in-memory, shared-cache), so it is the
one backend whose wire layer was covered at all, while MySQL, MariaDB and PostgreSQL — the
engines that ship most — were covered by hand. `main.rs` holds a macro that expands **one** suite
(`suite.rs`) into a module per server, so a claim cannot hold on MySQL and quietly rot on
PostgreSQL, and a failure reads `mysql::introspection_finds_the_seeded_table` rather than a loop
that stopped at the first leg. What genuinely differs between servers is read off the leg's
`Target` (`namespace`) rather than branched on the engine — the same reason production code asks
a capability instead of comparing a dialect. **MariaDB is a leg of its own**, not a MySQL
stand-in: the divergences are in exactly what this crate reads, which is how a MySQL 8
`CHECK_CLAUSE` escaping quirk once hid behind a MariaDB that returned runnable text.
**`pg_catalog.rs` and `mariadb_catalog.rs` are two of the three modules outside the macro, and
of the three that name an engine** (`mssql.rs`, below, is the third). `pg_catalog.rs` holds `core::pg_builtins::PG_FUNCTIONS` to what the server under
test reports — every name the server has is in the catalog, the overhang is exactly the 24
grammar-only forms (`over_listing`), and those 24 are really in the catalog
(`the_grammar_forms_are_in_the_catalog`, so the allowance cannot outlive what it was granted for).
**Its fourth test holds the offered subset to the same server**:
`the_offered_subset_is_still_what_the_filter_answers` carries the plumbing cut as SQL
(`SUGGEST_ORACLE`) and asserts `PG_SUGGESTED` equals that query's answer unioned with
`GRAMMAR_ONLY`, in **both** directions — a name the filter keeps and the list lacks is one
autocomplete will never suggest, a name the list carries and the filter no longer keeps is the
popup offering this server's plumbing. The query text lives in the test rather than beside
`PG_SUGGESTED` for the same reason `ORACLE` does: a generated file checked by re-running the
generator against itself checks nothing. It carries a row-count floor (>500) on the same
reasoning as the others, and it was watched fail — deleting `btrim` from the list produced
``pg at 127.0.0.1:5432 as schemaic offers 1 name(s) `PG_SUGGESTED` does not … ["btrim"]``.
**Its fifth is about the catalog the app reads off the *server*, not the one it ships.**
`an_installed_extensions_functions_are_names_the_checker_knows` creates `citext`, `intarray`,
`cube`, `earthdistance`, `tablefunc` and `hstore` in the tier's own scratch database and holds
`fetch_schema` to putting their names in `DbSchema::extension_routines` — the half only a real
server can answer, the unit half being that a name in that field is exempt. It asserts the
premise first (>100 names came back, or every assertion after it is vacuous), then that `sort`,
`icount` and `earth_distance` are there and **stayed out of `routines`**, so the fix cannot drift
into undoing what `routine_filter` exists for, and finally that not one of the server's extension
names is squiggled.
The tier's rule still stands and this is not a hole in it: the subject of each is one engine's
*data file* rather than a claim about how the DB layer behaves, and there is no version of "is
PostgreSQL's builtin list complete" MariaDB could answer. Each carries its own `enabled()` check
for its own target, which is what the macro would otherwise have given it.
**`mariadb_catalog.rs` is that rule's second instance, for `intel::FUNCTIONS`** — the catalog the
checker trusts on MySQL and MariaDB, and the one that had no oracle of any kind until this module.
Its design point is **three oracles rather than one plus a hand-written excuse**:
`information_schema.SQL_FUNCTIONS` (MariaDB 10.11+, 261 rows on 10.11.14) lists only the builtins
the parser resolves through its function-creator hash, so every builtin spelled as its own grammar
rule — `LEFT`, `IF`, `AVG`, `CURRENT_DATE`, the `COLUMN_*` dynamic-column forms — is absent from
it. Those turn out to be exactly MariaDB's *reserved words*, so `information_schema.KEYWORDS` (696
rows) accounts for them without anybody writing a list. **This paragraph then said the union of
the two views is what the server claims, and it is not.** A builtin registered natively and
resolved by its own path is in *neither*: `ST_Area` is absent from `SQL_FUNCTIONS` while the same
server answers `SELECT st_area()` with *"incorrect parameter count in the call to **native
function** 'st_area'"*, the server's own words for a builtin. The entire OGC spatial family sat in
that gap, so `ST_Length` was squiggling under correct SQL on the default engine while every test
in this file was green. The third oracle is therefore the **parser**, which both servers have and
which answers the question the editor actually asks — can this server call this name — and
`over_listing` measures the catalog against all three. That is the deliberate contrast with
`pg_catalog`, whose
over-listing direction needs the hand-written `GRAMMAR_ONLY` and its 24 names: a list in a test
file is a second thing that has to be kept true, and here the server keeps it. Two small
hand-written lists do remain, each held by a test of its own. `MYSQL_ONLY` is the five names MySQL
8 has and MariaDB never got (`bin_to_uuid`, `is_uuid`, `json_storage_size`, `regexp_like`,
`uuid_to_bin`), carried because `FUNCTIONS` serves both engines, and
`the_mysql_only_names_are_in_the_catalog` keeps the excuse from outliving what it excuses. It is
this module's own lower-cased copy, and `intel::MYSQL_ONLY` now holds the same five for a
different job — the offer filter rather than the oracle's allowance — re-measured against both
servers by the parser test below.
`NOT_CALLABLE` is one name, `schemas`, which `SQL_FUNCTIONS` reports and the parser will not accept
as a call because it is the reserved word of `SHOW SCHEMAS` — a claim about the parser, so
`the_uncallable_name_is_still_uncallable` *executes* `SELECT schemas()` and requires a syntax
error, and requires the name absent from `FUNCTIONS` as well, so the completion popup cannot come
to offer something that will not parse. Both oracles carry a row-count floor, for the reason the
whole tier exists: an empty result would pass the missing-name test having asserted nothing.
**The partition behind `over_listing` is computed there now, where the module doc used to state
it as a number**: "39 names covered by `KEYWORDS`, and the five left over are
`MYSQL_ONLY` to a name" does not add up to the forty-nine it was partitioning — the real figure is
44 — and nothing computed it, which is the whole argument against writing one down. It was also a
*two*-way split of a **three**-way set: the names `SQL_FUNCTIONS` misses are those with their own
grammar rule (MariaDB's reserved words, so in `KEYWORDS`), MySQL 8's (`MYSQL_ONLY`), and the
natively-registered ones neither view reports, which only the parser can vouch for. The test sums
all three against what `SQL_FUNCTIONS` leaves out and asserts none of them is empty, so a
release that moves the split fails there rather than leaving a sentence that reads plausibly and
is false. **The `KEYWORDS` excuse is put to the parser as well**, because reserved-word-ness does
not imply callability and this half was using it as if it did: 696 keywords, 681 of them not in
`SQL_FUNCTIONS`, standing as a blanket excuse for the 44 catalog names that need one. Add
`f("TABLESPACE", …)` to `intel::FUNCTIONS` and the first assertion never sees it while the popup
offers a name that completes to a syntax error. The two are distinguishable: a name with its own
grammar rule answers `1064` to `NAME(1,2)` because `(1,2)` is not its syntax — measured, `CAST`,
`COUNT`, `IF`, `PARTITION` and `SAVEPOINT` all do — while an invented one answers `1630`, which is
measured to be exactly what `TABLESPACE` gives.
**What counts as "no such function" is one rule now, `is_not_callable`, because two tests were
asking it and only one had it right.** `1305` *and* `1630` are both absence — MariaDB uses the
second for a name whose shape reaches builtin resolution and finds nothing (`AUTO_INCREMENT`,
`GOTO`, `JSON_TABLE`, `MERGE`, `RESTART` all answer it), and reading only `1305` credits the
server with five functions it has not got. `1064` is the third answer and separates a builtin from
a *type* or a reserved word: `mysql.help_topic` indexes `DATETIME`, `BEGIN` and `CALL` beside the
functions, and MySQL 8.4 answers 1064 for `SRID` and `CONTAINS`, which it keeps as reserved words
after removing the functions. A name that cannot be typed as a call cannot be squiggled as one
either — and must not be offered.
**"MySQL is unguarded here" stood in this paragraph as a stated limitation and was a live defect.**
It is true that MySQL ships no `SQL_FUNCTIONS` (`mysql.func` holds loadable UDFs, not builtins), so
there is no *catalogue* oracle to write — and that is not the only kind.
`each_server_is_only_credited_with_the_builtins_it_really_has` is the missing leg and **the parser
is its oracle**, which is the better one because it answers the question the editor actually asks:
can this server call this name. It walks every name in `FUNCTIONS` on **both** servers, one
`SELECT <name>(1,2)` each, and asserts both directions — a
name the server lacks that nothing in `intel` withholds, and a name `intel` withholds that the
server does have — plus that the pass found something, so it cannot pass vacuously. Until it
landed, `over_listing` was green **by construction** over the forty-nine names a MySQL 8 tab was
being offered and cannot call. **It reads three outcomes, not two**, and the third is why: `1305`
or `1630` is absence, anything else is "has it, called wrongly", *except* `1064`, which this probe
cannot read either way. `COUNT(1,2)`, `CAST(1,2)`, `EXTRACT(1,2)` and the whole grammar-rule
family answer it because `(1,2)` is not their syntax, and so do `SRID(…)` and `CONTAINS(…)` on
MySQL 8.4, which removed the functions and kept the words reserved — measured, `SRID(1)`,
`SRID(ST_GeomFromText(…))` and `CONTAINS(1)` are all 1064 there while `COUNT(1)` is not, so no
arity makes them parse. Calling 1064 "present" made the test demand MySQL be offered two names it
cannot type; calling it "absent" made it demand MariaDB's `COUNT` be withheld; so it is neither,
and the withheld-but-present assertion skips it. Two load-bearing details: it runs over **one
pinned `Session`**, the documented exception to one-connection-per-operation, because a
`Db::fetch_query` per name would open a connection per name; and it scopes that session to the
`mysql` database, because an
unqualified unknown name with no database selected answers `1046 No database selected` instead of
1305 and would report every name as present. It runs in 0.2 s and was watched failing — deleting
`NVL` from `MARIADB_ONLY` gives ``mysql … has no such function, and nothing in `intel` withholds
it — so a tab on this server is offered 1 name(s) it cannot call: ["NVL"]``.
**`no_name_this_server_can_call_is_squiggled_under_correct_sql` is the completeness direction
asked of the thing that actually goes wrong**, and it is the leg the spatial families needed.
`every_function_the_server_reports_is_in_the_catalog` asks `SQL_FUNCTIONS`, whose own constant doc
called it "every builtin the parser registers by name" and which is nothing of the sort; the OGC
family sat in that gap and `ST_Length` squiggled under correct SQL while this file was green, the
other eighty being silent only because no catalogued name happened to sit within a near-miss of
them, which is luck rather than a guard. So this one asks the **name source the server ships** —
`mysql.help_topic`, which both engines have — filters it through the parser, and drives each
survivor through `intel::diagnostics`. That last step is the point, and it is the only test
anywhere that composes the catalog with the checker that reads it: a name absent from `FUNCTIONS`
is a *defect* only when it produces a warning under correct SQL, and adding an unrelated entry
that brings a real builtin within edit distance fails here where no membership test can see it.
**`mssql.rs` is SQL Server's whole leg, and it is outside the macro for the opposite reason**:
not because its subject is a data file but because the shared suite writes rows back, applies
DDL and pins sessions, and SQL Server applies only a fraction of the second.
So it tests what the engine does do, on its own terms — ping and the database list, every type
`cell_value` renders against the text SQL Server's tools print, `SELECT *` provenance, the
read-only rollback (`a_read_only_session_rolls_back_what_a_select_hides`: a `SELECT … INTO`
under `Enforce::ReadOnly` reports its two rows and leaves no table behind, and the same guard
over a batch leaves a `DELETE`'s rows in place), validation that names a missing table and runs
nothing, a script split at `GO`, introspection, a table's generated DDL rebuilding the table it
was read from (`a_tables_ddl_rebuilds_the_table_it_was_read_from`), a cancel (of a running
statement, and of one still compiling behind another session's lock), activity (the poll's own
session absent from it, and `a_lock_wait_lists_the_waiter_and_its_holder`: a pinned `Session`
holds a row in an open transaction and a spawned `UPDATE` of it blocks — pinned rather than a
holder batch on a `WAITFOR` timer, which could roll back before a loaded machine's poll saw the
wait, and did. `fetch_locks` is polled, up to a hundred times at 100 ms, until a waiting lock is
blocked by `holder.server_id()`; then the wait must lead, the holder must have a granted,
killable lock on the table, and `contended_only` must be false. The table is accepted as
`.dbo.t` *or* as `": object "` — the poll's nameless fallback, which another session's DDL
elsewhere puts it on, and a concurrent `the_lock_list_answers_while_ddl_holds_a_table` does), the
grid's write-back (seven tests, from
`a_staged_edit_lands_on_its_row_and_reads_back` to `a_stopped_commit_is_undone`, under
`mssql.rs` above), `run_ddl` and the designer's plans (from a designed table and a
failing plan rolled back whole to an existing table's edit landing as drafted, an edit keeping
what it did not change, an identity toggle withheld over an index the rebuild cannot restate and
one rebuilding around an index's included columns, a
table rebuilt three ways and refused once by the rebuild's guard, then by each of its arms in
turn (`every_arm_of_the_rebuild_guard_refuses_its_table`, twenty-one tables, under the rebuild in
`ddl.rs` above), an in-place change refused by
its own guard over a masked or sparse column and over a dependent a retype would re-create
without what it carries, thirteen cases, and over a rebuilt computed column carrying a permission
(`a_masked_or_sparse_column_is_not_altered_in_place`,
`a_retype_is_refused_where_a_dependent_carries_what_it_would_drop`,
`a_rebuilt_computed_column_is_refused_where_it_carries_what_the_drop_takes`) while a retype under
a disabled or untrusted key applies and keeps both states
(`a_retype_under_a_disabled_or_untrusted_key_keeps_its_state`), the views and inline function
that select `*` from a table refreshed after a rebuild, a rebuilt computed
column and a dropped column (`a_rebuild_refreshes_the_views_that_select_star_from_it` and its two
siblings), a clustered index and a nonclustered key
keeping their clustering and every
AdventureWorksLT table round-tripping where that sample is installed, also under `mssql.rs`),
a file import (four tests, from every row across batches to a cancel rolled back, under
`mssql.rs` too), Manual mode on a pinned session (six tests, from
`a_manual_session_commits_only_when_told` to `a_read_only_connection_is_refused_a_manual_session`,
under `mssql.rs` as well), and the
three catalogues only it has — `every_allowlisted_function_is_a_builtin` (the read gate's lists,
by error 195), `every_builtin_snippet_runs`, and `every_catalogued_builtin_is_one_the_server_knows`
(`core::mssql_builtins`, by the same error 195 — 208 for a rowset name in `FROM` — in the
over-listing direction only, and version-aware, under `mssql_builtins.rs`). It is not a `Target`, so `endpoint.rs` carries
`OUTSIDE_THE_SUITE` for it, `leg_enabled` to answer `SCHEMAIC_IT_ENGINES` for a leg with no
`Target`, and `note_leg_skipped` to say so when it is left out; its endpoint is
`SCHEMAIC_IT_MSSQL_HOST`/`_PORT`/`_USER`/`_PASSWORD` (defaults `127.0.0.1`/`1433`/`sa`/
`Schemaic_2026`, the local container's and CI's `mcr.microsoft.com/mssql/server:2022-latest`
service's). **The leg runs twice in CI**, the second time against a `mssql2025` service
(`mcr.microsoft.com/mssql/server:2025-latest` on 1434) in its own *Test (live, SQL Server 2025)*
step — `--test live -- mssql::` under `SCHEMAIC_IT_MSSQL_PORT=1434`, the name filter rather
than `SCHEMAIC_IT_ENGINES` keeping it to that leg, since the tier refuses the variable in CI
(`endpoint::engines_var`: a leg it leaves out reports as a pass). The builtin
catalog spans both versions, and each direction of its version line needs a server that can
answer it; locally a `schemaic-mssql25` container on 1434 plays the same part. The whole leg is
green on 2025 (17.0.5005.3, RTM-CU9). When enough of the suite answers, it joins the macro.
**The two sign-ins that are not a password have a leg each, and both are opt-in**, because no CI
runner has either endpoint and no container can be one. `mssql-windows` is
`a_windows_sign_in_is_this_processs_own_identity` (`cfg(windows)`), against
`SCHEMAIC_IT_MSSQL_WINDOWS_HOST`/`_PORT`, by default `127.0.0.1`/`1435` — a local SQL Server 2025
Developer instance moved off 1433, which WSL's container holds: `SUSER_SNAME()` is
`USERDOMAIN\USERNAME`, `sys.dm_exec_connections.auth_scheme` is `NTLM` or `KERBEROS`, and the same
connection in `Password` mode is refused with 18456, which is what makes the sign-in the mode's
doing. `mssql-azure` is `an_entra_sign_in_is_the_azure_clis_user`, against
`SCHEMAIC_IT_MSSQL_AZURE_HOST` — required, with no default, it being somebody's own server — and
`_DATABASE` (`schemaic_it`), under `VerifyFull` since Azure's certificates are public: the login is
an Entra UPN (it contains `@`), a second connection reads the same `ORIGINAL_LOGIN()` off the
cached token, and a SQL login is refused on the Entra-only server. The second connection's time is
printed rather than asserted — about 0.4 s where it was run, against the CLI's second or two. Both
were green locally when they landed.
**`mssql-on-azure` is the `mssql` leg's own tests run against Azure SQL Database**, opt-in like
the two above and in place of the local server when `SCHEMAIC_IT_ENGINES` names it (`on_azure`,
which `base_db` and `enabled` both ask). It cannot make a scratch database per test, because on
Azure a `CREATE DATABASE` is a new *billable* database outside the free offer. So
`Scratch::create` hands off to `Scratch::azure`, which takes the one existing database —
`SCHEMAIC_IT_MSSQL_AZURE_HOST`/`_DATABASE`, default `schemaic_it`, the Entra leg's — signed in
through the Azure CLI (`azure_base_db`, `VerifyFull`), and gives it to one test at a time: a
process-wide tokio mutex, `AZURE_TURN`, is locked for as long as the scratch's `_turn` field
lives (tokio's because it neither poisons on a panicking test nor makes the scratch `!Send`).
The database is emptied both before a test — a run stopped mid-test leaves its tables behind —
and in `Drop`, by the `WIPE` batch, which tries every drop (system versioning off, foreign
keys, database triggers, objects, types, XML schema collections, schemas, users, roles),
swallows each failure and repeats pass by pass while anything is left — dependency order found
by trying rather than worked out — then after ten passes fails naming what survived; it also
turns a database-scoped `PREVIEW_FEATURES` back off. Since that empties a whole database, it
has its own name guard, `assert_azure_scratch`, which allows only `schemaic_it` or the tier's
`schemaic_it_*`, checked on the way in and again on the way out. **A test may hold only one
Azure scratch at a time**: a second `Scratch::create` waits for the turn its own first holds,
and hangs that test and every one queued behind it — which is what
`a_tables_ddl_rebuilds_the_table_it_was_read_from` did with its second database, so on Azure it
drops the original and rebuilds it in place. What the one database cannot host says so through
`azure_cannot` (`note_leg_no_op`) and asserts nothing: `a_dump_restores_into_an_empty_database`,
which needs a second database, and the login round trip (above, under `mssql.rs`).
`a_contained_user_is_created_signs_in_and_is_reset` runs there without its sign-ins, the server
being Entra-only, and prints that it left them unchecked — its `listed.scope.contained` is what
pins `fetch_principals`' Azure containment, and `the_account_scope_is_read_with_the_list` the
rest of that arm. `every_catalogued_builtin_is_one_the_server_knows` counts
`EngineEdition` 5 as having 2025's block, since Azure reports major version 12 whatever it runs.
The leg was 56 of 56 there when it landed, as on local 2022 and 2025.
**`endpoint.rs` is where a leg comes from**, and it is the whole environment contract: three
`SCHEMAIC_IT_<ENGINE>_HOST`/`_PORT`/`_USER`/`_PASSWORD` groups with localhost defaults (four with
SQL Server's), plus `SCHEMAIC_IT_ENGINES` as the one way to run fewer than all of them. An *unreachable* endpoint is a
hard failure rather than a skip — a tier that green-lights by not running is the failure mode
this whole directory exists to avoid — so narrowing it costs a developer a deliberate sentence
(`SCHEMAIC_IT_ENGINES=mariadb,pg`), and CI refuses that variable outright. **The one exception to
"every leg runs" is `opt_in_leg_enabled`**, which answers yes only when `SCHEMAIC_IT_ENGINES`
names the leg — the two sign-in legs above and `mssql-on-azure`, listed in `OUTSIDE_THE_SUITE`
beside `mssql` so the variable accepts their names. Left out, such a leg says so through
`note_leg_skipped` like any other. The cost is named rather than hidden: CI runs none of the
three, so both sign-ins, and everything Azure SQL Database answers differently, are covered only
where somebody runs them by hand.
**The type matrix (`cases.rs`) is what the tier is for.** A value's journey from a column to a
cell is decided by the driver, the wire protocol and this crate's decoding together, and `core`'s
tests can only assert what `Value` does with text it is *given*. Each case is a column type, a
literal and the text the grid must show; each is asserted twice — that it renders, and that the
same text written back through `export::sql_literal` reads back identically. The second is the one
that bites, because a lossy rendering *looks* right: reintroducing the historical `bit_cell`
defect (a bit-field's number wrapped in `Value::Str` rather than `Value::UInt`) leaves the
rendering matrix green — `"170"` and `170` are the same cell — and fails the write back on both
MySQL legs with `ERROR 1406: Data too long`, which is exactly how that bug reached a release. A
case with no expected text (`timestamptz`, whose rendering follows the server's `TimeZone`) still
asserts the NULL row and the write back; both matrix tests assert how many cases ran, since a
matrix that walked nothing at all is otherwise a pair of passes. **That is now an equality against
the leg's own count, not a floor.** It was one `TYPE_CASE_FLOOR = 20` across legs of 21 and 25, so
PostgreSQL could lose its entire numeric family and both matrix tests would still pass — the exact
failure the floor was added to catch, one size down. The expected count is derived from the leg's
own slices (`Target::type_cases`), so it needs no maintenance and there is nothing to be gained by
weakening it back.
**The write-back half is about `export::sql_literal`, and until recently not one case held a `'`
or a `\`** — the two characters that quoter's *only* per-dialect branch is about. Whether the
escaping this crate emits is what the server accepts is a fact about the server
(`NO_BACKSLASH_ESCAPES` inverts the MySQL arm, `standard_conforming_strings` the PostgreSQL one),
so no pure test can reach it, and the pure tests pin the string `sql_literal` produces while
nothing pinned that a server round-trips it. Four cases, two per family, both halves watched
failing: deleting the backslash doubling from the MySQL arm gives
`text_backslash … showed "a\b", wrote 'a\b', read back Some("a\u{8}")` on MariaDB and MySQL with
PostgreSQL unaffected, and deleting the `''` doubling from the PostgreSQL/SQLite arm gives
`text_quote … writing 'it's' back failed: syntax error at or near "s"`. The per-leg
`expected_cases`/`expected_writable_cases` move with the lists, which is the hand-maintained
guard `every_leg_declares_the_number_of_cases_it_has` exists to keep honest.
**`editable.rs` is the other half nothing else reaches.** Every `edit::analyze_edit` unit test
hands the ladder a `ColumnOrigin` written out by hand, so what they prove is that it works on
metadata a test *imagined*; whether a real driver reports `org_table` for an aliased column, a
`table_oid` for each side of a join, a primary-key flag at all, or anything whatsoever for an
expression is decided on the wire, and the two halves used to meet only inside the running app.
`Scratch::edit_model` closes that seam — it runs the query, introspects the scratch database and
builds the model from both, which is the app's own composition. Each rung of the ladder has a test
and so does each refusal, and the refusals are the ones that matter: a wrong key does not fail
loudly, it writes to a row nobody asked for, with only the 1-row net behind it. Removing the
`NOT NULL` guard from the unique-index rung fails `a_nullable_unique_index_is_no_key_at_all` on all
three legs and nothing else — checked, not assumed.
`a_join_with_one_unkeyed_side_still_offers_no_insert_target` is the join shape the pure fixtures
could not reach: the driver decides what provenance each side of a join reports, and the answer
`EditModel::insert_target` used to get wrong is the one where only *one* side is keyable (see
`core::edit` for the row that was destroyed by it).
**`a_write_built_from_the_resolved_key_lands_on_that_row` is the composition the rungs' own tests
cannot reach.** It walks every writable type case, keys a table on that type where the server
allows one — asked by *trying* it and falling back, not by a hand-kept exception list, since
PostgreSQL has no btree operator class for `json` and MySQL refuses a `TEXT` key without a
length — resolves the key through `analyze_edit`, writes through `GridWrite` and reads the row
back. It asserts every writable case was reached **and** how many really went through a resolved
key — `Target::expected_keyed_cases`, declared per leg (20 / 20 / 24) the way `expected_cases` is.
**The first of those two was a tautology and is the declared number now.** `keyed` and `plain` are
incremented exactly once per iteration of `type_cases().filter(writable)` — the `keyed -= 1;
plain += 1` re-classification preserves the sum — so asking that same iterator for its length made
both sides one pure function of one input and the equality could not fail: delete two cases from
`cases::MYSQL_FAMILY` and both numbers fall together while the test reports green having written
nothing through them. That is verbatim the failure `suite.rs::report` was rewritten to end, and
the accessor built for it, `Target::expected_writable_cases()`, was already here and called from
`suite.rs` alone. `every_leg_declares_the_number_of_cases_it_has` now bounds the *keyed* number
too — `> 0`, and `<= expected_writable_cases` — because a hand-maintained count with no relation
to anything is exactly what it exists to keep honest.
**That was `assert!(keyed > 0)` and it guarded nothing**, which is `suite.rs`'s `TYPE_CASE_FLOOR`
failure one file over: the test rebuilds the fixture on an `INTEGER` key whenever the model
refuses the case's own type — correctly, that being C2/C4 — so a change to `edit.rs`'s refusal
that widened it from float/binary to every non-integer type would send every case down that path
and leave the test green, `tinyint_min` alone keeping the count above zero. Widening the
FLOAT/DOUBLE refusal to `!ty.contains("INT")` drops all three legs to **4** and the failure
message lists the nineteen (MySQL family) / twenty-three (PostgreSQL) types that fell back; the
old assertion passes that mutation unmoved.
**A composite key reached a real server for the first time here too.** Every fixture in
`editable` and `writeback` had a one-column key — the type matrix records more than one as a
*failure* — so `build_update`'s `WHERE` construction had only ever met one predicate and one
parameter, and with one of each, four implementations are indistinguishable: joining with `", "`
instead of `" AND "`, pushing the key parameters before the `SET` ones, and emitting only
`key[0]`. The last corrupts quietly, a partial key matching a superset that the 1-row net waves
through whenever that superset happens to be one row. The fixture is therefore two rows **sharing
`a`**, which is the shape the net can still refuse, and both builders were watched failing:
`.take(1)` on `build_update`'s key loop fails the two MySQL legs and the same on `pg::where_key`
fails PostgreSQL, all three with `affected 2 rows (expected exactly 1) — rolled back all changes`.
**`writeback.rs` asserts the number the 1-row net reads, where `model.rs` asserts the verdict
given one.** Two of its claims exist only at this seam. `CLIENT_FOUND_ROWS`: MySQL counts
*changed* rows by default, so an edit setting a cell to the value it already holds affects 0, the
net reads that as "the row is gone" and rolls back a perfectly good batch —
`an_update_to_an_unchanged_value_still_counts_as_one_row` is the only thing anywhere that can
tell the flag is still set, and clearing it fails exactly the two MySQL legs. And `Rollback::note`:
the same doomed batch runs twice on the MySQL legs, once on `InnoDB` and once on `MyISAM`, and the
error has to promise a complete rollback in the first case and admit the surviving rows in the
second. **And both of those claims were being asserted for the outer transaction only.** Every
live write went through `Db::commit_writes` on a fresh connection, which runs `write_on` with
`TxScope::Own`, while a Manual tab's Commit goes through `Session::commit_writes` — the same
`write_on` with `TxScope::Savepoint`, a different begin/rollback/commit triple where the undo is
`ROLLBACK TO SAVEPOINT` and MySQL's warning-1196 reading is applied to a savepoint rather than to
a transaction. `TxScope` and `Savepoint` appeared nowhere in the tier. All three halves of
`StmtOutcome::FailedIsolated`'s promise are now asserted against a real server — the batch is
undone, the statement the user already ran inside the transaction is *not*, and the transaction is
still committable, the last proved from outside by committing and reading through a fresh
connection. Handing `write_on` `TxScope::Own` in `Session::commit_writes` reports `Failed` where
`FailedIsolated` is required on all three legs, which on PostgreSQL is the difference between a
recoverable batch and telling the user every uncommitted statement is gone.
**One premise here turned out to be wrong and is recorded rather than papered over**: deleting the
explicit `rollback` from `one_row_verdict`'s arm does **not** fail the >1-row test, on any leg.
`write_on` never reaches its `COMMIT` on that path, so the connection's own implicit rollback
undoes the batch regardless. The explicit `ROLLBACK` earns its place on the non-transactional
table, where `Rollback::note` is the whole point and
`a_failed_batch_says_what_the_rollback_actually_undid` is what asserts it. What that test's second
statement *does* catch is a per-statement commit, or an undo reaching only the offending
statement — which is why it now holds the two-statement batch its 0-row sibling already had and
asserts the earlier successful write was reverted too, rather than promising a "rest" it did not
have. **`ddl.rs`** carries the designer's round trip — introspect → draft → diff → emit → run →
introspect — whose real subject is the *asymmetric* failure: an emitter writing something the
introspector reads back differently leaves a table that is correct on the server and permanently
dirty in the designer, and neither half's own tests can see it. Inverting `ddl::defaults_equal`
fails **seventeen** of them with `AlterColumn { from: X, to: X }`, which is that symptom exactly —
measured, and the *inversion* is the point: a break that stays reflexive (`a == b`) fails nothing
at all, because an identity gate compares a value with itself and any reflexive comparator
satisfies it.
**That round trip is `assert_round_trips`, and its name is the fix.** It was called
`assert_settled` and was the *primary* assertion in five of the seven tests, which made it read as
a check that the applied table matches the draft. It is not: both sides come from the table read
back *after* the apply — re-anchored, because a `TableDraft` is anchored to the `TableInfo` it was
made from and re-diffing the pre-apply draft would ask a question the designer never asks — so the
draft that was edited is not in the comparison at all. Run against an emitter that dropped a
neighbouring column's `DEFAULT` alongside the column it was asked to drop, it passes: the
resulting table round-trips through its own draft perfectly well. **`assert_matches_draft` is the
half that was missing**, comparing the applied table with the **drafted** one field by field and
per field, so a failure names what moved rather than printing two structs. The seven tests checked
the column *names* and, in three cases, three or four attributes of the one column they had
touched; nothing checked the columns they did not touch, and nothing checked indexes, keys, checks
or table options at all, so an emitter losing a `DEFAULT`, a `NOT NULL`, a collation, a comment, a
generated expression or an index method while doing what it was asked went green. The type name
goes through `ddl::types_equal`, a server rewriting `INT` as `int(11)` being no loss, and a column
the draft *added* is compared only on what the draft **stated** — `auto_increment` and an implicit
key's type are the engine's to fill. **`invisible` is the exception to that, and is asked
unconditionally**, unlike the collation, comment and `ON UPDATE` beside it: those are fields a
server fills in or does not read back, so asking about one the draft left empty is a false
failure, while a column coming back *visible* against a draft that said invisible is the whole
defect — the retirement undone, `SELECT *` returning the column again — and a conditional check
could not see it. Five more of the apply tests were given `assert_matches_draft` as well, since a
helper only half the file calls is a helper that half the file's emitters are unguarded by.
**Its three set comparisons — indexes, foreign keys, checks — go both ways now.** They iterated the
draft alone and looked for a match on the server, so an object the draft *removed* and the emitter
failed to remove was invisible here, and invisible to `assert_round_trips` too, since a leftover
index round-trips perfectly well through its own re-read draft: a `DropIndex` arm emitting nothing
at all passed both, and this module's own doc already named dropping an index as untested. The
column check has always been an `assert_eq!` over two full vectors; these are the same strength
now. The one thing the reverse direction has to tolerate is an object the *server* invented — MySQL
backs a foreign key with an index of the constraint's name unless one already covers it, and
PostgreSQL backs a unique constraint the same way — and both are excused **by name**, against the
draft's own foreign-key and index names, rather than by dropping the direction.
`a_renamed_column_keeps_its_indexs_kind` is what that bought:
`FULLTEXT` is an index *kind* rather than a flag, any edit to a MySQL index is a drop-and-add, and
an edit that restates the key without it leaves a plain `KEY` behind so every `MATCH … AGAINST`
answers `ERROR 1191` — which `assert_round_trips` passes over, and which the test closes by making
the server run a `MATCH` rather than trusting the model.
Its `SHAPES` are **deliberately awkward rather than representative**, and the sixth is
`checks_and_fks`: no shape carried a `CHECK` or a foreign key, so `CheckDraft` and
`ForeignKeyDraft` went through the identity gate on nothing at all and
`ddl::alter_column_disturbs_checks` — the capability deciding whether a column change has to route
around a constraint — had zero live coverage. A `CHECK` clause is also where MySQL 8 and MariaDB
disagree about escaping, the divergence that file's own module doc names. It needs a `PARENT`
table created before every shape, a `REFERENCES` to a table that is not there being refused
outright. `a_reordered_column_lands_where_it_was_put` is the other addition, gated on
`ddl::supports_column_reorder` the way the view rename is gated on its capability: on MySQL a
reorder is an `ALTER … MODIFY … AFTER`, which restates the column's **whole definition**, so
getting it wrong silently drops a default or a `NOT NULL` along with moving it — and it asserts
the column's **data** arrived with it, since a reorder implemented as a drop-and-add would pass a
test that checked only the order. PostgreSQL returns early with a comment, having no column order
to change, rather than quietly asserting nothing.
**Every change class the designer can raise now reaches a server, and for a long time only one
did.** All seven tests mutated `draft.columns` and nothing else across all three legs, so
`AddIndex`, `DropForeignKey`, `AddCheck`, `RenameTable` and `TableOptions` — every change that is
not about one column's own definition — had been applied nowhere, while the module doc named only
views and triggers as absent and so made the rest read as covered. Six tests close that, each
asserting the half a catalogue read cannot see: the unique index refuses its duplicate, the check
refuses both the row its numeric half forbids and the row its quoted half does (the escaping MySQL
8 and MariaDB disagree about), the dropped foreign key stops being enforced, and the renamed table
keeps its rows and its index. `a_column_inserted_in_the_middle_lands_there` is the one with a
defect behind it — every other test appends, the one insertion position that raises no reposition,
and the reorder test moves an existing column with no `AddColumn` beside it, so the plan carrying
both (where `MODIFY … AFTER <the new column>` was emitted before the `ADD COLUMN` that creates it,
ERROR 1054) was unreachable here. That defect was already fixed, so the test is green on arrival
and closes the gap in the tier rather than implying a red it did not have. **Writing them found
that a PostgreSQL table comment set through the designer was write-only** — see `pg.rs`'s
`table_list_sql` for the four-line read that fixed it.
**And three of the five column-change tests ran against *empty* tables**, so the tier's "did the
data survive?" property held for two of the five change kinds it exercised and for none at all on
PostgreSQL, whose reorder returns early and took the only other data assertion with it. A table
with no rows cannot tell a preserved column from a recreated one: the plan applies cleanly, the
column is there afterwards with the right type and nullability, and every other assertion in the
file is satisfied. `assert_row_survived` is the shared shape so a new test inherits the check
rather than having to remember it, and the add, drop and retype tests seed a row and read it back.
`clearing_a_generated_expression_keeps_the_column_values` is the case the helper exists for and
the one with a measured red behind it: with `pg_column_clauses`' `from.generated.is_some() &&
to.generated.is_none()` arm disabled, the PostgreSQL leg fails with
`left: ["3", "NULL"], right: ["3", "6"]` on 16.15, while MariaDB and MySQL stay green through that
mutation, both reaching the column through `MODIFY` with the clause left off.
**Capability differences are recorded as leg data rather than discovered per test** — the count
grows, so it is not stated here — since a
test asserting one answer would be wrong on two servers out of three: `Target::non_transactional`
(MySQL's `MyISAM`, which accepts `BEGIN` and ignores it), `Target::transactional_ddl`
(PostgreSQL wraps a DDL plan, so a refused one applies **nothing** and `DdlError::applied` is 0,
while MySQL commits each `ALTER` as it runs and the count is what the preview reports),
`Target::grants_are_database_scoped` (whether a grant list covers one database, and so whether
`Grants::note` is there to qualify it), and `Target::disable_index_sql` — the statement that hides
an index, which is a capability *and* a spelling, MySQL 8's `INVISIBLE` and MariaDB 10.6's
`IGNORED` having arrived in different releases under different words. `None` is PostgreSQL, which
has no equivalent, and its leg returns early rather than asserting a property the engine does not
have. The two most recent are the same shape: `Target::primary_key_include` — the clause, not a
bool, so the leg that has `PRIMARY KEY (id) INCLUDE (payload)` also says how it spells it, and
MySQL and MariaDB are `None` because there is nothing there to mistake for a key column — and
`Target::error_names_the_value`, whether a refused write carries a **separate detail field**
naming the offending value, which is PostgreSQL's `DETAIL` and nobody else's: MySQL and MariaDB
put everything in one message and name the constraint rather than the value, so the value is not
theirs to lose. The rest are the same shape and are listed here so a new one is added beside them
rather than as an `if engine ==` in a test body: `Target::expression_index_sql` (a functional key
part, `None` on MariaDB 10.11, which rejects `((a + b))` outright and has no clause to give),
`Target::view_prefix_options`/`view_suffix_options` (the clauses a seeded view carries — the check
option on all three, `SQL SECURITY INVOKER` on the MySQL family — two fields because the two
families put them on opposite sides of the body), `Target::trigger_condition` and
`trigger_update_columns` (a `WHEN` guard and an `UPDATE OF` column list, PostgreSQL's alone), and
`Target::accounts_have_hosts`, which replaced an inline `engine == MySql` so the host assertions
and the host fixture cannot disagree about which legs have one. The newest is
`Target::cancel_aborts_transaction` — whether cancelling a statement takes the enclosing
transaction block down with it, true on PostgreSQL and on neither MySQL-family server — which
decides what a Stop test is allowed to assert about the commit that follows it.
**`runtime.rs` covers the four paths that need a connection to *behave*** — `.sql` scripts, bulk
imports, the pinned manual-transaction `Session`, and cancelling a statement already running — and
every one of them is an exception to something, which is precisely what a pure test cannot check.
That `run_script` holds **one** connection for a whole file is asserted by making a temporary
table in one statement and reading it in the next: under a connection per statement the second
fails, and so would a dump's opening `SET FOREIGN_KEY_CHECKS = 0`. That a `Session`'s transaction
is real is asserted from a *second* connection, which must not see the uncommitted row.
The same pinned `Session` is what gives `a_lock_wait_lists_the_waiter_and_its_holder` a lock to
see: it holds a row in an open transaction, a spawned autocommit `UPDATE` of that row blocks, and
`Db::fetch_locks` — polled until the wait shows, up to fifty times at 100 ms — must list a waiting
lock blocked by the holder's `server_id`, the holder's own granted lock, and a wait at the head of
the list. It runs on MariaDB, MySQL and PostgreSQL; SQL Server has its own, under `mssql.rs`.
**A Stop inside that transaction is where the last two of those paths meet**, and
`a_stop_inside_a_manual_transaction_leaves_the_connection_usable` asserts the **agreement** rather
than the outcome: stage a row, cancel a long statement on the same pinned connection, commit, and
require the app's report and a second connection's row count to say the same thing. Either answer
is defensible on its own — a commit may legitimately fail — so demanding one would be asserting a
policy; what cannot be defended is the app telling the user one thing while the server holds
another, which is exactly what a dropped `mysql_async` future produces (see `cancel_awaited`
above). PostgreSQL runs it and has always passed, `Client` being a channel handle onto a
connection task that drains whatever the receiver does, and keeping that leg is what makes that a
measured fact here rather than a claim.
**Its PostgreSQL branch also records a finding it deliberately does not assert.** A cancelled
statement aborts the enclosing transaction block there — the engine behaviour `Session::fence_read`
already exists for, carried here as `Target::cancel_aborts_transaction` — so the staged row is
gone by the engine's own rule, and that is all this leg requires. What was *measured* beside it is
that `Session::commit` returns `Ok(())` for a `COMMIT` PostgreSQL had turned into a `ROLLBACK`,
over `rows=0`: the same shape as the fault the test was written for, the app reporting a commit
that did not happen, and an entirely different cause. It is stated in a comment rather than
asserted, so that an open question does not become a fixture before anybody has decided what the
app should do about it.
**Cancelling an import is two tests because the answer is two answers**, and they are what
`Target::non_transactional` was recorded for:
`a_cancelled_import_rolls_back_and_says_so` asserts `Err(Cancelled)` **and** that nothing is in the
table, since the first alone passes against a path that killed the query and disconnected without
rolling back — which is what that path used to do — while
`a_cancelled_import_on_a_non_transactional_table_says_the_rows_remain` makes `MyISAM` raise its
warning 1196 and requires the error to say the rows are still there. `Err(Cancelled)` from *that*
one is a failure with the surviving row count in the panic message, because it is the sentence the
modal renders as "nothing was written". PostgreSQL returns early, having no non-transactional table
to make. Feeding a
script is **a task of its own**, not tidiness: the test channel holds 16, so filling it inline
meant the seventeenth `send` awaited a receiver nothing was polling yet — `run_script` is called
on the next line — and a 17-statement script test would have hung forever rather than failed. It
is also the arrangement the app itself uses (`script::feed` alongside `Db::run_script`), so the
test now drives the shape it is testing.
The
cancellation test asserts `DbError::Cancelled` **specifically** and that the call returned well
before the five-second sleep would have ended: asking only "did it fail, and quickly?" is
satisfied by a sleep statement the server does not understand, which is how that test passed on
all three legs while cancelling nothing — pointing `Target::sleep_sql` at an undefined function
now fails it, and did not before. **And it now asks the *server* whether the statement is still
running**, through `Target::running_sleeps`. Measuring how long the *client* took to return
`Cancelled` is a question `tokio::select!` answers on its own: deleting `kill_query` and
`cancel_query` from both arms left all six legs passing in ~250 ms while three servers slept on.
The probe counts `sleep_sql` statements in the server's own session view, and its marker is split
across a concatenation (`'…Cancel' + 'Marker…'`) so the probe does not count itself — its text is
in the very view it reads. `sleep_template` is a template and `sleep_sql()` the one place it meets
`SLEEP_SECS`: it was `"SELECT SLEEP(5)"` in three places beside a constant documented as linked to
them and held together by nothing, so changing the constant left three servers sleeping for the
old duration and the assertion measuring against the new one.
**The marker is a parameter, and there is one per cancellation test rather than one shared.**
`running_sleeps` reads a server-wide view — `PROCESSLIST` / `pg_stat_activity`, filtered by neither
connection nor database — and libtest runs a leg's two cancellation tests concurrently, so on a
shared marker each test's "the server stopped" assertion counted the *other's* statement while it
was still being killed and failed saying the server was still running the statement about somebody
else's. `READ_CANCEL_MARKER` and `SCRIPT_CANCEL_MARKER` are the two; splitting the marker in half
for the pattern happens inside `running_sleeps` rather than at the call site, with a `debug_assert`
on four ASCII bytes as the floor, so "the probe does not count itself" stays a fact about the
function instead of a rule every caller has to remember. `74387d2` named this coupling and removed
it from the script test's *arming* probe; these two constants are the same removal at the two
assertions it left.
**That is also why the *script* cancel arms on something else.** Its token used
to fire when `running_sleeps` saw a sleep, and that count matched on a marker every leg's sleep
carried — so with the read-cancellation test running beside it the probe answered about somebody
else's statement and the token fired before this script had started. It waits for the script's own
inserted row instead: statement 2 is committed by the time a second connection can see it, so a
visible row means both statements before the sleep have run. A fixed delay is the other thing it
cannot be — under the whole tier's load PostgreSQL lost that race, the token fired with `ran == 0`,
and the test read as the accounting bug it exists to catch. **`ARM_FALLBACK` is the blind-arming
budget, and it is strictly inside the elapsed limit the test asserts.** It was the same 3 s
(`SLEEP_SECS - CANCEL_MARGIN`), so a leg slow enough to miss the row — a cold CI container, or the
tier's threaded leg-tests contending on one server — fired the token at the instant the assertion
expired and failed with *"the cancel took 3.0s, so the sleep ran to completion"* about a sleep that
had never started. It is that subtraction minus one more second, written from the same two
constants so shortening either keeps the ordering, and a `saw_row` flag fails the blind arming as
itself, above the assertions that would otherwise describe it as something else.
**`views.rs` and `triggers.rs`** carry the same round trip for the two objects the engines model
differently. A view's body is never the text that went in — MySQL fully qualifies and back-quotes
it, PostgreSQL re-prints it from the parse tree — so the identity diff is doing real work there;
perturbing `ViewDraft::from_table`'s body by one space fails it on all three legs.
**The settle assertions had to stop comparing a value with itself.** `diff_view` computes its old
body by calling `ViewDraft::from_table(current)`, so `diff_view(&after, &from_table(&read_again()))`
is one expression against itself and answers "no change" for any body the server could return, an
empty one included. The identity test says exactly that in its own doc and is built around a
deliberately stale reading to escape it; `an_edited_view_body_lands_and_settles` and
`a_view_that_drops_a_column_takes_the_destructive_arm_where_it_must` took their second reading
*after* the apply and walked straight back into it. `assert_writes_back_unchanged` is the shared
escape — stale copy, emit, run, compare two readings the server produced at different times.
**And no fixture carried a view option**, so `diff_view`'s `draft.options != old_options` half and
`create_view_sql`'s restatement of them had no live assertion anywhere; `seed_view` now puts each
leg's own options on it (`Target::view_prefix_options`/`view_suffix_options`). The two halves fail
together for one reason: disabling `create_view_sql`'s check-option clause leaves all three legs
reporting `check_option: None` after the write-back, on both tests — a view created to refuse
writes silently stops refusing them, the rows it returns are unchanged, and the old
self-comparison sees nothing, neither reading having the option. The writable-view key builder in
this file goes through `export::sql_literal` with `IS NULL` for a NULL now, rather than a
hand-rolled `''` escape on the family where `\` is also an escape; that branch is unreachable on
all three legs today — no driver here attributes a view column to a base table — so **no test can
be made to fail on it**, which is said rather than implied. A trigger is
the object they disagree about most (MySQL carries the body, PostgreSQL calls a function with its
own lifetime), so every trigger test ends by making it fire or proving it no longer does: a
trigger that sits in the catalogue doing nothing is the failure that reads as success everywhere
else. Dropping `TriggerDraft::from_info`'s `original` — the same identity-field class as
`ColumnDraft::original` — fails the trigger round trips on every leg.
**The fixture was at the narrow end of the width the identity test was checking.** `new_trigger`
set `timing = Before`, one event, row level and an action and nothing else, so `TriggerInfo`'s
`condition` and `update_columns` — the two fields the model widened itself for — were at their
defaults on every leg, and `AFTER` never occurred at all. The whole-struct
`assert_eq!(after.triggers, table.triggers)` is strong enough to catch an emitter that dropped
either on the drop-and-create every trigger edit performs; it was simply never handed one to
catch. `wide_trigger` is a second trigger carrying whatever its leg's grammar has — `UPDATE OF
name`, `WHEN (NEW.name IS NOT NULL)`, and `AFTER` where the action allows it — with the timing
riding on the same `(body, function)` distinction `new_trigger` already switches on rather than a
new field, because MySQL's body assigns to `NEW` (which `AFTER` may not do) and PostgreSQL's
function returns `NEW` (which an `AFTER ROW` trigger ignores). Both halves were watched failing on
the PostgreSQL leg on their own mutations — dropping `when_group` from the non-SQLite emitter, and
emitting `UPDATE` in place of `UPDATE OF {cols}` — with MariaDB and MySQL green through both,
having neither clause. What is still uncovered is named in `wide_trigger`'s own doc rather than
left to be re-derived: `TriggerLevel::Statement` needs a per-leg action that references no row,
and `InsteadOf` needs a view to be on. **A view result is
read-only on all three**, and that is asserted rather than assumed: the test that meant to check
it returned early instead and was a no-op on every leg until a probe said so. Its other branch
still stands, because a driver may reasonably start attributing a view's columns to the table
underneath, and the key the resolver then picks has to identify exactly one row — which the test
checks by counting what it matches.
**Two arms in these two files had never met a server.**
`a_view_that_drops_a_column_takes_the_destructive_arm_where_it_must` narrows a view's `SELECT`,
which is the redefinition `pg_replaceable` answers `Some(false)` to — so the change set carries
`recreate: true` and the plan has a `DROP` in it. No live case satisfied that predicate, and the
assertion is per leg because the answer genuinely differs, which is what `supports_or_replace_view`
and `pg_replaceable` exist to say. `one_of_two_triggers_can_be_dropped_without_the_other` is the
first fixture here with **two** triggers on one table, so `diff_triggers`' set semantics are asked
something at last: with zero or one, "the set changed" and "this trigger changed" are the same
statement, and a diff that re-emitted the whole set would pass. The second trigger differs by
**event** rather than timing, since MySQL refuses `SET NEW.x` in an `AFTER` trigger (ERROR 1362),
which makes timing the wrong axis to vary.
**What that leaves is two triggers in two different *groups*, and the ordering clause needed two
in one.** Varying the event is what keeps MySQL's `SET NEW.x` legal, and it also puts the pair in
different `(table, timing, event)` groups — so no live leg ever carried a `FOLLOWS`/`PRECEDES`,
`TriggerInfo::order` was `None` on every one of them, and the apply path emitted its first
ordering clause in front of a user (see `core::ddl`'s `trigger_statements` for what that cost).
`an_ordered_pair_of_triggers_writes_back_without_naming_a_dropped_one` is the missing fixture:
it seeds an ordered pair, edits the group's **second** trigger, and reads the resulting firing
order off the row the triggers write — so a fix that simply stripped every clause fails it from
the other side. It is gated on `Target::trigger_body` rather than on an engine, PostgreSQL having
no ordering clause in its `CREATE TRIGGER` grammar at all, and that leg returns early rather than
asserting something the engine cannot express.
`a_body_that_opens_like_an_ordering_clause_survives_the_round_trip` is the other addition, and its
first draft is worth recording as a near-miss of this repository's standing test rule. Asserted
over `table_of`, it was **green against the unfixed tree**: introspection reads
`information_schema.ACTION_STATEMENT` and never reaches `mysql::trigger_body_of` at all. Pointed
at `Db::trigger_source` — the call the trigger editor actually makes — it fails on both MySQL legs
with the damaged body verbatim. The seam was the pure reader's composition with its caller, which
is where these keep being.
**`streaming.rs` and `namespaces.rs`** close the last two. The export's assertions are about
completeness and about how a failure reaches the *writer*: a channel that simply closes reads as
"the table ended", so a half-written file would be reported as finished — both failure tests check
the channel, not just the return value. `a_cancelled_export_tells_the_writer_it_stopped` is the
case that was missing: every other test there fails the *statement* and none of them cancels, so
the file's own opening claim had no case for the one path where the channel closes with nothing
wrong. `namespaces.rs` needs two namespaces to exist at all,
which is what `Scratch::alt_namespace` is for: a schema on PostgreSQL, a second scratch database
on MySQL, so one test means the same thing on both. What it guards is silent — the statement
succeeds, one row is affected, the net is satisfied, and the wrong table changed — so it asserts
on the table that must *not* have moved. Collapsing the schema out of `analyze_edit`'s grouping
key fails three PostgreSQL tests including that one; the MySQL legs are correctly unmoved, their
namespace being the database, and that half rests on the same assertion's `database` comparison.
**`blob.rs` is the seam that has no pure test available at all.** No `ResultSet` on any engine
ever holds real bytes, so nothing in the pure tier can hold one either: `core::blob` can be told
what a PNG header looks like and the SQLite suite can round-trip a blob through a file-less
database, but only a server answers whether MySQL's `SUBSTRING` is byte-indexed on a binary
string and whether a `bytea` survives the simple-query protocol's hex text intact — both were
assumptions when it was written. Six cases per leg, over the same `DEADBEEF` bytes `cases.rs`
already spells for each dialect, every one of them outside ASCII because that is the exact class
of value the `<n bytes>` placeholder exists to stop being rendered as mojibake: a byte-for-byte
read back, the key landing on the row it names (three rows of one distinguishable byte each,
since a `WHERE` that failed to narrow would still hand over a plausible blob under the
`LIMIT 1`), a NULL cell and a vanished row both reporting nothing, a stored PNG still sniffing as
one afterwards, and `a_binary_cell_in_a_real_result_resolves_and_fetches`, which is the
composition the two halves' own tests cannot reach — it runs a real query, asserts the grid holds
`<4 bytes>` and that the write model still refuses the column, then derives the `BlobRef` from
that result's own provenance and fetches with it. The sixth is the only **write** here —
`staged_bytes_reach_the_column_as_bytes`, stage → commit → re-read on all three servers, which is
what proves each engine's `cell_param` binding (see `schemaic-db`'s entry for what it deliberately
does not pin).
**`enforced.rs` is the session's half of the read-only gates**, and only a server can hold it:
`read_only_reason` reads text, and `SELECT purge_all()` is a read head naming no denied word while
`purge_all()` empties a table. `Target::purging_function` is each server's `CREATE FUNCTION` for it
— `purge_all` rather than `purge`, `PURGE` being reserved on MySQL, and `DETERMINISTIC` on the MySQL
family, which is a lie about the function and is what lets a server with binary logging on accept
it from a user without `log_bin_trust_function_creators`.
`a_read_only_session_refuses_a_write_a_select_hides` asserts the premise first — the gate passes the statement, which is the composition the bug lived
in — then that `Enforce::ReadOnly` fails it **and** that all three rows are still there, since an
error alone could be a session that refuses everything; `a_read_only_session_still_reads` is the
other side of that, and `a_session_pinned_to_the_gates_lexer_still_writes` holds `AsJudged` to
*not* being read-only, `exec` on a writable connection running on it. The editor's four other
enforced paths have a case each, over the same `SELECT purge_all()` and the same three-rows check:
`a_read_only_pinned_session_refuses_a_write_a_select_hides` (`Session::open_enforced`, then
`ensure_tx`), `a_read_only_explain_analyze_refuses_a_write_a_select_hides` (`Db::explain` with
`read_only`), `a_read_only_stream_refuses_a_write_a_select_hides` (`stream_query_enforced`) and
`a_read_only_batch_refuses_a_write_a_select_hides` (`run_batch_enforced`, with a read ahead of the
hidden write, so the batch must answer read-then-refused). Each runs an unenforced control that
must succeed, so the refusal is the session's and not a function that fails anyway — and the
stream and batch controls assert the table then *is* empty.
**Three more ask that a statement cannot lift the session for the one after it**, over
`Target::lifting_call` — PostgreSQL's `SELECT set_config('default_transaction_read_only', 'off',
false)`, a builtin, and on the MySQL family `SELECT lift_read_only()`, a stored function
(`lifting_function`, created first) running `SET SESSION TRANSACTION READ WRITE`:
`a_read_only_batch_cannot_be_lifted_for_a_later_statement` (`run_batch_enforced`, the lift then a
plain `DELETE`), `a_read_only_text_cannot_lift_itself_for_its_next_statement` (one enforced text,
the lift then `SELECT purge_all()`, the multi-statement query MariaDB's lift reached) and
`a_read_only_pinned_session_cannot_be_lifted_across_a_commit` (the lift inside one `ensure_tx`
transaction, a commit, then a `DELETE` in the next). The pinned case leaves whether the lift itself
is refused to the server; each asserts the three rows are still there.
`a_read_only_session_reads_the_catalog_before_a_users_functions` is PostgreSQL's alone, and asks
which function a bare name reaches rather than whether a write lands: it creates a
`public.version()` answering `'shadowed'`, gives the connection the init SQL `SET search_path TO
public, pg_catalog`, and has plain `fetch_query` return `'shadowed'` — the premise, so a fixture
that fails to shadow fails here — before `fetch_query_enforced` with `Enforce::ReadOnly` had to
return the builtin's `PostgreSQL …`. It was red on PostgreSQL 16 before the read-only setup put
`pg_catalog` first, by its author's report. **It expects a refusal now**, not the builtin's
answer: `version` is a name the read-only gate allows, so the setup's shadow check refuses the
session with `DbRefusal::ShadowedBuiltin` naming `public`/`version` before the statement is sent.
That check has four cases of its own, registered beside it, each over the private
`read_only_entries` — a read, a batch, a plan and a pinned session, every one opened enforced, so
the four paths that run `pg::set_up_read_only` are asked together where this case asks only the
first. `a_read_only_session_refuses_a_path_that_shadows_a_builtin` plants `public.lower(varchar)`
and first has an unenforced `SELECT lower('A'::varchar)` answer `'shadowed'` on a `pg_catalog,
public` path — the premise, that the catalogue first is not enough — then requires each entry to
refuse with the variant naming `public`, `lower`, not an operator.
`a_read_only_session_refuses_a_path_with_a_users_operator` plants `public.===` over integers and
requires the same refusal, `operator` true. `a_read_only_session_runs_beside_an_extensions_functions`
is the control: `CREATE EXTENSION citext SCHEMA public` puts an allowlisted `lower(citext)` and
operators there, and every entry must still run; it is skipped, saying so on stderr, on a server
without `citext`. `a_read_only_session_refuses_what_an_owner_added_to_an_extension` is the
control's counterpart: membership alone is not the extension's word, since a database's
non-superuser owner can install a trusted `citext` and `ALTER EXTENSION citext ADD FUNCTION` one of
their own — measured on PostgreSQL 16. The fixture builds that end state as the suite's superuser:
it creates the role `schemaic_it_extension_owner` `NOLOGIN`, installs `citext`, plants
`public.lower(varchar)`, hands it to that role and adds it to the extension; every entry must then
refuse naming `public`, `lower`, not an operator, and the role is dropped after the scratch
database. It is skipped, saying so, without `citext` or without the right to create a role, and it
was red before the check asked who owns a member ("read: a read-only session trusted the owner's
addition"), by its author's report. The four have no unenforced control beyond the first's premise; the refusal's
variant and its fields are what tell it from a session that refuses everything.
`introspection_calls_no_function_the_database_planted`, in `namespaces.rs`, is the catalogue
reads' case (`pg::connect_catalogue`, under `schemaic-db`): it plants `public.obj_description(oid,
text)`, `public.format_type(oid, integer)` and a `public.=` on `oid`, each recording in a table
that it ran, has an unenforced read on a `public, pg_catalog` init-SQL path reach all three — the
premise — then runs `fetch_schema` and `fetch_table_list` with and without that path and requires
the record to be empty.
`a_read_only_session_calls_no_users_function_while_setting_itself_up`, PostgreSQL's alone too and
registered beside it, asks the same of the setup itself, which runs while the hostile path is still
in force: it plants a `public.set_config(text, text, boolean)` that turns
`default_transaction_read_only` off, gives the connection the same init SQL, has an unenforced
`SELECT set_config('application_name', 'x', false)` fail to answer `x` — the premise, that the
bare name reaches the plant — and then has an enforced `DELETE` refused with the three rows still
there. It failed against the setup that called `set_config` by its bare name ("a read-only
session wrote"), by the fix author's report.
`a_read_only_plan_refuses_a_write_its_planner_makes` is the plain `EXPLAIN`'s case, for the
editor's Plan and the MCP server's `explain` at once, and its statement is the planner's rather
than the executor's: an `IMMUTABLE` plpgsql function that `PERFORM`s `purge_all()` on
PostgreSQL, `SELECT * FROM (SELECT purge_all() AS x) d` on the MySQL family. It was red on
MariaDB and PostgreSQL before `Db::explain`'s plain form honoured `read_only` — the rows deleted —
and green on all three legs after. It has no unenforced control; it asserts the refusal and the
three rows, and then that a read-only plan of `DELETE FROM t` answers on PostgreSQL and is a
`DbError::Refused` naming "read-only" on MySQL/MariaDB. SQLite's batch leg is pure, in
`sqlite.rs` (`a_read_only_batch_refuses_a_write_and_still_reads`). **The `sql_mode` pin has no
case here**: no leg sets a mode that moves a quote, so the smuggle `AsJudged` closes on the MySQL
family was measured once, by hand, on a throwaway MariaDB 11.8, and what holds it now is
`core::sql`'s pure tests over the strip and the read-back. Nothing in the tier reproduces that run.
**`init_sql.rs` is a connection's init SQL against the real servers**, its own group in
`live_suite!`, so MariaDB 10.11, MySQL 8.4 and PostgreSQL 16 each run all three — each with a
setting the engine reads back, `SET application_name` on PostgreSQL and `SET time_zone` on the
MySQL family. `the_init_sql_is_in_force_on_the_operations_connection` reads it back through plain
`fetch_query`, whose connection is a fresh one, so it can only hold the value if the connect ran
the init; `a_read_only_session_with_init_sql_still_refuses_a_write` reads it back through
`fetch_query_enforced` with `Enforce::ReadOnly` and then has the same enforced path refuse an
`INSERT`, the count afterwards still `0` — the ordering half, which no pure test can hold; and
`a_refused_init_statement_fails_the_operation` sets a listed setting to a value the server
refuses — `SET statement_timeout = 'banana'` on PostgreSQL, `SET time_zone = 'Nowhere/Nope'` on
the MySQL family, since a name on no list is now refused before the connect and would never
reach the server — and asserts the operation fails with an error naming the init SQL. SQL Server is not a leg of the macro, so its
case is `mssql.rs`'s `the_init_sql_is_in_force_and_a_refused_one_fails_the_operation` — `SET
LANGUAGE British; SET DATEFIRST 3` read back, which is also what proves the batch-not-RPC send,
and `SET LANGUAGE NoSuchLanguage` failing the operation — passing on 2022 and 2025 by the
change's author's report.
**`partitions.rs` and `dump_replay.rs` hold a dump to what it does when it is run**, both in
`live_suite!`. `partitions.rs` reads a partitioned table back as one, dumps it and restores it
with every partition's rows, replays one partition beside its siblings, and plans a compared
partitioned table the server takes; its `dump_of(scratch, tables)` — the app's `plan` and
`render_rows` over the rows read on `Db::for_rows`, as `cli::dump::run` reads them — and `count`
are shared with the other module. `dump_replay.rs` is the **replay onto the source**, the case a
dump meets most and a fresh-database restore never exercises: a key a table outside the file
holds, the views and materialized views outside it, a materialized view and the routines it
carries, types and sequences left as they stand, a MariaDB sequence restored and never rewound
past where it stands, a MariaDB routine's grants kept, a `TIMESTAMP` restored as the instant it
was across session zones, a locked-down PostgreSQL routine the cascade would take refused rather
than reopened, and a restored partition firing none of its parent's triggers. Each is gated on the
capability it needs — `drop_cascade`, `partitions_are_tables`, the dialect where only one engine
has the object — and notes a no-op where the leg cannot hold it.
**In `users.rs` the reads are of accounts that were already there and the writes make their own.**
An account is server-wide — it is not inside the scratch database and would not go away with it —
so the read half asks about the account the suite **connected as** (`Target::user()`), that being
the only one every leg is guaranteed to have, while the write half creates one named with the
tier's `scratch::PREFIX` and drops it in a guard. That is the same bargain `Scratch` makes with
databases and it is checked by the same `assert_scratch_name`, called on the way **in and again on
the way out**, so the rule that nothing here touches what it did not create holds for accounts too
— and it has to carry more weight here, since an account is not namespaced by anything the server
enforces. A `ScratchAccount` dropped without `teardown` — a test that panicked — **really drops
the account**, on its own thread with its own runtime, and prints the name only when that fails;
it used to print and nothing else, so exactly the case the guard exists for left a real login on a
real server, named but present. What this covers is the half nothing else reaches: three servers, four
catalogues, and a `mysql.user` column set that differs between MySQL 8 and MariaDB 10 in both
directions, where a query naming a column the server hasn't got fails outright and only a live
server says so. Four read tests — the connected account is in the list and carries a host part on
exactly the engines where an account *is* the pair (`Target::accounts_have_hosts`, which replaced
an inline `engine == MySql` so the assertions and the fixtures agree about which legs have one);
its privileges come back as `GRANT` statements
on both catalogues; the note is present exactly where `grants_are_database_scoped` says and names
the database it covers; and `no_password_material_survives_the_fetch`, which is the assertion that
the redaction is on the fetch rather than on one view that happens to call it.
**The write tests go through the real emit-and-run path** — `ddl::account` →
`ChangeSet::emit` → `Db::run_ddl` — rather than asserting statement text, because a statement no
engine accepts is exactly what only a server can tell you:
`a_created_account_is_one_the_server_then_lists`,
`an_account_created_at_a_host_is_listed_and_dropped_at_it`,
`a_created_account_can_log_in_with_the_password_it_was_given`,
`a_reset_password_replaces_the_one_the_account_had`,
`a_salted_password_logs_in_on_create_and_on_reset`,
`a_role_the_server_made_is_never_offered_a_password_reset`,
`a_created_role_is_one_the_server_accepts`,
`a_granted_privilege_comes_back_and_a_revoke_takes_it_off`,
`a_granted_role_comes_back_and_a_revoke_takes_it_off`,
`a_grant_at_every_level_reads_back_naming_that_object` and
`a_dropped_account_is_gone_from_the_list` — every one of them fanned to MariaDB 10.11, MySQL 8.4
and PostgreSQL 16 by `live_suite!`. SQL Server is not a leg of that macro, so its accounts have
their own end-to-end tests in `tests/live/mssql.rs`,
`a_login_and_its_user_are_created_granted_reset_and_dropped` and
`a_contained_user_is_created_signs_in_and_is_reset` (under `db::mssql`), and these call
`fetch_principals(None)`, a database being an argument only SQL Server reads.
**The list is the count**, and deliberately so: the sentence
here used to say "the seven write tests … twenty-one in all" over an enumeration of seven that
had already left three of them out, which is the failure `AGENTS.md` names — a stated total is one
more thing to keep in step with the code, and the names are what a reader is actually looking for.
**What none of them checked was the *host* an account is listed at.** `from_mysql_rows` folds
`mysql.user.Host` into `Principal::host` and `users.rs`'s own doc calls that the one place the two
catalogues genuinely disagree about what an account *is* — yet one site asked `host.is_some()`,
two matched on `p.name` alone, and `drop_named` named the *drafted* principal rather than the
listed one. A fold returning the wrong field passed all of it: `Some(r.user.clone())`, or
`Some(String::new())`, which is what MariaDB stores for a role and which this file records as
having produced ERROR 1141, ERROR 1133 and a `DROP ROLE` syntax error. The connected-account test
now asserts the host is a host and that `display` is built from both halves; the created-account
test finds the **listed** principal, compares its host to the drafted one and drops through it,
the way the role test already did. And a leg-gated test creates an account at `10.0.0.%`, which no
fixture had ever done — `AccountDraft::host` was empty everywhere, so `account_sql`'s host
literal, quoted through `ddl_string` precisely because a host of `it's` would close the quote and
change the statement, had never round-tripped through a server and no `DROP USER` had ever named a
real host. With the fold returning `Some(r.user.clone())` both create tests fail on MariaDB and
MySQL with the account listed at a host equal to its own name, while
`the_account_we_connected_as_is_in_the_list` passes that mutation unmoved (`schemaic@schemaic`
being a non-empty host that displays consistently) — which is why the assertion that matters is on
an account whose host the test chose.
**The second of those is the one branch that writes a credential to a server, and until it existed
nothing in the workspace took it.** `ScratchAccount::create` drafted an empty password and
`account_draft_sql` emits the `IDENTIFIED BY`/`PASSWORD` clause only for a non-empty one, so every
`CREATE USER`/`CREATE ROLE` this tier had ever run was the passwordless form on all three legs —
with two findings sitting on exactly that branch, a `*`-containing replacement creating a real
account with a mangled password and a backslash doubled unconditionally for MySQL.
`ScratchAccount::create_with_password` is the path and `Target::db_as` is the assertion: the server
accepts `IDENTIFIED BY 'hun'` exactly as readily as `… BY 'hunter2***'`, so **only a login tells
the two apart**, and the failure being guarded is an account that exists with a credential nobody
holds — written when the app had no `ALTER USER` path to repair one, which it now has and which
the paragraph below covers. A wrong password is asserted to be
*refused* beside it, or the login would pass on a server that accepts anything. The password
carries `'`, `\` and `*` on purpose — the first two are `ddl_string`'s job (and MySQL's
`NO_BACKSLASH_ESCAPES` is where that went wrong) and the third is the character the account
editor's mask is written in.
**`a_reset_password_replaces_the_one_the_account_had` is the repair, and it is here because which
engine spells it which way is a claim only a server settles.** `ALTER USER … IDENTIFIED BY` and
`ALTER ROLE … PASSWORD` are two statements for one act, and MySQL's `ALTER ROLE` is about roles
rather than about a user's password — so the pair taken backwards parses on one engine and means
something else on the other. **It asserts both halves**: the new password is accepted *and* the old
one is then refused, because a server that ignored the statement entirely would pass a test that
only tried the new one, the account still being there and the new login the only thing a one-sided
test looks at. It reads the account back out of the catalogue before resetting
(`listed_principal`) — the distinction `a_created_role_is_one_the_server_accepts` was written for,
since MariaDB stores a host the draft does not and an `ALTER USER` naming the wrong one is an error
rather than a silent miss. Green on MariaDB, MySQL and PostgreSQL.
**`a_salted_password_logs_in_on_create_and_on_reset` is the same pair of logins over what the
account form now sends** — a `scram_salt` stamped on the create (`ScratchAccount::create_salted`,
which `create_with_password` delegates to with `None`) and on the reset. On PostgreSQL that makes
the clause a `core::scram` verifier, and a verifier computed wrong is stored verbatim by a server
that reports success, so only a login with the *typed* password can tell — after the create,
with a wrong one refused beside it, and after the reset, with the replaced one refused. That login
also proves the server stored the verifier rather than hashing its text as a password, which would
refuse it. On MySQL and MariaDB the salt is ignored, and the test pins that it changes nothing
there. The unsalted tests keep `scram_salt: None` on purpose — the backslash test below above all,
being about how the literal is parsed, and a base64 verifier carries no backslash to parse. Green
on MariaDB 10.11, MySQL 8.4 and PostgreSQL 16 under a `scram-sha-256` `pg_hba`; and run once more
by hand on PostgreSQL with a temporary `pg_hba` rule sending `/^schemaic_it_` roles through `md5`
— shown to be in force by a control, an md5-stored role that logged in under that prefix and was
refused outside it — where it still passed, PostgreSQL switching to SCRAM for a SCRAM-stored
verifier. The rule was removed afterwards; nothing in the tier reproduces that run.
**`a_server_password_policy_decides_what_is_stored` is the same form's policy half**, PostgreSQL
only — the MySQL legs return, there being no verifier for a policy to shape. It asserts first that
`fetch_principals` brings a policy back at all, since a read that failed quietly leaves `None`,
which is the SCRAM default and passes everything else here; then it creates through
`ScratchAccount::create_with_policy` (which `create_salted` now delegates to with `None`) and reads
`rolpassword` out of `pg_authid`, because the statement succeeding says nothing about what was
stored: under an `Md5` policy it starts `md5`, and under 5000 iterations it starts
`SCRAM-SHA-256$5000:` and the typed password logs in. The md5 role is checked by what is stored,
not by a login. Watched failing with the policy ignored.
**`a_password_with_a_backslash_is_stored_as_it_was_typed` is the leg-gated one beside it, and
what it pins is a *connection option* rather than anything this file emits.** PostgreSQL only —
MySQL's mirror hazard is `NO_BACKSLASH_ESCAPES` and `mysql::run_ddl` already sends
`MYSQL_LITERAL_MODE_SQL` for it, SQLite has no backslash escape to get wrong, and the gate is
`Target::accounts_have_hosts` being false rather than an engine comparison. It turns
`standard_conforming_strings` **off on the role the tier connects as**, asserts that a plan's
connection reports `on` anyway — because `pg::connect_probe` pins it on the startup packet — then
resets the account's password to `p\ass'q` through `Db::run_ddl` and logs in with the typed value
while the mangled `pass'q` is refused. **Everything that needs the GUC off sits between the set
and the `RESET`, with no assertion inside**: a panic there would leave the tier's own role
parsing literals the old way for every test after it, the setting being cluster-wide and
outliving the scratch database — so the observations are captured, the role is put back, and the
assertions come after. It is `RESET` and not `SET … = on` for the tier's standing rule that it
touches nothing it did not create: setting it explicitly would leave a `pg_db_role_setting` row
on a cluster that had none. The finding it was written for turned out not to hold, and this test
is what settled it — the first version's *premise* assertion, that a plan's connection inherits
the role setting, is the one that failed.
**`a_role_the_server_made_is_never_offered_a_password_reset` is the one test in this file that
asserts an *offer* rather than a statement, and it is here because a pure test could not reach the
fault.** `a_role_cannot_have_its_password_reset` builds a `Principal` with `kind = Role` and asks
the predicate; both halves are correct and the defect sat between them, MySQL 8 publishing no
`is_role` for the fold to label a role with and a write gate then reading that label. So the
assertion is over `Db::fetch_principals`' own output — `listed_principal` for a role and for a
user the same run creates — and it was watched **red on the `mysql::` leg alone** before the fix
and green on all three after, MariaDB answering from `is_role` and PostgreSQL from `rolcanlogin`.
**Both directions**, because a gate that refused everything would pass the first half: the user
beside the role goes through the same statement path, the same fetch and the same predicate and
must still be offered a reset. It asserts the *plan* as well as the offer
(`users::set_password_sql` returns `None`), so neither gate is carrying the other.
**Its scratch-account suffix is its own, and that is load-bearing rather than tidy.** An account
name here is `{PREFIX}{pid}_{leg}_{suffix}` and the scratch *database* is not part of it, so
within one leg the suffix is the whole of an account's identity — this test and
`a_reset_password_replaces_the_one_the_account_had` both asked for `"r"` and raced for one name
whenever they overlapped, which is a duplicate-key `CREATE` or one test's teardown dropping the
other's account out from under it. The role test moved to `"crl"`; a new test in this file picks a
suffix nothing else uses. The grant round trip reads its
privilege **off `users::privileges_for`** rather than naming one, and that is the tier earning its
keep: naming `SELECT` was the first version and PostgreSQL refused the plan, a database being an
*object* there that carries only `CONNECT`, `CREATE` and `TEMPORARY` rather than a shorthand for
everything in it. Taking the engine's own first entry is what makes it one test instead of three,
and it exercises the same list the grant form is built from.
**And it asserts the grant's *scope*, not just that the word came back**, because
`s.contains(&privilege)` is satisfied identically by `GRANT SELECT ON *.*`: collapse
`users::GrantLevel::object_sql`'s `Database` arm and the check passes unchanged, on the one feature
whose entire safety property is scope. `a_grant_at_every_level_reads_back_naming_that_object` then
walks every level `users::levels_for(dialect)` offers, two-sided at each — the read-back names that
object, and below `Global` is not a whole-server grant, since one side alone is what let the
substring check stand. `as_read_back` is the helper that makes the comparison possible and the
thing to read before touching it: MySQL echoes the **stored `LIKE` pattern**, so a database-level
grant on a scratch database reads back with a backslash before each underscore, while PostgreSQL
rebuilds the statement from the catalogue and leaves an ordinary lower-case name bare. Comparing
against the raw name would pass only for a name with no wildcard character in it, which is the one
case the escaping does not matter for — so this is also the live guard on
`export::ident_pattern_sql`, which nothing before it reached.
**Both revoke assertions hold a sibling grant the revoke must not touch**, and until they did,
every one of them got *easier* the broader the revoke was. With one grant on the account, a
`REVOKE` naming the whole server satisfies "the privilege is gone" perfectly — so collapsing
`object_sql`'s revoke path one level had nothing to fail it, on the one feature whose entire
safety property is which object a statement names. PostgreSQL makes that sharper still: revoking a
privilege a role does not hold is a silent no-op there rather than an error, so an over-broad
revoke leaves no trace at all, where the MySQL family answers `ERROR 1141` — which is why the gap
is PostgreSQL-shaped. Each test now holds a second grant on a **different object at the same
level** — a second scratch database, a second table, a second sequence — revokes the named one and
asserts the sibling is still there; collapsing `object_sql`'s revoke path one level (Table →
Schema) takes the sibling with it and is what fails. The sibling's *name* is load-bearing and
says so at both sites: these are substring tests over the server's own grant text, so a sibling
called `gt2` beside `gt`, or `grantrt2` beside `grantrt`, cannot be told from the original by
`contains` — hence `gx` and `sibdb`. The sibling is also revoked before teardown, PostgreSQL
refusing to drop a role anything still grants to.
**The role test reads its role back out of the catalogue before touching it, and that is what makes
it a test.** It created and dropped one through the principal it had *drafted* —
`AccountDraft::principal` gives a role no host — so the catalogue's own representation, the one
every action in the browser acts on, was never exercised, and `core::users`'s MariaDB role-host
defect sat under a green leg. It now finds the role in `fetch_principals`, reads its grants (the
statement the browser issues the moment a row is clicked, and the one MariaDB refused with 1141)
and drops *that* principal through `ScratchAccount::drop_as`, which exists to drop an account as
some other principal describes it and asserts the two name the same account. Watched red against
the unfixed fold, with the server's own ERROR 1141 as the failure.
**What the tier deliberately does not cover: SSH tunnels and TLS.** Not an oversight and not
difficulty in the tests — the obstacle is that both need a server configured *before* it starts,
and GitHub Actions brings `services:` containers up before any step runs, so the repository is not
checked out yet and a generated certificate or key pair does not exist. Covering them in CI means
baking custom images, committing a test CA and key pair, or dropping those servers out of
`services:` and starting them with `docker run` inside a step: a CI restructure, decided on its
own terms. Until then they keep the instrument they have — `db/examples/tls_matrix.rs` over
`scripts/tls-testbed/`, which reports the transport each mode actually negotiated and is better
at that than a pass/fail test would be. Also uncovered, for ordinary reasons of nobody having
needed it yet: streaming a genuinely large export, and multi-schema PostgreSQL.
**It is gated as a *target*, not at runtime.** The manifest declares the target
`required-features = ["live-tests"]`, so `cargo test --workspace` does not build it and the pure
tier stays pure by construction. It is **669 tests on Windows, 668 elsewhere,** as this is
written (re-derived with `--list`) — 159 suite functions expanded across the three legs by
`main.rs`'s macro (477), SQL Server's own 169 in `mssql.rs` (168 off Windows: one,
`a_windows_sign_in_is_this_processs_own_identity`, is `cfg(windows)`), the seventeen in the two
catalog oracles outside the macro (`pg_catalog`'s ten and `mariadb_catalog`'s seven), and the
six that need no server (the four name-guard cases, `endpoint.rs`'s declared-case count, and the
skip-notice source gate). The figure this sentence
carried for a while was 306, a count that had never included the oracles, and then 388, which had
never included SQL Server's leg; both went stale as the suite grew, so `cargo test -p schemaic-db
--features live-tests --test live -- --list` is where to re-derive it rather than adjust it by
hand. It
is reachable from this Windows environment again,
which several fixes in the same range were measured against after being written blind. With the feature on, an unreachable server is a **failure** —
a harness that noticed a missing endpoint and returned would report a green suite that asserted
nothing, which is the decoration the testing rules already name. The one exclusion is
`SCHEMAIC_IT_ENGINES`, which a developer has to type, and which is refused outright when `CI` is
set: libtest has no runtime skip, so an excluded leg reports as a *pass*, and that is tolerable
only where a human chose it. **The whole mitigation for that exception is the skip notice, and
libtest was swallowing it.** It was an `eprintln!` in the test body, and libtest prints a test's
captured output only for *failing* tests — a skipped leg being a passing one — so narrowing the
tier to `mariadb` reported green under all three legs' names with nothing on screen separating
the leg that ran from the two-thirds that returned immediately. `endpoint::note_skipped` writes
to the locked stderr handle instead, which is past the macro's capture-aware path, and
`no_skip_notice_goes_through_the_captured_macro` scans for the macro
spelling — a source assertion because it has to be one: a `#[test]` cannot observe libtest's
capture of its own run. **Its corpus is the whole `tests/live/` directory, and was the literal
list `["main.rs", "endpoint.rs"]`** — the two files that already answered correctly — while
`routines.rs` held two live violations on the day the gate landed. A gate whose corpus is
narrower than the rule it states is the shape this tier keeps producing, so it reads the directory
rather than a list somebody has to remember to extend, with a floor (`seen >= 10`) that notices a
`read_dir` matching nothing, and two exemptions named in it: `scratch.rs` and `users.rs` report a
*leak* on a path that is about to fail the test anyway, so libtest will print it. **And a leg that
runs and asserts nothing is the other silent green, which now has a channel of its own.**
`endpoint::note_no_op(target, reason)` is `note_skipped`'s sibling for a test whose subject is one
engine's feature and which returns early on the other two — the more common case, and the one
`routines.rs`'s two `eprintln!`s were: four of six leg-tests reported green with nothing on
screen, on every run including CI's, under a doc claiming the opposite. It is deliberately *not*
deduped the way `note_skipped` is, that one being about a target and this one about a leg with a
different reason each time. `endpoint::note_leg_no_op(name, reason)` is the same notice by the
leg's name, for a leg with no `Target` — `OUTSIDE_THE_SUITE`'s, as `note_leg_skipped` is
`note_skipped`'s — and `note_no_op` delegates to it. It exists because the gate caught one more:
`mssql.rs`'s AdventureWorksLT round trip returned through an `eprintln!` where the sample is not
installed, a green that asserted nothing, on the one leg the macro does not cover. Endpoints
come from one environment variable per field (no URL to parse, no password to encode), defaulting to this project's own test bed.
**Nothing here touches a database it did not create.** `scratch.rs` generates every name with the
`schemaic_it_` prefix, the process id and the leg, and both the create and the drop path assert
the prefix — the drop guard runs during unwinding, where nothing else is checking anything, and
it deliberately does not panic while panicking (a second panic would abort and take the real
failure's message with it).
**The name's uniqueness is now *enforced* rather than stated.** The label is hand-written at 65
call sites and the suite runs threaded, so two tests picking the same one share a database and
each drops the other's tables, with failures that read as anything but a name collision — and
nothing checked it, since a duplicate only reaches `CREATE DATABASE` as an error if the two happen
to overlap, which is exactly the race that does not reproduce. `claim_name`/`release_name` hold a
process-wide set of the whole name (so the same label on two legs is fine, the leg being part of
it) and release it on teardown, so create → tear down → create again is not refused. Teardown is
**bounded at `TEARDOWN_TIMEOUT` (30 s)**: a `DROP DATABASE` blocks behind an open transaction —
measured on MariaDB, and the manual-transaction tests are what supply one — and an unbounded
`block_on` inside a `Drop` hangs the whole binary with no output at all, where a timeout turns
that into the named leftover the message is for. (The `timeout` is constructed *inside*
`block_on`, since building a `Sleep` before the runtime is entered panics.) And `Scratch` marks
itself `torn` **only once it is**, which is the whole point of the flag: set at the top, a panic
from the extra-databases loop left `torn = true` with the primary database still there and `Drop`
returning immediately — the one path where the tier silently did not clean up after itself.
`ScratchAccount::drop` closes the matching hole on the account side: it **used to only print**, so
a panicking test left a real login on a real server, named but present. It now drops the account
on its own thread with its own runtime, on `Scratch::drop`'s terms — a drop is synchronous and
blocking on the current runtime from inside one deadlocks — and prints rather than panicking while
panicking. CI runs the tier as a blocking `live` job with three service
containers, **under `timeout-minutes: 15`**: the tier has both a channel that can wait forever and
a `DROP DATABASE` a held transaction can block, and without a ceiling a wedged test holds a runner
for the six-hour default while the failure reads as "still running" rather than as itself
(generous against a cold service container — the whole tier runs in ~10 s locally). The lint job
compiles it via `--features schemaic-db/live-tests`, since otherwise it
would be the one code in the repository no push compiles.
**The SQL Server legs, 2022's and 2025's, run on the default 2 MiB test thread** — no
`RUST_MIN_STACK` — because `mssql::connect_with` boxes the driver's login (see `mssql.rs`).
Before it did, `mssql::a_sequence_restart_is_honoured_and_not_compared` overflowed that thread on
Windows, and the cause was never the compare or the differ recursing: every `Db` call against
SQL Server carried the ~20 KiB login future inline, and a debug poll frame gives each `.await`
site its own slot for the future it builds, so a test with a few dozen awaits had a ~1.4 MB poll
frame. Boxed, that test's body starts with 1.62 MB of stack left where it had 676 KB, and the
whole leg passes on both servers unaided (measured by the change's author). The pin is
`mssql::tests::a_sql_server_future_does_not_carry_the_drivers_login`.
