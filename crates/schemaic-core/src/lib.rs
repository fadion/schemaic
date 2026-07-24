//! Core domain types + pure logic for Schemaic — no UI, and (except for
//! [`persist`], which reads/writes the JSON config files) no IO.
//!
//! The result-set model lives in [`model`]; the pure SQL/edit/export/diff/plan
//! logic in [`sql`]/[`edit`]/[`export`]/[`diff`]/[`plan`]; connection + schema
//! models in [`connection`]/[`schema`]; and config persistence in [`persist`].

pub mod connection;
pub mod db_color;
pub mod diff;
pub mod edit;
pub mod export;
pub mod format;
pub mod history;
pub mod model;
pub mod palette;
pub mod persist;
pub mod plan;
pub mod resource;
pub mod schema;
pub mod sql;
pub mod sqlfmt;
pub mod text_ops;
pub mod transcript;

/// Product name, shown in the window title and about screen.
pub const APP_NAME: &str = "Schemaic";

/// Current app version (mirrors the workspace package version).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
