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
backs up and opens its CLI, behind its own read-only gate (`docs/architecture/db/duckdb.md`). What
is left is below. ClickHouse would come after DuckDB.

- **Decided — the unattended paths cannot read files:** done in phase 1 (`Db::sealed`, called by
  `cli::query`). The editor reads files as normal.
- **Decided — linked in, extensions off:** done in phase 1.
- **Decided — one instance per file, shared while operations overlap:** done in phase 1, and
  stated in the invariants. A Manual tab's session will hold the instance — and with it the file's
  lock — for as long as the tab is pinned, which is the one thing phase 2 must say in the UI.
- **Phase 2 — writes:**
  - **Grid write-back.** Column provenance first: DuckDB has no `column_metadata`, so a result's
    columns have to be attributed from the statement (`intel::provenance_sources`, as SQLite's
    were before rusqlite 0.40), refusing anything it cannot verify. Then `commit_writes`,
    `refetch_rows` and `fetch_blob`. `rowid` is DuckDB's implicit column
    (`intel::implicit_columns`); **measure whether it survives a delete and a checkpoint** before
    it is let near an edit key — a key that moves writes the wrong row.
  - **Import** (`import_rows`, then `import::supports_import`), the **dump** (`dump::supports_dump`;
    every arm already answers), and **Manual tabs** (`Session` for DuckDB, then
    `tx::supports_manual_mode`; `BEGIN TRANSACTION READ ONLY` is the read-only session).
  - **The table designer and the view and macro editors.** `ddl::duckdb_supports` refuses every
    change today. **Trap:** `schema.rs`'s and `ddl.rs`'s DDL emitters branch on
    `dialect == Postgres` in many places, so DuckDB falls onto the MySQL shape wherever a `pg`
    flag decides — audit each one, turning it into a capability, before turning a change on. DuckDB
    also has no `ALTER TABLE … ADD CONSTRAINT` for a key or a foreign key, no index rename, and
    refuses to rename or retype a column anything depends on, so several changes need a rebuild.
  - **Open data file:** a CSV, Parquet or JSON file opened in a throwaway in-memory session —
    which the test-only `memory:` target in `db::duckdb` already models. **Trap:** an in-memory
    instance is its data, so it must be held for as long as the session is open, not closed with
    its last operation as a file's is.
  - **Schema compare** between two DuckDB files.
- **Loose ends from phase 1:** `duckdb::run_ddl` checks its token only before it starts — hand
  it an interrupt handle as `run_script` has, before a plan can reach it; the read-only half of the
  seal (`AccessMode::ReadOnly`) has no test, the suites' in-memory instance having no file to open
  read-only; `db::ident_duckdb` is not in the test that binds the engine quoters (`mysql.rs`); and
  *Compare with* / *Copy to* are offered on a DuckDB connection with every change refused behind
  them — gate the entries, or land the designer first.
- **Phase 3:** translate the new strings (`db-refused-duckdb-*`, `connections-duckdb-*`,
  `backup-kind-duckdb` are English in every catalogue but `en-GB`); measure
  `intel::select_list_subquery_sees_earlier_aliases` (answered `false` unmeasured); the full CI
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
