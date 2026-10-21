# `db/sqlite.rs`

**`sqlite.rs` is the third engine**, and five things make it unlike the other two rather than a
third set of catalogue queries. **There is no server**: a connection is a *file*
(`Connection::file`), so host/port/user/password/SSH are all inert, `fetch_databases` has nothing
to enumerate (it reports the one database SQLite calls `main`), and `Db::connect` refuses to let
a tunnel port repoint the file. **It still opens the file to answer**, through `ping` — the schema
sidebar empties the tree when a listing fails, which the other two engines get for free because
their listing is a query; answering `main` without touching the file gave a missing or locked one
a node in the tree whose every child fetch then printed the connect error *inside* it, under a
database that isn't there and beside a header already saying "Disconnected". **"Inert" has to be enforced twice, because the engine picker is
editable on a saved connection and the SQLite form renders no SSH block** — so a connection
switched over from a tunnelled MySQL one kept `ssh.enabled` set with no control anywhere that
could unset it, and every operation on a local file dialled a bastion with a stored credential and
failed outright when that host was down. `Connection::uses_tunnel()` is the one answer every
tunnel site asks — the app's six, the CLI's `connect`, and `secrets::unused_kinds`, which decides
whether the CLI reads the SSH secrets at all. The CLI asked `ssh.enabled` for both for a while, so
`schemaic ping` on such a file read the SSH password from the keyring and dialled the bastion
(`a_file_connection_reads_no_ssh_secret`). And `Connection::sanitized()` — which `DraftSignals::to_connection`
returns through — drops the server side on save so the state cannot exist. `is_networked` likewise
has a single definition in `core::connection`, which `db::Engine` and the form's `DbKind` both
delegate to; it had two, and the third consumer not asking at all is what this cost. That third
  consumer asks now, and so do the two beside it: `Connection::uses_tunnel`, `Connection::uses_tls`
  and `conn_import::needs_password` read `is_networked(&db_type)` where each spelled `!is_sqlite`
  for itself, thirty lines from the predicate whose own doc already said the tunnel decision was
  not asking. Nothing changes today — the hand-spelling was the same boolean — and what the
  delegation buys is that a fifth engine with no server is **one** edit;
  `a_server_less_engine_reaches_no_server_by_any_route` fails if a site goes back to deciding for
  itself, and `a_networked_engine_still_reaches_its_server` is its floor, so the predicate cannot
  pass by refusing everything. `SslMode::caveat` is deliberately **not** among them: its
  `verify_ca_checks_host_name` asks which *driver* this engine runs on, for a `mysql_async`
  defect, not whether there is a server to reach. That predicate is an exhaustive match over the
  dialect because its first spelling, `!is_postgres && !is_sqlite`, sorted SQL Server onto
  MySQL's side when it arrived. The form then told a SQL Server user that Verify CA "behaves as
  Verify full", while that engine's `NameAgnosticVerifier` really skips the name. **The driver is blocking**, so every call runs in
`spawn_blocking` and opens its own connection there — which is not a compromise but exactly the
one-connection-per-operation invariant, at microsecond cost on a local file; cancellation goes
through `Connection::get_interrupt_handle`, the analogue of `KILL QUERY` that needs no second
connection. **A batch is one operation, not one per statement**, and `sqlite::run_batch` is the
arm that makes it so: one connection in one `spawn_blocking`, the whole statement list run on it,
each outcome sent over an unbounded mpsc channel so the UI still fills result tabs progressively,
and the `InterruptHandle` handed out **before** any work so Stop reaches a mid-flight statement —
the same shape `fetch_query` and `run_script` use. `spawn_blocking` rather than `run_script`'s
`block_in_place`, because the results have to reach an async `FnMut` as they arrive and the suite
runs on a current-thread runtime, where `block_in_place` panics. "Stop on error" is kept as the
other two arms keep it: every statement after a failure reports `Cancelled`, those statements
having been written against a state that never happened. The arm this replaced looped
`fetch_query` — a connection per statement — which made both pragmas the twelve-step rebuild
writes into its own plan inert and cost two Criticals; see the connection invariant for what they
were, and `the_script_guards_itself_when_run_outside_run_ddl` (which was itself green against the
cascade it guards, having opened one connection where the real path opened many) and
`a_rebuild_survives_a_view_over_the_table_it_rebuilds` for the tests that pin it now. **The arm's
own failure paths have a test module of their own** — `sqlite::run_batch_tests`, over in-memory
SQLite, the one backend this workspace may ask directly. Four tests: a failed statement stops the
batch and the rest report `Cancelled`, with the middle entry carrying the *server's* own text at
its own index (the UI keys result tabs on that index) and the table proving statement 3 never ran;
an already-cancelled token reports every index as `Cancelled` and writes no rows; a connection that
cannot be opened is **statement 0's error** with the rest cancelled by the drain, which is the shape
the MySQL arm has and the one arm here that never runs a statement at all; and a succeeding batch
delivers each statement once, in order, so the other three are about the failure arms rather than
about the loop.
**`run_ddl` wears that shape too now, and it was the leg left out of it.** The DDL preview offers
Stop on SQLite — `ddl_rolls_back_as_a_whole(Sqlite)` is true, so the footer's only enabled button
is a red *Stop* and Escape maps to it — while the runner consulted the token only *between*
statements. A rebuild's copy step is one `INSERT INTO t_schemaic_rebuild (…) SELECT … FROM t` over
the whole table, so on a multi-million-row table the modal could not be dismissed and its backdrop
covered the workspace for the length of the copy with the button still reading Stop. On a
**single-statement** plan it was worse than a hang: the token is checked *before* each statement,
so a Stop arriving during the only statement was never seen and the plan ran on to its `COMMIT`.
It now takes `get_interrupt_handle` off the connection before entering the blocking section —
simpler than `run_script`'s, which has to hand the handle back *out* because it opens its
connection inside — with an async watcher that fires it when the token trips. An interrupt
surfaces as an ordinary `rusqlite` error, so the token is what tells the two apart: reporting
`SQLITE_INTERRUPT` as a failure would name the user's own Stop as a fault in their plan. The pin
is `a_stopped_plan_interrupts_the_statement_rather_than_waiting_for_it`.
**A long read blocks a write here, and on neither other engine** — SQLite takes one
write lock over the whole file, so a whole-table export or import holds a write off until it
finishes. Measured against a real 53 MB file in rollback-journal mode (`journal_mode = delete`,
the default for a file not already in WAL): a `stream_query` over 400,000 rows drained slowly,
with an `UPDATE` on a second connection issued a quarter-second in, **waited 3,495 ms and then
succeeded** when the export finished inside the busy timeout and **failed after 5,536 ms** when it
did not. **How long a write waits is this app's number, not the driver's**: `open` sets
`busy_timeout` to 15 s explicitly, where rusqlite's own 5 s was an implementation detail the app's
behaviour rested on — both figures above are that default, and both move if it changes. Fifteen,
because the band between 5 and 15 is where a wait actually *resolves* (a chunk flush, a commit, an
import's transaction), while past it the blocker is a read as long as the table is big and waiting
minutes with no way to cancel is worse than a refusal that says why. That refusal is
`query_err` answering `QueryFailed(QueryFailure::SqliteLocked)` — SQLite's own sentence with the
advice under it, its English in core — on `SQLITE_BUSY`/`SQLITE_LOCKED`, read off `ErrorCode` and
never matched in the message text (`is_lock_failure`) so a reworded SQLite build cannot quietly
drop it: the engine's own sentence is *database is locked*, which names neither the user's other
operation nor the way out, and someone who has only met two MVCC engines has no reason to connect a
failed cell edit to the export running in another tab. **WAL is named rather than applied** — the
advice spells out `PRAGMA journal_mode = WAL` and says Schemaic will not run it, because
`journal_mode` is a persistent property of the user's file, it adds `-wal`/`-shm` siblings, and it
is unavailable on some filesystems; changing someone's database as a side effect of a failed cell
edit is not this layer's call. `db::lock_wait_sql` answers the empty string here for the matching
reason: SQLite has no lock-timeout *setting*, the wait being this per-connection busy timeout, so
the failure that statement bounds elsewhere has no analogue. **Values are dynamically typed**: a declared type is an *affinity*, so `value_of`
reads the storage class of the value in front of it rather than trusting the column, and a BLOB
renders as its size, having no lossless text form. **Column provenance comes from the driver, gated
on the statement's shape.** SQLite's C API has `sqlite3_column_table_name`/`origin_name`, but only
under `SQLITE_ENABLE_COLUMN_METADATA`, which is what rusqlite's `column_metadata` feature sets —
the workspace `Cargo.toml` turns it on and the comment above that dependency holds the
configuration detail. `attach_origins` reads it off the prepared statement it is now handed
(`Statement::columns_with_metadata`; the accessors live on `ColumnMetadata`, **not** on `Column`).
It could not have been asked for before rusqlite 0.40, which is the release that first offered the
feature at all — so the statement derivation it replaced was a description of the driver before it
was a choice. **What it buys is a join, a subquery or a CTE**: provenance is per column and resolves
straight through those to the real base column, so each column is attributed to the table it
actually came from and `edit::analyze_edit` groups them into an editable table each — which is what
MySQL and PostgreSQL have always done from their own wire provenance. The edit layer needed **no
change at all** for that: `analyze_edit` already groups by `(database, schema, table)` and
`resolve_key`'s duplicate-column guard is scoped per group, so the two `id` columns of a two-table
join do not collide. `a_join_attributes_each_column_to_its_own_table` walks one through
`analyze_edit` and asserts two keyed tables, and `a_subquery_resolves_through_to_the_base_table`
covers the derived table. An alias was already right under the positional derivation and still is —
measured, `SELECT name AS qty, qty AS name` reports the swapped real columns.
**The gate is not ceremony**: provenance names the table a result *column expression* resolves to,
not the table a result *row* came from, and `intel::provenance_sources` is where that argument and
its measurements are written down. Three shapes are refused here, and each refusal is deliberate
now rather than a side effect of a derivation that could only ever name one source table. A **set
operation at any depth**, because SQLite reports one branch's provenance for the whole result
(`a_set_operation_is_never_attributed`). A **view**, anywhere in what the statement reads — the
behaviour is unchanged but the reason has grown, because SQLite resolves provenance straight
through a simple view to its base table (measured: `SELECT * FROM v` reports `t`, not `v`), so a
view would otherwise be attributed, and this refusal is the **only** thing standing between a
compound hidden inside a view and a write to the wrong table: `SELECT * FROM big_union` is a plain
single-table read that no parse of the statement can see into
(`a_view_over_a_set_operation_stays_unattributed`). That check covers **every** relation the
statement reads rather than a single source, so a view anywhere in a join refuses the whole result
(`a_view_anywhere_in_a_join_refuses_the_whole_result`). The third refusal — an **`ATTACH`ed or
`temp` relation** — was briefly gone and is **back, deliberately**, which this paragraph did not
say for a week: `b48cab80` lifted it on the ground that provenance named the database per column
all along and only this crate's schema lookups could not follow, and then `b4637c58` put it back
as one of three Critical fixes, because attribution is an offer to *write* and the write runs on
a **different connection** — `commit_writes` opens a fresh one, on which no `ATTACH` has run and
no `temp` table exists. `attach_origins`' own doc states it ("that database must be `main`,
because the write connection has nothing else. An `ATTACH`ed or `temp` relation reads fine and
comes back read-only") and `nothing_outside_main_is_attributed` pins it. A doc claiming a write
is offered where the code refuses it is the damaging direction, so read the refusal as current
and the per-database work below as what makes the *resolution* honest rather than as what makes
an attached table editable.

The per-database plumbing is real and is still needed, because `resolve_relation` has to name the
database a relation is in before anything can decide whether it is `main`. Ten lookups take a
`db: &str` and pass it down —
`table_columns`, `has_rowid`, `table_declares_autoincrement`, `collations_of`, `generated_expr`,
`index_sql`, `index_columns`, `table_indexes`, `single_column_unique_indexes` and
`resolve_relation`. The pragma table-valued functions take the schema as a second bound argument
(`pragma_table_xinfo(?1, ?2)`, `pragma_index_list(?1, ?2)`, `pragma_index_xinfo(?1, ?2)`) and the
`sqlite_master` reads are qualified with `ident_sqlite(db)`. `fetch_schema` passes `MAIN`
**explicitly** rather than letting it be the default, because it introspects the one database
SQLite exposes for a connection and an attached file is not part of that tree; so does
`table_list_flags`, which sits behind the *designer* rather than write-back and so only ever
opens a table from the schema tree, where SQLite has the one `main` node. **`generated_expr` was
the tenth and was missed on the first pass**, found by reading the list back against the code
rather than by a test: a generated column's expression has no pragma behind it at all, only the
table's own `CREATE` text out of a `sqlite_master`, so an unqualified one silently answered
`main`'s — costing `ColumnInfo::generated`, and with it flipping `no_default` true for a NOT NULL
generated column in an attached table. `a_generated_columns_expression_is_read_from_its_own_database`
pins it, on the same fixture, where the two `note` tables generate their `slug` by `lower` and
`upper` respectively so the wrong database is a visibly wrong answer rather than an absent one.
**`is_view` is gone, replaced by `resolve_relation(conn, qualifier, name) -> Option<(String,
bool)>`** — the database the name resolves in, plus whether it is a view — and that is not a
rename, because an unqualified name does not necessarily live in `main`: SQLite resolves it
against `temp`, then `main`, then each attached database in `ATTACH` order. Measured,
`pragma_table_list(name)` lists the schemas holding a name in exactly that order, reports `type`
as `'table'`/`'view'`, and returns nothing for a name that isn't there — so the first row is the
one the statement actually read, where defaulting an unqualified name to `main` would have
described a different table whenever the name lives only in an attached file. It also shuts a
re-entry route for an old bug: the `sqlite_master` query it replaced needed an explicit
`COLLATE NOCASE`, a case-sensitive `=` having once made `SELECT * FROM ARTIST` open read-only
while `SELECT * FROM artist` was editable, and `pragma_table_list` matches the name SQLite's own
way, so it cannot come back through this door. `attach_origins` therefore resolves **every**
relation up front into a `(database, table)` list, refuses the whole result if any of them is a
view or isn't found, keys its per-table pragma cache on the pair, and attributes a column only to
a pair in that list — matched on the database as well as the name. `ColumnOrigin::database`
carries the real database instead of the constant `MAIN`, which is what `edit::analyze_edit`
already groups on, so two files each holding a `note` are two *distinct* tables rather than one —
and then only `main`'s is attributed, per the refusal above. Distinguishing them is what stops a
write meant for one landing on the other; it is not a licence to write to both.
**The fixture is the guard, and this passage used to name four tests of which three have never
existed.** It also quoted a mutation result — forcing `attach_origins`' schema reads back to
`MAIN` fails "the second of those and nothing else" — against a test that is not there, so the
one sentence claiming a measurement pointed at nothing. The real list is `sqlite.rs`'s five users
of the `attached()` fixture, where `main` and `side` each hold a `note` of the same name and a
deliberately different shape (`body` is NOT NULL in `main` and nullable in `side`; `slug` is
generated by `lower` in one and `upper` in the other, so a read from the wrong database is a
visibly wrong answer rather than an absent one):
`an_unqualified_name_resolves_the_way_sqlite_resolves_it`, which pins the `temp` → `main` →
attached resolution order against `pragma_table_list`'s own `main` → `temp` → attached row order;
`a_view_in_an_attached_database_is_still_refused`;
`a_generated_columns_expression_is_read_from_its_own_database`, the lookup that has no pragma
behind it and so is the easiest to leave unqualified; `nothing_outside_main_is_attributed`; and
`every_attributed_column_is_one_the_bare_write_statement_reaches`, which is the composition the
others stop one call short of. Widen that fixture rather than adding a sixth test beside it when
another lookup learns the qualifier. The helper those tests describe a table through is
`table_info_in(conn, db, name)`, with `table_info_of` delegating to it, and it mirrors the UI's
`schema_for` closure in being keyed on the origin's `database` — so a table of the same name in
`main` can never stand in for an attached one. In the running app that closure answers **`None`**
for an attached database rather than its schema, because the SQLite tree holds the single `main`
node `fetch_databases` reports; the write is still keyed, from the wire flags `attach_origins`
set, which is the `resolve_key` fallback the other two engines take when the schema isn't loaded.
What it costs is what the schema alone can answer — `byte_cap`'s `col_cap` is unfilled for those
columns.
**The rowid spelling is the one thing provenance cannot answer, so the statement still does.**
Measured: `_rowid_` and `oid` both come back with `origin_name` `"rowid"`, and on a table that has
taken `rowid` for a real column of its own the true rowid and that column are indistinguishable
through provenance — while the `WHERE` the write-back builds has to name the spelling that actually
reaches the rowid. So `attach_origins` still calls `intel::projection_of`, **solely** to recover
that spelling, and attributes no implicit key when the statement isn't the shape that produces one.
The positional zip survives for that and nothing else, width check included, so a projection it
cannot line up now costs an implicit key rather than misattributing a column. The
declared-column-wins-first ordering described next falls out of the structure: a declared `rowid`
column is found in the table's own column list before the implicit branch is reached at all.
**Every rowid table has a key, and it isn't a column.** A table with no primary key
and no usable unique index is read-only on the other two engines because there is genuinely no
way to name one of its rows; on SQLite there always is one, unless the table was declared
`WITHOUT ROWID`. Such a table is therefore opened as `SELECT rowid, * FROM t`
(`filter::table_query`) and its leading column marked `ColumnOrigin::implicit_key`, which is what
`edit::resolve_key` falls back to. Three details there are load-bearing. `has_rowid` asks
**`PRAGMA table_list`'s `wr` column** rather than searching the stored `CREATE` text, which a
comment, a newline, a trailing `, STRICT` or a column named `without_rowid` each defeat; a view or
a name that isn't there answers `false`. `implicit_row_key` **chooses** the spelling from
`ROWID_ALIASES = ["rowid", "_rowid_", "oid"]` instead of using the constant `"rowid"`, because
SQLite lets a table declare a column called `rowid` and then *that* column is what the word means:
the first unshadowed alias wins, case-insensitively, and a table that has taken all three has no
way left to name its rowid and stays read-only. And `attach_origins` looks a declared column up
**first**, synthesising an implicit-key origin (`not_null`, `auto_increment`, not `primary_key`)
only for an unshadowed alias on a table that has a rowid, recording the name **as written** so the
`WHERE` the write-back builds resolves to the same value the `SELECT` read. `GridWrite::plan`, the
delete → update → insert order, the 1-row safety net and `one_row_verdict` are untouched — the key
simply names a column the table doesn't have, and SQLite resolves `rowid` in a projection and in a
`WHERE` alike, which is also why the in-place splice re-fetch works
(`a_keyless_table_writes_back_through_its_rowid`,
`a_refetch_reads_a_keyless_row_back_by_its_rowid` — which asserts the template's `confirm_cols`
as well as its key columns and builds the key it re-fetches with through `edit::refetch_key`, the
app's own composition, rather than by hand).
**That second test's round trip is satisfied by `WHERE rowid = 2` alone**, which is to say it
passed with the engine's `.chain(confirm_cols)` deleted: the confirming columns are only visible
when they *disagree*. It has a second phase now — move one of them behind the key's back, the way
another session or an `AFTER UPDATE` trigger would, and the same re-fetch must match nothing,
while a truncated key would still return the row. The MySQL builder's copy of that chain has no
round trip at all to exercise it, `confirm_cols` being empty on every non-SQLite engine, so
`db/lib.rs`'s `build_refetch_sql_confirms_with_the_columns_after_the_key` asserts the `WHERE` it
emits from a template carrying both halves — key placeholders first, which is the order
`edit::refetch_key` binds in.
**But a rowid is not a row identity, and the safety net alone cannot see that.** SQLite reassigns
them: the twelve-step rebuild used to renumber a keyless table, an insert after a delete takes the
freed number, `VACUUM` compacts them — and nothing re-runs an open result tab when any of that
happens, so the grid can hold a number that now names a *different* row. Keyed on the number
alone the `UPDATE` lands on that row and affects exactly 1, which is the number `one_row_verdict`
is looking for; the net's whole premise is that a stale key matches **zero**. Two things restore
it. `EditTable::confirm_cols` carries the values the grid read into the same `WHERE` — the rowid
still does the identifying and the values only *confirm* it, which is why this is not the
match-on-all-values scheme a keyless table makes unsafe (duplicate rows are legal there, and the
rowid tells them apart). It is populated only for an implicit key, excludes any column
`edit::holds_bytes` answers for — its cell is a `<n bytes>` placeholder rather than the value, and
on SQLite that includes a blob living in a `TEXT`-declared column, which the wire flag alone
missed and which therefore went into the `WHERE` as the literal placeholder text and matched zero
rows — and `edit::row_key` is the one builder that appends it,
so update, delete and the row panel's immediate save cannot disagree about what a row is. **An
empty set is a refusal**, not a widening: with nothing to confirm with, the `WHERE` is the rowid
alone and the premise above is simply false again, so `analyze_edit` leaves the table out and the
result is read-only — which costs the blob write on a table whose every non-key column holds bytes,
and is what that shape had before the blob write existed. And
`sqlite_rebuild_sql`'s copy now names `rowid` explicitly — gated on `TableInfo::implicit_key`
being reachable and on no draft column shadowing any of the three spellings — which stops the
renumbering at source and preserves the gaps a delete left. A `WITHOUT ROWID` table is unchanged in every
respect — SQLite will not even prepare the statement against one — and a view gets
`implicit_key: None`. Verified end to end against a copy of the EdgeCases file: `keyless` opens as
`SELECT rowid, * FROM keyless ORDER BY rowid ASC LIMIT 100`, reports `editable = [false, true,
true]`, and an `UPDATE` plus a `DELETE` committed through it affected exactly 2 rows, while a bare
`SELECT * FROM keyless` stays read-only. **Showing the rowid is the trade, and it was the
argued-for one.** Hiding it would mean a result column that isn't a column, which `ResultSet` has
no notion of, so export, copy, aggregates, virtualization, widths, the header filter and the row
panel would each have to learn to skip it and every site that forgot would leak it — and the grid
would stop agreeing with the SQL in the editor. Re-fetching the rowid per row at write time is
circular: identifying the row is the problem, and matching on all non-key values is exactly what a
keyless table makes unsafe, duplicate rows being legal there. The cost is one extra column; the
gain is no new concept in `ResultSet` or the grid, the reason a table is editable staying legible
in the statement, and a hand-typed `SELECT rowid, * FROM t` that is editable with no
"this came from the tree" plumbing.
Introspection is `sqlite_master` plus the pragmas, and three of its decisions are
the sort that only show up against a real database: `table_xinfo` rather than `table_info`,
because only the former reports a **generated** column and a write path that can't see one offers
to insert into it; a non-`INTEGER` `PRIMARY KEY` is reported **nullable**, because SQLite
documents that it really is; and an index whose expression key the pragmas don't return is
marked `lossy`, since an index edit is a drop-and-create and would otherwise silently change it —
as is a partial index whose predicate `index_predicate_of` (below) cannot cut from its text, which
would otherwise come back widened to every row. That last one is also why every index carries `IndexInfo::create_sql`, the statement
that declared it — `sqlite::index_sql` is one `sqlite_master` query behind two consumers, Copy
DDL's appended `CREATE INDEX` block and the per-index text a rebuild replays instead of
re-emitting a `lossy` index from the model. It is `None` for an index SQLite wrote itself to back
a `UNIQUE` or `PRIMARY KEY`, whose `sql` is NULL because it is part of the table's declaration.
A fourth is a **non**-finding worth recording, because it looks like a gap: a table
whose primary key is an `INTEGER PRIMARY KEY` reports **no indexes at all**, since that column
*is* the rowid and SQLite builds no separate index for it. Nothing needs to be synthesised —
`edit::resolve_key` reads the primary key off `ColumnInfo::primary_key`, not off the index list,
so write-back keys on it correctly (verified end to end against a real file); inventing a
`PRIMARY` index entry would only put an object in the tree that the database doesn't have. That
is not in tension with the implicit key above, which *is* synthesised: there the table declares no
key at all and the rowid is the only identity it has, whereas here the table already has a key and
it is a real column — the rowid under another name.
**A generated column's *expression* has no pragma of its own**, so `generated_expr_of` digs it out of
the table's own `CREATE` text — and `as_expression`, the scan that finds this column's `AS (…)`,
did not actually track depth: its only write was `b'(' if depth > 0 => depth += 1`, an arm
unreachable from zero, so both terminators fired at the **first** comma or close paren at any
nesting. For `total NUMERIC(10,2) GENERATED ALWAYS AS (price*qty) STORED` that is the comma
inside the precision — digits are not `is_word_start` — so the expression was never found and the
column came back `generated: None`. A `VARCHAR(255)` is enough, and so is a column-level
`CHECK (…)` written ahead of the clause. What it cost is worse than a missing field:
`table_columns` still set `generated_stored` from `hidden == 3`, so the column *was* seen as
generated and only its expression was lost, while the emitter writes the clause only inside
`if let Some(expr)` — so the next rebuild declared a plain column, and `sqlite_rebuild_sql`'s copy
list filters on `generated.is_none()`, which *included* it in the `INSERT … SELECT` and carried
the stale values across with no error. `diff` could not see it either, both sides reading `None`,
so the round-trip gate stayed green. `ddl::unrestatable_sqlite_clauses` is the sibling that
tracks depth correctly, and is what this arm was meant to be.
**CHECK constraints have no pragma at all**, and `fetch_schema` used to hard-code
`check_constraints` empty — harmless until a rebuild could write the table from the draft, where a
check missing from the draft is a check the rebuild silently drops. `checks_of` reads them off the
table's own `CREATE` text instead, the position `generated_expr_of` is already in and with the
same tools: the shared boundary lexer, so a column called `check_sum` or the word inside a string
or a comment can't match, and `core::sql::balanced_paren_span` for the predicate, which may
perfectly well hold a comma or a `')'` inside a literal. A column-level and a table-level check
read alike, because SQLite makes no distinction once the table exists and a rebuild restates both
as table constraints; a constraint written without `CONSTRAINT <name>` keeps an **empty** name,
which is the honest answer rather than a gap (`sqlite::check_text_tests`/`check_schema_tests`).
**A partial index's predicate has no pragma either**, and `index_predicate_of` cuts it from the
index's own `CREATE INDEX` text (`index_sql`) with the same tools. The key list is the first `(`
in code, closed by `balanced_paren_span`; past any comments the next word must be `WHERE`, since
SQLite's `CREATE INDEX` has no other clause there; and the predicate runs to the last code byte
before the statement's `;` — **the stored text carries one**, which was found when a test read
back `deleted_at IS NULL;`. A comment *after* the condition is dropped, because restated in front
of a `;` a trailing `-- note` would comment the terminator out; one *inside* it is text and kept
(`index_predicate_tests`). `table_indexes` sets `predicate` from it and is `lossy` only for a
partial index whose text yields none, or an expression key. Until it did, every partial index
was `predicate: None` and `lossy` — so a rebuild refused any column rename under one, and the
index was invisible to the questions that read the field: `indexes_equal`, `ddl::index_hints`
(it counted as a plain index, so as redundant or as a foreign key's cover), and compare, which
flagged it uncertain. `a_partial_unique_index_is_not_offered_as_a_write_key` now asserts the
predicate is read and the index not lossy; `identifies_a_row` still refuses it on the predicate
itself rather than on `lossy`.
**One catalogue read of the trigger text feeds two consumers** — `trigger_sql` is the query, and
the two things made from it are not redundant (see `TableInfo::dependent_ddl` above).
`trigger_statements` re-terminates each one into `dependent_ddl`, because SQLite stores a
statement without its `;` and a trigger body is full of internal ones, so a replay would
otherwise run into whatever follows it. `triggers_of` parses the *same* text into `TriggerInfo`
through `ddl::sqlite_trigger_info`, this being the one engine where introspecting a trigger means
reading SQL; a statement the parse can't read is left out rather than guessed at, the direction
`view_body_of` already refuses in. `fetch_schema` fills `triggers` for a **view** as well as a
table — an `INSTEAD OF` trigger is the only way a SQLite view is written to — and it fills
`dependent_ddl` for both, since a view's edit is a `DROP VIEW` plus a `CREATE VIEW` on this engine
(there is no `CREATE OR REPLACE VIEW`) and SQLite takes the view's `INSTEAD OF` triggers down with
it, text and all. So the same one catalogue read feeds the same two consumers whichever it is, and
`ChangeSet` replays the statements in both places.
**`all_trigger_sql` is a third read of the same catalogue, and it is whole-database on purpose.**
`trigger_sql` answers per table, which is the wrong shape for `TableInfo::referring_ddl`: the
question there is which triggers *anywhere* name this table, and the table's own are exactly the
ones to leave out. So one `sqlite_master` scan runs ahead of the per-table loop and every table
filters that one list — by owner, then through `intel::code_names`, so a mention in a comment or
in a `'…'` default is not one. The cost is one extra scan per introspection rather than one query
per table, which is what asking the same question N times would have been.
A view now also gets `view_options: Some(…)` rather than `None`. SQLite has none of the options
the other two carry — no definer, security type, algorithm, storage parameters or check option —
and exactly one the re-create behind every view edit would otherwise drop: the explicit column
list, read by `view_columns_of`, a third positional reader over the shared lexer beside
`view_body_of` and `checks_of`. It is the one parenthesised group *before* the header's `AS` at
code position and paren depth zero, both qualifications carrying the weight they do in
`view_body_of` — after that `AS` a `(` is the user's own arithmetic or a subquery.
`EXPLAIN QUERY PLAN` is what `explain` runs — plain `EXPLAIN` disassembles to VDBE
opcodes, which is a different artefact and useless to `core::plan` — and there is no analyzing
form, since SQLite will not execute a statement to time it. **Manual transaction mode is refused**
(`Session::open`): a pinned `rusqlite::Connection` is blocking and `!Sync`, so holding one across
awaits means a dedicated thread and a channel, which is worth building deliberately rather than as
a side effect — and silently running the tab's statements on fresh connections would break exactly
the promise the mode makes. That refusal is the one thing in `session.rs` a test can reach, and it
is now tested twice: that a SQLite `Db` gets `DbError::Connect` rather than a session, and that it
gets it **before** anything is opened (a refusal arriving after a connect attempt would surface as
a file error instead of the explanation, and on a real path would create the file). The engine →
`tx::TxEngine` mapping moved out of the `Session` method into a free `tx_engine_of` for the same
reason — a `Session` cannot exist without a live connection, and reading MySQL's forgiving model
as PostgreSQL's poisoned one is the difference between "still committable" and "discard
everything".
**A backup is `VACUUM INTO`** (`backup_to`, behind `Db::backup_to_file` and the database menu's
*Back up*): a compacted, consistent copy taken under the read transaction the statement holds, so
a write landing meanwhile is in the copy entirely or not at all. The target is a path the user
picked, bound as `?1` rather than spliced in. It writes nothing to the source, which is why a
read-only connection may take one. **SQLite refuses a target that already exists, and that
refusal is passed on** rather than the file being removed first — so picking an existing file in
the save dialog ends in "The backup failed: …" in the terminal panel. The test copies into a
second shared in-memory database, opened first as an empty one, which `VACUUM INTO` accepts as a
target (`a_backup_copies_the_database_whole`), so the suite touches no file.
