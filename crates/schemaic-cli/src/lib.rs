//! Schemaic without a window: the headless front end behind `schemaic list`,
//! `databases`, `ping`, `tables`, `describe`, `query` and `exec`.
//!
//! **Why it is a library and not a binary.** It has two front ends and they
//! differ per platform. On Linux and macOS the `schemaic` binary takes an argv
//! branch into here before it ever builds a window — the same shape
//! `--mcp-serve` already uses — so the CLI *is* the app's own executable, which
//! is what keeps the macOS keychain ACL matching the process that wrote the
//! secrets. On Windows the app is GUI-subsystem and has no console to write to,
//! so a second, console-subsystem binary is built from this crate and shipped
//! as `schemaic.com` beside `schemaic.exe`; `PATHEXT` puts `.COM` ahead of
//! `.EXE`, so typing `schemaic` reaches the CLI while shortcuts keep launching
//! the GUI.
//!
//! **What it may do.** A saved connection is reachable only if the user has
//! turned on [`Connection::cli_access`] for it, which is off for every
//! connection saved before the flag existed. Reads are gated by
//! `core::sql::read_only_reason` — the same allowlist the MCP server uses, and
//! strictly stronger than the editor's guard because it has no confirm arm.
//! Writes are a separate subcommand with a separate guard.
//!
//! [`Connection::cli_access`]: schemaic_core::connection::Connection::cli_access

pub mod args;
pub mod catalog;
pub mod deadline;
pub mod exec;
pub mod format;
pub mod query;
pub mod run;
pub mod select;
