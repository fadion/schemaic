//! Small on-disk UI state persisted across app restarts.
//!
//! For M4 this is just the set of expanded schema-tree nodes, so the sidebar
//! reopens exactly as the user left it. Stored as JSON at
//! `%APPDATA%/schemaic/ui_state.json` (Windows) or `$XDG_CONFIG_HOME`/`~/.config`
//! elsewhere. All IO is best-effort: a missing or corrupt file yields defaults,
//! and write failures are swallowed (persistence is a nicety, not correctness).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::connection::Connection;

/// Corrupt-file recoveries that happened during this run, waiting to be shown.
///
/// A corrupt config is recoverable — the original is kept as `.corrupt` — but
/// only if the user knows to look, and from their side connections or
/// preferences simply vanished. The load path is called from a dozen places
/// before the UI exists, so it records here instead of returning a notice, and
/// the app drains it once at startup with [`take_recoveries`].
static RECOVERIES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Take (and clear) the recovery notices recorded so far — one line per file
/// that failed to parse. The app shows these in the error modal at startup.
pub fn take_recoveries() -> Vec<String> {
    RECOVERIES
        .lock()
        .map(|mut v| std::mem::take(&mut *v))
        .unwrap_or_default()
}

/// Which panel occupies the right column, persisted across sessions.
///
/// Deserialized through [`RightPanelRaw`], so an unrecognised value degrades to
/// the default instead of failing the file. See that type for why.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", from = "RightPanelRaw")]
pub enum RightPanelState {
    None,
    #[default]
    Ai,
    Terminal,
    History,
}

/// Parsing shim for [`RightPanelState`] — the public enum's variants plus a
/// catch-all, which is mapped to the default on the way in.
///
/// **Every persisted enum in Schemaic has one of these**, because these files
/// are read by *older* builds too: releases are plain binaries with no
/// auto-update, so rolling back a bad version is a file copy. A variant added by
/// a newer build then appears in the JSON, and without a catch-all serde rejects
/// the **whole document** for that one string — `classify` calls the file
/// corrupt, and the `.bak` can't help because the same newer build wrote it. For
/// `connections.json` that means every saved connection vanishes and the next
/// ordinary save persists the empty list.
///
/// `#[serde(default)]` on the field does not cover this: it supplies a value for
/// a *missing* field, not for a present one that fails to parse.
///
/// The shim exists rather than a public `Unknown` variant so the fallback can't
/// leak into the app — every match on [`RightPanelState`] stays total over real
/// states, and no call site has to remember to normalise.
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum RightPanelRaw {
    None,
    Ai,
    Terminal,
    History,
    #[serde(other)]
    Unknown,
}

impl From<RightPanelRaw> for RightPanelState {
    fn from(raw: RightPanelRaw) -> Self {
        match raw {
            RightPanelRaw::None => RightPanelState::None,
            RightPanelRaw::Ai => RightPanelState::Ai,
            RightPanelRaw::Terminal => RightPanelState::Terminal,
            RightPanelRaw::History => RightPanelState::History,
            RightPanelRaw::Unknown => RightPanelState::default(),
        }
    }
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

// Theme defaults: dark UI + Tokyo Night editor. Tokyo Night rather than the
// original One Dark Pro because it reads better against the app's chrome — the
// editor-adjacent surfaces (completion popup, run menu, context menus) are
// chrome-themed, and One Dark Pro's background sits close enough to the editor's
// own that the popup barely separates from it.
//
// Only affects a config with no `editor_theme` saved; nobody's choice changes.
fn default_ui_theme() -> String {
    "dark".to_string()
}
fn default_editor_theme() -> String {
    "tokyo-night".to_string()
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
    /// SQL-editor theme key: `tokyo-night` / `one-dark-pro` / `catppuccin-latte`.
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
    /// Validate the statement under the cursor against the live database
    /// (non-executing PREPARE) as you type, surfacing dialect-exact errors as
    /// squiggles. Adds a debounced DB round-trip per edit pause. Default: off.
    #[serde(default)]
    pub live_validate: bool,
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
            live_validate: false,
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
    /// The source table's PostgreSQL namespace, stored beside `source` rather than
    /// widening it to a triple so a session file written by an older build still
    /// restores its tabs (it just reads back as `None`, i.e. `public`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_schema: Option<String>,
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

/// Write `bytes` to `path`, **owner-only** where the platform supports it,
/// truncating an existing file.
///
/// Every config write goes through this. `connections.json` holds plaintext
/// credentials whenever the keyring is unavailable — the fallback `docs/architecture.md`
/// documents — and `std::fs::write` creates at `0o666 & !umask`, i.e. 0644 under
/// the usual umask, which on a shared Unix host hands the DB password, the SSH
/// password and the key passphrase to every other local account. The `.bak` and
/// `.tmp` siblings carry the same bytes, so they take the same mode.
///
/// On Windows the file inherits the user profile's ACL and is not exposed to
/// other accounts, so there is nothing to narrow.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_mode(path, bytes, false)
}

/// As [`write_private`], but **refuses an existing path** (`O_EXCL`).
///
/// What a file written into a world-writable directory needs: `O_EXCL` refuses
/// both a path another user pre-created (which would otherwise be opened and
/// written *without* the mode applying, since the mode only takes effect on
/// creation) and a symlink pointing somewhere they can read.
pub fn create_private_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_mode(path, bytes, true)
}

fn write_mode(path: &Path, bytes: &[u8], exclusive: bool) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true);
    if exclusive {
        opts.create_new(true);
    } else {
        opts.create(true).truncate(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    #[allow(unused_mut)]
    let mut f = opts.open(path)?;
    // `OpenOptions::mode` applies only when the call *creates* the file, so an
    // existing one — say a 0644 written by a build from before this — would keep
    // its mode. Narrow it through the open handle (no path re-resolution, so no
    // TOCTOU). Not needed in the exclusive case: nothing pre-existed.
    #[cfg(unix)]
    if !exclusive {
        use std::os::unix::fs::PermissionsExt;
        let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    f.write_all(bytes)
}

/// Narrow `dir` to owner-only on Unix (best effort). Applied to our own config
/// directory, so anything added to it later is protected by default rather than
/// by each writer remembering.
fn make_dir_private(dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = dir;
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
    read_bytes(&Fs, &path)
}

/// The load-and-recover ordering, over any [`FileStore`] — the other half of
/// [`write_bytes`], and what makes the pair testable together.
pub(crate) fn read_bytes<T: Default + for<'de> Deserialize<'de>>(
    store: &dyn FileStore,
    path: &Path,
) -> T {
    // A crash between the staged write and the rename leaves a `.tmp` holding a
    // full copy of the file — nothing reads it, and only the rename-failure path
    // removed it, so it would sit there forever. Loading is the natural sweep
    // point: the next save re-creates its own.
    store.remove(&sibling(path, ".tmp"));
    let primary = classify::<T>(store.read(path).ok().as_deref());
    let (value, corrupt) = recover(primary, || {
        classify::<T>(store.read(&sibling(path, ".bak")).ok().as_deref())
    });
    if let Some(err) = corrupt {
        tracing::warn!(
            file = %path.display(),
            error = %err,
            "config file did not parse; preserving as .corrupt and trying the backup"
        );
        // A released GUI build discards stderr, so also queue it for the error
        // modal — otherwise the user just sees their settings gone.
        if let Ok(mut v) = RECOVERIES.lock() {
            v.push(recovery_notice(path, &err));
        }
        let _ = store.rename(path, &sibling(path, ".corrupt"));
    }
    value
}

/// The user-facing notice for a config file that didn't parse: what failed, that
/// the original was kept, and where it went. Named by file name rather than full
/// path — the modal is a sentence, not a log line.
fn recovery_notice(path: &Path, err: &str) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    format!(
        "{name} could not be read ({err}).\n\
         The unreadable file was kept as {name}.corrupt; Schemaic fell back to its backup, or \
         to defaults if the backup was unreadable too."
    )
}

/// The filesystem operations the save/load dance needs, behind a seam.
///
/// The *decisions* on either side of this file are pure and tested — `classify`,
/// `recover`, `sibling`. What had no test was the **composition**: that what
/// `write_json` produces is what `recover` can actually recover from. That is
/// the part deciding whether a user's connections, tabs and history survive a
/// crash or a full disk, and it can't be reached without a filesystem.
///
/// So the filesystem becomes an argument, the way [`crate::secrets::SecretStore`]
/// already does for the keyring — same reason, same shape. The house rule that
/// tests stay off the disk stays intact, and the ordering can be asserted
/// against an in-memory fake, including the failures a real disk won't stage on
/// demand (a rename that won't work, a write that fails).
pub(crate) trait FileStore {
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>>;
    fn write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()>;
    fn remove(&self, path: &Path);
    /// Create the file's directory, owner-only where the platform has modes.
    fn ensure_parent(&self, path: &Path);
}

/// The real filesystem.
pub(crate) struct Fs;

impl FileStore for Fs {
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        std::fs::read(path)
    }
    fn write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        write_private(path, bytes)
    }
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::rename(from, to)
    }
    fn remove(&self, path: &Path) {
        let _ = std::fs::remove_file(path);
    }
    fn ensure_parent(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
            make_dir_private(parent);
        }
    }
}

fn write_json<T: Serialize>(path: Option<PathBuf>, value: &T) {
    let Some(path) = path else {
        return;
    };
    let Ok(json) = serde_json::to_vec_pretty(value) else {
        return;
    };
    write_bytes(&Fs, &path, &json);
}

/// The save ordering, over any [`FileStore`].
///
/// Write to a temp file, then atomically rename over the target, so a crash
/// mid-write can't truncate the real file (this JSON is the only copy). Keep the
/// prior good version as `.bak` for recovery. All three paths hold the same
/// bytes, so all three go through the store's `write` — the rename carries the
/// temp's mode onto the target.
pub(crate) fn write_bytes(store: &dyn FileStore, path: &Path, json: &[u8]) {
    store.ensure_parent(path);
    let tmp = sibling(path, ".tmp");
    if store.write(&tmp, json).is_err() {
        return;
    }
    // Re-write rather than `fs::copy`, which would carry the *old* file's mode
    // onto the backup — including a 0644 left by a build from before this.
    if let Ok(prev) = store.read(path) {
        let _ = store.write(&sibling(path, ".bak"), &prev);
    }
    if store.rename(&tmp, path).is_err() {
        // Cross-device or transient rename failure: fall back to a direct write
        // rather than leaving only the temp file.
        let _ = store.write(path, json);
        store.remove(&tmp);
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

/// Load `<config>/<file>` as **security state** rather than configuration:
/// `Err` when there is a file and nothing readable could be got out of it.
///
/// [`load_json`] is the right loader for a window size — an unreadable file
/// becomes `T::default()` and the user loses a preference. It is the wrong one
/// for a trust store, where the default value *is* the insecure answer: an empty
/// known-hosts map trusts every host on first sight, so "I could not read my
/// records" would silently mean "I have no records" and a previously-verified
/// host would be re-trusted with whatever key it now offers.
///
/// `Ok(T::default())` still means genuinely absent — no file, or no config
/// directory at all, both of which are a first run. The differences from
/// `load_json` are that an I/O error on an existing file is an error rather than
/// an absence, and that a corrupt file is **not** renamed to `.corrupt`: doing so
/// would turn a transient unreadable store into a permanently empty one, which is
/// the very downgrade this exists to refuse.
pub fn load_json_strict<T: Default + for<'de> Deserialize<'de>>(file: &str) -> Result<T, String> {
    let Some(path) = config_dir().map(|d| d.join(file)) else {
        return Ok(T::default());
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    match classify::<T>(bytes.as_deref()) {
        Load::Ok(v) => Ok(v),
        Load::Absent => Ok(T::default()),
        // The `.bak` is Schemaic's own snapshot of the same file, so recovering
        // from it is recovering the records — not defaulting past them.
        Load::Corrupt(err) => {
            match classify::<T>(std::fs::read(sibling(&path, ".bak")).ok().as_deref()) {
                Load::Ok(v) => Ok(v),
                _ => Err(err),
            }
        }
    }
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

/// Remove the `connections.json.bak` recovery copy (best effort).
///
/// [`write_json`] snapshots the *previous* file to `.bak` before each write, so
/// the first save that migrates legacy plaintext secrets into the keyring leaves
/// a `.bak` still holding those plaintext secrets. The secret layer calls this
/// right after a migration save to make sure no plaintext credential lingers at
/// rest; a fresh (already-sanitized) `.bak` is regenerated on the next save.
pub fn clear_connections_backup() {
    if let Some(path) = connections_path() {
        let _ = std::fs::remove_file(sibling(&path, ".bak"));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConnectionsFile, FileStore, Load, RECOVERIES, RightPanelState, UiState, classify,
        read_bytes, recover, recovery_notice, sibling, take_recoveries, write_bytes,
    };
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    // ── The save/load composition ─────────────────────────────────────────
    //
    // `classify`, `recover` and `sibling` are each tested below. What wasn't,
    // until this fake existed, is whether what `write_bytes` *produces* is what
    // `recover` can actually recover *from* — the pair that decides whether a
    // user's connections, tabs and history survive a crash or a full disk. The
    // filesystem is the boundary being tested, so it is modelled rather than
    // used, which keeps the no-disk rule intact and makes failures that a real
    // disk won't stage on demand (a rename that won't work, a write that fails)
    // ordinary test setup.

    #[derive(Default)]
    struct FakeFs {
        files: RefCell<HashMap<PathBuf, Vec<u8>>>,
        /// Paths whose `write` fails, and whether `rename` fails at all.
        unwritable: RefCell<Vec<PathBuf>>,
        rename_fails: RefCell<bool>,
    }

    impl FakeFs {
        fn get(&self, path: &str) -> Option<Vec<u8>> {
            self.files.borrow().get(Path::new(path)).cloned()
        }
        fn put(&self, path: &str, bytes: &str) {
            self.files
                .borrow_mut()
                .insert(PathBuf::from(path), bytes.as_bytes().to_vec());
        }
        fn err() -> std::io::Error {
            std::io::Error::other("fake")
        }
    }

    impl FileStore for FakeFs {
        fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            self.files
                .borrow()
                .get(path)
                .cloned()
                .ok_or_else(FakeFs::err)
        }
        fn write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
            if self.unwritable.borrow().iter().any(|p| p == path) {
                return Err(FakeFs::err());
            }
            self.files
                .borrow_mut()
                .insert(path.to_path_buf(), bytes.to_vec());
            Ok(())
        }
        fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
            if *self.rename_fails.borrow() {
                return Err(FakeFs::err());
            }
            let Some(bytes) = self.files.borrow_mut().remove(from) else {
                return Err(FakeFs::err());
            };
            self.files.borrow_mut().insert(to.to_path_buf(), bytes);
            Ok(())
        }
        fn remove(&self, path: &Path) {
            self.files.borrow_mut().remove(path);
        }
        fn ensure_parent(&self, _path: &Path) {}
    }

    const CFG: &str = "/cfg/ui.json";

    /// Serialises the tests that touch [`RECOVERIES`], which is a process global:
    /// one test asserting it holds exactly what it queued, and another whose
    /// subject *is* a recovery and so queues a notice as a side effect. Without
    /// this they race, and the failure looks like the wrong test's bug.
    fn recovery_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn a_second_save_keeps_the_first_as_the_backup() {
        let fs = FakeFs::default();
        write_bytes(&fs, Path::new(CFG), br#""A""#);
        write_bytes(&fs, Path::new(CFG), br#""B""#);
        assert_eq!(fs.get(CFG).as_deref(), Some(&br#""B""#[..]));
        assert_eq!(fs.get("/cfg/ui.json.bak").as_deref(), Some(&br#""A""#[..]));
        // The staging file never survives a successful save.
        assert!(fs.get("/cfg/ui.json.tmp").is_none());
    }

    #[test]
    fn a_corrupt_primary_is_recovered_from_the_backup_a_save_left() {
        // The whole point of the composition: these two halves have to agree
        // about where the previous version lives.
        let _guard = recovery_lock();
        let fs = FakeFs::default();
        write_bytes(&fs, Path::new(CFG), br#""A""#);
        write_bytes(&fs, Path::new(CFG), br#""B""#);
        fs.put(CFG, "{ this is not json");

        let v: String = read_bytes(&fs, Path::new(CFG));
        assert_eq!(v, "A", "the backup is what recovery reads");
        // The unreadable original is preserved, not overwritten.
        assert!(fs.get("/cfg/ui.json.corrupt").is_some());
        // …and the user is told, in the notice the startup modal drains.
        let notices = take_recoveries();
        assert!(notices.iter().any(|n| n.contains("ui.json")), "{notices:?}");
    }

    #[test]
    fn a_failed_rename_still_leaves_the_new_value_and_no_orphan() {
        // The non-atomic fallback. It is the one path that writes the target
        // directly, and the thing that must not happen is losing the value *and*
        // leaving a stray `.tmp` holding it.
        let fs = FakeFs::default();
        write_bytes(&fs, Path::new(CFG), br#""A""#);
        *fs.rename_fails.borrow_mut() = true;
        write_bytes(&fs, Path::new(CFG), br#""B""#);

        assert_eq!(fs.get(CFG).as_deref(), Some(&br#""B""#[..]));
        assert!(fs.get("/cfg/ui.json.tmp").is_none(), "no orphaned temp");
        let v: String = read_bytes(&fs, Path::new(CFG));
        assert_eq!(v, "B");
    }

    #[test]
    fn a_save_that_cannot_stage_leaves_the_previous_value_untouched() {
        // A full disk. Failing before the rename is what makes this safe — the
        // target and its backup must both still hold the last good version.
        let fs = FakeFs::default();
        write_bytes(&fs, Path::new(CFG), br#""A""#);
        fs.unwritable
            .borrow_mut()
            .push(PathBuf::from("/cfg/ui.json.tmp"));
        write_bytes(&fs, Path::new(CFG), br#""B""#);

        assert_eq!(fs.get(CFG).as_deref(), Some(&br#""A""#[..]));
        let v: String = read_bytes(&fs, Path::new(CFG));
        assert_eq!(v, "A");
    }

    #[test]
    fn loading_sweeps_an_orphaned_temp_from_a_crash() {
        let fs = FakeFs::default();
        write_bytes(&fs, Path::new(CFG), br#""A""#);
        fs.put("/cfg/ui.json.tmp", r#""half-written"#);
        let v: String = read_bytes(&fs, Path::new(CFG));
        assert_eq!(v, "A");
        assert!(fs.get("/cfg/ui.json.tmp").is_none());
    }

    #[test]
    fn a_first_run_reads_defaults_and_writes_nothing() {
        // An absent file is not a corrupt one: no `.corrupt` is left behind.
        // (The recovery *notice* isn't asserted here — `RECOVERIES` is a process
        // global, so a test that drains it steals from whichever test is running
        // beside it.)
        let fs = FakeFs::default();
        let v: String = read_bytes(&fs, Path::new(CFG));
        assert_eq!(v, String::default());
        assert!(fs.get("/cfg/ui.json.corrupt").is_none());
    }

    // ── Forward compatibility: an unknown enum variant must cost one field,
    //    not the whole file ───────────────────────────────────────────────────
    //
    // These files are read by *older* builds too — Schemaic ships plain release
    // binaries with no auto-update, so rolling back is a file copy. `#[serde(
    // default)]` doesn't help here: it fills a *missing* field, not a present
    // one that fails to parse. Without `#[serde(other)]` serde rejects the whole
    // document, `classify` calls it corrupt, and the `.bak` is no use because it
    // was written by the same newer build — so every connection disappears and
    // the next save persists the empty list.

    #[test]
    fn an_unknown_right_panel_defaults_and_the_rest_of_the_file_survives() {
        let json = br#"{"right_panel":"erd","schema_w":123.0}"#;
        let s: UiState = serde_json::from_slice(json).expect("file must still parse");
        assert_eq!(s.schema_w, 123.0, "the rest of the document survives");
        assert_eq!(s.right_panel, RightPanelState::Ai, "unknown → the default");
    }

    /// The case that costs the most: a connection file is the one whose loss the
    /// user can't reconstruct.
    ///
    /// The fixture is a *real* serialization with the two enum values swapped
    /// for ones this build doesn't know, so it can't rot as fields are added —
    /// which is what a newer build writing this file actually does.
    #[test]
    fn an_unknown_ssh_auth_or_environment_keeps_every_connection() {
        use crate::connection::{Connection, Environment, SshAuth};

        let c = Connection {
            id: 1,
            name: "keep me".to_string(),
            db_type: "MySQL".to_string(),
            host: "h".to_string(),
            port: 3306,
            user: "u".to_string(),
            password: String::new(),
            file: String::new(),
            ssh: crate::connection::SshTunnel {
                auth: SshAuth::Agent,
                ..Default::default()
            },
            color: None,
            prominent_color: false,
            read_only: false,
            environment: Environment::Production,
        };
        let file = ConnectionsFile {
            connections: vec![c],
            active: Some(1),
        };

        let json = serde_json::to_string(&file)
            .unwrap()
            .replace("\"Agent\"", "\"Fido2\"")
            .replace("\"Production\"", "\"Sandbox\"");
        assert!(
            json.contains("Fido2") && json.contains("Sandbox"),
            "fixture"
        );

        let back: ConnectionsFile =
            serde_json::from_str(&json).expect("one unknown variant must not fail the file");
        assert_eq!(back.connections.len(), 1, "the connection is not lost");
        assert_eq!(back.connections[0].name, "keep me");
        assert_eq!(back.connections[0].ssh.auth, SshAuth::Password, "→ default");
        assert_eq!(
            back.connections[0].environment,
            Environment::None,
            "→ default"
        );
    }

    /// A file that is genuinely not JSON must still be classified corrupt —
    /// tolerating unknown variants mustn't tolerate garbage.
    #[test]
    fn a_malformed_file_is_still_corrupt() {
        assert!(matches!(
            classify::<UiState>(Some(b"{not json")),
            Load::Corrupt(_)
        ));
    }

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
    fn recovery_notice_names_the_file_the_error_and_where_the_original_went() {
        // What the user sees when their connections "vanish" — it has to say the
        // original is still there, or the recovery is invisible.
        let n = recovery_notice(
            Path::new("/cfg/connections.json"),
            "unknown variant `agent`",
        );
        assert!(n.contains("connections.json could not be read"), "{n}");
        assert!(n.contains("unknown variant `agent`"), "{n}");
        assert!(n.contains("connections.json.corrupt"), "{n}");
        // No directory noise — the modal is a sentence, not a log line.
        assert!(!n.contains("/cfg/"), "{n}");
    }

    #[test]
    fn take_recoveries_drains_what_was_queued() {
        let _guard = recovery_lock();
        RECOVERIES.lock().unwrap().push("first".to_string());
        RECOVERIES.lock().unwrap().push("second".to_string());
        assert_eq!(take_recoveries(), vec!["first", "second"]);
        // Drained, so a second startup pass shows nothing.
        assert!(take_recoveries().is_empty());
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

    // ── File modes ────────────────────────────────────────────────────────
    //
    // The one place the suite touches the filesystem, and deliberately: the mode
    // a file is created with *is* the boundary, so there is nothing purer to test.
    // Unix-only — on Windows the file inherits the profile ACL and there is no
    // mode to assert.
    #[cfg(unix)]
    mod modes {
        use super::super::*;
        use std::os::unix::fs::PermissionsExt;
        use std::sync::atomic::{AtomicU32, Ordering};

        /// A fresh directory under the system temp dir, removed by `Dir`'s drop.
        struct Dir(PathBuf);

        impl Dir {
            fn new(tag: &str) -> Dir {
                static SEQ: AtomicU32 = AtomicU32::new(0);
                let n = SEQ.fetch_add(1, Ordering::Relaxed);
                let p = std::env::temp_dir()
                    .join(format!("schemaic-test-{tag}-{}-{n}", std::process::id()));
                let _ = std::fs::remove_dir_all(&p);
                std::fs::create_dir_all(&p).unwrap();
                Dir(p)
            }
        }

        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        fn mode(path: &Path) -> u32 {
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777
        }

        #[test]
        fn every_config_write_is_owner_only() {
            // `connections.json` holds plaintext credentials whenever the keyring
            // is unavailable, and `fs::write` would create it 0644 — readable by
            // every other account on a shared host. The `.bak` carries the same
            // bytes, so it must be narrowed too (`fs::copy` would have carried the
            // *old* file's mode onto it).
            let dir = Dir::new("modes");
            let path = dir.0.join("connections.json");
            write_json(Some(path.clone()), &"first");
            write_json(Some(path.clone()), &"second");
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(&sibling(&path, ".bak")), 0o600);
            // And the directory itself, so anything added later is protected by
            // default rather than by each writer remembering.
            assert_eq!(mode(&dir.0), 0o700);
        }

        #[test]
        fn write_private_narrows_an_existing_world_readable_file() {
            // The upgrade path: a file left 0644 by an earlier build.
            let dir = Dir::new("upgrade");
            let path = dir.0.join("connections.json");
            std::fs::write(&path, b"old").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            write_private(&path, b"new").unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"new");
            // `OpenOptions::mode` only applies on creation, so re-opening an
            // existing file leaves its mode alone — narrow it explicitly.
            assert_eq!(mode(&path), 0o600);
        }

        #[test]
        fn create_private_new_refuses_a_path_someone_else_made() {
            // The world-writable-temp-dir attack: another user pre-creates the
            // path (or symlinks it), `create` opens it, and `.mode(0o600)` never
            // applies because nothing was created. `O_EXCL` refuses both.
            let dir = Dir::new("excl");
            let path = dir.0.join("mcp.json");
            std::fs::write(&path, b"planted").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
            assert!(create_private_new(&path, b"secret").is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"planted", "not written to");
            // On a free path it creates owner-only.
            let fresh = dir.0.join("fresh.json");
            create_private_new(&fresh, b"secret").unwrap();
            assert_eq!(mode(&fresh), 0o600);
        }

        #[test]
        fn loading_sweeps_a_temp_file_left_by_an_interrupted_save() {
            // A crash between the staged write and the rename leaves a full copy
            // nothing would ever remove.
            let dir = Dir::new("sweep");
            let path = dir.0.join("ui_state.json");
            write_json(Some(path.clone()), &"v");
            let tmp = sibling(&path, ".tmp");
            std::fs::write(&tmp, b"orphan").unwrap();
            let v: String = read_json(Some(path.clone()));
            assert_eq!(v, "v");
            assert!(!tmp.exists(), "the orphaned .tmp must be swept");
        }
    }
}
