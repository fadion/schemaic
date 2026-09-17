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
    Snippets,
    Activity,
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
    Snippets,
    Activity,
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
            RightPanelRaw::Snippets => RightPanelState::Snippets,
            RightPanelRaw::Activity => RightPanelState::Activity,
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

// AI Assistant defaults. Empty CLI path = auto-detect the harness's binary.
fn default_ai_harness() -> String {
    "claude".to_string()
}
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
/// Interface scale — `normal`, i.e. the size every release before this one drew
/// at. Keep in step with `themes::UiScale::from_key`'s fallback.
fn default_ui_scale() -> String {
    "normal".to_string()
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
    /// **Legacy.** Keys of expanded schema-tree nodes (`db:<name>`,
    /// `tbl:<db>:<name>`) with no connection dimension — read once at startup and
    /// folded into `expanded_rules` by [`crate::expanded::migrate_flat`], then
    /// never written again. Kept as a field for `hidden_dbs`' reason: an upgrade
    /// must not lose the setting, and removing it silently would make a
    /// `Vec<String>` where a `Vec<Rule>` is expected fail the whole file's parse.
    #[serde(default)]
    pub expanded: Vec<String>,
    /// Expanded schema-tree nodes, keyed by connection — see
    /// [`crate::expanded`] for why the key has to carry one. Default: nothing
    /// expanded.
    #[serde(default)]
    pub expanded_rules: Vec<crate::expanded::ExpandedRule>,
    /// Show each table's on-disk size in the schema tree. Default: off — it
    /// costs a catalogue query per expanded database (see
    /// [`crate::stats`]), so it is something the user asks for rather than
    /// something every session pays for.
    #[serde(default)]
    pub show_table_sizes: bool,
    /// **Legacy.** Bare names of databases hidden from the schema panel and
    /// search, with no connection dimension — read once at startup and folded
    /// into `hidden_db_rules` by [`crate::db_hidden::migrate_flat`], then never
    /// written again. Kept as a field so an upgrade does not lose the setting,
    /// and *not* removed silently, since a `Vec<String>` where a `Vec<Rule>` is
    /// expected would fail the whole file's parse.
    #[serde(default)]
    pub hidden_dbs: Vec<String>,
    /// Databases hidden from the schema panel and search, keyed by connection —
    /// see [`crate::db_hidden`] for why the key has to carry one. Default: none
    /// hidden (every database is shown).
    #[serde(default)]
    pub hidden_db_rules: Vec<crate::db_hidden::DbHiddenRule>,
    /// Whether the schema sidebar is shown. Default: shown.
    #[serde(default = "default_true")]
    pub schema_visible: bool,
    /// Which panel occupies the right column. Default: AI.
    #[serde(default)]
    pub right_panel: RightPanelState,
    /// Server Activity — the auto-refresh interval chosen per connection
    /// ([`crate::activity::IntervalRule`]). A connection with no entry polls at
    /// [`crate::activity::DEFAULT_POLL_SECS`]; `0` is off. Read through
    /// [`crate::activity::interval_for`], which clamps, so a hand-edited file
    /// can't leave the picker with nothing selected.
    #[serde(default)]
    pub activity_intervals: Vec<crate::activity::IntervalRule>,
    /// Schema sidebar width (px), set by its resize divider.
    #[serde(default = "default_schema_w")]
    pub schema_w: f64,
    /// Right column (AI/Terminal) width (px), set by its resize divider.
    #[serde(default = "default_right_w")]
    pub right_w: f64,
    /// Query-editor height (px); the results grid takes the rest.
    #[serde(default = "default_editor_h")]
    pub editor_h: f64,
    /// AI Assistant — which agent CLI drives the panel: `claude` / `codex` /
    /// `antigravity`.
    ///
    /// `gemini` was a fourth and is gone; a settings file still naming it takes
    /// the unrecognised-value path below rather than a migration of its own.
    ///
    /// One of `claude` / `codex` / `antigravity` / `opencode`
    /// (`Harness::key`), written by Settings → AI.
    ///
    /// Defaults to `claude`, which is what every settings file written before
    /// this field existed meant — the app drove that CLI and nothing else.
    ///
    /// An unrecognised value is **not** silently replaced with the default —
    /// and that promise used to be false one save later, which is the more
    /// interesting half. Serde keeps whatever the file said, but the app
    /// persisted `Harness::key()` of the *fallback*, so the next thing that
    /// touched the settings overwrote the name the user's file carried and a
    /// build that later grew that harness had nothing to restore. The app holds
    /// the raw key and writes it back until the user picks a harness themselves;
    /// the fallback is reported rather than substituted, because a panel driving
    /// a different CLI than the file names is invisible to the person reading
    /// it. Loading one also clears `ai_model` and `ai_cli_path`, which were set
    /// for a CLI this build does not have.
    #[serde(default = "default_ai_harness")]
    pub ai_harness: String,
    /// AI Assistant — override path to the agent CLI binary. Empty = auto-detect.
    ///
    /// Keyed to whichever harness is selected: switching harness and leaving a
    /// path behind would point the new CLI's spawn at the old CLI's binary.
    #[serde(default)]
    pub ai_cli_path: String,
    /// AI Assistant — model id, passed to the selected harness verbatim.
    ///
    /// **Free text, not a closed set.** This used to be documented as
    /// `haiku` / `sonnet` / `opus` and backed by an enum that narrowed every
    /// unrecognised value to Haiku; it is now whatever string the chosen CLI
    /// accepts, and the accepted shapes differ — Claude takes bare aliases,
    /// OpenCode requires `provider/model`. Empty means "the harness's own
    /// default" and omits the flag entirely, which is the only value that is
    /// correct on all four; the field is cleared on a harness switch for exactly
    /// that reason.
    ///
    /// The default is still `haiku` because the harness default beside it is
    /// `claude`, where that alias is valid. The two are only meaningful as a
    /// pair — a file naming one without the other is the case
    /// `default_ai_model` cannot get right, and clearing on switch is what keeps
    /// the pair consistent in practice.
    #[serde(default = "default_ai_model")]
    pub ai_model: String,
    /// AI Assistant — effort: `minimal` / `low` / `medium` / `high` / `xhigh` /
    /// `max`, clamped to the levels the selected harness actually advertises
    /// (`Harness::effort_levels`, and `AiEffort::clamped_to` for the clamp).
    #[serde(default = "default_ai_effort")]
    pub ai_effort: String,
    /// AI Assistant — extra instructions appended to the system prompt.
    #[serde(default)]
    pub ai_instructions: String,
    /// AI Assistant — schema context scope: `active` / `all` / `none`.
    #[serde(default = "default_ai_scope")]
    pub ai_schema_scope: String,
    /// AI Assistant — draw the accent rule down the right edge of the
    /// assistant's replies. Default: on. Purely presentational, and off is a
    /// taste rather than a fallback: with no rule the reply keeps equal insets
    /// on both sides, and the small-caps `CLAUDE` label alone marks whose turn
    /// it is.
    #[serde(default = "default_true")]
    pub ai_gutter: bool,
    /// **Legacy.** The old global "let the assistant run read-only queries"
    /// switch, replaced by the per-connection
    /// [`AiData`](crate::connection::AiData) level.
    ///
    /// Still loaded, because it is what a first run after the upgrade resolves
    /// each connection's unset level from, and still written back **unchanged**
    /// so downgrading to an older build finds the setting it left. Nothing else
    /// reads it: a global answer to "may the assistant read data" is exactly
    /// what the per-connection level exists to stop.
    #[serde(default = "default_true")]
    pub ai_run_queries: bool,
    /// Interface (chrome) theme key: `dark` / `light`.
    #[serde(default = "default_ui_theme")]
    pub ui_theme: String,
    /// SQL-editor theme key: `tokyo-night` / `one-dark-pro` / `catppuccin-latte`.
    #[serde(default = "default_editor_theme")]
    pub editor_theme: String,
    /// Interface scale key: `small` / `normal` / `large` / `huge`. Multiplies the
    /// chrome's type and layout metrics (not the editor or terminal font, which
    /// have their own size settings).
    #[serde(default = "default_ui_scale")]
    pub ui_scale: String,
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
    /// Cancel a running statement after this many seconds. **`0` means no
    /// timeout**, which is the default and the behaviour every release before
    /// this one had — a runaway `SELECT` ran until somebody noticed.
    ///
    /// Seconds rather than a `Duration` because this is a JSON file a human may
    /// edit, and `{"secs":30,"nanos":0}` is not something to ask them to type.
    /// [`statement_timeout`] is the one place the `0` is read as "off".
    #[serde(default)]
    pub statement_timeout_secs: u64,
}

/// The configured statement timeout as a duration, or `None` when there is
/// none.
///
/// **`0` is off, not "cancel immediately"** — a zero-second timeout would kill
/// every statement the instant it started, and it is the value a fresh install,
/// a hand-edited file and `#[serde(default)]` all produce. One function so that
/// reading is never spelled a second way.
pub fn statement_timeout(secs: u64) -> Option<std::time::Duration> {
    (secs > 0).then(|| std::time::Duration::from_secs(secs))
}

/// A statement timeout in words — `"No timeout"`, `"1 minute"`, `"15 minutes"`,
/// `"1 hour"`.
///
/// Here rather than beside the settings dropdown that shows it, because the
/// message a timed-out statement leaves in the results pane quotes the same
/// value back at the user: two spellings of "15 minutes" is how the setting and
/// the error come to disagree about what was configured. **Computed from the
/// value**, never looked up in the option list — a label with a list to fall off
/// the end of is the trap `row_limit_label` documents.
pub fn statement_timeout_label(secs: u64) -> String {
    let plural = |n: u64, unit: &str| format!("{n} {unit}{}", if n == 1 { "" } else { "s" });
    match secs {
        0 => "No timeout".to_string(),
        s if s.is_multiple_of(3600) => plural(s / 3600, "hour"),
        s if s.is_multiple_of(60) => plural(s / 60, "minute"),
        s => plural(s, "second"),
    }
}

// Manual `Default` (not derived) so a missing file defaults `schema_visible` to
// `true` and `right_panel` to `Ai` — `bool`'s derived default would be `false`.
impl Default for UiState {
    fn default() -> Self {
        Self {
            expanded: Vec::new(),
            expanded_rules: Vec::new(),
            show_table_sizes: false,
            hidden_dbs: Vec::new(),
            hidden_db_rules: Vec::new(),
            schema_visible: true,
            right_panel: RightPanelState::Ai,
            activity_intervals: Vec::new(),
            schema_w: default_schema_w(),
            right_w: default_right_w(),
            editor_h: default_editor_h(),
            ai_harness: default_ai_harness(),
            ai_cli_path: String::new(),
            ai_model: default_ai_model(),
            ai_effort: default_ai_effort(),
            ai_instructions: String::new(),
            ai_schema_scope: default_ai_scope(),
            ai_gutter: true,
            ai_run_queries: true,
            ui_theme: default_ui_theme(),
            editor_theme: default_editor_theme(),
            ui_scale: default_ui_scale(),
            editor_font_size: default_editor_font(),
            row_limit: default_row_limit(),
            confirm_writes: true,
            tab_width: default_tab_width(),
            soft_tabs: true,
            word_wrap: false,
            restore_tabs: true,
            live_validate: false,
            statement_timeout_secs: 0,
        }
    }
}

/// One persisted query tab, for "restore tabs on startup". Holds the editor text
/// plus the connection/database it ran against (by id — never a credential URL)
/// and the `(database, table)` it was opened from, so a restored tab lands on the
/// same connection and highlights its source table in the schema sidebar.
///
/// A tab opened from (or saved to) a `.sql` file also carries `path` and how to
/// write it back, so the binding survives a relaunch. The file's *contents* are
/// never persisted here — `query` is the session's copy, and the file on disk is
/// its own record.
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
    /// The `.sql` file this tab is bound to (Open/Save), if any. Restored so a
    /// file-backed tab still knows where Ctrl+S writes after a relaunch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// The file's line endings were CRLF, so a save writes them back that way
    /// (see [`crate::sqlfile`]). Meaningless without `path`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub file_crlf: bool,
    /// The file began with a UTF-8 BOM, which a save has to put back
    /// (see [`crate::sqlfile::SqlFormat::bom`]). Meaningless without `path`.
    ///
    /// Its own field beside `file_crlf` rather than a nested struct, because a
    /// `tabs.json` written by an older build has neither and must keep restoring
    /// — `#[serde(default)]` on a flat `bool` is what makes that free.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub file_bom: bool,
    /// Schemaic could not read every byte of the file as UTF-8 and substituted
    /// replacement characters (see [`crate::sqlfile::SqlFormat::lossy`]).
    ///
    /// Persisted because the warning has to survive a relaunch: the restored tab
    /// holds the *decoded* text, so nothing in it would show that saving would
    /// destroy the original bytes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub file_lossy: bool,
    /// `query` differed from the file on disk when the session was saved.
    ///
    /// Persisted as one bit rather than storing a second copy of the file's text:
    /// a restore takes `query` as-is either way, and this is only what the modified
    /// dot on the tab needs to come back honest. `false` lets the restore treat
    /// the text it already has as the on-disk content; `true` leaves that unknown,
    /// which the tab shows as modified until the next save or reload settles it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub file_dirty: bool,
}

/// The set of open tabs at last save, plus which one was active (its index).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SavedTabsFile {
    #[serde(default)]
    pub tabs: Vec<SavedTab>,
    #[serde(default)]
    pub active: usize,
}

impl SavedTabsFile {
    /// The tabs whose text exists **nowhere else**: a file-backed tab with
    /// unsaved edits.
    ///
    /// What "restore tabs on startup" is off means is *don't bring my session
    /// back* — and for every other tab that is a complete answer, because the text
    /// is a query the user can retype or a table they can reopen. A file tab with
    /// unsaved edits is the one exception: the edits are not on disk (that is what
    /// makes it dirty) and they are not in the file (the file still holds the old
    /// bytes), so a quit that dropped them would be the only unprompted loss of
    /// unrecoverable text in the application — every other route to losing that
    /// tab goes through the close guard, and a window quit cannot be vetoed on
    /// floem 0.2.
    ///
    /// So this subset is what is written and what is read while the setting is
    /// off, on both sides of the same rule: nothing else is stored against the
    /// user's preference, and a full session left over from when the setting was
    /// *on* is not silently restored either.
    ///
    /// `active` is re-pointed at the survivor nearest the one that was active, so
    /// the index cannot outrun the shortened list.
    pub fn unsaved_files_only(&self) -> SavedTabsFile {
        let kept: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.path.is_some() && t.file_dirty)
            .map(|(i, _)| i)
            .collect();
        let active = kept
            .iter()
            .position(|i| *i >= self.active)
            .unwrap_or(kept.len().saturating_sub(1));
        SavedTabsFile {
            tabs: kept.iter().map(|i| self.tabs[*i].clone()).collect(),
            active,
        }
    }
}

/// Saved connections plus which one is active.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ConnectionsFile {
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub active: Option<u64>,
    /// The highest connection id this install has ever handed out — a
    /// **high-water mark**, which only rises.
    ///
    /// Deleting the highest-numbered connection lowers the maximum id in use, so
    /// `Connection::next_id` would hand that id straight back — and the id is the
    /// whole of the keyring account string, so the next connection created
    /// inherits whatever a failed `forget` left behind. See
    /// [`Connection::next_id_after`], which is the only thing that reads this.
    ///
    /// `#[serde(default)]`, so a file written before this field reads `0` and the
    /// answer is the old one: the guarantee starts at the first save rather than
    /// being claimed for ids handed out before it existed.
    #[serde(default)]
    pub highest_id: u64,
}

/// Our config directory (`%APPDATA%/schemaic`, or XDG/`~/.config` elsewhere).
///
/// Public because the app's log file lives here too, beside the state files —
/// one directory a user can be pointed at when something needs diagnosing.
///
/// **An empty or relative base is no base.** `std::env::var_os` answers
/// `Some("")` for a variable that is *set and empty* — which is what a login
/// script clearing one typically leaves, and what a container image or a
/// launcher writes as `export XDG_CONFIG_HOME=`. `PathBuf::from("").join(
/// "schemaic")` is the **relative** path `schemaic`, resolved against the
/// process's working directory: launch from `~/projects/acme`, add three
/// connections, quit, and `connections.json`, `chats.json`, `history.json` and
/// `tabs.json` are in that git working tree. Launch from anywhere else and
/// every one of them reads absent and falls back to defaults — "all my
/// connections vanished". Where the keyring is unavailable it is worse: that
/// is the documented fallback in which `connections.json` holds **plaintext**
/// database passwords, SSH passwords and key passphrases, and the file lands
/// in a directory the user may be about to `git add`.
///
/// The freedesktop Base Directory spec says an empty `$XDG_CONFIG_HOME` must
/// be treated as unset, and the same reasoning covers `HOME` and `APPDATA`; a
/// value that resolves against the working directory is refused for the same
/// reason an empty one is, since nothing about this directory should depend on
/// where the app was started.
/// Falling through to the next candidate, and finally to `None`, is what the
/// callers already handle.
pub fn config_dir() -> Option<PathBuf> {
    let base = |k: &str| {
        std::env::var_os(k)
            .map(PathBuf::from)
            .filter(|p| usable_base(p))
    };
    let dir = base("APPDATA")
        .or_else(|| base("XDG_CONFIG_HOME"))
        .or_else(|| base("HOME").map(|h| h.join(".config")))?;
    Some(dir.join("schemaic"))
}

/// Is this environment variable's value something a config directory can be
/// built on? — see [`config_dir`].
fn usable_base(p: &Path) -> bool {
    // `has_root`, not `is_absolute`: the question is whether the path depends
    // on the *working directory*, which is the thing that moves. A rooted
    // Windows path with no drive letter (`\schemaic`) does not, and a
    // Unix-shaped `HOME` handed to a Windows build by Git Bash is rooted too
    // — while `C:schemaic`, which is drive-relative, is exactly the shape to
    // refuse and `has_root` refuses it.
    !p.as_os_str().is_empty() && p.has_root()
}

/// Which [`UiState::ai_harness`] key a save should write.
///
/// `unknown` is the raw key the file carried when this build did not recognise
/// it; `running` is `Harness::key()` of the one actually driving the panel.
///
/// **The rule, rather than one line at the persist site**, because that line was
/// the whole bug: the field's doc promised an unrecognised value would not be
/// silently replaced with the default, and the save wrote the *fallback's* key
/// over it. `an_unknown_harness_survives_the_round_trip_rather_than_being_
/// corrected` was green throughout, because it tested serde in isolation and
/// never the composition with the caller that overwrote.
pub fn ai_harness_to_persist(unknown: Option<&str>, running: &str) -> String {
    unknown.unwrap_or(running).to_string()
}

/// Path to the persisted UI-state file, if we can determine a config directory.
pub fn config_path() -> Option<PathBuf> {
    Some(config_dir()?.join("ui_state.json"))
}

/// The credential store's file name, in one place because two questions read
/// it: where the file lives, and — in [`corrupt_sibling_is_swept`] — whether a
/// recovery notice about it may promise the copy it names will still be there.
const CONNECTIONS_FILE: &str = "connections.json";

fn connections_path() -> Option<PathBuf> {
    Some(config_dir()?.join(CONNECTIONS_FILE))
}

/// Where something that must not be world-readable belongs, given a config
/// directory.
///
/// Pure, and separate from [`private_dir`], because the *choice* is the part
/// worth pinning and the failure it exists for is silent. Its consumer is
/// `ai::mcp_dir`, which holds the per-session files carrying the database host,
/// user and **plaintext password**: those lived in `std::env::temp_dir()`, which
/// every account on the machine can list, and the whole of the defence was
/// `O_EXCL` plus a random name.
///
/// **Not the AI session's working directory**, which it used to be and no longer
/// is. That directory is handed to an agent CLI whose file readers may be live,
/// so what matters about it is that nothing sensitive sits *above* it — see
/// `ai::session_cwd`, which creates one per session with no ancestor of ours.
pub fn private_dir_in(config: &Path, name: &str) -> PathBuf {
    config.join(name)
}

/// [`private_dir_in`] under our own [`config_dir`], created owner-only.
///
/// `None` when there is no config directory or it cannot be created — the caller
/// decides what to do, and every caller here treats it as a refusal rather than
/// falling back to somewhere world-readable.
pub fn private_dir(name: &str) -> Option<PathBuf> {
    let dir = private_dir_in(&config_dir()?, name);
    create_private_dir(&dir).ok()?;
    Some(dir)
}

/// `mkdir -p` at `0o700` where the platform has modes. On Windows the directory
/// inherits the user profile's ACL and is not exposed to other accounts, so
/// there is nothing to narrow — the same split [`write_private`] makes.
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        // `mode` applies only to directories this call creates, so an existing
        // one — a 0755 from a build before this — keeps its mode otherwise.
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir)?;
    Ok(())
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

/// Open `path` for **appending**, owner-only where the platform supports it —
/// [`write_private`]'s rule for a file that is written a line at a time rather
/// than replaced.
///
/// The log is the caller: it carries the SSH tunnel's account and endpoints, the
/// DB username and the client's source IP out of server error text, fragments of
/// the user's own SQL (MySQL error 1064 quotes the offending statement), the full
/// path of a SQLite database that failed to open, and every panic payload and
/// backtrace. `OpenOptions` with no `.mode(…)` creates at `0o666 & !umask` — 0644
/// under the usual one — so on a shared Unix host every other local account could
/// read all of that, while every *config* write in the same directory was
/// deliberately 0600.
///
/// It lives here rather than in the logger for the reason `write_mode` gives:
/// one place knows the mode. On Windows the file inherits the user profile's ACL
/// and there is nothing to narrow.
pub fn open_private_append(path: &Path) -> std::io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    #[allow(unused_mut)]
    let mut f = opts.open(path)?;
    // As in `write_mode`: the mode applies only on creation, so a file an older
    // build already created 0644 keeps it. Narrowed through the open handle, so
    // there is no path to re-resolve and no TOCTOU. A rotation is a `rename`,
    // which preserves the inode, so `schemaic.log.1` inherits whatever this set.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = &mut f;
    Ok(f)
}

/// The log file this process is **actually writing**, once whoever opened it has
/// said so — `None` until then, and `None` for good if the open failed.
///
/// A process global rather than a value threaded through the UI, and a *fact*
/// rather than a derivation: the Settings row that answers "where is the log" used
/// to build a path out of `config_dir()`, which performs no I/O, so on a machine
/// whose config directory exists but is not writable it named a file nobody had
/// written. The one row whose purpose is to be trusted about this has to ask the
/// writer.
static ACTIVE_LOG: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();

/// Record the log this process opened (or `None` if it could not). First call
/// wins; the logger calls it once at startup.
pub fn set_active_log(path: Option<PathBuf>) {
    let _ = ACTIVE_LOG.set(path);
}

/// [`set_active_log`]'s answer.
pub fn active_log() -> Option<PathBuf> {
    ACTIVE_LOG.get().cloned().flatten()
}

/// Create `dir` (and its parents) and narrow it to owner-only — what every
/// creator of the config directory owes, rather than whichever one happens to run
/// first deciding for the rest.
///
/// The logger runs before any config write, so on a fresh install the directory
/// used to be created 0755 by `logging::init` and narrowed only later, by the
/// first save.
pub fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    make_dir_private(dir);
    Ok(())
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

/// What [`recover`] did with the primary, and what the caller owes the user
/// because of it.
///
/// Three of the four arms are news. The one that is not — `Primary` — is the
/// ordinary load.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Recovered {
    /// The primary parsed. Nothing to preserve and nothing to say.
    Primary,
    /// No primary, and no sibling holding anything either: a genuine first run.
    /// Defaults, silently.
    FirstRun,
    /// The primary did not parse. Preserve it as `.corrupt` before the next save
    /// overwrites it, and tell the user. Carries the parse error.
    Corrupt(String),
    /// **The primary was gone and a sibling still held the data.** There is
    /// nothing to preserve — no primary to rename — but it is still news: a
    /// config file that vanished is worth a line whether or not the contents
    /// came back, and the sibling it came from is the only copy until the next
    /// save lands. Carries which sibling answered.
    Restored(&'static str),
}

/// Decide the loaded value and what the caller owes the user, from the primary's
/// classification and the lazily-classified siblings that may still hold a copy.
/// Pure — the caller performs the file reads and renames.
///
/// **An absent primary is not a first run.** It used to be treated as one, and
/// silently: no warning, no `RECOVERIES` entry, the app up with an empty
/// connection list, and — because the next save writes the primary while
/// leaving `.bak` alone, and the save *after* that copies the now-defaulted
/// primary onto it — the last copy gone within two ordinary saves. Saves are
/// frequent (a tab change, a history entry). The two ways to arrive there are
/// a crash between the staged write and the rename on a *first* save, which
/// leaves a `.tmp` holding everything, and the file being removed from outside
/// — the Settings modal's own *Open folder* button puts the user in that
/// directory, and a roaming-profile sync, a backup restore or an AV quarantine
/// reach it too.
///
/// The ladder is `.tmp` then `.bak`, because `write_bytes` stages the *new*
/// value into `.tmp` and copies the *old* primary into `.bak` — so where both
/// survive, the staged one is the newer. A sibling that does not parse is
/// stepped over rather than believed, which is what stops a half-written `.tmp`
/// shadowing an intact `.bak`.
///
/// A **corrupt** primary still consults `.bak` alone, as it always has: that
/// path is pinned by tests and by a shipped `.corrupt` rename, and widening it
/// is a separate decision from the one this arm is about.
fn recover<T: Default>(
    primary: Load<T>,
    staged: impl FnOnce() -> Load<T>,
    backup: impl FnOnce() -> Load<T>,
) -> (T, Recovered) {
    match primary {
        Load::Ok(v) => (v, Recovered::Primary),
        Load::Absent => {
            if let Load::Ok(v) = staged() {
                return (v, Recovered::Restored(".tmp"));
            }
            if let Load::Ok(v) = backup() {
                return (v, Recovered::Restored(".bak"));
            }
            (T::default(), Recovered::FirstRun)
        }
        Load::Corrupt(err) => {
            // Do NOT silently reset — that would let the next save overwrite the
            // file with defaults. Recover from `.bak` if it parses, else default;
            // either way signal that the primary must be preserved as `.corrupt`.
            let value = match backup() {
                Load::Ok(v) => v,
                _ => T::default(),
            };
            (value, Recovered::Corrupt(err))
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
    let primary = classify::<T>(store.read(path).ok().as_deref());
    // **Classify before sweeping.** The `.tmp` a crash left holds a full copy of
    // the file, and the sweep used to run unconditionally, first — so on the one
    // load where that copy was the *only* one, the loader destroyed it before
    // reading anything and then reported a first run.
    let healthy = matches!(primary, Load::Ok(_));
    let (value, outcome) = recover(
        primary,
        || classify::<T>(store.read(&sibling(path, ".tmp")).ok().as_deref()),
        || classify::<T>(store.read(&sibling(path, ".bak")).ok().as_deref()),
    );
    // Only now, and only with a good primary in hand: nothing reads the orphan
    // otherwise, and only the rename-failure path removed it, so it would sit
    // there forever. The next save re-creates its own.
    if healthy {
        store.remove(&sibling(path, ".tmp"));
    }
    match outcome {
        Recovered::Primary | Recovered::FirstRun => {}
        Recovered::Corrupt(err) => {
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
        Recovered::Restored(from) => {
            tracing::warn!(
                file = %path.display(),
                from = from,
                "config file was missing; recovered from its {from} sibling"
            );
            if let Ok(mut v) = RECOVERIES.lock() {
                v.push(missing_notice(path, from));
            }
        }
    }
    value
}

/// Whether a later save is going to remove this store's `.corrupt` sibling out
/// from under the user.
///
/// **One store, and it is the credential file.** The `.corrupt` an unreadable
/// primary leaves behind is the only copy of what was in it, and
/// [`recovery_notice`] points the user straight at it — so nothing may delete it
/// as the side effect of an unrelated deletion. `connections.json` is the
/// exception, because under the no-keyring fallback that copy holds the DB
/// password, the SSH password and the key passphrase in the clear, in a file
/// that cannot be parsed and therefore cannot be rewritten without them. There
/// the scrub wins and the *notice* has to be the qualified one, which is the
/// question this predicate answers.
///
/// A predicate rather than the check written into the notice, because the scrub
/// and the sentence describing it are 500 lines apart and drifted once already.
fn corrupt_sibling_is_swept(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n == CONNECTIONS_FILE)
}

/// The user-facing notice for a config file that didn't parse: what failed, that
/// the original was kept, and where it went. Named by file name rather than full
/// path — the modal is a sentence, not a log line.
///
/// For the one store whose copy is swept later it says so. The notice is an
/// instruction — *your data is in this file, go and get it* — and for
/// `connections.json` the next connection delete takes the file away; a user
/// told only the first half plans around a file that will not be there.
fn recovery_notice(path: &Path, err: &str) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let temporary = if corrupt_sibling_is_swept(path) {
        "\nThat copy can hold passwords in the clear, so it is removed the next time you \
         delete a connection — recover anything you need from it before then."
    } else {
        ""
    };
    format!(
        "{name} could not be read ({err}).\n\
         The unreadable file was kept as {name}.corrupt; Schemaic fell back to its backup, or \
         to defaults if the backup was unreadable too.{temporary}"
    )
}

/// Queue a startup notice for the modal that drains [`RECOVERIES`].
///
/// Not only for a config file that failed to parse. Everything loaded before the
/// window is drawn shares one problem — there is no surface yet to say anything
/// on — and one channel is better than each loader inventing its own. The
/// keyring is the second caller: a locked one is *the* reason connections stop
/// authenticating, and it used to reach neither a banner nor a log line.
pub fn queue_notice(notice: String) {
    if let Ok(mut v) = RECOVERIES.lock() {
        v.push(notice);
    }
}

/// The user-facing notice for a config file that was **gone** and came back off
/// a sibling.
///
/// Said, rather than repaired quietly, because the disappearance is the news:
/// something outside Schemaic removed the file, and the copy it was recovered
/// from is the only one until the next save lands. A user who does not know
/// that has no reason to take a backup, and no reason to wonder why it went.
fn missing_notice(path: &Path, from: &str) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    format!(
        "{name} was missing.\n\
         Schemaic recovered it from {name}{from}, which was still there. It will be written \
         back the next time this setting changes — until then that sibling is the only copy."
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

/// What a save is doing to the generation that was there.
///
/// **Every deletion this app confirms is undone at rest without this.** The
/// history panel's trash button asks *"Delete N recorded queries for this
/// connection? This can't be undone"*, and the save that answered Yes copied
/// the pre-clear file to `history.json.bak` on its way past — so every statement
/// the user had just confirmed the deletion of sat in a sibling file until the
/// *next* save of that store, which needs another query run. A clear followed by
/// a quit left it there indefinitely. The user's own SQL is content this module
/// already treats as sensitive ([`open_private_append`] narrows the log for
/// exactly that reason), and the Settings modal's **Log file** row is an
/// explicit invitation to open that folder and share it.
///
/// It is a property of the save, not of a particular store: `chats.json`,
/// `snippets.json`, `favorites.json`, `db_colors.json`, `diagrams.json` and
/// `ssh_known_hosts.json` all have it, which is why this is an argument to
/// [`write_bytes`] rather than a second `clear_*_backup` per file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Saving {
    /// The ordinary save. The previous generation is kept as `.bak`, which is
    /// what makes a crash, a corrupt primary or a vanished one recoverable.
    Replacing,
    /// **The point of this save is that something is gone.** No `.bak` is taken,
    /// and any earlier one is removed once the new file is safely in place — so
    /// the erasure is an erasure on disk too.
    Erasing,
}

fn write_json<T: Serialize>(path: Option<PathBuf>, value: &T, saving: Saving) {
    let Some(path) = path else {
        return;
    };
    let Ok(json) = serde_json::to_vec_pretty(value) else {
        return;
    };
    write_bytes(&Fs, &path, &json, saving);
}

/// The save ordering, over any [`FileStore`].
///
/// Write to a temp file, then atomically rename over the target, so a crash
/// mid-write can't truncate the real file (this JSON is the only copy). Keep the
/// prior good version as `.bak` for recovery — unless the save *is* a deletion,
/// which is what [`Saving`] decides. All three paths hold the same bytes, so all
/// three go through the store's `write` — the rename carries the temp's mode
/// onto the target.
///
/// The old `.bak` is removed **after** the new file is in place, never before:
/// a save that fails must leave the recovery copy it found.
///
/// **The `.bak`, not the rename, is what makes this crash-safe**, and it is
/// worth saying which. A rename is atomic with respect to other *processes*; it
/// is not, on its own, ordered after the data blocks with respect to a power
/// loss, and this path does not `sync_data` the temp before renaming the way
/// [`write_file_atomic`] does. On XFS, btrfs, ZFS, an SMB/NFS share and NTFS the
/// rename's metadata can reach stable storage while the contents have not — but
/// a zero-length `connections.json` then fails `serde_json::from_slice`, comes
/// back as [`Load::Corrupt`], and the backup is read. That is the recovery, and
/// it is why the sync is not paid for here: [`write_file_atomic`] keeps no
/// `.bak` by design, so it has nothing else and does sync.
///
/// Answers **whether the new content landed**, because a caller that has more
/// to sweep than the `.bak` — [`write_secret_store`] — owes the sweep the same
/// ordering this function gives its own: a save that could not land must leave
/// every copy it found.
pub(crate) fn write_bytes(store: &dyn FileStore, path: &Path, json: &[u8], saving: Saving) -> bool {
    store.ensure_parent(path);
    let tmp = sibling(path, ".tmp");
    if store.write(&tmp, json).is_err() {
        return false;
    }
    // Re-write rather than `fs::copy`, which would carry the *old* file's mode
    // onto the backup — including a 0644 left by a build from before this.
    if saving == Saving::Replacing
        && let Ok(prev) = store.read(path)
    {
        let _ = store.write(&sibling(path, ".bak"), &prev);
    }
    let mut landed = true;
    if store.rename(&tmp, path).is_err() {
        // Cross-device or transient rename failure: fall back to a direct write
        // rather than leaving only the temp file.
        landed = store.write(path, json).is_ok();
        store.remove(&tmp);
    }
    if saving == Saving::Erasing && landed {
        store.remove(&sibling(path, ".bak"));
    }
    landed
}

/// Every sibling copy of `path`, for a store whose contents are **secrets**.
///
/// **Two, and the second one is not free.** The `.bak` is what
/// [`Saving::Erasing`] was written about, and every erasing save reaches it: it
/// is the previous generation of the file being written, so everything in it
/// except the erased row is already in the new primary and removing it loses
/// exactly what the user asked to lose.
///
/// The `.corrupt` is a different kind of file and the difference is the whole
/// reason this is not on the ordinary erasing path. [`read_bytes`] renames an
/// unparseable primary aside *whole* and falls back to the `.bak`, which is one
/// generation **older** — so the `.corrupt` can hold rows the primary being
/// written never had, and [`recovery_notice`] tells the user in as many words to
/// go and read them out of it. Nothing else in the workspace removes that file;
/// for a while every `Saving::Erasing` save did, which meant deleting one
/// snippet silently destroyed the recovery file the startup modal had just
/// named, along with the snippets that existed only in it.
///
/// `connections.json` is the one store where the scrub still wins: under the
/// no-keyring fallback that copy holds the DB password, the SSH password and the
/// key passphrase in the clear, in a file that by definition cannot be parsed
/// and so cannot be rewritten without them, and a confirm reading "This can't be
/// undone" over a credential is a claim about the directory. The cost — a
/// connection delete taking the copy that also held the *other* connections — is
/// paid for by [`corrupt_sibling_is_swept`], which is what makes that store's
/// recovery notice say the copy is temporary.
fn remove_secret_siblings(store: &dyn FileStore, path: &Path) {
    store.remove(&sibling(path, ".bak"));
    store.remove(&sibling(path, ".corrupt"));
}

/// [`write_bytes`] for a store whose siblings are credential files: the erasure
/// reaches the `.corrupt` as well, once the new content is safely in place.
///
/// Separate from [`write_bytes`] rather than a third [`Saving`] arm, because the
/// question is about the *store* and `write_bytes` is deliberately store-blind —
/// it takes a `&dyn FileStore` and a `&Path` and cannot tell `connections.json`
/// from `snippets.json`. Asking it to decide is how the `.corrupt` scrub came to
/// apply to every store in the first place.
///
/// **Which stores those are is [`corrupt_sibling_is_swept`], asked here** rather
/// than settled by the choice of function. The same predicate decides whether
/// [`recovery_notice`] warns the user that the copy it names is temporary, so
/// the sweep and the sentence describing it cannot come apart: a second secret
/// store routed through here without being added to the predicate gets the
/// ordinary notice *and* the ordinary treatment, not one of each.
///
/// Takes the [`FileStore`] as an argument like everything else here, so it is
/// testable without a disk; [`save_connections`] is the only production caller.
fn write_secret_store<T: Serialize>(store: &dyn FileStore, path: &Path, value: &T, saving: Saving) {
    let Ok(json) = serde_json::to_vec_pretty(value) else {
        return;
    };
    let landed = write_bytes(store, path, &json, saving);
    if saving == Saving::Erasing && landed && corrupt_sibling_is_swept(path) {
        remove_secret_siblings(store, path);
    }
}

/// The file a save should actually replace: `path` with any symlink standing in
/// front of it resolved away.
///
/// `fs::rename` acts on the **link**, not on its target, so a dotfile manager's
/// `~/.antigravity/settings.json` → a chezmoi/stow repo — the common shape for
/// exactly that kind of file — was replaced by an ordinary file, and the managed
/// copy silently stopped receiving anything. The plain `fs::write` the atomic
/// write replaced followed the link correctly, so this is a regression the
/// atomicity introduced rather than one it inherited.
///
/// `canonicalize` rather than one `read_link`, because a chain of links is one
/// link too, and the resolved path is where the staging sibling has to go for
/// the rename to stay on one device.
fn resolved_target(path: &Path) -> PathBuf {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
        }
        _ => path.to_path_buf(),
    }
}

/// Stage `bytes` in a fresh sibling of `target` and hand back its path, with the
/// data already on stable storage.
///
/// **A unique name, created exclusively.** The fixed `<path>.schemaic-tmp` this
/// replaced was a pure function of the target, so every writer of one path
/// derived the same name — and the Antigravity marker is *machine-global* by
/// design, with the whole `Claim` mechanism existing because two Schemaic
/// windows contend for it. Two windows launching together both staged the same
/// sibling; the loser's rename returned `NotFound` and it fell through to the
/// truncating write this function exists to avoid. `create_new` closes the other
/// half at the same time: `fs::write` is `File::create`, which follows a symlink
/// and truncates, so another local account pre-creating
/// `/tmp/notes.sql.schemaic-tmp` as a link redirected the victim's own text at
/// the victim's privileges. The module already owned that rule and documented it
/// for this scenario ([`create_private_new`]); this path did not use it.
///
/// The mode of an existing target is carried onto the staged file before the
/// rename. The rename swaps the **inode**, so without that the user's
/// `chmod 600 seed.sql` came back 0644 — [`write_file_atomic`]'s doc promises it
/// narrows nothing, and widening is the same broken promise in the other
/// direction.
///
/// `sync_data` before returning, because a rename is atomic with respect to
/// other *processes* and is not, on its own, ordered after the data blocks with
/// respect to a power loss. On XFS, btrfs, ZFS, an SMB/NFS share and NTFS the
/// rename's metadata can land while the contents have not, and this function has
/// no `.bak` to fall back on.
fn stage_beside(target: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut last = std::io::Error::other("no staging name was tried");
    for _ in 0..64 {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp = sibling(target, &format!(".{}-{n}.schemaic-tmp", std::process::id()));
        let mut f = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                last = e;
                continue;
            }
            Err(e) => return Err(e),
        };
        let staged = (|| {
            f.write_all(bytes)?;
            // Unix only: `Permissions` on Windows carries the read-only bit and
            // nothing else, and setting it here would make the rename itself
            // fail. A Windows file inherits the directory's ACL on creation,
            // which is what the replaced file had unless someone set one by
            // hand — that case is not recoverable through a rename.
            #[cfg(unix)]
            if let Ok(meta) = std::fs::metadata(target) {
                let _ = f.set_permissions(meta.permissions());
            }
            f.sync_data()
        })();
        if let Err(e) = staged {
            drop(f);
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        return Ok(tmp);
    }
    Err(last)
}

/// Write `bytes` over `path` **atomically**, for a file that isn't ours.
///
/// A `.sql` script the user opened is the one artefact in this application that
/// Schemaic cannot regenerate, and `fs::write` truncates before it writes: a
/// full disk, a dropped network share or a crash between the two leaves the
/// user's file empty or half-written, with the only copy of the text still in a
/// tab the app is about to close. Staging beside it and renaming over makes the
/// replacement a single step.
///
/// Unlike [`write_bytes`] it keeps **no `.bak`**: this is the user's own file in
/// the user's own directory, and leaving `orders.sql.bak` behind after every
/// Ctrl+S is not ours to do. It narrows no permissions either — and, since the
/// rename swaps the inode, it now takes care to *keep* the ones the file had
/// rather than handing it whatever `umask` would give a fresh one
/// ([`stage_beside`]). The staging sibling is removed on any failure, so a
/// failed save leaves the directory as it was.
///
/// **What a rename cannot keep, and this does not pretend to.** The new inode is
/// a new inode: a second hard link to the file goes on pointing at the old
/// content, and there is no way to have both that and a replacement that is one
/// step. Atomicity wins, because the file being protected is often the only copy
/// of the text. A *symlink* is different and is handled — see
/// [`resolved_target`].
pub fn write_file_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let target = resolved_target(path);
    let tmp = stage_beside(&target, bytes)?;
    match std::fs::rename(&tmp, &target) {
        Ok(()) => {
            // Best effort, and unavailable on Windows, where a directory cannot
            // be opened as a file: without it the rename's own metadata is no
            // better ordered than the data was.
            if let Some(dir) = target.parent()
                && let Ok(d) = std::fs::File::open(dir)
            {
                let _ = d.sync_all();
            }
            Ok(())
        }
        Err(_) => {
            // A Windows share or a scanner refusing the replace — no longer
            // another writer having consumed the staging file, which is what the
            // unique name closed. A direct write is what the caller asked for and
            // is still better than failing with the text only in memory.
            let direct = std::fs::write(&target, bytes);
            // Keep the staged copy when the fallback failed too: it is then the
            // only place on disk the text exists.
            if direct.is_ok() {
                let _ = std::fs::remove_file(&tmp);
            }
            direct
        }
    }
}

/// Load persisted UI state, falling back to defaults on any error.
pub fn load_ui_state() -> UiState {
    read_json(config_path())
}

/// Persist UI state (best effort — errors are intentionally ignored).
///
/// **Takes its [`Saving`]**, because one of its callers is a deletion. This
/// store holds the tree's expansion rules, the hidden-database rules and the
/// per-connection Server Activity intervals, all keyed by connection — so
/// deleting a connection prunes them and saves, and an ordinary save left every
/// pruned rule in `ui_state.json.bak`.
pub fn save_ui_state(state: &UiState, saving: Saving) {
    write_json(config_path(), state, saving);
}

/// The legacy `ai_run_queries` flag **as a file actually recorded it**, or
/// `None` when no file did.
///
/// [`load_ui_state`] cannot answer this: it is best-effort, and its default for
/// the flag is `true` — so an *absent* `ui_state.json` (a restored
/// `connections.json`, a moved config directory, deleted preferences) reads as
/// "the user had the assistant running queries", and the one-way `AiData`
/// migration promotes every saved connection to [`crate::connection::AiData::Full`]
/// on that evidence. Widening a consent setting is not a decision to make from
/// an absence, and the migration never re-resolves.
pub fn legacy_ai_run_queries() -> Option<bool> {
    legacy_ai_run_queries_in(&std::fs::read(config_path()?).ok()?)
}

/// [`legacy_ai_run_queries`]'s decision, over the bytes rather than the file.
///
/// Split out because the decision is the whole point and the `std::fs::read`
/// put it out of reach: what counts as *evidence* that the user had the
/// assistant running queries. Four answers, and three of them are `None` —
/// unparsable JSON, no such key, a key holding something that is not a boolean.
/// Only a recorded boolean is evidence, because the thing it decides is a
/// one-way promotion of every saved connection to
/// [`crate::connection::AiData::Full`].
pub(crate) fn legacy_ai_run_queries_in(bytes: &[u8]) -> Option<bool> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    value.get("ai_run_queries")?.as_bool()
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
    write_json_store(file, value, Saving::Replacing);
}

/// [`save_json`] for a save whose point is that something is **gone** — the
/// history panel's trash, a deleted connection taking its chats and history with
/// it, one history row removed from its menu.
///
/// It does not keep the pre-deletion generation as `.bak`, and it removes any
/// earlier one. Everything [`Saving::Erasing`] says about why is there; the
/// short version is that a confirm reading "This can't be undone" has to be
/// true of the disk as well as of the panel.
pub fn save_json_erasing<T: Serialize>(file: &str, value: &T) {
    write_json_store(file, value, Saving::Erasing);
}

/// [`save_json`] with the [`Saving`] chosen by the caller.
///
/// **For a store written by one shared closure with callers on both sides of
/// the question.** The colour, favourite and formatter stores are each saved by
/// a single `Rc<dyn Fn(Saving)>` that a menu upsert and a connection deletion
/// both reach; neither of the two fixed-verb spellings above can serve that, and
/// the alternative — the closure picking for itself — is how those three stores
/// came to keep a deleted connection's databases, tables and columns in their
/// `.bak` siblings under a confirm saying they could not be recovered.
///
/// Not a third policy: it is the same [`write_json`], with the argument passed
/// through instead of written in.
pub fn write_json_store<T: Serialize>(file: &str, value: &T, saving: Saving) {
    write_json(config_dir().map(|d| d.join(file)), value, saving);
}

/// Load saved connections (best effort).
pub fn load_connections() -> ConnectionsFile {
    read_json(connections_path())
}

/// Persist saved connections (best effort).
///
/// **The one store whose `.bak` is a credential file.** Pass
/// [`Saving::Erasing`] from the path that *removes* a connection: an ordinary
/// save copies the pre-delete generation aside, so the deleted row's host,
/// port, user, database and SSH coordinates — and, with no working keyring, its
/// three plaintext secrets — land in `connections.json.bak` at the moment the
/// user confirms a modal telling them the opposite. Ordinary saves stay
/// [`Saving::Replacing`]: this is the only config file with no second copy
/// anywhere, and losing it loses every connection.
///
/// It is also the one store whose erasure reaches the `.corrupt` sibling;
/// [`remove_secret_siblings`] is where that costs something and why it is still
/// the right answer here.
pub fn save_connections(file: &ConnectionsFile, saving: Saving) {
    let Some(path) = connections_path() else {
        return;
    };
    write_secret_store(&Fs, &path, file, saving);
}

/// Remove the sibling copies of `connections.json` (best effort).
///
/// [`write_json`] snapshots the *previous* file to `.bak` before each write, so
/// the first save that migrates legacy plaintext secrets into the keyring leaves
/// a `.bak` still holding those plaintext secrets. The secret layer calls this
/// right after a migration save to make sure no plaintext credential lingers at
/// rest; a fresh (already-sanitized) `.bak` is regenerated on the next save.
///
/// The `.corrupt` sibling goes with it, for the reason
/// [`remove_secret_siblings`] gives: it is the same plaintext, in a file the
/// migration cannot rewrite and nothing else ever deletes.
pub fn clear_connections_backup() {
    if let Some(path) = connections_path() {
        remove_secret_siblings(&Fs, &path);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConnectionsFile, FileStore, Load, RECOVERIES, Recovered, RightPanelState, Saving, UiState,
        ai_harness_to_persist, classify, legacy_ai_run_queries_in, missing_notice, private_dir_in,
        read_bytes, recover, recovery_notice, remove_secret_siblings, sibling, statement_timeout,
        statement_timeout_label, take_recoveries, usable_base, write_bytes, write_secret_store,
    };
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// **A set-but-empty or relative base is no base.**
    ///
    /// `std::env::var_os` answers `Some("")` for a variable a login script
    /// cleared, and `PathBuf::from("").join("schemaic")` is the *relative*
    /// path `schemaic` — resolved against wherever the app was launched. Every
    /// state file then lands in that directory, including the plaintext
    /// fallback `connections.json`, and the next launch from elsewhere reads
    /// none of them: "all my connections vanished", with a credential file
    /// left in whatever tree the user was standing in.
    ///
    /// The predicate, not `config_dir` itself: reading it means setting
    /// process-wide environment variables, which is not something a parallel
    /// test suite may do.
    #[test]
    fn only_an_absolute_non_empty_base_is_usable() {
        assert!(!usable_base(&PathBuf::from("")));
        assert!(!usable_base(&PathBuf::from("schemaic")));
        assert!(!usable_base(&PathBuf::from("./config")));
        assert!(!usable_base(&PathBuf::from("../config")));

        // The real shapes, on both platforms.
        assert!(usable_base(&PathBuf::from("/home/u/.config")));
        assert!(usable_base(&PathBuf::from("/")));
        if cfg!(windows) {
            assert!(usable_base(&PathBuf::from(r"C:\Users\u\AppData\Roaming")));
        }
    }

    /// And `config_dir` asks it — the predicate alone is a decoration, and the
    /// bug was three `var_os` calls that did not ask anything.
    #[test]
    fn config_dir_filters_every_base_it_considers() {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("persist.rs"),
        )
        .expect("persist.rs");
        let at = src
            .find("pub fn config_dir()")
            .expect("`config_dir` is gone — this gate is stale");
        let end = at + src[at..].find("\n}").expect("its end");
        let f = &src[at..end];
        assert!(
            f.contains("usable_base("),
            "`config_dir` no longer refuses an empty or relative base:\n{f}"
        );
        assert_eq!(
            f.matches("var_os").count(),
            1,
            "each candidate must go through the one filtered helper:\n{f}"
        );
    }

    use std::path::{Path, PathBuf};

    /// The one thing that matters about a directory holding a plaintext
    /// credential: it is **ours**, not the shared temp dir. Asserted as a
    /// property rather than a string, so it keeps holding if the sub-directory
    /// is renamed.
    #[test]
    fn a_private_child_directory_is_under_our_config_dir_and_never_the_temp_dir() {
        let config = PathBuf::from("/home/u/.config/schemaic");
        let dir = private_dir_in(&config, "ai-mcp");
        assert!(dir.starts_with(&config), "{dir:?}");
        assert_ne!(
            dir, config,
            "a sub-directory of its own, not the config dir"
        );
        assert_ne!(dir, std::env::temp_dir(), "{dir:?}");
        assert!(!dir.starts_with(std::env::temp_dir()), "{dir:?}");
    }

    // ── Statement timeout ─────────────────────────────────────────────────

    /// **Zero is off, not instant death.** It is what a fresh install, a
    /// hand-edited `ui_state.json` and `#[serde(default)]` all produce, and
    /// reading it as `Duration::from_secs(0)` would cancel every statement the
    /// moment it started — an app that cannot run a query, from a setting
    /// nobody touched.
    #[test]
    fn a_zero_statement_timeout_means_no_timeout() {
        assert_eq!(statement_timeout(0), None);
    }

    #[test]
    fn a_configured_statement_timeout_is_that_many_seconds() {
        assert_eq!(
            statement_timeout(30),
            Some(std::time::Duration::from_secs(30))
        );
        assert_eq!(
            statement_timeout(900),
            Some(std::time::Duration::from_secs(900))
        );
    }

    /// The default has to stay `0`: every release before the setting existed
    /// ran statements unbounded, and shipping a default timeout would start
    /// killing the long imports and reports people already rely on.
    #[test]
    fn the_default_ui_state_has_no_statement_timeout() {
        assert_eq!(UiState::default().statement_timeout_secs, 0);
        assert_eq!(
            statement_timeout(UiState::default().statement_timeout_secs),
            None
        );
    }

    #[test]
    fn the_statement_timeout_label_reads_in_the_largest_whole_unit() {
        assert_eq!(statement_timeout_label(0), "No timeout");
        assert_eq!(statement_timeout_label(60), "1 minute");
        assert_eq!(statement_timeout_label(300), "5 minutes");
        assert_eq!(statement_timeout_label(1_800), "30 minutes");
        assert_eq!(statement_timeout_label(3_600), "1 hour");
        assert_eq!(statement_timeout_label(7_200), "2 hours");
    }

    /// Singular and plural both, because "1 minutes" in the one dropdown whose
    /// job is to report a setting accurately is the kind of thing nobody fixes.
    #[test]
    fn the_statement_timeout_label_gets_its_singulars_right() {
        assert_eq!(statement_timeout_label(1), "1 second");
        assert_eq!(statement_timeout_label(2), "2 seconds");
        assert_eq!(statement_timeout_label(90), "90 seconds");
    }

    /// Computed from the value, never looked up — so a value outside the
    /// dropdown's option list (a hand-edited file, or an option added or
    /// removed later) still labels as itself rather than as the default's
    /// label. Same trap `row_limit_label`'s doc comment records.
    #[test]
    fn a_statement_timeout_outside_the_offered_list_labels_as_itself() {
        assert_eq!(statement_timeout_label(45), "45 seconds");
        assert_eq!(statement_timeout_label(120), "2 minutes");
        assert_ne!(statement_timeout_label(45), statement_timeout_label(0));
    }

    /// A `ui_state.json` written by an older build has no such key at all, and
    /// it must load as "off" rather than failing the whole file.
    #[test]
    fn a_ui_state_from_before_the_setting_loads_with_it_off() {
        let old = r#"{"row_limit": 1000}"#;
        let state: UiState = serde_json::from_str(old).expect("older files still parse");
        assert_eq!(state.statement_timeout_secs, 0);
        assert_eq!(state.row_limit, 1000);
    }

    /// Every `ui_state.json` written before Schemaic drove more than one agent
    /// CLI has no `ai_harness` key, and it means Claude — that was the only
    /// harness there was. Defaulting to anything else would silently move a
    /// working AI panel onto a CLI the user has never installed.
    #[test]
    fn a_ui_state_from_before_multiple_harnesses_loads_as_claude() {
        let old = r#"{"ai_model": "opus", "ai_cli_path": ""}"#;
        let state: UiState = serde_json::from_str(old).expect("older files still parse");
        assert_eq!(state.ai_harness, "claude");
        // …and the settings that lived beside it are untouched.
        assert_eq!(state.ai_model, "opus");
    }

    /// The value round-trips verbatim. Nothing here maps it onto a known set:
    /// an unrecognised harness has to survive the file so the app can *report*
    /// it, which it cannot do if the parse quietly replaced it.
    #[test]
    fn an_unknown_harness_survives_the_round_trip_rather_than_being_corrected() {
        let raw = r#"{"ai_harness": "some-future-cli"}"#;
        let state: UiState = serde_json::from_str(raw).expect("parses");
        assert_eq!(state.ai_harness, "some-future-cli");
        let back: UiState =
            serde_json::from_str(&serde_json::to_string(&state).expect("serializes"))
                .expect("re-parses");
        assert_eq!(back.ai_harness, "some-future-cli");

        // **Serde is only half of it, and the other half is the caller.** This
        // test was green while the app wrote `Harness::key()` of the *fallback*
        // on every save, so the key survived the round trip here and was gone
        // from the user's file one save later. The composition is the rule
        // below, and the app calls it rather than spelling it out at the persist
        // site.
        assert_eq!(
            ai_harness_to_persist(Some(&state.ai_harness), "claude"),
            "some-future-cli",
            "the fallback overwrote the name the file carried"
        );
        assert_eq!(ai_harness_to_persist(None, "codex"), "codex");
        // …and once the user picks something, the file names what runs.
        assert_eq!(ai_harness_to_persist(None, "claude"), "claude");
    }

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
    /// The credential store, whose erasure sweeps one sibling more than an
    /// ordinary store's does. The real file name, because the name is what
    /// `corrupt_sibling_is_swept` reads — a path spelled anything else is an
    /// ordinary store and gets the ordinary treatment.
    const CREDS: &str = "/cfg/connections.json";

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
        write_bytes(&fs, Path::new(CFG), br#""A""#, Saving::Replacing);
        write_bytes(&fs, Path::new(CFG), br#""B""#, Saving::Replacing);
        assert_eq!(fs.get(CFG).as_deref(), Some(&br#""B""#[..]));
        assert_eq!(fs.get("/cfg/ui.json.bak").as_deref(), Some(&br#""A""#[..]));
        // The staging file never survives a successful save.
        assert!(fs.get("/cfg/ui.json.tmp").is_none());
    }

    /// **"This can't be undone" has to be true of the disk too.** The history
    /// panel's trash button says exactly that, and the save answering it copied
    /// the pre-clear file to `history.json.bak` on its way past — where it sat
    /// until the *next* save of that store, which needs another query run.
    #[test]
    fn an_erasing_save_leaves_no_copy_of_what_it_erased() {
        let fs = FakeFs::default();
        write_bytes(&fs, Path::new(CFG), br#"["a","b"]"#, Saving::Replacing);
        write_bytes(&fs, Path::new(CFG), br#"["a"]"#, Saving::Erasing);
        assert_eq!(fs.get(CFG).as_deref(), Some(&br#"["a"]"#[..]));
        assert!(
            fs.get("/cfg/ui.json.bak").is_none(),
            "the deleted entry is still on disk"
        );
        assert!(fs.get("/cfg/ui.json.tmp").is_none());
    }

    /// The `.bak` an erasing save finds is removed **after** the new file is in
    /// place, never before — a save that cannot land must leave the recovery
    /// copy it found, or a full disk turns a deletion into a total loss.
    #[test]
    fn an_erasing_save_that_cannot_stage_keeps_the_backup_it_found() {
        let fs = FakeFs::default();
        write_bytes(&fs, Path::new(CFG), br#"["a","b"]"#, Saving::Replacing);
        write_bytes(&fs, Path::new(CFG), br#"["a","b","c"]"#, Saving::Replacing);
        assert!(fs.get("/cfg/ui.json.bak").is_some());
        fs.unwritable
            .borrow_mut()
            .push(PathBuf::from("/cfg/ui.json.tmp"));
        write_bytes(&fs, Path::new(CFG), br#"[]"#, Saving::Erasing);
        assert_eq!(fs.get(CFG).as_deref(), Some(&br#"["a","b","c"]"#[..]));
        assert!(fs.get("/cfg/ui.json.bak").is_some(), "nothing was erased");
    }

    /// **A `.corrupt` is not a generation of the store being written.** The
    /// `.bak` an erasing save sweeps is the previous generation of *this* file:
    /// everything in it except the erased row is already in the new primary, so
    /// removing it loses exactly what the user asked to lose. The `.corrupt` is
    /// an orphan from a generation that *broke*, and the primary now being
    /// written came back off the `.bak`, which is one generation **older** — so
    /// it can hold rows the primary never had. Deleting one snippet must not
    /// take them with it.
    #[test]
    fn an_erasing_save_keeps_a_corrupt_sibling_it_did_not_write() {
        let fs = FakeFs::default();
        fs.put("/cfg/ui.json.corrupt", r#"["a","b","only-here"]"#);
        write_bytes(&fs, Path::new(CFG), br#"["a","b"]"#, Saving::Replacing);
        write_bytes(&fs, Path::new(CFG), br#"["a"]"#, Saving::Erasing);
        assert_eq!(
            fs.get("/cfg/ui.json.corrupt").as_deref(),
            Some(&br#"["a","b","only-here"]"#[..]),
            "deleting one row destroyed the recovery file the startup notice named"
        );
        // The `.bak` half is untouched by this: it *is* a generation of this
        // store, and the erasure still has to reach it.
        assert!(fs.get("/cfg/ui.json.bak").is_none());
    }

    /// **The composition, on the store the notice is about.** `read_bytes` is
    /// what creates a `.corrupt` and `recovery_notice` is what tells the user to
    /// go and read it; an erasing save 500 lines away is what used to destroy
    /// it. This runs the real sequence: a primary goes bad, recovery renames it
    /// aside and falls back to the older backup, the user deletes a *different*
    /// row, and what only the renamed file still holds is there to be recovered.
    #[test]
    fn a_recovery_then_a_one_row_erasure_keeps_the_rows_it_did_not_erase() {
        let _guard = recovery_lock();
        let fs = FakeFs::default();
        // Two saves, so the `.bak` is one generation behind: the recovery falls
        // back to a file that never had the second row.
        write_bytes(&fs, Path::new(CFG), br#"["keep"]"#, Saving::Replacing);
        write_bytes(
            &fs,
            Path::new(CFG),
            br#"["keep","only-in-the-primary"]"#,
            Saving::Replacing,
        );
        // Truncated, not replaced: the shape a power loss mid-write leaves, and
        // the reason a `.corrupt` is worth keeping at all.
        fs.put(CFG, r#"["keep","only-in-the-primary""#);
        let _: Vec<String> = read_bytes(&fs, Path::new(CFG));
        let _ = take_recoveries();
        assert!(
            fs.get("/cfg/ui.json.corrupt").is_some(),
            "the fixture needs the rename to have happened"
        );
        // The user deletes the row that *did* survive into the primary.
        write_bytes(&fs, Path::new(CFG), br#"[]"#, Saving::Erasing);
        let left = fs.get("/cfg/ui.json.corrupt").unwrap_or_default();
        assert!(
            String::from_utf8_lossy(&left).contains("only-in-the-primary"),
            "the row the user never erased is gone, and nothing said so"
        );
    }

    /// **The credential store is where the `.corrupt` scrub still belongs.** A
    /// `connections.json` that failed to parse once holds the plaintext fallback
    /// DB password, the SSH password and the key passphrase for the life of the
    /// install, in a file the app cannot rewrite because it cannot parse it. The
    /// user then confirms a delete saying "This can't be undone", so both
    /// siblings go — and the price, the copy also holding the connections they
    /// kept, is what `corrupt_sibling_is_swept` makes the notice admit.
    #[test]
    fn deleting_a_connection_sweeps_the_corrupt_sibling_too() {
        let fs = FakeFs::default();
        fs.put("/cfg/connections.json.corrupt", r#"{"password":"hunter2"}"#);
        write_secret_store(&fs, Path::new(CREDS), &vec!["a", "b"], Saving::Replacing);
        write_secret_store(&fs, Path::new(CREDS), &vec!["a", "b"], Saving::Replacing);
        assert!(fs.get("/cfg/connections.json.bak").is_some());
        write_secret_store(&fs, Path::new(CREDS), &vec!["a"], Saving::Erasing);
        assert!(fs.get("/cfg/connections.json.bak").is_none());
        assert!(
            fs.get("/cfg/connections.json.corrupt").is_none(),
            "the deleted connection's credentials are still readable in the .corrupt sibling"
        );
    }

    /// The same land-before-you-sweep ordering the `.bak` removal has, on the
    /// sibling one function further out: a save that could not stage leaves
    /// **every** copy it found, because the primary is now the only thing that
    /// did not get written.
    #[test]
    fn a_credential_erasure_that_cannot_stage_keeps_both_siblings() {
        let fs = FakeFs::default();
        fs.put("/cfg/connections.json.corrupt", r#"{"password":"hunter2"}"#);
        write_secret_store(&fs, Path::new(CREDS), &vec!["a", "b"], Saving::Replacing);
        write_secret_store(&fs, Path::new(CREDS), &vec!["a", "b"], Saving::Replacing);
        assert!(fs.get("/cfg/connections.json.bak").is_some());
        fs.unwritable
            .borrow_mut()
            .push(PathBuf::from("/cfg/connections.json.tmp"));
        write_secret_store(&fs, Path::new(CREDS), &Vec::<&str>::new(), Saving::Erasing);
        assert!(
            fs.get("/cfg/connections.json.bak").is_some(),
            "nothing was erased"
        );
        assert!(
            fs.get("/cfg/connections.json.corrupt").is_some(),
            "nothing was erased"
        );
    }

    /// **The composition, not either half.** `read_bytes` is what creates a
    /// `.corrupt`, and the credential store's erasing save is what has to answer
    /// for it — the two are 500 lines apart and each is correct on its own. This
    /// runs the real sequence: a primary goes bad, recovery renames it aside,
    /// the user deletes a connection, and the deleted connection's secrets must
    /// not still be in the directory.
    #[test]
    fn a_recovery_then_an_erasure_leaves_nothing_of_the_erased_row() {
        let _guard = recovery_lock();
        let fs = FakeFs::default();
        write_secret_store(
            &fs,
            Path::new(CREDS),
            &vec!["keep", "secret"],
            Saving::Replacing,
        );
        write_secret_store(
            &fs,
            Path::new(CREDS),
            &vec!["keep", "secret"],
            Saving::Replacing,
        );
        // Truncated, not replaced: the shape a power loss mid-write leaves, and
        // the reason a `.corrupt` is worth keeping at all — every byte that was
        // in the file is still readable in it.
        fs.put(CREDS, r#"["keep","secret""#);
        let _: Vec<String> = read_bytes(&fs, Path::new(CREDS));
        let _ = take_recoveries();
        assert!(
            fs.get("/cfg/connections.json.corrupt").is_some(),
            "the fixture needs the rename to have happened"
        );
        write_secret_store(&fs, Path::new(CREDS), &vec!["keep"], Saving::Erasing);
        for sibling in ["/cfg/connections.json.bak", "/cfg/connections.json.corrupt"] {
            let left = fs.get(sibling).unwrap_or_default();
            assert!(
                !String::from_utf8_lossy(&left).contains("secret"),
                "{sibling} still holds the erased row"
            );
        }
    }

    /// `clear_connections_backup`'s one job, over the store rather than over
    /// `std::fs`: the migration that rewrites `connections.json` blanked has to
    /// leave no sibling still holding what it blanked — `.corrupt` included,
    /// which is where a file that failed to parse *once* puts them forever.
    #[test]
    fn clearing_the_siblings_reaches_both_of_them() {
        let fs = FakeFs::default();
        fs.put("/cfg/connections.json.bak", r#"{"password":"hunter2"}"#);
        fs.put("/cfg/connections.json.corrupt", r#"{"password":"hunter2"}"#);
        remove_secret_siblings(&fs, Path::new(CREDS));
        assert!(fs.get("/cfg/connections.json.bak").is_none());
        assert!(fs.get("/cfg/connections.json.corrupt").is_none());
    }

    #[test]
    fn a_corrupt_primary_is_recovered_from_the_backup_a_save_left() {
        // The whole point of the composition: these two halves have to agree
        // about where the previous version lives.
        let _guard = recovery_lock();
        let fs = FakeFs::default();
        write_bytes(&fs, Path::new(CFG), br#""A""#, Saving::Replacing);
        write_bytes(&fs, Path::new(CFG), br#""B""#, Saving::Replacing);
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
        write_bytes(&fs, Path::new(CFG), br#""A""#, Saving::Replacing);
        *fs.rename_fails.borrow_mut() = true;
        write_bytes(&fs, Path::new(CFG), br#""B""#, Saving::Replacing);

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
        write_bytes(&fs, Path::new(CFG), br#""A""#, Saving::Replacing);
        fs.unwritable
            .borrow_mut()
            .push(PathBuf::from("/cfg/ui.json.tmp"));
        write_bytes(&fs, Path::new(CFG), br#""B""#, Saving::Replacing);

        assert_eq!(fs.get(CFG).as_deref(), Some(&br#""A""#[..]));
        let v: String = read_bytes(&fs, Path::new(CFG));
        assert_eq!(v, "A");
    }

    #[test]
    fn loading_sweeps_an_orphaned_temp_from_a_crash() {
        let fs = FakeFs::default();
        write_bytes(&fs, Path::new(CFG), br#""A""#, Saving::Replacing);
        fs.put("/cfg/ui.json.tmp", r#""half-written"#);
        let v: String = read_bytes(&fs, Path::new(CFG));
        assert_eq!(v, "A");
        assert!(fs.get("/cfg/ui.json.tmp").is_none());
    }

    /// **Absence is not evidence of a first run once a sibling still holds the
    /// data.** A crash between the staged write and the rename on a *first*
    /// save leaves no primary and a `.tmp` holding everything; a file removed
    /// from outside (the Settings modal's own *Open folder* button puts the user
    /// in that directory, and a roaming-profile sync, a backup restore or an AV
    /// quarantine reach it too) leaves a `.bak` holding the last good version.
    /// Read as a first run, the app came up empty and **two ordinary saves then
    /// overwrote the last copy** — and saves are frequent.
    ///
    /// Asserted through `read_bytes` rather than through `recover`, because the
    /// loss is the composition: the unconditional `.tmp` sweep at the top of the
    /// loader destroyed one of the two copies before anything was classified.
    #[test]
    fn an_absent_primary_recovers_from_the_backup_instead_of_reading_as_a_first_run() {
        let _guard = recovery_lock();
        let _ = take_recoveries();
        let fs = FakeFs::default();
        fs.put("/cfg/ui.json.bak", r#""from-bak""#);
        let v: String = read_bytes(&fs, Path::new(CFG));
        assert_eq!(v, "from-bak");
        // Nothing to preserve — there is no primary — so no `.corrupt`.
        assert!(fs.get("/cfg/ui.json.corrupt").is_none());
        // And the recovered copy is still there for the next attempt.
        assert!(fs.get("/cfg/ui.json.bak").is_some());
        // **Said, not repaired quietly.** The whole recovery apparatus used to
        // stay silent here: no warning, no notice, no `.corrupt`.
        let notices = take_recoveries();
        assert_eq!(notices, vec![missing_notice(Path::new(CFG), ".bak")]);
        assert!(notices[0].contains("ui.json was missing"), "{notices:?}");
    }

    #[test]
    fn an_absent_primary_recovers_from_the_staged_temp_a_crash_left() {
        let _guard = recovery_lock();
        let _ = take_recoveries();
        let fs = FakeFs::default();
        fs.put("/cfg/ui.json.tmp", r#""from-tmp""#);
        let v: String = read_bytes(&fs, Path::new(CFG));
        assert_eq!(v, "from-tmp");
        // **Not swept.** It is the only copy until a save lands.
        assert!(fs.get("/cfg/ui.json.tmp").is_some());
        let _ = take_recoveries();
    }

    /// The staged copy is the *newer* of the two — `write_bytes` writes `.tmp`
    /// from the new value and `.bak` from the old one — so it wins.
    #[test]
    fn a_staged_temp_outranks_the_backup_when_the_primary_is_gone() {
        let _guard = recovery_lock();
        let _ = take_recoveries();
        let fs = FakeFs::default();
        fs.put("/cfg/ui.json.tmp", r#""newer""#);
        fs.put("/cfg/ui.json.bak", r#""older""#);
        let v: String = read_bytes(&fs, Path::new(CFG));
        assert_eq!(v, "newer");
        let _ = take_recoveries();
    }

    /// A sibling that does not parse is not a copy. Falling through to the next
    /// one is what stops a half-written `.tmp` shadowing an intact `.bak`.
    #[test]
    fn an_unparsable_sibling_is_stepped_over_rather_than_believed() {
        let _guard = recovery_lock();
        let _ = take_recoveries();
        let fs = FakeFs::default();
        fs.put("/cfg/ui.json.tmp", r#""half-writt"#);
        fs.put("/cfg/ui.json.bak", r#""intact""#);
        let v: String = read_bytes(&fs, Path::new(CFG));
        assert_eq!(v, "intact");
        let _ = take_recoveries();
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

    /// Every `RightPanelState` has to survive a write and a read back. The
    /// `RightPanelRaw` shim is a *second* list of the same variants, and a new one
    /// added to the public enum and forgotten there reads back as the default —
    /// the panel silently reverting to AI on every restart, with nothing failing.
    #[test]
    fn every_right_panel_round_trips_through_the_shim() {
        for p in [
            RightPanelState::None,
            RightPanelState::Ai,
            RightPanelState::Terminal,
            RightPanelState::History,
            RightPanelState::Snippets,
            RightPanelState::Activity,
        ] {
            let json = serde_json::to_string(&p).expect("serializes");
            let back: RightPanelState = serde_json::from_str(&json).expect("parses");
            assert_eq!(back, p, "{json} did not survive the round trip");
        }
    }

    #[test]
    fn activity_intervals_round_trip_and_default_to_none_recorded() {
        let s: UiState = serde_json::from_slice(br"{}").expect("parses");
        assert!(s.activity_intervals.is_empty());
        assert_eq!(
            crate::activity::interval_for(&s.activity_intervals, 7),
            crate::activity::DEFAULT_POLL_SECS,
            "no entry means the default, not zero (which would be 'off')"
        );

        let json = br#"{"activity_intervals":[{"conn_id":3,"secs":0}]}"#;
        let s: UiState = serde_json::from_slice(json).expect("parses");
        assert_eq!(crate::activity::interval_for(&s.activity_intervals, 3), 0);
    }

    /// **The expansion set's upgrade, both directions.**
    ///
    /// A file written before the set gained a connection dimension carries a
    /// flat `expanded` list; one written after carries `expanded_rules` and an
    /// empty flat field. Both must read, and the legacy one must not be dropped
    /// on the floor — the flat list is removed only once the migration has
    /// actually consumed it, which needs the connection ids to be loaded.
    #[test]
    fn the_expanded_set_reads_both_the_flat_list_and_the_keyed_rules() {
        // A pre-upgrade file.
        let json = br#"{"expanded":["db:sys","db:world"]}"#;
        let s: UiState = serde_json::from_slice(json).expect("parses");
        assert_eq!(s.expanded.len(), 2);
        assert!(s.expanded_rules.is_empty());
        let migrated =
            crate::expanded::migrate_flat(&s.expanded, &[1, 2]).expect("connections are loaded");
        assert!(crate::expanded::is_expanded(&migrated, 1, "db:sys"));
        assert!(
            crate::expanded::is_expanded(&migrated, 2, "db:sys"),
            "the flat list meant everywhere, so the migration means everywhere"
        );

        // A post-upgrade file: the flat field is empty and the rules carry it.
        let json = br#"{"expanded":[],"expanded_rules":[{"conn_id":2,"key":"db:sys"}]}"#;
        let s: UiState = serde_json::from_slice(json).expect("parses");
        assert!(s.expanded.is_empty());
        assert!(crate::expanded::is_expanded(&s.expanded_rules, 2, "db:sys"));
        assert!(
            !crate::expanded::is_expanded(&s.expanded_rules, 1, "db:sys"),
            "which is the whole point of the upgrade"
        );

        // And a file from before either existed.
        let s: UiState = serde_json::from_slice(br"{}").expect("parses");
        assert!(s.expanded.is_empty() && s.expanded_rules.is_empty());
        assert_eq!(
            crate::expanded::migrate_flat(&s.expanded, &[]),
            None,
            "no connections is 'not yet', so the flat field stays on disk"
        );
    }

    /// The case that costs the most: a connection file is the one whose loss the
    /// user can't reconstruct.
    ///
    /// The fixture is a *real* serialization with the enum values swapped for
    /// ones this build doesn't know, so it can't rot as fields are added —
    /// which is what a newer build writing this file actually does.
    ///
    /// Every `#[serde(other)]` shim on a connection belongs here, not only in
    /// its own unit test: the shim is correct in isolation and still useless if
    /// the field it guards is not reached through it.
    #[test]
    fn an_unknown_ssh_auth_environment_or_ssl_mode_keeps_every_connection() {
        use crate::connection::{Connection, Environment, SshAuth, SslMode, Tls};

        let c = Connection {
            id: 1,
            name: "keep me".to_string(),
            db_type: "MySQL".to_string(),
            host: "h".to_string(),
            port: 3306,
            user: "u".to_string(),
            password: String::new(),
            file: String::new(),
            database: "defaultdb".to_string(),
            ssh: crate::connection::SshTunnel {
                auth: SshAuth::Agent,
                ..Default::default()
            },
            tls: Tls {
                mode: SslMode::VerifyFull,
                ca_path: "/etc/ca.crt".to_string(),
                ..Default::default()
            },
            color: None,
            prominent_color: false,
            read_only: false,
            environment: Environment::Production,
            ai_data: None,
        };
        let file = ConnectionsFile {
            connections: vec![c],
            active: Some(1),
            highest_id: 0,
        };

        let json = serde_json::to_string(&file)
            .unwrap()
            .replace("\"Agent\"", "\"Fido2\"")
            .replace("\"Production\"", "\"Sandbox\"")
            .replace("\"VerifyFull\"", "\"VerifyEverything\"");
        assert!(
            json.contains("Fido2") && json.contains("Sandbox") && json.contains("VerifyEverything"),
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
        // **Not `→ default`, which is what this line used to assert.** The two
        // above degrade to a default because a wrong guess about SSH auth or an
        // environment badge costs a re-pick. This one decides whether the
        // password goes on the wire in the clear, so the unknown value resolves
        // to the *strictest* rung: a rollback that cannot connect until the
        // mode is corrected, rather than one that silently connects in
        // plaintext and then saves `"Disable"` over the user's choice.
        assert_eq!(
            back.connections[0].tls.mode,
            SslMode::STRICTEST,
            "→ strictest, not default"
        );
        assert_eq!(
            back.connections[0].tls.ca_path, "/etc/ca.crt",
            "the rest of the TLS block survives the unknown mode"
        );
    }

    /// **The high-water mark survives a round trip, and an older file reads
    /// `0`.**
    ///
    /// `highest_id` is what stops a deleted connection's id — and the keyring
    /// entries a failed `forget` left under it — being handed to the next
    /// connection created. It is worth nothing if it does not persist, and the
    /// back-compatible default is the half that decides whether the barrier can
    /// be rolled out at all: a `connections.json` written before the field
    /// existed must load, not fail, and must fall back to `Connection::next_id`'s
    /// answer rather than to a mark of `0` that blocks nothing *and* claims to.
    #[test]
    fn the_connection_high_water_mark_round_trips_and_defaults_to_nothing() {
        let file = ConnectionsFile {
            connections: Vec::new(),
            active: None,
            highest_id: 12,
        };
        let json = serde_json::to_string(&file).expect("serialize");
        let back: ConnectionsFile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.highest_id, 12);

        // A file from before the field: it loads, and the mark is nothing.
        let old: ConnectionsFile =
            serde_json::from_str(r#"{"connections": [], "active": null}"#).expect("an older file");
        assert_eq!(old.highest_id, 0);
        assert_eq!(
            crate::connection::Connection::next_id_after(&old.connections, old.highest_id),
            crate::connection::Connection::next_id(&old.connections),
            "an older file must answer exactly as it did before the field existed"
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
    fn recover_uses_primary_without_reading_either_sibling() {
        // A healthy primary must never consult a sibling (laziness guard).
        let (v, out) = recover(
            Load::Ok(5),
            || panic!("the staged copy must not be read"),
            || panic!("backup must not be read"),
        );
        assert_eq!(v, 5);
        assert_eq!(out, Recovered::Primary);
    }

    /// A first run is `Absent` **and** no sibling holding anything. It used to
    /// be `Absent` alone — this test asserted that, by name, and so pinned the
    /// bug it was written to describe. Kept, with the missing half added.
    #[test]
    fn recover_absent_defaults_only_when_no_sibling_answers() {
        let (v, out) = recover::<i32>(Load::Absent, || Load::Absent, || Load::Absent);
        assert_eq!(v, 0); // i32::default()
        assert_eq!(out, Recovered::FirstRun);
        // The staged copy is the newer of the two, so it is asked first — and
        // where it answers, the backup is not read at all.
        let (v, out) = recover::<i32>(Load::Absent, || Load::Ok(7), || panic!("not reached"));
        assert_eq!(v, 7);
        assert_eq!(out, Recovered::Restored(".tmp"));
        // A sibling that does not parse is not a copy.
        let (v, out) = recover::<i32>(
            Load::Absent,
            || Load::Corrupt("half".into()),
            || Load::Ok(11),
        );
        assert_eq!(v, 11);
        assert_eq!(out, Recovered::Restored(".bak"));
    }

    #[test]
    fn recover_corrupt_prefers_valid_backup_and_flags_preserve() {
        let (v, out) = recover(
            Load::Corrupt("bad".to_string()),
            || panic!("the corrupt path asks the backup, not the staged copy"),
            || Load::Ok(9),
        );
        assert_eq!(v, 9);
        // Primary preserved as `.corrupt`.
        assert_eq!(out, Recovered::Corrupt("bad".to_string()));
    }

    #[test]
    fn recover_corrupt_falls_back_to_default_when_backup_unusable() {
        // Backup absent → default, still preserve the corrupt primary.
        let (v, out) = recover::<i32>(
            Load::Corrupt("e".to_string()),
            || Load::Absent,
            || Load::Absent,
        );
        assert_eq!(v, 0);
        assert!(matches!(out, Recovered::Corrupt(_)));
        // Backup also corrupt → default, still preserve.
        let (v, out) = recover::<i32>(
            Load::Corrupt("e".to_string()),
            || Load::Absent,
            || Load::Corrupt("e2".to_string()),
        );
        assert_eq!(v, 0);
        assert!(matches!(out, Recovered::Corrupt(_)));
    }

    /// **Widening a consent setting is not a decision to make from an absence.**
    /// The one-way `AiData` migration promotes every saved connection to `Full`
    /// on this answer and never re-resolves, so the three ways a file can fail
    /// to say anything all have to read as "no evidence" — which is exactly what
    /// `load_ui_state`'s `true` default got wrong, and the reason this function
    /// exists at all. It opened with `std::fs::read`, so nothing could reach it.
    #[test]
    fn only_a_recorded_boolean_is_evidence_of_the_legacy_flag() {
        // A recorded flag is read as written — both ways round.
        assert_eq!(
            legacy_ai_run_queries_in(br#"{"ai_run_queries": true}"#),
            Some(true)
        );
        assert_eq!(
            legacy_ai_run_queries_in(br#"{"ai_run_queries": false}"#),
            Some(false)
        );
        // A file that never mentioned the flag says nothing about it.
        assert_eq!(legacy_ai_run_queries_in(b"{}"), None);
        // Nor does one holding something that is not a boolean…
        assert_eq!(
            legacy_ai_run_queries_in(br#"{"ai_run_queries": "yes"}"#),
            None
        );
        assert_eq!(legacy_ai_run_queries_in(br#"{"ai_run_queries": 1}"#), None);
        assert_eq!(
            legacy_ai_run_queries_in(br#"{"ai_run_queries": null}"#),
            None
        );
        // …nor a file nothing can be got out of, nor an empty one.
        assert_eq!(legacy_ai_run_queries_in(b"{"), None);
        assert_eq!(legacy_ai_run_queries_in(b""), None);
        // A top-level value that isn't an object has no key to read.
        assert_eq!(legacy_ai_run_queries_in(b"true"), None);
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

    /// **The sweep and the sentence about it read one predicate.** They are 500
    /// lines apart, and the version of this that shipped had the scrub with no
    /// sentence at all — so the pin is the *composition*: the store whose notice
    /// stays silent must also keep its `.corrupt` through an erasing save, even
    /// on the path that is allowed to sweep it.
    #[test]
    fn a_store_the_notice_makes_no_promise_about_keeps_its_corrupt_sibling() {
        let fs = FakeFs::default();
        fs.put("/cfg/ui.json.corrupt", r#"["a","b"]"#);
        write_secret_store(&fs, Path::new(CFG), &vec!["a", "b"], Saving::Replacing);
        write_secret_store(&fs, Path::new(CFG), &vec!["a"], Saving::Erasing);
        assert!(
            !recovery_notice(Path::new(CFG), "eof").contains("delete a connection"),
            "the notice promises nothing about this store, so the sweep may not touch it"
        );
        assert!(
            fs.get("/cfg/ui.json.corrupt").is_some(),
            "swept a sibling the notice did not warn about"
        );
    }

    /// **The one store whose `.corrupt` really is temporary says so.** The
    /// notice names the file and the user goes and opens it — for
    /// `connections.json` the next connection delete scrubs it, because that
    /// copy holds the plaintext fallback DB password, the SSH password and the
    /// key passphrase and nothing else ever removes it. A promise the app
    /// breaks by design has to be written as the qualified thing it is; the
    /// other stores' notice must *not* carry the warning, because for them it
    /// is now false.
    #[test]
    fn only_the_credential_store_warns_that_its_corrupt_copy_is_temporary() {
        let creds = recovery_notice(
            Path::new("/cfg/connections.json"),
            "unexpected end of input",
        );
        assert!(creds.contains("delete a connection"), "{creds}");
        let ordinary = recovery_notice(Path::new("/cfg/snippets.json"), "unexpected end of input");
        assert!(!ordinary.contains("delete a connection"), "{ordinary}");
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

    // ── The session's unrecoverable half ──────────────────────────────────

    /// A saved tab, spelled by what this rule asks about.
    fn saved(path: Option<&str>, dirty: bool, query: &str) -> super::SavedTab {
        super::SavedTab {
            query: query.into(),
            conn_id: 1,
            database: None,
            source: None,
            source_schema: None,
            name: None,
            pinned: false,
            path: path.map(PathBuf::from),
            file_crlf: false,
            file_bom: false,
            file_lossy: false,
            file_dirty: dirty,
        }
    }

    /// With the setting off, the only tab worth keeping is the one whose text is
    /// nowhere else: a file tab with unsaved edits. A query tab is retypeable, a
    /// clean file tab is on disk, and a quit is the one close path that cannot
    /// ask first.
    #[test]
    fn only_a_dirty_file_tab_survives_the_setting_being_off() {
        let file = super::SavedTabsFile {
            tabs: vec![
                saved(None, false, "select 1"),
                saved(Some("/sql/a.sql"), false, "select 2"),
                saved(Some("/sql/b.sql"), true, "select 3 -- edited"),
            ],
            active: 0,
        };
        let kept = file.unsaved_files_only();
        assert_eq!(kept.tabs.len(), 1);
        assert_eq!(kept.tabs[0].query, "select 3 -- edited");
        assert_eq!(kept.active, 0, "the index must not outrun the list");
    }

    /// Nothing unsaved, nothing kept — and the active index still has to be one
    /// this file can be indexed by.
    #[test]
    fn a_session_with_nothing_unsaved_keeps_nothing() {
        let file = super::SavedTabsFile {
            tabs: vec![
                saved(None, false, "select 1"),
                saved(None, true, "select 2"),
            ],
            active: 1,
        };
        let kept = file.unsaved_files_only();
        assert!(
            kept.tabs.is_empty(),
            "a dirty tab with no file is retypeable"
        );
        assert_eq!(kept.active, 0);
    }

    /// The active tab follows the survivor nearest to it, from either side, so
    /// the restored window lands on a tab that is actually there.
    #[test]
    fn the_active_index_follows_the_surviving_tabs() {
        let tabs = vec![
            saved(Some("/sql/a.sql"), true, "a"),
            saved(None, false, "q"),
            saved(Some("/sql/b.sql"), true, "b"),
        ];
        let at = |active: usize| {
            super::SavedTabsFile {
                tabs: tabs.clone(),
                active,
            }
            .unsaved_files_only()
            .active
        };
        assert_eq!(at(0), 0, "itself");
        assert_eq!(at(1), 1, "the query tab is gone; the next survivor");
        assert_eq!(at(2), 1, "itself, renumbered");
    }

    /// A fresh directory under the system temp dir, removed by `Dir`'s drop.
    ///
    /// Shared by the two modules below that must touch a real filesystem —
    /// a file's *mode* and a rename's *identity* are both properties of the
    /// filesystem, so there is nothing purer to test them against.
    struct Dir(PathBuf);

    impl Dir {
        fn new(tag: &str) -> Dir {
            use std::sync::atomic::{AtomicU32, Ordering};
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

    // ── The atomic save of a file that is not ours ────────────────────────
    //
    // Cross-platform, unlike `mod modes` below: the inode swap this asserts
    // around was measured on Windows 11 / NTFS, and it is a Windows-first
    // product.
    mod atomic_writes {
        use super::super::*;
        use super::Dir;

        /// Every staging sibling this module's writes could have left.
        fn leftovers(dir: &Path) -> Vec<String> {
            std::fs::read_dir(dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.contains(".schemaic-tmp"))
                .collect()
        }

        #[test]
        fn a_successful_save_leaves_no_staging_file_behind() {
            let dir = Dir::new("atomic-clean");
            let path = dir.0.join("orders.sql");
            write_file_atomic(&path, b"V1").unwrap();
            write_file_atomic(&path, b"V2").unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"V2");
            assert_eq!(leftovers(&dir.0), Vec::<String>::new());
        }

        /// **The staging name used to be a pure function of the target**, so two
        /// writers of one path shared one file — and the Antigravity marker is
        /// machine-global by design, with two Schemaic windows contending for it
        /// on every launch. The loser's rename found the file gone and fell
        /// through to `fs::write`, which truncates: verbatim the failure that
        /// call site's own doc says it uses this function to avoid.
        ///
        /// Driven here as the deterministic half of the same defect — a planted
        /// file of that name, which `fs::write` would have opened and written
        /// (it is `File::create`: no `O_EXCL`, follows symlinks, truncates).
        #[test]
        fn a_planted_staging_file_captures_nothing() {
            let dir = Dir::new("atomic-plant");
            let path = dir.0.join("notes.sql");
            std::fs::write(&path, b"original").unwrap();
            let planted = dir.0.join("notes.sql.schemaic-tmp");
            std::fs::write(&planted, b"planted").unwrap();

            write_file_atomic(&path, b"mine").unwrap();

            assert_eq!(std::fs::read(&path).unwrap(), b"mine");
            assert_eq!(
                std::fs::read(&planted).unwrap(),
                b"planted",
                "the write went into a file someone else owned"
            );
        }

        /// Two saves that overlap must not be able to derive one staging name.
        /// Asserted on the names rather than by racing threads, because the
        /// race's outcome is timing and the property is not.
        #[test]
        fn two_saves_of_one_path_never_stage_through_one_name() {
            let dir = Dir::new("atomic-unique");
            let target = dir.0.join("shared.json");
            let a = stage_beside(&target, b"A").unwrap();
            let b = stage_beside(&target, b"B").unwrap();
            assert_ne!(a, b);
            assert_eq!(std::fs::read(&a).unwrap(), b"A");
            assert_eq!(std::fs::read(&b).unwrap(), b"B");
            let _ = std::fs::remove_file(&a);
            let _ = std::fs::remove_file(&b);
        }

        /// A path with no file behind it is the ordinary "Save As to a new name"
        /// case, and staging must not need a target to copy anything from.
        #[test]
        fn a_first_write_to_a_new_path_works() {
            let dir = Dir::new("atomic-new");
            let path = dir.0.join("fresh.sql");
            write_file_atomic(&path, b"hello").unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"hello");
        }

        /// `fs::rename` acts on the link, not its target, so the managed copy in
        /// a dotfile repo stopped receiving writes and the link became an
        /// ordinary file. `~/.antigravity/settings.json` — another vendor's
        /// config, and the highest-blast-radius write in the app — is exactly
        /// the shape people symlink.
        #[cfg(unix)]
        #[test]
        fn a_symlinked_target_is_replaced_through_the_link() {
            let dir = Dir::new("atomic-link");
            let real = dir.0.join("managed.json");
            let link = dir.0.join("settings.json");
            std::fs::write(&real, b"V1").unwrap();
            std::os::unix::fs::symlink(&real, &link).unwrap();

            write_file_atomic(&link, b"V2").unwrap();

            assert_eq!(std::fs::read(&real).unwrap(), b"V2", "through the link");
            assert!(
                std::fs::symlink_metadata(&link)
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "the link itself must survive"
            );
        }

        /// The rename replaces the inode, so a file the user or another vendor
        /// had narrowed came back at whatever `umask` gives a fresh one — 0644
        /// under the usual one. A `seed.sql` holding a
        /// `CREATE USER … IDENTIFIED BY` became world-readable on Ctrl+S.
        #[cfg(unix)]
        #[test]
        fn a_saved_file_keeps_the_mode_it_had() {
            use std::os::unix::fs::PermissionsExt;
            let dir = Dir::new("atomic-mode");
            let path = dir.0.join("seed.sql");
            std::fs::write(&path, b"V1").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

            write_file_atomic(&path, b"V2").unwrap();

            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "the mode the user set");
            assert_eq!(std::fs::read(&path).unwrap(), b"V2");
        }
    }

    // ── File modes ────────────────────────────────────────────────────────
    //
    // The mode a file is created with *is* the boundary, so there is nothing
    // purer to test. Unix-only — on Windows the file inherits the profile ACL
    // and there is no mode to assert.
    #[cfg(unix)]
    mod modes {
        use super::super::*;
        use super::Dir;
        use std::os::unix::fs::PermissionsExt;

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
            write_json(Some(path.clone()), &"first", Saving::Replacing);
            write_json(Some(path.clone()), &"second", Saving::Replacing);
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
            write_json(Some(path.clone()), &"v", Saving::Replacing);
            let tmp = sibling(&path, ".tmp");
            std::fs::write(&tmp, b"orphan").unwrap();
            let v: String = read_json(Some(path.clone()));
            assert_eq!(v, "v");
            assert!(!tmp.exists(), "the orphaned .tmp must be swept");
        }
    }
}
