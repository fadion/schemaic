# `conn/secrets.rs`

`conn/secrets.rs` — the real, keyring-backed `SecretStore` behind `core::secrets`, plus the
`load_connections`/`save_connections`/`forget_connection` wrappers every connection save routes
through and the process-level `last_hydration` they share. It arrived here from `schemaic-app`
unchanged; what it decides — `classify_get`/`classify_delete` as free functions, why a refused
delete is reported only for a secret this load actually read a value out of — is written up
under `core/secrets.rs`, beside the pure half it serves.
**`load_connections_readonly` is that wrapper's headless twin, and the difference is that it
reports instead of repairing.** The app's load self-heals — a `connections.json` still carrying
legacy plaintext is migrated into the keyring and the on-disk copy rewritten blanked — which a
one-shot `schemaic query` must not do as a side effect while the GUI may be running and about to
save its own copy — nor the AI panel's `--mcp-serve` subprocess, which runs *while* the GUI is,
and reads its whole connection this way (`app::ai::keyring_connection`). So it reads through `persist::read_connections_unrecovered` (for that
function's own reasons) and hands back the `ConnectionsFile` **unhydrated** — `list` shows no
secret — and `hydrate_for_cli(conn, supplied)` fills in only the one connection a command runs
against, and only the secrets it will use, through `core::secrets::hydrate_connection`, returning the strings the caller puts on
stderr: `Hydration::cli_notice`, and one saying this connection's secrets are still plaintext
in `connections.json` and to open Schemaic once. It used to hydrate every saved connection —
a keyring round trip per secret the command would never use — and a locked keyring then
reported all of them in the app's Markdown `notice`, which tells the user to leave form fields
blank. `supplied` is what the caller got another way, `DbPassword` under `--password-stdin`,
and the keyring is not asked for it at all. It never touches
`last_hydration`, which exists to stop a later *save* deleting a secret it could not read, and
there is no later save here. An unmigrated file still works — the plaintext it carries hydrates
a connection exactly as a keyring entry would — it simply stays plaintext.
The target-gated `keyring` backends
came with it and are in this crate's manifest now: Windows Credential Manager, macOS Keychain,
and on Linux the **pure-Rust** `async-secret-service` and not `sync-secret-service`, which pulls
`dbus-secret-service` → `libdbus-sys` and would put a C system library into the portable
glibc-2.31 zigbuild the tarball and the AppImage are built from. `schemaic-app`'s manifest keeps
a two-line pointer where those blocks used to be.
**The manifest asks for `async-io`, and for a long time did not get it.** floem's `rfd-tokio`
feature turned on `rfd/tokio` → `ashpd/tokio` → `zbus/tokio`, cargo unifies features across the
build, and zbus's `tokio` backend won — so every blocking keyring call on Linux ran
`Runtime::block_on` on a runtime zbus kept, and Tokio panics when that happens on a thread
already driving one. This
entry and the manifest both used to say the opposite ("blocks on its own executor rather than
nesting a tokio runtime"), and `schemaic query -c <conn>` exited 101 on Linux with zbus's
"Cannot start a runtime from within a runtime" (issue #1): `cli::run::main` does everything
inside `rt.block_on(dispatch(..))`, and `hydrate_for_cli` → `KeyringStore::get` ran there.
`schemaic list` reads no secret and never tripped it; the GUI reaches the keyring from its UI
thread, which drives no runtime; Windows' Credential Manager is synchronous. **So
`KeyringStore::{get,set,delete}` each run their whole body, `Entry::new` included, through
`off_runtime`**, a `std::thread::scope` thread with no runtime context whatever the caller's,
whose panic is resumed on the caller. It is fixed at this boundary rather than at the CLI's call
site so that no caller, present or future, can trip it; the price is a thread spawn per secret,
against a D-Bus round trip per secret. Don't "simplify" it back to a direct call because the
GUI works without it — **nor because zbus is on `async-io` now.** The same `rfd-tokio` was
silently killing every Linux file dialog (*Floem 0.2 gotchas*), and the switch to
`rfd-async-std` that fixed those gave this manifest the backend it asks for; `off_runtime` stays
regardless, because "a keyring call never runs on the caller's thread" should not rest on a
backend any dependency in the build can flip back.
`a_keyring_call_made_inside_a_runtime_does_not_start_one_on_its_thread`
runs a nested `block_on` inside an outer one through `off_runtime`, and with `off_runtime`
stubbed to `f()` it panics with the issue's exact message. It is why `tokio` is a
dev-dependency here, and only that. `zbus_is_not_on_its_tokio_backend` pins the backend itself:
it `include_str!`s the workspace `Cargo.lock` and fails if zbus's resolved dependency list names
`tokio` — the outcome of feature unification across the whole build, which no single manifest
shows. It checks every `zbus` the lock resolves, not the first, since a second major version
can sit alongside and either one on `tokio` is the bug.
**The move took this file out of the UI crate's source-gate census for a while**, and it is
back in: `source_gate::workspace_sources()` lists `schemaic-conn` with a floor of its own (2),
and `the_wider_scan_reaches_the_keyring_store` asserts this file is among what it reads —
`ui/source_gate.rs` has the per-label floors.
