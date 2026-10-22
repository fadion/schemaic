# Schemaic — roadmap

The larger pieces of work that are planned but not started, each with the decisions already taken,
what is deliberately out of scope, and the order it should land in — several sessions, a design
choice or two, traps worth writing down before anyone walks into them.

Keep it disjoint from its neighbours: a fact about the system as it *is* belongs in
the architecture document (`architecture.md` and `architecture/`), an instruction about how to
work belongs in `AGENTS.md`. When a phase here lands, its facts move into the architecture files in
the same change and the phase is deleted from this file rather than ticked off.

The two macOS entries below came out of the first hands-on run on a Mac, as did quitting when the
last window closes, which has landed. They are meant to ship together, with whatever the next macOS
round turns up, as one release.

## Read connection secrets when connecting, not at startup

After an update, macOS asked for the login password several times in a row, apparently once per
stored secret, until *Always allow* was clicked. Two causes compound:

- **The keychain ties *Always allow* to the app's code signature**, and the macOS build ships
  unsigned (`docs/architecture/app/update.md`), so on Apple Silicon it carries only the linker's
  ad-hoc signature, which is a hash of the binary. Every update is a new app to the keychain, and
  every item asks again. This entry does not fix that. Only a stable signing identity would.
- **The GUI reads every secret eagerly at startup**, one keychain item per connection per
  `SecretKind` (`core::secrets::hydrate_file`, through `conn::secrets::KeyringStore`), so every
  prompt arrives at launch, for connections the user may not open that day.

### Decided

- **Keep the storage layout: one keyring entry per connection per secret, on every platform.**
  The layout stays identical on macOS, Windows and Linux. A single macOS item holding every secret
  was considered and rejected because macOS would then store secrets differently from the other
  two. The cost accepted with it: after an update macOS still asks once per secret, but only when
  that connection is used.
- **The GUI hydrates a connection's secrets the first time something needs them**: connecting
  (the tree, a tab's run, a pinned `Session`, the Live Monitor, an import, a compare), opening its
  connection form, or handing the AI panel's `--mcp-serve` child its password. It reads them
  once per process and keeps them in memory, so one *Always allow* per item lasts until the next
  update. It reads only the kinds the connection uses (`unused_kinds`, as the CLI's
  `hydrate_connection` does). The form is the exception: it reads all three, because it can
  switch the auth method.
- **Lazy on every platform, not macOS only.** One code path. A locked Linux keyring benefits the
  same way: the unlock prompt waits until a connection needs it.
- **Startup reads nothing.** A connection the restored session reconnects to at launch (the active
  connection's tree, restored tabs) still asks then. That is the user's own connection being
  used, so it is acceptable.

### Traps

- **"Never read" is a third state, and the save path only knows two.** `sanitize_file` deletes the
  entry for every empty field that is not `unreadable`, and before this change an empty field
  meant "read and absent" or "the user cleared it". With lazy hydration every connection not yet
  used has empty fields, and the first ordinary save, such as the read-only toggle, would delete
  their credentials. `Hydration` has to record what was *read*, and the save has to leave alone
  any field it never read. This is the regression to test first, red against today's
  `sanitize_file`.
- **Don't rewrite every secret on every save either.** `sanitize_file` currently `set`s every
  non-empty field. A write to an item another signature created can prompt on macOS too, so a save
  should write only the secrets that changed.
- **Every `Connection` a `Db` is built from must have been hydrated first.** The eager load made
  that true for free, and lazy loading makes it a step a caller can forget. A connection that skips
  it shows up as an authentication failure with an empty password, not as a missing secret. Put
  it in the one place connection handles are minted, not in each caller.
- **The prompt runs on whatever thread asks.** Today it is the UI thread at startup, before the
  window is up. Lazily it would come mid-session, and a blocking keychain call on the UI thread
  freezes the window while the dialog is open. Read off the UI thread, as the connect it precedes
  already runs.
- **The legacy-plaintext migration (`needs_resave`) runs at load today.** It reads the file, not
  the keyring, so it can stay eager.

## macOS: check the CLI through its PATH symlink against the keychain

On macOS the CLI is the app's own binary, so the keychain sees the same app that stored the secrets
(`docs/architecture/cli/overview.md`). That was reasoned, not observed, and nobody has run it on a
Mac. Check it in the same macOS round as the entry above. After *Always allow* in the GUI,
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
