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

1. **Data and editor:** *Compare and sync table data* and the two chart kinds.
2. **DuckDB**, alone. A new engine answers every capability predicate and needs its own read-only
   allowlist, so it gets a release with nothing else in it.

## Desktop app

### Small gaps

Each of these is a one-session change.

- **Charts:** add scatter and stacked bar.

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
