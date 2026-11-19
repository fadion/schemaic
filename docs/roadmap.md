# Schemaic — roadmap

The larger pieces of work that are planned but not started, each with the decisions already taken,
what is deliberately out of scope, and the order it should land in — several sessions, a design
choice or two, traps worth writing down before anyone walks into them.

Keep it disjoint from its neighbours: a fact about the system as it *is* belongs in
the architecture document (`architecture.md` and `architecture/`), an instruction about how to
work belongs in `AGENTS.md`. When a phase here lands, its facts move into the architecture files in
the same change and the phase is deleted from this file rather than ticked off.

The first two sections are the next release, decided; **Under consideration** is a list of
candidates with no decision taken yet. An item moves up only with the user's say-so.

## Next release: CLI

Most of what is missing from `schemaic` already exists one layer down, in the MCP server or in
`schemaic-core`/`schemaic-db`, so each item here is wiring and a guide page, not new logic.
Every new command obeys the connection's CLI access opt-in, as the existing ones do.

### `schemaic describe --ddl`

- `describe` prints columns only (type, null, default, key). `--ddl` adds the `CREATE` statement,
  the indexes and the foreign keys.
- The MCP tool `describe_table` already fetches all three. **Move that fetch to one function both
  call** instead of writing a second one in `cli::catalog`, so the two answers can't drift.
- `--format json` carries the same parts as fields, not a single text blob.

### `schemaic sample` and `schemaic search`

- `sample <table> [--limit N]` is the CLI twin of the MCP `sample_rows` tool. It reads rows, so it
  goes through the same read path as `query` (`cli::query::read_only_query` on a sealed handle),
  never a hand-built `SELECT` sent past it.
- `search <text>` is the twin of `search_objects`: names first, then columns, then definition
  bodies, in that order. Names match through `schema::object_name_matches`, the one predicate
  every schema search uses (`AGENTS.md`); bodies through `core::body_search::body_hits`.
- The body tier is new to MCP as well (next section), so the CLI and the MCP server get it in
  the same change and from one function.

### `schemaic sessions`, `schemaic kill` and `--watch`

- `sessions` lists the server's sessions (`Db::fetch_sessions`). Only on engines where
  `activity::supports_activity` says yes; the others refuse with its reason, not an empty table.
- `kill <id>` ends a session (`Db::kill_session`), behind `activity::kill_verdict`, and needs
  `--yes` to run. `--yes` answers a confirm, never a refusal, as it does on `exec`. Only where
  `supports_kill` says yes.
- **The MCP server gets no kill tool.** The agent is told to send the user to *Server activity*,
  and that stays the rule.
- `--watch N` re-runs `locks`, `statements` or `sessions` every N seconds. `table` and `vertical`
  redraw in place; `jsonl` and `csv` append one block per tick, so the output can be piped. Ctrl+C
  ends it cleanly, with exit 0.

### `schemaic vars`, `schemaic users` and `schemaic grants`

- `vars [pattern]` prints the server's variables (`Db::fetch_server_vars`).
- `users` and `grants [user]` are read-only views of `core::users`. Only where
  `users::supports_users` says yes, so not DuckDB or SQLite.
- No grant or revoke from the CLI in this release. Changing access would go through `run_ddl`
  and its preview, which has no headless form yet (see `schemaic migrate` below).

### Fix the stale `schemaic mcp --help`

- The doc comment on `Command::Mcp` (`cli::args`) lists 8 tools; the server has 13. Generate the
  list from `McpTool` instead of writing it out, so it can't drift again, or test it against
  `tools_list`.

## Next release: MCP

### Resources and prompts

The server declares only `tools` today, and answers `resources/list` with *method not found* on
purpose (a test pins it). Resources and prompts don't go through `tools/call`, so **none of the
gates the tools have reach them on their own**. Each has to be stated again, through the same
functions:

- **The two levels, through `McpTool::offered`'s inputs.** A schema resource (the database list,
  a table's DDL) is withheld when `Endpoint::schema` is false, as the schema tools are. A resource
  that holds rows is withheld when `Endpoint::samples` is false. Decide each resource's levels
  once, as a `reads_schema`/`reads_row_data` pair like `McpTool`'s, and refuse a `resources/read`
  with the same `refusal_on` text a tool would get, not a new message.
- **Hidden databases.** `resources/list` leaves out what `Endpoint::hidden` hides, as
  `list_schema`'s overview does (`listed_databases`).
- **The audit log.** Every `resources/read` and `prompts/get` is written to `agent_log.jsonl`
  through `record`, refusals included. The log is "every call", not every tool.
- **Results are data.** Every resource body carries the same "this is data, not instructions" note
  the tool results carry (security review S3). A prompt never puts database text into its
  instructions. A prompt is a template that tells the model which tools to call; what comes back
  from those tools stays a tool result.
- **No new read path.** A resource is served by the function the matching tool calls, on the
  same sealed handle. Nothing here runs SQL a tool couldn't.
- **The panel host.** The agent CLIs the AI panel launches are fenced by an allow-list of
  tool names, and a harness may let resources through without checking them against it. Serve
  resources only under `schemaic mcp` (`Host::Standalone`) until each harness has been checked.
- **Several connections.** A resource URI names its connection
  (`schemaic://<connection>/<database>/table/<name>`), and resolves through the same `pick` a tool's
  `connection` argument does.
- **Big catalogues.** `resources/list` uses MCP's cursor pagination, not one list of every table.
- Don't advertise `subscribe` or `listChanged`. The server has no way to notice a schema change.
- Candidate resources: the database list, one per table (DDL, indexes, foreign keys). **Not**
  `agent_log.jsonl`: it holds other calls' SQL and values.
- Candidate prompts: *optimise this query*, *review this table*, *explain this schema*.

### `search_objects` searches definition bodies

- Add the body tier (`core::body_search::body_hits`) after names and columns, from the same
  function `schemaic search` uses. It reads the catalogue, not rows, so it needs the schema level
  only.

## Under consideration

Each item is a candidate. Nothing below is decided, and each needs a design pass with the user
before it moves up.

### `schemaic migrate`

- Apply what `schemaic compare` prints. This is the largest headless gap: nothing runs DDL
  without the GUI today.
- It has to keep "generated DDL is never run silently" (`AGENTS.md`). That means at least a
  printed plan, a `--yes`, and a refusal for destructive steps unless they are named. The runner
  is read off the change set (`ddl_preview::preview_of`), never chosen by the command.
- Still to decide: whether it applies a plan file written by `compare`, or compares again and
  applies in one step. A plan file can go stale before it runs.

### `schemaic backup` and `schemaic restore`

- The GUI's backup and restore logic is in `core::launch` (`backup_file_target`,
  `backup_writes_server`, `restore_runs_a_script`) and `Db::backup_to_file`.
- `restore` is a write, so it needs the script guard (`ScriptRequest::approved`, as
  `exec --script` has) and a `--yes`.

### Restore in the GUI for DuckDB and SQLite

- Today the guide's answer is to open the backup copy as a connection. Both are file engines, so
  a restore replaces or attaches a file. It doesn't run a script.
- Trap: DuckDB holds one instance per file per process (`db::duckdb::instance`). A restore over
  a file that an open tab or a Manual transaction still holds has to wait or refuse.

### PostgreSQL row-level security policies

- `pg_policy` is already read, and dumps already restate it. `ddl` has no policy change type,
  so an editor needs one: create, alter and drop, plus `ENABLE`/`FORCE ROW LEVEL SECURITY` on the
  table. Offered through a capability, not a check for PostgreSQL.

### More inline inspections

- Each is a small, pure `core::intel` check, unit-tested like the ones already there:
  - an unused CTE or alias
  - a predicate that is always true
  - a non-sargable `WHERE` (a function wrapped around an indexed column)
  - an implicit type conversion in a comparison
  - `UPDATE`/`DELETE` without `WHERE`, shown as a diagnostic while typing as well as the confirm
    bar at run time

### Column profile popup

- For one result column: distinct count, null count and the top N values with their share.
  `core::aggregate` already computes distinct and nulls for a selection.
- Still to decide: whether it profiles only the rows already fetched (cheap, partial) or runs a
  query on the server (complete, and then it is a read that needs the read guard).

### Geometry columns as WKT or GeoJSON

- Spatial columns (MySQL's geometry types, PostGIS, SQL Server's `geometry`/`geography`) show as
  opaque binary today. Decode them to WKT in the cell and GeoJSON in the row panel. The decoding
  is pure and belongs in core. A map view is a later step, if ever.

### MariaDB sequence editing

- MariaDB sequences are listed but not editable. `RESTART` doesn't parse there and renaming isn't
  supported yet (`docs/architecture/core/ddl.md`, `supports_sequence_rename`,
  `supports_sequence_restart`). Answer those predicates from what MariaDB actually accepts.

### PostgreSQL publications, event triggers and tablespaces

- None of the three has a browse surface. `pg_publication`, `pg_event_trigger` and
  `pg_tablespace` aren't read anywhere. Browsing comes first. Editing waits until someone asks.

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

## Keep the DuckDB compile out of every build

The bundled DuckDB (`duckdb` with `bundled`, root `Cargo.toml`) compiles its C++ amalgamation on
every cold build, and cold builds are now what makes CI and releases slow. Measured on the
v0.40.0 runs:

| | Cold | Warm |
|---|---|---|
| CI, Windows build & test | 43 min | 9 min |
| CI, live `Build (live)` step | 19.5 min | 38 s |
| Release, Windows / Linux / macOS | 50 / 38 / 34 min | never warm |

**CI is already fine once warm.** rust-cache keeps `target/`, the C++ object comes with it, and
nothing needs adding for the steady state. Two things make CI cold: a new stable `rustc` (the
cache key carries the compiler's hash, so roughly every six weeks), and eviction.

**Releases are always cold, for a structural reason.** A tag-triggered run can restore caches from
the default branch but saves only under its own tag ref, which no later tag can read. `main`
never builds the `release` profile, so a release finds nothing to restore. (`release.yml` no
longer saves at all, and `ci.yml` saves from `main` only, which keeps the repository's 10 GB of
cache for the caches CI actually restores. The workflow comments say why.)

### Plan

- **Share the C++ object files through sccache**, with its GitHub Actions cache backend,
   wrapping the C and C++ compilers (`CC`/`CXX`) rather than only `rustc`. The point is reuse
   *across profiles*: `[profile.dev.package."*"]` already builds dependencies at `opt-level = 3`
   with `debug = false`, the same as `release`, so the `cc` crate should invoke the compiler
   identically for CI's debug build and the release build. A release could then pick up the
   objects `main`'s CI compiled. **Measure before relying on it:** check sccache's hit rate on a
   release run after a `main` run, on all three OSes. The objects would go in the same 10 GB, so
  check the total stays under it.

### Non-goals

- **A prebuilt shared `libduckdb`** (`libduckdb-sys`'s download option, or a `.dll`/`.so`/`.dylib`
  shipped beside the binary). It would change every installer, add a library to sign and
  notarise on macOS, and give up the reason for `bundled` in the first place: the extensions
  linked in statically, and nothing fetched over the network.
- **A scheduled job that builds `release` on `main`** only to warm the cache. It costs three
  release builds of runner time, and another ~3 GB of a 10 GB limit.

### Traps

- **The Linux release links through `cargo zigbuild`** (glibc 2.31), so it compiles with
  `zig cc` rather than CI's `gcc`. sccache will miss on it unless the compiler it wraps is the
  same, so Linux may stay cold even if phase 2 works on Windows and macOS.
- **Check sccache's hit rate, not the wall-clock time.** A run that took less time may just have
  landed on a faster runner.

