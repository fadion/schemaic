//! Saved connections, hydrated.
//!
//! The seam between [`schemaic_core`]'s pure connection model and the two
//! OS-facing things a saved connection needs before it can be used: the
//! `connections.json` file (which core already owns, in [`schemaic_core::persist`])
//! and the OS keyring that holds the secrets the file deliberately does not.
//!
//! **It exists as its own crate so the headless CLI can reach a saved connection
//! without linking the GUI.** [`secrets::load_connections`] used to live in
//! `schemaic-app`, which pulls floem and the whole wgpu stack; everything here is
//! `schemaic-core` plus `keyring`.

pub mod secrets;
