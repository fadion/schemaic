//! Core domain types + pure logic for Schemaic — no UI, and (except for
//! [`persist`], which reads/writes the JSON config files) no IO.
//!
//! The result-set model lives in [`model`]; the pure SQL/edit/export/diff/plan/
//! monitor logic in [`sql`]/[`edit`]/[`export`]/[`diff`]/[`plan`]/[`monitor`];
//! connection + schema models in [`connection`]/[`schema`]; and config
//! persistence in [`persist`].

pub mod activity;
pub mod aggregate;
pub mod blob;
pub mod celledit;
pub mod chat;
pub mod compare;
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
pub mod script;
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
pub mod users;
pub mod window_chrome;

/// Product name, shown in the window title and the Settings version line.
pub const APP_NAME: &str = "Schemaic";

/// Current app version (mirrors the workspace package version).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// `Schemaic v0.24.0` — the app's identity line, for the top of Settings.
///
/// Composed here rather than in the view so the one thing a release changes
/// ([`APP_VERSION`], inherited from `[workspace.package].version`) is also the
/// only thing that has to change: bumping the version updates what the UI
/// renders, with no second edit to remember.
pub fn app_version_label() -> String {
    format!("{APP_NAME} v{APP_VERSION}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The label has to be *derived*, because the only thing a release bumps is
    /// `[workspace.package].version` — a view that spelled the number out would
    /// go stale at the next `chore: release` and nothing would say so.
    ///
    /// Same-crate `env!` on both sides means this cannot fail the *instant*
    /// someone replaces the derivation with a literal; it fails one release
    /// later, when the literal and the package version part ways. That is the
    /// guard this test actually offers.
    #[test]
    fn the_version_label_names_the_app_and_carries_the_package_version() {
        assert_eq!(
            app_version_label(),
            format!("Schemaic v{}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn the_version_label_ends_in_a_three_part_numeric_version() {
        let label = app_version_label();
        let version = label
            .strip_prefix("Schemaic v")
            .unwrap_or_else(|| panic!("label should start with `Schemaic v`, got {label:?}"));
        let parts: Vec<&str> = version.split('.').collect();
        assert_eq!(
            parts.len(),
            3,
            "expected major.minor.patch, got {version:?}"
        );
        assert!(
            parts
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())),
            "every component should be a number, got {version:?}"
        );
    }
}
