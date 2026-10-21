# `schemaic-conn`

`schemaic-conn` — a saved connection and the OS keyring that holds its secrets, and nothing else.
Its whole dependency list is `schemaic-core`, `tracing` and the target-gated `keyring`: **no
floem**, which is the entire reason it exists. `secrets.rs` was `schemaic-app`'s, and
`schemaic-app` is bin-only and links floem and the whole wgpu stack, so nothing without a GUI
could reach `load_connections` to open a saved connection. This crate is that seam, and
`schemaic-cli` is what walks through it. It stays thin
on purpose — the pure transforms remain in `core::secrets` and the `connections.json` I/O in
`core::persist`; what moved is only the half that talks to the OS.
