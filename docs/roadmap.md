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

