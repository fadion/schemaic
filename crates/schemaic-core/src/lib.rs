//! Core domain types + pure logic for Schemaic — no UI, and (except for
//! [`persist`], which reads/writes the JSON config files) no IO.
//!
//! The result-set model lives in [`model`]; the pure SQL/edit/export/diff/plan/
//! monitor logic in [`sql`]/[`edit`]/[`export`]/[`diff`]/[`plan`]/[`monitor`];
//! connection + schema models in [`connection`]/[`schema`]; and config
//! persistence in [`persist`].

pub mod activity;
pub mod aggregate;
pub mod celledit;
pub mod chat;
pub mod conn_import;
pub mod connection;
pub mod date;
pub mod db_color;
pub mod db_hidden;
pub mod ddl;
pub mod diff;
pub mod dump;
pub mod edit;
pub mod erd;
pub mod erd_export;
pub mod export;
pub mod favorite;
pub mod filter;
pub mod format;
pub mod health;
pub mod history;
pub mod import;
pub mod intel;
pub mod jsontree;
pub mod model;
pub mod monitor;
pub mod pairs;
pub mod palette;
pub mod params;
pub mod persist;
pub mod plan;
pub mod prompt;
pub mod propose;
pub mod resource;
pub mod resultsel;
pub mod rowjson;
pub mod schema;
pub mod search_history;
pub mod secrets;
pub mod seed;
pub mod skeleton;
pub mod snippet;
pub mod sql;
pub mod sqlfile;
pub mod sqlfmt;
pub mod stats;
pub mod summary;
pub mod tabsel;
pub mod text;
pub mod text_ops;
pub mod transcript;
pub mod tx;
pub mod typename;
pub mod update;
pub mod window_chrome;

/// Product name, shown in the window title and about screen.
pub const APP_NAME: &str = "Schemaic";

/// Current app version (mirrors the workspace package version).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
