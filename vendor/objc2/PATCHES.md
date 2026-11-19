# objc2 0.4.1, vendored

This is the `objc2` crate as published to crates.io at 0.4.1, used through
`[patch.crates-io]` in the workspace `Cargo.toml`. Nothing in Schemaic names it:
it reaches the macOS build through `floem-winit` 0.29.5 and `icrate` 0.0.4, and
0.4.1 is the last release on its line, so no upstream version bump is
available. Everything under `src/` is upstream's, byte for byte, except the
change listed below.

**Left out of the copy:** `tests/`, `benches/`, `CHANGELOG.md`,
`Cargo.toml.orig` and the published `Cargo.lock`. The build reads none of them.
`examples/` *is* copied: doc comments in `src/` pull six of its files in with
`include_str!`, so the crate does not compile without them.
`Cargo.toml` is the normalized manifest crates.io serves, with the changes
below.

**Added for vendoring, not changes to objc2:**

- this file;
- `rustfmt.toml`, which keeps `cargo fmt --all` from reformatting upstream's
  code;
- in `Cargo.toml`: the `[[bench]]` target and the `dev-dependencies` are
  removed, because the files they name were not copied, and a `[lints]` table
  allowing every lint is added, as for `vendor/floem` and `vendor/tiberius`,
  because Cargo caps a registry crate's warnings but not a path dependency's.

When the tree no longer pulls objc2 0.4 (a Floem whose winit uses a newer
objc2), delete this directory and the `[patch]` entry.

## Changes against upstream

Each change is marked with a `schemaic patch (PATCHES.md)` comment, so
`grep -rn "schemaic patch" src` finds every one.

### 1. `Ivar<I>`'s `nth` and `nth_back` overrides removed

**Why.** Rust 1.100 rejects both, with E0277 (the `Deref` target's `Output`
is not an iterator), in `src/declare/ivar_forwarding_impls.rs`. That broke the
macOS build in CI on the first run under 1.100. The same impl tripped a
nightly in 2023 (rust-lang/rust#109632), and that time rustc changed back.

**What.** The two methods are deleted, not rewritten. Both are optional
overrides that forward to the inner iterator, so the trait's default `nth` and
`nth_back` take over and the observable result is the same. The compiler's own
suggestion, a `where` clause on each method, is not an option: a trait impl's
method may not require more than the trait declares.
