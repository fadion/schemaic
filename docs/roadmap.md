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
