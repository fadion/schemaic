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

