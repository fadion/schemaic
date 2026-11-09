# Schemaic — roadmap

The larger pieces of work that are planned but not started, each with the decisions already taken,
what is deliberately out of scope, and the order it should land in — several sessions, a design
choice or two, traps worth writing down before anyone walks into them.

Keep it disjoint from its neighbours: a fact about the system as it *is* belongs in
the architecture document (`architecture.md` and `architecture/`), an instruction about how to
work belongs in `AGENTS.md`. When a phase here lands, its facts move into the architecture files in
the same change and the phase is deleted from this file rather than ticked off.

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
2. **Charts:** the two chart kinds.

## Desktop app

### Small gaps

Each of these is a one-session change.

- **Charts:** add scatter and stacked bar.
- **A view's and a routine's comment in *Copy DDL* and the dump, on PostgreSQL and SQL Server.**
  Their scripts drop both today — `COMMENT ON VIEW` / `COMMENT ON FUNCTION`, and the
  `MS_Description` property — while a table's is kept. DuckDB's are restated
  (`ddl::script_restates_object_comments`); turn the predicate on per engine with the statement
  its edits already write, and check each engine's dump tests.

### DuckDB as a fifth engine

Phase 1 has landed: the engine reads, runs scripts and DDL plans, introspects, exports, plans,
backs up and opens its CLI, behind its own read-only gate (`docs/architecture/db/duckdb.md`).
Phase 2's grid write-back has landed too, on provenance read from the statement (`rowid` is never
a key, because a checkpoint renumbers it), and so have import, the `.sql` dump and Manual tabs (a
failed grid commit there rolls the whole transaction back, DuckDB having no savepoint), and so
has the table designer — in place where DuckDB's `ALTER TABLE` can, by a rebuild where it cannot
— and so have the view and macro editors, **Open data file**, a CSV, Parquet or JSON file
read through a view in an in-memory session that is never saved, and **schema compare** between
two DuckDB files, which creates and drops schemas and sequences as well
(`docs/architecture/db/duckdb.md`). Phase 2 is done; what is left is below. ClickHouse would
come after DuckDB.

- **Decided — the unattended paths cannot read files:** done in phase 1 (`Db::sealed`, called by
  `cli::query`). The editor reads files as normal.
- **Decided — linked in, extensions off:** done in phase 1.
- **Decided — one instance per file, shared while operations overlap:** done in phase 1, and
  stated in the invariants. A Manual tab's session holds the instance — and with it the file's
  lock — for as long as the tab stays in Manual mode, and the connection form's hint says so.
- **Loose ends:** nothing alters a DuckDB sequence once made (`ALTER SEQUENCE` refuses every
  option on 2.0.0), so a compare that finds one changed has no statement for it, and its form
  edits only the comment; a re-create would have to drop it, which DuckDB refuses while a column
  default uses it. A file named
  `main.duckdb` makes `main.t` both a schema and a catalog, and its results are not attributed;
  accept the reading where both resolve to the same table, if anyone hits it. A user type is read
  for the dump alone (`DbSchema::user_types`): the tree does not show one and schema compare does
  not compare them. In a Manual tab, a read the user stops — the
  binary-cell panel dismissed mid-read, a re-fetch stopped — aborts the transaction (measured),
  since there is no savepoint to fence it in; reading such a cell on a fresh connection would see
  none of the tab's uncommitted writes, so it is left an honest loss rather than a wrong answer.
  An Open data file session is out of the AI panel's reach: its MCP server is a subprocess that
  reads `connections.json`, where a session never is, and could not share the in-process database
  if it did.
  The read-time options of `read_csv` (delimiter, header, types) are DuckDB's sniffer's alone; a
  file it guesses wrong needs the view rewritten by hand.
- **Phase 3:** the full CI bar on every leg, which builds the amalgamation cold (about five
  minutes on this machine).

## MCP server

### Non-goals

- **HTTP or SSE transport.** Stdio keeps access control at the process boundary. A listening socket
  would need auth, and adds attack surface for little gain.
- **MCP resources and prompts.** Few clients use them well, and the tools already cover what they
  would carry.
