//! **A settings bundle** — the user's preferences and snippets in one file, to
//! carry to another machine or hand to a colleague, and read back.
//!
//! Pure over text and values: the app gathers the current [`UiState`] and
//! snippets, picks the file, and applies what [`parse`] and [`apply`] answer.
//!
//! **Only what travels.** [`PORTABLE_KEYS`] is an allowlist of `UiState`
//! fields — themes, scale, the editor, formatting, query behaviour and the
//! assistant's instructions — and everything else is left out: panel widths
//! and the schema filter (this window's), the expanded and hidden sets and the
//! activity intervals (keyed by connection ids, which differ per install), the
//! assistant's harness, model and CLI path (which name what is installed
//! here), and the legacy consent flag. An allowlist rather than a denylist so a
//! field added later stays home until someone decides it travels —
//! `every_portable_key_is_a_field_ui_state_writes` keeps the list honest.
//!
//! **A bundle never lowers a guard.** `confirm_writes` is imported only when it
//! turns confirmation on: a file from someone else must not quietly remove the
//! question before a write.
//!
//! Snippets travel when they are the user's own and scoped to everyone or to
//! an engine; one scoped to a connection names an id that means nothing on
//! another install, and is left out as the connection export leaves ids out.
//!
//! **The terminal's preferences travel too**, from `terminal.json` under their
//! own allowlist ([`TERMINAL_KEYS`]): its font, cursor and copy-on-select, but
//! never its shell, which names a program installed on this machine — and a
//! file from someone else must not choose what the terminal launches. The
//! terminal's settings type lives in a crate this one does not depend on, so
//! both halves go through [`apply_keys`], generic over the type that reads them.

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};

use crate::persist::UiState;
use crate::snippet::{Scope, Snippet, Source};

/// The envelope's version. A bundle naming a later one is refused rather than
/// half-read.
pub const BUNDLE_VERSION: u32 = 1;

/// What an exported bundle's file name ends with.
pub const BUNDLE_FILE_SUFFIX: &str = "schemaic-settings.json";

/// The `UiState` fields a bundle carries — see the module doc for what is left
/// out and why.
pub const PORTABLE_KEYS: &[&str] = &[
    "ui_theme",
    "editor_theme",
    "ui_scale",
    "ui_language",
    "editor_font_size",
    "tab_width",
    "soft_tabs",
    "word_wrap",
    "editor_rainbow_brackets",
    "editor_show_whitespace",
    "editor_indent_guides",
    "editor_predictions",
    "editor_remembered_predictions",
    "completion_table_aliases",
    "format_keyword_case",
    "format_comma_style",
    "format_statement_gap",
    "format_join_on_own_line",
    "format_river",
    "format_align",
    "format_wrap_width",
    "format_function_case",
    "format_type_case",
    "row_limit",
    "statement_timeout_secs",
    "confirm_writes",
    "live_validate",
    "notify_finished",
    "restore_tabs",
    "ai_effort",
    "ai_instructions",
    "ai_schema_scope",
    "ai_gutter",
];

/// The `terminal.json` fields a bundle carries — everything but `shell`; see
/// the module doc.
pub const TERMINAL_KEYS: &[&str] = &[
    "font_size",
    "copy_on_select",
    "cursor_style",
    "cursor_blink",
];

/// A bundle as read: the portable settings it names (only those — any other
/// key in the file is dropped here), the terminal's, and its snippets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bundle {
    pub settings: Map<String, Value>,
    pub terminal: Map<String, Value>,
    pub snippets: Vec<Snippet>,
}

/// The bundle for `state`, the terminal's settings `terminal` and `snippets`,
/// as the file's text.
pub fn export_json(state: &UiState, terminal: &impl Serialize, snippets: &[Snippet]) -> String {
    let snippets: Vec<&Snippet> = snippets.iter().filter(|s| travels(s)).collect();
    let mut out = Map::new();
    out.insert("schemaic_settings".into(), Value::from(BUNDLE_VERSION));
    out.insert("settings".into(), Value::Object(pick(state, PORTABLE_KEYS)));
    out.insert(
        "terminal".into(),
        Value::Object(pick(terminal, TERMINAL_KEYS)),
    );
    out.insert(
        "snippets".into(),
        serde_json::to_value(snippets).unwrap_or(Value::Array(Vec::new())),
    );
    serde_json::to_string_pretty(&Value::Object(out)).unwrap_or_default()
}

/// Read a bundle, or say why it is not one.
pub fn parse(json: &str) -> Result<Bundle, String> {
    let v: Value =
        serde_json::from_str(crate::text::strip_bom(json)).map_err(|e| format!("not JSON: {e}"))?;
    let version = v
        .get("schemaic_settings")
        .and_then(Value::as_u64)
        .ok_or("not a Schemaic settings file")?;
    if version > u64::from(BUNDLE_VERSION) {
        return Err(format!(
            "written by a newer Schemaic (format {version}); update to read it"
        ));
    }
    let allowed = |section: &str, keys: &[&str]| -> Map<String, Value> {
        v.get(section)
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter(|(k, _)| keys.contains(&k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default()
    };
    let settings = allowed("settings", PORTABLE_KEYS);
    // A bundle from before the terminal travelled has no section: nothing to apply.
    let terminal = allowed("terminal", TERMINAL_KEYS);
    // A snippet this build cannot read is dropped alone, not the whole file.
    let snippets = v
        .get("snippets")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|s| serde_json::from_value::<Snippet>(s.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    Ok(Bundle {
        settings,
        terminal,
        snippets,
    })
}

/// `state` with the bundle's settings applied, and how many of them changed
/// anything. A value this build cannot read for its field leaves the field as
/// it was; `confirm_writes` is taken only when it turns confirmation on.
///
/// Each key is tried on its own against `UiState`'s own deserialisation, so a
/// value is accepted exactly when the file it came from would have been read.
pub fn apply(settings: &Map<String, Value>, state: &UiState) -> (UiState, usize) {
    apply_with(settings, state, |_| {})
}

/// [`apply`], with `normalise` — the app's own reading of a `UiState` (its
/// theme keys' fallbacks, its clamps) — asked of each value too: **one the
/// app would rewrite on reading is refused**, since it would otherwise be
/// counted as a change and then land as the fallback rather than as what the
/// file said.
pub fn apply_with(
    settings: &Map<String, Value>,
    state: &UiState,
    normalise: impl Fn(&mut UiState),
) -> (UiState, usize) {
    apply_keys(PORTABLE_KEYS, settings, state, normalise)
}

/// The rule [`apply_with`] applies, for any settings type `T` and allowlist
/// `keys` — the terminal's half of a bundle goes through it with
/// [`TERMINAL_KEYS`]. Each key is tried alone against `T`'s own reading, then
/// against `normalise`; a value either refuses is skipped. `confirm_writes`,
/// wherever it is allowed, is taken only when it turns confirmation on.
pub fn apply_keys<T: Serialize + DeserializeOwned + Clone>(
    keys: &[&str],
    settings: &Map<String, Value>,
    state: &T,
    normalise: impl Fn(&mut T),
) -> (T, usize) {
    let Ok(Value::Object(mut current)) = serde_json::to_value(state) else {
        return (state.clone(), 0);
    };
    let mut changed = 0;
    for (key, value) in settings {
        if !keys.contains(&key.as_str()) || current.get(key) == Some(value) {
            continue;
        }
        if key == "confirm_writes" && value != &Value::Bool(true) {
            continue;
        }
        let mut candidate = current.clone();
        candidate.insert(key.clone(), value.clone());
        let Ok(read) = serde_json::from_value::<T>(Value::Object(candidate)) else {
            continue;
        };
        // What the field reads back as — a shim may have normalised it.
        let Ok(Value::Object(read)) = serde_json::to_value(&read) else {
            continue;
        };
        // And what the app would make of it: a value its own reading rewrites
        // is not the file's value at all.
        let mut honoured = match serde_json::from_value::<T>(Value::Object(read.clone())) {
            Ok(s) => s,
            Err(_) => continue,
        };
        normalise(&mut honoured);
        let Ok(Value::Object(honoured)) = serde_json::to_value(&honoured) else {
            continue;
        };
        if honoured.get(key) != read.get(key) {
            continue;
        }
        if read.get(key) != current.get(key) {
            current = read;
            changed += 1;
        }
    }
    match serde_json::from_value(Value::Object(current)) {
        Ok(s) => (s, changed),
        Err(_) => (state.clone(), 0),
    }
}

/// Add `incoming` snippets to `existing` as the user's own, each with a fresh
/// id, never used, and travelling-scope only; one whose name and body a
/// snippet already has is skipped, and an abbreviation already taken is
/// dropped rather than left to collide. Answers how many were added.
pub fn merge_snippets(existing: &mut Vec<Snippet>, incoming: Vec<Snippet>) -> usize {
    let mut added = 0;
    for mut s in incoming.into_iter().filter(travels) {
        if existing
            .iter()
            .any(|e| e.name == s.name && e.body == s.body)
        {
            continue;
        }
        if let Some(a) = &s.abbrev
            && existing.iter().any(|e| {
                e.abbrev
                    .as_deref()
                    .is_some_and(|b| b.eq_ignore_ascii_case(a))
            })
        {
            s.abbrev = None;
        }
        s.id = crate::snippet::next_id(existing);
        s.last_used = None;
        s.source = Source::User;
        existing.push(s);
        added += 1;
    }
    added
}

/// The phrase the confirmation says after "changes": how many settings would
/// change and how many snippets would be added.
pub fn summary(changed: usize, snippets: usize) -> String {
    let settings = crate::text::plural(changed, "setting", "settings");
    let snips = crate::text::plural(snippets, "snippet", "snippets");
    format!("{changed} {settings} and adds {snippets} {snips}")
}

/// The `keys` fields of `value` as written, those it has.
fn pick(value: &impl Serialize, keys: &[&str]) -> Map<String, Value> {
    let all = serde_json::to_value(value).unwrap_or(Value::Null);
    keys.iter()
        .filter_map(|k| Some((k.to_string(), all.get(*k)?.clone())))
        .collect()
}

/// Does `s` travel in a bundle?
fn travels(s: &Snippet) -> bool {
    s.source == Source::User && matches!(s.scope, Scope::Global | Scope::Dialect(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intel::SqlDialect;

    fn snip(id: u64, name: &str, body: &str, scope: Scope) -> Snippet {
        Snippet {
            id,
            name: name.into(),
            abbrev: None,
            body: body.into(),
            scope,
            source: Source::User,
            last_used: Some(5),
        }
    }

    /// The shape of `schemaic_term::TerminalSettings`, which this crate cannot
    /// name — its shell included, to prove it stays home.
    #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Term {
        shell: Option<String>,
        font_size: u16,
        copy_on_select: bool,
        cursor_style: String,
        cursor_blink: bool,
    }

    impl Default for Term {
        fn default() -> Self {
            Term {
                shell: Some("C:/Windows/System32/cmd.exe".into()),
                font_size: 13,
                copy_on_select: false,
                cursor_style: "block".into(),
                cursor_blink: true,
            }
        }
    }

    #[test]
    fn the_terminal_travels_without_its_shell() {
        let term = Term {
            font_size: 16,
            ..Term::default()
        };
        let json = export_json(&UiState::default(), &term, &[]);
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["terminal"]["font_size"], 16);
        assert!(v["terminal"].get("shell").is_none());
        assert_eq!(
            v["terminal"].as_object().unwrap().len(),
            TERMINAL_KEYS.len()
        );
        assert_eq!(parse(&json).unwrap().terminal["font_size"], 16);
    }

    #[test]
    fn a_shell_named_in_a_bundle_is_never_applied() {
        let b = parse(
            r#"{"schemaic_settings": 1, "terminal": {"shell": "evil.exe", "cursor_style": "bar"}}"#,
        )
        .unwrap();
        assert!(b.terminal.get("shell").is_none());
        // Even handed straight to the applier, the allowlist holds.
        let mut sneaky = b.terminal.clone();
        sneaky.insert("shell".into(), Value::String("evil.exe".into()));
        let (applied, changed) = apply_keys(TERMINAL_KEYS, &sneaky, &Term::default(), |_| {});
        assert_eq!(changed, 1);
        assert_eq!(applied.cursor_style, "bar");
        assert_eq!(applied.shell, Term::default().shell);
    }

    #[test]
    fn the_terminal_half_refuses_what_its_own_reading_would_change() {
        let mut m = Map::new();
        m.insert("font_size".into(), Value::from(400u64));
        m.insert("copy_on_select".into(), Value::from("yes"));
        m.insert("cursor_blink".into(), Value::Bool(false));
        let offered = |t: &mut Term| {
            if ![12, 13, 14, 16, 18].contains(&t.font_size) {
                t.font_size = 13;
            }
        };
        let (applied, changed) = apply_keys(TERMINAL_KEYS, &m, &Term::default(), offered);
        assert_eq!(changed, 1);
        assert_eq!(applied.font_size, 13);
        assert!(!applied.cursor_blink);
    }

    #[test]
    fn a_bundle_from_before_the_terminal_travelled_reads_with_none() {
        let b = parse(r#"{"schemaic_settings": 1, "settings": {}}"#).unwrap();
        assert!(b.terminal.is_empty());
    }

    #[test]
    fn every_portable_key_is_a_field_ui_state_writes() {
        let written = serde_json::to_value(UiState::default()).unwrap();
        for key in PORTABLE_KEYS {
            assert!(written.get(*key).is_some(), "{key} is not a UiState field");
        }
    }

    #[test]
    fn an_export_carries_only_the_portable_settings_and_travelling_snippets() {
        let state = UiState {
            ui_theme: "light".into(),
            schema_w: 999.0,
            ai_cli_path: "C:/tools/claude.exe".into(),
            ..UiState::default()
        };
        let snippets = vec![
            snip(1, "everyone", "SELECT 1", Scope::Global),
            snip(
                2,
                "pg only",
                "SELECT 2",
                Scope::Dialect(SqlDialect::Postgres),
            ),
            snip(3, "one server", "SELECT 3", Scope::Conn(7)),
        ];
        let json = export_json(&state, &Term::default(), &snippets);
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["schemaic_settings"], BUNDLE_VERSION);
        let settings = v["settings"].as_object().unwrap();
        assert_eq!(settings["ui_theme"], "light");
        assert!(settings.get("schema_w").is_none());
        assert!(settings.get("ai_cli_path").is_none());
        assert_eq!(settings.len(), PORTABLE_KEYS.len());
        let names: Vec<&str> = v["snippets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["everyone", "pg only"]);
    }

    #[test]
    fn a_bundle_reads_back_what_was_written() {
        let state = UiState {
            editor_font_size: 17.0,
            ..UiState::default()
        };
        let json = export_json(
            &state,
            &Term::default(),
            &[snip(1, "a", "SELECT 1", Scope::Global)],
        );
        let b = parse(&json).unwrap();
        assert_eq!(b.settings["editor_font_size"], 17.0);
        assert_eq!(b.snippets.len(), 1);
        // A BOM in front is no matter.
        assert!(parse(&format!("\u{feff}{json}")).is_ok());
    }

    #[test]
    fn a_file_that_is_not_a_bundle_or_is_from_a_later_build_is_refused() {
        assert!(parse("{").is_err());
        assert!(parse(r#"{"connections": []}"#).is_err());
        let later = format!(
            r#"{{"schemaic_settings": {}, "settings": {{}}}}"#,
            BUNDLE_VERSION + 1
        );
        let err = parse(&later).unwrap_err();
        assert!(err.contains("newer"), "{err}");
    }

    #[test]
    fn a_key_outside_the_allowlist_is_never_applied() {
        let b = parse(
            r#"{"schemaic_settings": 1, "settings": {"schema_w": 5.0, "ai_cli_path": "evil.exe", "ui_theme": "light"}}"#,
        )
        .unwrap();
        assert_eq!(b.settings.len(), 1);
        let (applied, changed) = apply(&b.settings, &UiState::default());
        assert_eq!(applied.ui_theme, "light");
        assert_eq!(applied.ai_cli_path, UiState::default().ai_cli_path);
        assert_eq!(changed, 1);
    }

    #[test]
    fn applying_counts_only_what_changed_and_skips_an_unreadable_value() {
        let state = UiState::default();
        let mut settings = Map::new();
        settings.insert("word_wrap".into(), Value::Bool(state.word_wrap));
        settings.insert("tab_width".into(), Value::String("wide".into()));
        settings.insert("row_limit".into(), Value::from(1234u64));
        let (applied, changed) = apply(&settings, &state);
        assert_eq!(changed, 1);
        assert_eq!(applied.row_limit, 1234);
        assert_eq!(applied.tab_width, state.tab_width);
    }

    #[test]
    fn a_bundle_can_turn_confirmation_on_but_never_off() {
        let on = UiState {
            confirm_writes: true,
            ..UiState::default()
        };
        let off = UiState {
            confirm_writes: false,
            ..UiState::default()
        };
        let say = |v: bool| {
            let mut m = Map::new();
            m.insert("confirm_writes".into(), Value::Bool(v));
            m
        };
        assert!(apply(&say(false), &on).0.confirm_writes);
        assert_eq!(apply(&say(false), &on).1, 0);
        assert!(apply(&say(true), &off).0.confirm_writes);
    }

    /// **A value the app would rewrite on reading is refused**, not counted
    /// and then quietly replaced: a theme key from a newer build would land
    /// as the default theme, and an out-of-range font as the clamp.
    #[test]
    fn a_value_the_apps_own_reading_would_change_is_refused() {
        let state = UiState::default();
        let mut settings = Map::new();
        settings.insert("ui_theme".into(), Value::String("from-the-future".into()));
        settings.insert("editor_font_size".into(), Value::from(400.0));
        settings.insert("row_limit".into(), Value::from(77u64));
        let normalise = |s: &mut UiState| {
            if s.ui_theme == "from-the-future" {
                s.ui_theme = "dark".into();
            }
            s.editor_font_size = s.editor_font_size.min(32.0);
        };
        let (applied, changed) = apply_with(&settings, &state, normalise);
        assert_eq!(changed, 1);
        assert_eq!(applied.ui_theme, state.ui_theme);
        assert_eq!(applied.editor_font_size, state.editor_font_size);
        assert_eq!(applied.row_limit, 77);
    }

    #[test]
    fn the_summary_counts_both_halves_in_words() {
        assert_eq!(summary(1, 0), "1 setting and adds 0 snippets");
        assert_eq!(summary(3, 1), "3 settings and adds 1 snippet");
    }

    #[test]
    fn merged_snippets_get_fresh_ids_and_never_collide() {
        let mut mine = vec![snip(1, "a", "SELECT 1", Scope::Global)];
        mine[0].abbrev = Some("sel".into());
        let mut theirs_abbrev = snip(1, "b", "SELECT 2", Scope::Global);
        theirs_abbrev.abbrev = Some("sel".into());
        let incoming = vec![
            // The same name and body: already here.
            snip(9, "a", "SELECT 1", Scope::Global),
            theirs_abbrev,
            snip(2, "c", "SELECT 3", Scope::Dialect(SqlDialect::MySql)),
            // Connection-scoped or built-in: does not travel.
            snip(3, "d", "SELECT 4", Scope::Conn(4)),
            Snippet {
                source: Source::Builtin,
                ..snip(4, "e", "SELECT 5", Scope::Global)
            },
        ];
        assert_eq!(merge_snippets(&mut mine, incoming), 2);
        let ids: Vec<u64> = mine.iter().map(|s| s.id).collect();
        assert_eq!(ids, [1, 2, 3]);
        assert_eq!(mine[1].name, "b");
        assert_eq!(mine[1].abbrev, None, "a taken abbreviation is dropped");
        assert!(mine[1..].iter().all(|s| s.last_used.is_none()));
        assert!(mine.iter().all(|s| s.source == Source::User));
    }
}
