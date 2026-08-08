//! Core domain types + pure logic for Schemaic — no UI, and (except for
//! [`persist`], which reads/writes the JSON config files) no IO.
//!
//! The result-set model lives in [`model`]; the pure SQL/edit/export/diff/plan/
//! monitor logic in [`sql`]/[`edit`]/[`export`]/[`diff`]/[`plan`]/[`monitor`];
//! connection + schema models in [`connection`]/[`schema`]; and config
//! persistence in [`persist`].

pub mod chat;
pub mod connection;
pub mod db_color;
pub mod diff;
pub mod edit;
pub mod erd;
pub mod export;
pub mod favorite;
pub mod filter;
pub mod format;
pub mod history;
pub mod intel;
pub mod jsontree;
pub mod model;
pub mod monitor;
pub mod pairs;
pub mod palette;
pub mod persist;
pub mod plan;
pub mod resource;
pub mod rowjson;
pub mod schema;
pub mod search_history;
pub mod secrets;
pub mod seed;
pub mod sql;
pub mod sqlfmt;
pub mod summary;
pub mod tabsel;
pub mod text;
pub mod text_ops;
pub mod transcript;
pub mod tx;

/// Product name, shown in the window title and about screen.
pub const APP_NAME: &str = "Schemaic";

/// Current app version (mirrors the workspace package version).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
