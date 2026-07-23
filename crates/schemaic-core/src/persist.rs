//! Small on-disk UI state persisted across app restarts.
//!
//! For M4 this is just the set of expanded schema-tree nodes, so the sidebar
//! reopens exactly as the user left it. Stored as JSON at
//! `%APPDATA%/schemaic/ui_state.json` (Windows) or `$XDG_CONFIG_HOME`/`~/.config`
//! elsewhere. All IO is best-effort: a missing or corrupt file yields defaults,
//! and write failures are swallowed (persistence is a nicety, not correctness).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::connection::Connection;

/// Which panel occupies the right column, persisted across sessions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RightPanelState {
    None,
    #[default]
    Ai,
    Terminal,
    History,
}

fn default_true() -> bool {
    true
}

// Default panel-divider sizes (logical px), matching the hardcoded layout.
fn default_schema_w() -> f64 {
    300.0
}
fn default_right_w() -> f64 {
    350.0
}
fn default_editor_h() -> f64 {
    248.0
}

// AI Assistant defaults. Empty CLI path = auto-detect the `claude` binary.
fn default_ai_model() -> String {
    "haiku".to_string()
}
fn default_ai_effort() -> String {
    "medium".to_string()
}
fn default_ai_scope() -> String {
    "active".to_string()
}

// Theme defaults: dark UI + One Dark Pro editor (the original look).
fn default_ui_theme() -> String {
    "dark".to_string()
}
fn default_editor_theme() -> String {
    "one-dark-pro".to_string()
}
fn default_editor_font() -> f32 {
    14.0
}
fn default_row_limit() -> usize {
    200_000
}
fn default_tab_width() -> usize {
    4
}

/// Everything we remember about the UI between sessions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiState {
    /// Keys of expanded schema-tree nodes (`db:<name>`, `tbl:<db>:<name>`).
    #[serde(default)]
    pub expanded: Vec<String>,
    /// Names of databases hidden from the schema panel and search. Default: none
    /// hidden (every database is shown).
    #[serde(default)]
    pub hidden_dbs: Vec<String>,
    /// Whether the schema sidebar is shown. Default: shown.
    #[serde(default = "default_true")]
    pub schema_visible: bool,
    /// Which panel occupies the right column. Default: AI.
    #[serde(default)]
    pub right_panel: RightPanelState,
    /// Schema sidebar width (px), set by its resize divider.
    #[serde(default = "default_schema_w")]
    pub schema_w: f64,
    /// Right column (AI/Terminal) width (px), set by its resize divider.
    #[serde(default = "default_right_w")]
    pub right_w: f64,
    /// Query-editor height (px); the results grid takes the rest.
    #[serde(default = "default_editor_h")]
    pub editor_h: f64,
    /// AI Assistant — override path to the `claude` CLI. Empty = auto-detect.
    #[serde(default)]
    pub ai_cli_path: String,
    /// AI Assistant — model alias: `haiku` / `sonnet` / `opus`.
    #[serde(default = "default_ai_model")]
    pub ai_model: String,
    /// AI Assistant — effort: `low` / `medium` / `high` / `xhigh`.
    #[serde(default = "default_ai_effort")]
    pub ai_effort: String,
    /// AI Assistant — extra instructions appended to the system prompt.
    #[serde(default)]
    pub ai_instructions: String,
    /// AI Assistant — schema context scope: `active` / `all` / `none`.
    #[serde(default = "default_ai_scope")]
    pub ai_schema_scope: String,
    /// AI Assistant — allow the assistant to run read-only queries.
    #[serde(default = "default_true")]
    pub ai_run_queries: bool,
    /// Interface (chrome) theme key: `dark` / `light`.
    #[serde(default = "default_ui_theme")]
    pub ui_theme: String,
    /// SQL-editor theme key: `one-dark-pro` / `tokyo-night` / `catppuccin-latte`.
    #[serde(default = "default_editor_theme")]
    pub editor_theme: String,
    /// SQL-editor font size (px).
    #[serde(default = "default_editor_font")]
    pub editor_font_size: f32,
    /// Max rows fetched per query (the results-grid cap).
    #[serde(default = "default_row_limit")]
    pub row_limit: usize,
    /// Confirm before running any write/DDL statement. Default: on.
    #[serde(default = "default_true")]
    pub confirm_writes: bool,
    /// Editor tab width (columns).
    #[serde(default = "default_tab_width")]
    pub tab_width: usize,
    /// Editor uses soft tabs (spaces) rather than a literal tab. Default: spaces.
    #[serde(default = "default_true")]
    pub soft_tabs: bool,
    /// Wrap long editor lines to the viewport width. Default: off (scroll).
    #[serde(default)]
    pub word_wrap: bool,
    /// Reopen the previous session's query tabs on startup. Default: on.
    #[serde(default = "default_true")]
    pub restore_tabs: bool,
}

// Manual `Default` (not derived) so a missing file defaults `schema_visible` to
// `true` and `right_panel` to `Ai` — `bool`'s derived default would be `false`.
impl Default for UiState {
    fn default() -> Self {
        Self {
            expanded: Vec::new(),
            hidden_dbs: Vec::new(),
            schema_visible: true,
            right_panel: RightPanelState::Ai,
            schema_w: default_schema_w(),
            right_w: default_right_w(),
            editor_h: default_editor_h(),
            ai_cli_path: String::new(),
            ai_model: default_ai_model(),
            ai_effort: default_ai_effort(),
            ai_instructions: String::new(),
            ai_schema_scope: default_ai_scope(),
            ai_run_queries: true,
            ui_theme: default_ui_theme(),
            editor_theme: default_editor_theme(),
            editor_font_size: default_editor_font(),
            row_limit: default_row_limit(),
            confirm_writes: true,
            tab_width: default_tab_width(),
            soft_tabs: true,
            word_wrap: false,
            restore_tabs: true,
        }
    }
}

/// One persisted query tab, for "restore tabs on startup". Holds the editor text
/// plus the connection/database it ran against (by id — never a credential URL)
/// and the `(database, table)` it was opened from, so a restored tab lands on the
/// same connection and highlights its source table in the schema sidebar.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedTab {
    pub query: String,
    pub conn_id: u64,
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub source: Option<(String, String)>,
    /// User-assigned tab name (double-click to rename); `None` = the default
    /// "Query N" label. Persisted so a restored tab keeps its name.
    #[serde(default)]
    pub name: Option<String>,
    /// Whether this tab was pinned. Saved in pinned-first order, so a restore
    /// preserves both the flag and the left-of-strip position.
    #[serde(default)]
    pub pinned: bool,
}

/// The set of open tabs at last save, plus which one was active (its index).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SavedTabsFile {
    #[serde(default)]
    pub tabs: Vec<SavedTab>,
    #[serde(default)]
    pub active: usize,
}

/// Saved connections plus which one is active.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ConnectionsFile {
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub active: Option<u64>,
}

/// Our config directory (`%APPDATA%/schemaic`, or XDG/`~/.config` elsewhere).
fn config_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(dir.join("schemaic"))
}

/// Path to the persisted UI-state file, if we can determine a config directory.
pub fn config_path() -> Option<PathBuf> {
    Some(config_dir()?.join("ui_state.json"))
}

fn connections_path() -> Option<PathBuf> {
    Some(config_dir()?.join("connections.json"))
}

/// Append a suffix to a path's file name (`foo.json` → `foo.json.bak`).
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut os = path.to_path_buf().into_os_string();
    os.push(suffix);
    PathBuf::from(os)
}

/// Classification of a config file's contents: parsed, absent (first run), or
/// present-but-corrupt (carrying the parse error for the diagnostic message).
enum Load<T> {
    Ok(T),
    Absent,
    Corrupt(String),
}

/// Classify raw file bytes: `None` (file missing) → [`Load::Absent`]; bytes that
/// parse → [`Load::Ok`]; bytes that don't → [`Load::Corrupt`]. Pure.
fn classify<T: for<'de> Deserialize<'de>>(bytes: Option<&[u8]>) -> Load<T> {
    match bytes {
        None => Load::Absent,
        Some(b) => match serde_json::from_slice(b) {
            Ok(v) => Load::Ok(v),
            Err(e) => Load::Corrupt(e.to_string()),
        },
    }
}

/// Decide the loaded value and whether the primary file must be preserved as
/// `.corrupt`, from the primary's classification and a lazily-classified backup.
/// Pure — the caller performs the file reads/renames. Returns `(value,
/// corrupt_error)`: `corrupt_error` is `Some` (the primary's parse error) exactly
/// when the primary was corrupt and so must be preserved before the next save
/// overwrites it. The `backup` thunk is only consulted for a corrupt primary, so
/// a healthy or absent primary never reads the `.bak`.
fn recover<T: Default>(primary: Load<T>, backup: impl FnOnce() -> Load<T>) -> (T, Option<String>) {
    match primary {
        Load::Ok(v) => (v, None),
        Load::Absent => (T::default(), None), // first run → defaults, silently
        Load::Corrupt(err) => {
            // Do NOT silently reset — that would let the next save overwrite the
            // file with defaults. Recover from `.bak` if it parses, else default;
            // either way signal that the primary must be preserved as `.corrupt`.
            let value = match backup() {
                Load::Ok(v) => v,
                _ => T::default(),
            };
            (value, Some(err))
        }
    }
}

fn read_json<T: Default + for<'de> Deserialize<'de>>(path: Option<PathBuf>) -> T {
    let Some(path) = path else {
        return T::default();
    };
    let primary = classify::<T>(std::fs::read(&path).ok().as_deref());
    let (value, corrupt) = recover(primary, || {
        classify::<T>(std::fs::read(sibling(&path, ".bak")).ok().as_deref())
    });
    if let Some(err) = corrupt {
        eprintln!(
            "schemaic: could not parse {} ({err}); preserving as .corrupt and trying backup",
            path.display()
        );
        let _ = std::fs::rename(&path, sibling(&path, ".corrupt"));
    }
    value
}

fn write_json<T: Serialize>(path: Option<PathBuf>, value: &T) {
    let Some(path) = path else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(json) = serde_json::to_vec_pretty(value) else {
        return;
    };
    // Write to a temp file then atomically rename over the target, so a crash
    // mid-write can't truncate the real file (this JSON is the only copy). Keep
    // the prior good version as `.bak` for recovery.
    let tmp = sibling(&path, ".tmp");
    if std::fs::write(&tmp, &json).is_err() {
        return;
    }
    if path.exists() {
        let _ = std::fs::copy(&path, sibling(&path, ".bak"));
    }
    if std::fs::rename(&tmp, &path).is_err() {
        // Cross-device or transient rename failure: fall back to a direct write
        // rather than leaving only the temp file.
        let _ = std::fs::write(&path, &json);
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Load persisted UI state, falling back to defaults on any error.
pub fn load_ui_state() -> UiState {
    read_json(config_path())
}

/// Persist UI state (best effort — errors are intentionally ignored).
pub fn save_ui_state(state: &UiState) {
    write_json(config_path(), state);
}

/// Load a JSON value from `<config>/<file>` (best effort → default on error).
pub fn load_json<T: Default + for<'de> Deserialize<'de>>(file: &str) -> T {
    read_json(config_dir().map(|d| d.join(file)))
}

/// Persist a JSON value to `<config>/<file>` (best effort).
pub fn save_json<T: Serialize>(file: &str, value: &T) {
    write_json(config_dir().map(|d| d.join(file)), value);
}

/// Load saved connections (best effort).
pub fn load_connections() -> ConnectionsFile {
    read_json(connections_path())
}

/// Persist saved connections (best effort).
pub fn save_connections(file: &ConnectionsFile) {
    write_json(connections_path(), file);
}

#[cfg(test)]
mod tests {
    use super::{Load, classify, recover, sibling};
    use std::path::Path;

    #[test]
    fn classify_absent_ok_and_corrupt() {
        assert!(matches!(classify::<i32>(None), Load::Absent));
        assert!(matches!(classify::<i32>(Some(b"42")), Load::Ok(42)));
        assert!(matches!(
            classify::<Vec<String>>(Some(br#"["a","b"]"#)),
            Load::Ok(v) if v == vec!["a".to_string(), "b".to_string()]
        ));
        assert!(matches!(
            classify::<i32>(Some(b"not json")),
            Load::Corrupt(_)
        ));
    }

    #[test]
    fn recover_uses_primary_without_reading_backup() {
        // A healthy primary must never consult `.bak` (laziness guard).
        let (v, corrupt) = recover(Load::Ok(5), || panic!("backup must not be read"));
        assert_eq!(v, 5);
        assert!(corrupt.is_none());
    }

    #[test]
    fn recover_absent_defaults_without_reading_backup() {
        let (v, corrupt) = recover::<i32>(Load::Absent, || panic!("backup must not be read"));
        assert_eq!(v, 0); // i32::default()
        assert!(corrupt.is_none());
    }

    #[test]
    fn recover_corrupt_prefers_valid_backup_and_flags_preserve() {
        let (v, corrupt) = recover(Load::Corrupt("bad".to_string()), || Load::Ok(9));
        assert_eq!(v, 9);
        assert_eq!(corrupt.as_deref(), Some("bad")); // primary preserved as .corrupt
    }

    #[test]
    fn recover_corrupt_falls_back_to_default_when_backup_unusable() {
        // Backup absent → default, still preserve the corrupt primary.
        let (v, corrupt) = recover::<i32>(Load::Corrupt("e".to_string()), || Load::Absent);
        assert_eq!(v, 0);
        assert!(corrupt.is_some());
        // Backup also corrupt → default, still preserve.
        let (v, corrupt) = recover::<i32>(Load::Corrupt("e".to_string()), || {
            Load::Corrupt("e2".to_string())
        });
        assert_eq!(v, 0);
        assert!(corrupt.is_some());
    }

    #[test]
    fn sibling_appends_suffix_to_file_name() {
        assert_eq!(
            sibling(Path::new("/cfg/ui_state.json"), ".bak"),
            Path::new("/cfg/ui_state.json.bak")
        );
        assert_eq!(
            sibling(Path::new("connections.json"), ".corrupt"),
            Path::new("connections.json.corrupt")
        );
    }
}
