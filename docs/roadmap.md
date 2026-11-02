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
The headless release's decisions are taken; the other entries' are not. What an entry lists under
*Open* is for the user to settle before work starts, and the traps are the ones known today.

They ship in this order, one release each. Releases 2 and 3 merge if 3 turns out small.

1. **Headless:** every CLI entry, plus the MCP server's audit log, several connections in one
   server, `compare_schemas` and `propose_sql`. All of it is in `schemaic-cli` and shares the
   CLI's guards.
2. **Performance:** *Workload and performance insight* with the MCP performance tools, which share
   its backend, plus the Server Activity keyboard path.
3. **Data and editor:** *Compare and sync table data*, the custom keymap and the two chart kinds.
4. **DuckDB**, alone. A new engine answers every capability predicate and needs its own read-only
   allowlist, so it gets a release with nothing else in it.

## Desktop app

### Small gaps

Each of these is a one-session change.

- **Server Activity:** Kill session and Cancel query can only be reached with a right-click, apart
  from the lock-wait banner's Kill. Give them a keyboard path.
- **Charts:** add scatter and stacked bar.

### Custom keymap

The shortcuts can't be rebound. Add a keymap file, plus a settings page for it, that rebinds
palette commands. Conflicts are reported, not resolved silently.

- **Trap:** Floem's focus and Tab handling (`floem-gotchas.md`). Some bindings are consumed before
  a keymap would ever see them.

### DuckDB as a fifth engine

DuckDB is embedded like SQLite, sqlparser has a DuckDB dialect, and it queries CSV and Parquet
files directly. That brings "open a Parquet file", plus Parquet import and export, almost for free.
It is far cheaper than Oracle. ClickHouse would come after DuckDB.

- **Trap:** a new engine has to answer every capability predicate listed in `AGENTS.md`. Answer
  each from what DuckDB actually does, never by putting it on SQLite's side of an `== Sqlite`
  check.
- **Trap:** the read-only gate is an allowlist per dialect. DuckDB's file-reading functions
  (`read_csv`, `read_parquet`, `COPY … TO`) read and write the *client's* filesystem, so they need
  their own entries. `COPY … TO` is a write.
- **Open:** a file-backed database only, or also in-memory sessions over loose files?

### Workload and performance insight

Server Activity shows *who* is running and what they are waiting on, but not *why it's slow*. Table
statistics (`core::stats`) already cover one table at a time: its sizes, row counts, dead rows and
free space, and per-index usage with `IndexStats::is_unused`. What is missing is the server-wide
view. This is shared with the MCP performance tools below, so build the backend once in
`schemaic-db` and `schemaic-core`, reusing `core::stats` rather than reading the same figures twice.

- **Sources:**
  - `pg_stat_statements`.
  - MySQL and MariaDB `performance_schema` statement digests.
  - SQL Server Query Store, plus the missing-index DMVs.
- **Views:**
  - Top statements by total and mean time.
  - Unused and duplicate indexes across the whole database, not one table at a time.
  - The largest tables and indexes, ranked.
- **Trap:** each source may be absent:
  - `pg_stat_statements` is an extension that might not be installed.
  - `performance_schema` can be off.
  - Query Store has to be turned on per database.

  Ask a capability of the live server and explain what is missing; don't show an empty view.
- **Trap:** suggesting an index produces DDL. That goes through the normal preview modal and is
  never run silently.

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

### Audit log of agent calls

`exec` writes to the write log, but MCP reads are logged nowhere. On a production connection, what
the agent looked at matters too.

- **Decided:** a separate `agent_log.jsonl` beside the write log, in its format and with its
  masking. Each line holds the tool, the connection, the database, the SQL, the row count, the
  outcome, and whether the call came from the panel or a standalone server.
- **Decided:** on by default. Agent reads are frequent, so unlike the write log it is capped:
  past a size limit it rolls over to one `.1` file.

### Several connections in one server

One process serves one connection today, so working across databases needs one client entry per
connection.

- **Decided:** repeat `-c`. The first is the default for a tool called without a `connection`
  argument; `list_connections` names the rest. There is no `--all`: each client config names
  exactly what it exposes.
- **Decided:** `--url` works here as on the other commands, with `--ai-data` for its data level,
  defaulting to schema only — the one CLI command that reads the level. Its password comes from
  `SCHEMAIC_PASSWORD` only: stdin is the protocol's.
- **Trap:**
  - Each connection still needs CLI access.
  - The `AiData` level is read per connection, not once for the server.
  - SSH tunnels open lazily, per connection.

### `compare_schemas`

A tool that wraps `cli/compare.rs` and returns the migration as text. It executes nothing.

- **Decided:** both sides are connections this server was given, so it lands after *Several
  connections*. It reads only the catalogue, so like `list_schema` it needs no data level. Both
  sides are the same engine, as in the CLI.

### `propose_sql`

Today the only proposal tool is `propose_table_change`. Add a `propose_sql` that validates through
`run_verdict` and, in the panel, opens the usual preview. It executes nothing.

- **Decided:** panel only, like `propose_table_change`. A standalone server has nobody reading
  the proposal.

### Performance tools

Top statements, index-usage health, missing and unused indexes, table sizes and current locks. They
share their backend with the app's *Workload and performance insight* entry, so they land after it.
They read only, and any index they suggest comes back as DDL to propose, never to run.

### Non-goals

- **HTTP or SSE transport.** Stdio keeps access control at the process boundary. A listening socket
  would need auth, and adds attack surface for little gain.
- **MCP resources and prompts.** Few clients use them well, and the tools already cover what they
  would carry.
