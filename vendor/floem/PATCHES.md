# Floem 0.2.0, vendored

This is the `floem` crate as published to crates.io at 0.2.0 (upstream commit
`574638db95a4cf898f6156fa1db375de7a1d49c3`), used through `[patch.crates-io]`
in the workspace `Cargo.toml`. Everything under `src/` is upstream's, byte for
byte, except the changes listed below — and line endings, which git normalizes
(upstream ships `src/nav.rs` with CRLF).

**Left out of the copy:** `docs/`, `.github/`, `.devcontainer/`,
`CHANGELOG.md`, `CONTRIBUTING.md` and `Cargo.toml.orig` — none of them is read
by the build. `Cargo.toml` is the normalized manifest crates.io serves.

**Added for vendoring, not changes to Floem:**

- this file;
- `rustfmt.toml`, which keeps `cargo fmt --all` — which also formats local
  path dependencies — from reformatting upstream's code;
- a `[lints]` section at the end of `Cargo.toml` allowing every lint. Cargo caps
  a registry crate's warnings but not a path dependency's, so without it
  upstream's dozen warnings print on every build of the workspace. It is the
  manifest, not the source, so `src/` stays upstream's.

To take a new Floem release: vendor it the same way, re-apply every entry
below, and drop any that upstream has absorbed. When the list is empty, delete
this directory and the `[patch]` entry.

## Changes against upstream

None yet.
