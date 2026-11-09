# Schemaic — roadmap

The larger pieces of work that are planned but not started, each with the decisions already taken,
what is deliberately out of scope, and the order it should land in — several sessions, a design
choice or two, traps worth writing down before anyone walks into them.

Keep it disjoint from its neighbours: a fact about the system as it *is* belongs in
the architecture document (`architecture.md` and `architecture/`), an instruction about how to
work belongs in `AGENTS.md`. When a phase here lands, its facts move into the architecture files in
the same change and the phase is deleted from this file rather than ticked off.

The macOS entry below came out of the first hands-on run on a Mac, as did quitting when the last
window closes and reading connection secrets when a connection is first used, both of which have
landed (`docs/architecture/core/secrets.md`). It is meant to ship with them, and with whatever the
next macOS round turns up, as one release.

## macOS: check the CLI through its PATH symlink against the keychain

On macOS the CLI is the app's own binary, so the keychain sees the same app that stored the secrets
(`docs/architecture/cli/overview.md`). That was reasoned, not observed, and nobody has run it on a
Mac. Check it in the same macOS round as the lazy secret reads. After *Always allow* in the GUI,
run `schemaic query` through the `~/.local/bin` symlink Settings installs, and it must not prompt.
If it prompts per item, `--password-stdin` is the workaround, and the PATH entry should point at
the real binary inside the bundle instead of the symlink. This is a code-signature question,
independent of reading secrets lazily.

## Test against the hosted services

Every engine has been tested only against a self-hosted server. Each service below gets a free tier.
Fix whatever introspection trips on, and answer the `supports_*` predicates from what the server
actually offers. Don't assume it behaves like the engine it is compatible with.

- **PostgreSQL-compatible:** Supabase, Neon, CockroachDB, YugabyteDB, TimescaleDB, Aurora
  PostgreSQL.
- **MySQL-compatible:** PlanetScale, TiDB, Vitess, Aurora MySQL.
- **Cloud IAM sign-in:** verify AWS RDS IAM and GCP IAM against a real cloud account. There is
  none here yet, which is what blocks it.

A service that differs from its engine is answered by a capability, never by a check of which
service it is (`AGENTS.md`).

---

The entries below are grouped by surface and ordered roughly cheapest first within each group.
None of their decisions has been taken yet. What an entry lists under *Open* is for the user to
settle before work starts, and the traps are the ones known today.

They ship in this order, one release each. The headless and performance releases before them
have landed.

1. **DuckDB**, alone. A new engine answers every capability predicate and needs its own read-only
   allowlist, so it gets a release with nothing else in it. Moved ahead of the data and editor
   release by the user on 2026-11-09.
2. **Data and editor:** *Compare and sync table data* and the two chart kinds.

## Desktop app

### Small gaps

Each of these is a one-session change.

- **Charts:** add scatter and stacked bar.

### DuckDB as a fifth engine

Phase 1 has landed: the engine reads, runs scripts and DDL plans, introspects, exports, plans,
backs up and opens its CLI, behind its own read-only gate (`docs/architecture/db/duckdb.md`).
Phase 2's grid write-back has landed too, on provenance read from the statement (`rowid` is never
a key, because a checkpoint renumbers it), and so have import, the `.sql` dump and Manual tabs (a
failed grid commit there rolls the whole transaction back, DuckDB having no savepoint), and so
has the table designer — in place where DuckDB's `ALTER TABLE` can, by a rebuild where it cannot
— and so have the view and macro editors, and **Open data file**, a CSV, Parquet or JSON file
read through a view in an in-memory session that is never saved (`docs/architecture/db/duckdb.md`).
What is left is below. ClickHouse would come after
DuckDB.

- **Decided — the unattended paths cannot read files:** done in phase 1 (`Db::sealed`, called by
  `cli::query`). The editor reads files as normal.
- **Decided — linked in, extensions off:** done in phase 1.
- **Decided — one instance per file, shared while operations overlap:** done in phase 1, and
  stated in the invariants. A Manual tab's session holds the instance — and with it the file's
  lock — for as long as the tab stays in Manual mode, and the connection form's hint says so.
- **Phase 2 — the rest of the writes:**
  - **Schema compare** between two DuckDB files.
- **Loose ends:** the read-only half of the seal (`AccessMode::ReadOnly`) has no test, the
  suites' in-memory instance having no file to open read-only. The designer offers a column
  collation, which `ALTER` refuses to change and the rebuild writes, but the reader does not read
  one back, so a collation set there shows as an edit again on the next open. A file named
  `main.duckdb` makes `main.t` both a schema and a catalog, and its results are not attributed;
  accept the reading where both resolve to the same table, if anyone hits it. The dump carries no
  user type (`CREATE TYPE … AS ENUM`, a `STRUCT` alias): `duckdb_tables().sql` inlines it into each
  column, so the table restores without it, but a view or macro naming the type fails its restore
  — none is read into the model. Nor does it carry a view's or a macro's
  comment (`docs/architecture/db/duckdb.md`). In a Manual tab, a read the user stops — the
  binary-cell panel dismissed mid-read, a re-fetch stopped — aborts the transaction (measured),
  since there is no savepoint to fence it in; reading such a cell on a fresh connection would see
  none of the tab's uncommitted writes, so it is left an honest loss rather than a wrong answer.
  A macro parameter's default (`b := 2`) is in no catalogue column, so such a macro is found by
  binding and left listed but not editable, and its *Copy DDL* and dump say above the `CREATE`
  that the defaults are not restated (unmeasured: whether `EXPORT DATABASE` writes them, which
  would be a source for them).
  A view's **column** comments are cleared by `CREATE OR REPLACE VIEW` too (measured), and an
  edit does not restate them, the new body's columns being unknown until it runs.
  An Open data file session is out of the AI panel's reach: its MCP server is a subprocess that
  reads `connections.json`, where a session never is, and could not share the in-process database
  if it did. A `memory:` path typed into the connection form is not refused at Save — it is only
  never saved, and connecting says the session is closed; a form-level refusal would be clearer.
  The read-time options of `read_csv` (delimiter, header, types) are DuckDB's sniffer's alone; a
  file it guesses wrong needs the view rewritten by hand.
- **Phase 3:** translate the new strings (`db-refused-duckdb-*`, `connections-duckdb-*`,
  `ddl-withheld-duckdb-referenced`, `table-problem-sequence-no-default`, and retranslate
  `ddl-withheld-strands-generated-column`, which no longer names SQLite in `en-GB` now DuckDB
  shares it,
  `backup-kind-duckdb`, `dump-drop-hint-up-front-no-grants`, `write-rollback-transaction`,
  `routine-editor-returns-placeholder-duckdb`, `ddl-risk-view-cost-duckdb` are
  English in every catalogue but `en-GB`); measure `intel::select_list_subquery_sees_earlier_aliases` (answered `false` unmeasured); the full CI
  bar on every leg, which builds the amalgamation cold (about five minutes on this machine).

### Compare and sync table data

There is schema compare and there is compare of two results, but no row-level compare of two tables
with a sync script.

- **Shape:**
  1. Pair rows by primary key or a unique key the user picks.
  2. Show them as added, removed or changed.
  3. Produce `INSERT`, `UPDATE` and `DELETE` for the ticked ones.
- **Trap:** a table with no usable key cannot be paired. Refuse it; don't guess.
- **Trap:** the sync script is a write of user data. It goes through the write guard, and like any
  other write it is transactional, with the report claiming only what the engine delivered.
- **Trap:** large tables can't be held in memory on both sides. Stream both sides ordered by the key
  and merge them.
- **Open:** cross-engine sync, or same engine only, as schema compare is today?

## MCP server

### Non-goals

- **HTTP or SSE transport.** Stdio keeps access control at the process boundary. A listening socket
  would need auth, and adds attack surface for little gain.
- **MCP resources and prompts.** Few clients use them well, and the tools already cover what they
  would carry.
