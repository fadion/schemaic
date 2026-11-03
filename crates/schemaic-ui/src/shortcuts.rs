//! The app's keyboard shortcuts, in **two tables** — the rebindable commands
//! of [`schemaic_core::keymap`], and the fixed keys of [`SHORTCUTS`] — and the
//! test that keeps the second one honest.
//!
//! Together they are the app's only keyboard documentation: the Shortcuts modal
//! ([`crate::shortcuts_view::help_overlay`]) renders both, and for a binding like
//! Ctrl+H or Ctrl+G there is no other affordance anywhere in the UI. A binding
//! missing here is a feature nobody can find.
//!
//! **A command's key is the keymap's**, read through [`keymap`]: the handlers
//! resolve a press through it ([`chord_of`], `Keymap::resolve`), and the modal,
//! the palette ([`command_keys`]) and the menus ([`MenuKey`]) show the binding it
//! holds. None of them spells a command's key, so a key the user moved is moved
//! everywhere at once.
//!
//! It used to be a literal inside the modal, hand-maintained against `match` arms
//! in three files, and it drifted exactly as its own comment predicted it would:
//! Alt+↑/↓ (move line up/down), Ctrl+↑/↓ (recall an AI prompt), Ctrl+Shift+C/V
//! (terminal copy/paste) and Ctrl+Home/End (first/last cell) were all implemented
//! and documented nowhere.
//!
//! # What the test does, and what it deliberately doesn't
//!
//! The fixed keys' handlers can't render *from* this table — they are `match`
//! arms on `Key::Character` spread across several files. So the
//! guarantee is enforced from the other side: the tests below scan the files in
//! `KEY_FILES` for the five idioms the codebase actually uses to bind a
//! **Ctrl/Alt + letter** key, and fail when one isn't named here.
//!
//! That scope is the point, and it is narrower than "every key". Modified-letter
//! keys are where every undiscoverable binding lives, because they are the ones
//! with no menu item, no button and no hover text. Two things are therefore
//! outside the gate, and neither is an oversight:
//!
//! - **Plain keys** (arrows, Enter, Escape, Delete) are bound in dozens of
//!   places for ordinary navigation, so gating them would be all noise, and a
//!   user guesses them anyway.
//! - **Named modified keys** — `Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+Enter`,
//!   Ctrl+1…9 — are matched as `NamedKey`s, not characters, so the letter scan
//!   cannot see them and nothing checks that their rows are still true. They are
//!   in the table because they belong there; the claim this module makes about
//!   them is only that somebody wrote them down.
//!
//! Like [`doc_coverage`](../../../schemaic-core/tests/doc_coverage.rs), it is a
//! deliberately weak test. It proves a binding was *thought about*, not that its
//! row is accurate or in the right group — it only has to catch the one failure
//! that keeps recurring, which is a binding added and never written down.

use std::borrow::Cow;

use floem::keyboard::Modifiers;
use schemaic_core::keymap::{Chord, Command, Context, FixedKey, Key as ChordKey, Keymap};

use crate::i18n::tr;

/// A heading of the Shortcuts modal — and the scope the tests below check a
/// binding against, which is why it is an enum and not its label: a group is
/// named in `KEY_FILES`, `COMMAND_KEYS` and `MenuKey::row`, and none of those
/// may change with the language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Group {
    Global,
    Editor,
    ResultsGrid,
    SchemaTree,
    AiPanel,
    Terminal,
    ErDiagram,
    ServerActivity,
}

impl Group {
    /// The heading, in the interface's language.
    pub(crate) fn label(self) -> String {
        match self {
            Group::Global => tr!("shortcuts-group-global"),
            Group::Editor => tr!("shortcuts-group-editor"),
            Group::ResultsGrid => tr!("shortcuts-group-results-grid"),
            Group::SchemaTree => tr!("shortcuts-group-schema-tree"),
            Group::AiPanel => tr!("shortcuts-group-ai-panel"),
            Group::Terminal => tr!("shortcuts-group-terminal"),
            Group::ErDiagram => tr!("shortcuts-group-er-diagram"),
            Group::ServerActivity => tr!("shortcuts-group-server-activity"),
        }
    }

    /// Where the group's keys are listened for, as the keymap names it.
    pub(crate) fn context(self) -> Context {
        match self {
            Group::Global => Context::Global,
            Group::Editor => Context::Editor,
            Group::ResultsGrid => Context::ResultsGrid,
            Group::SchemaTree => Context::SchemaTree,
            Group::AiPanel => Context::AiPanel,
            Group::Terminal => Context::Terminal,
            Group::ErDiagram => Context::ErDiagram,
            Group::ServerActivity => Context::ServerActivity,
        }
    }

    /// The group a command's row is shown in.
    pub(crate) fn of(command: Command) -> Group {
        let context = command.context();
        SHORTCUTS
            .iter()
            .map(|(g, _)| *g)
            .find(|g| g.context() == context)
            .unwrap_or(Group::Global)
    }
}

/// What a command does, in the interface's language — its row in the
/// Shortcuts modal.
pub(crate) fn command_label(command: Command) -> String {
    match command {
        Command::FindAnywhere => tr!("shortcuts-find-anywhere"),
        Command::CommandPalette => tr!("shortcuts-command-palette"),
        Command::NewTab => tr!("shortcuts-new-tab"),
        Command::CloseTab => tr!("shortcuts-close-tab"),
        Command::NextTab => tr!("shortcuts-next-tab"),
        Command::PreviousTab => tr!("shortcuts-previous-tab"),
        Command::ReopenTab => tr!("shortcuts-reopen-tab"),
        Command::OpenFile => tr!("shortcuts-open-file"),
        Command::SaveFile => tr!("shortcuts-save-file"),
        Command::SaveFileAs => tr!("shortcuts-save-file-as"),
        Command::ToggleSchema => tr!("shortcuts-toggle-schema"),
        Command::ToggleAi => tr!("shortcuts-toggle-ai"),
        Command::ToggleTerminal => tr!("shortcuts-toggle-terminal"),
        Command::FormatCode => tr!("shortcuts-format-code"),
        Command::FormatStatement => tr!("shortcuts-format-statement"),
        Command::MakeSingleLine => tr!("shortcuts-single-line"),
        Command::GoToLine => tr!("shortcuts-go-to-line"),
    }
}

// ── The keymap ───────────────────────────────────────────────────────────────

thread_local! {
    static KEYMAP: std::cell::RefCell<
        Option<(floem::reactive::RwSignal<Keymap>, floem::reactive::Scope)>,
    > = const { std::cell::RefCell::new(None) };
}

/// The keymap as it stands — window-wide, in a detached scope like
/// [`modifier_keys`]. The defaults until [`load_keymap`] has read the user's
/// file; every handler, the modal and the palette read this one signal.
pub(crate) fn keymap() -> floem::reactive::RwSignal<Keymap> {
    KEYMAP.with(|cell| {
        if cell.borrow().is_none() {
            let scope = floem::reactive::Scope::new();
            let sig = scope.create_rw_signal(Keymap::defaults());
            *cell.borrow_mut() = Some((sig, scope));
        }
        cell.borrow().as_ref().unwrap().0
    })
}

/// Read `keymap.json` into [`keymap`], and say what in it could not be kept —
/// `None` when everything was. The notice is the caller's to show.
pub fn load_keymap() -> Option<String> {
    use floem::reactive::SignalUpdate;
    let file = schemaic_core::persist::load_json::<schemaic_core::keymap::KeymapFile>(
        schemaic_core::keymap::FILE,
    );
    let (map, problems) = Keymap::load(&file, &fixed_keys());
    keymap().set(map);
    if problems.is_empty() {
        return None;
    }
    let lines: Vec<String> = problems.iter().map(problem_text).collect();
    Some(format!(
        "{}\n\n{}",
        tr!("keymap-problems"),
        lines.join("\n")
    ))
}

/// The keymap's changes from the defaults, as a settings bundle carries them.
pub fn keymap_file() -> schemaic_core::keymap::KeymapFile {
    use floem::reactive::SignalWith;
    keymap().with_untracked(Keymap::to_file)
}

/// What a settings bundle's keymap would make the keymap: the keymap itself,
/// how many commands it binds differently from now, and the notice for any
/// entry of it that could not be applied (`None` when all could).
pub fn keymap_from_bundle(
    file: &schemaic_core::keymap::KeymapFile,
) -> (Keymap, usize, Option<String>) {
    use floem::reactive::SignalWith;
    let (map, problems) = Keymap::load(file, &fixed_keys());
    let changes = keymap().with_untracked(|now| now.changes_from(&map));
    let notice = (!problems.is_empty()).then(|| {
        let lines: Vec<String> = problems.iter().map(problem_text).collect();
        format!("{}\n\n{}", tr!("keymap-problems"), lines.join("\n"))
    });
    (map, changes, notice)
}

/// Make `map` the keymap and write it to `keymap.json`.
pub fn save_keymap(map: Keymap) {
    use floem::reactive::SignalUpdate;
    schemaic_core::persist::save_json(schemaic_core::keymap::FILE, &map.to_file());
    keymap().set(map);
}

/// One entry of `keymap.json` that could not be kept, as a line of the notice.
fn problem_text(problem: &schemaic_core::keymap::Problem) -> String {
    use schemaic_core::keymap::Problem;
    match problem {
        Problem::UnknownCommand(id) => tr!("keymap-problem-unknown", id = id.clone()),
        Problem::Unreadable { command, text, .. } => tr!(
            "keymap-problem-unreadable",
            command = command_label(*command),
            keys = text.clone()
        ),
        Problem::Refused {
            command,
            chord,
            refusal,
        } => tr!(
            "keymap-problem-refused",
            command = command_label(*command),
            keys = chord_label(*chord),
            reason = refusal_text(*refusal)
        ),
        Problem::Fixed {
            command,
            chord,
            fixed,
        } => tr!(
            "keymap-problem-fixed",
            command = command_label(*command),
            keys = chord_label(*chord),
            holder = fixed_rows()
                .get(*fixed)
                .map(|r| (r.describe)())
                .unwrap_or_default()
        ),
        Problem::Conflict {
            command,
            chord,
            holder,
        } => tr!(
            "keymap-problem-taken",
            command = command_label(*command),
            keys = chord_label(*chord),
            holder = command_label(*holder)
        ),
    }
}

/// Why no command may have a chord, in the interface's language.
pub(crate) fn refusal_text(refusal: schemaic_core::keymap::Refusal) -> String {
    use schemaic_core::keymap::Refusal;
    let primary = keys_label("Ctrl").into_owned();
    match refusal {
        Refusal::NeedsPrimary => tr!("keymap-refusal-needs-primary", primary = primary),
        Refusal::NeedsModifier => tr!(
            "keymap-refusal-needs-modifier",
            primary = primary,
            alt = keys_label("Alt").into_owned()
        ),
        Refusal::Escape => tr!("keymap-refusal-escape"),
        Refusal::Editing => tr!("keymap-refusal-editing"),
        Refusal::System => tr!("keymap-refusal-system"),
        Refusal::AltGr => tr!("keymap-refusal-altgr"),
    }
}

/// A chord as this platform and language read it: `Ctrl+Shift+P`, `Cmd+…` on
/// macOS, `Strg+Umschalt+P` in German.
pub(crate) fn chord_label(chord: Chord) -> String {
    keys_label_on(&chord.to_string(), PRIMARY_IS_CMD, key_name).into_owned()
}

/// The key a command has now, as the screen shows it — `None` when the user
/// cleared it.
pub(crate) fn command_chord_label(command: Command) -> Option<String> {
    use floem::reactive::SignalWith;
    keymap()
        .with_untracked(|m| m.chord(command))
        .map(chord_label)
}

/// A fixed row of [`SHORTCUTS`], one chord of it at a time: the key the keymap
/// may not move a command onto, and what the modal says it does.
#[derive(Clone, Copy)]
pub(crate) struct FixedRow {
    pub key: FixedKey,
    pub describe: Describe,
}

/// Every chord the fixed rows bind, each with its row's description. A row of
/// several bindings (`Tab / Shift+Tab`, `Ctrl+1…9`) gives one entry per chord.
pub(crate) fn fixed_rows() -> Vec<FixedRow> {
    let mut out = Vec::new();
    for (group, rows) in SHORTCUTS {
        for (keys, describe) in *rows {
            for chord in row_chords(keys) {
                out.push(FixedRow {
                    key: FixedKey {
                        context: group.context(),
                        chord,
                    },
                    describe: *describe,
                });
            }
        }
    }
    out
}

/// [`fixed_rows`]' keys alone, as the keymap's checks take them. The index of
/// one is the index of its row.
pub(crate) fn fixed_keys() -> Vec<FixedKey> {
    fixed_rows().into_iter().map(|r| r.key).collect()
}

/// The chords a [`SHORTCUTS`] key string names. `Ctrl+1…9` is nine of them.
fn row_chords(keys: &str) -> Vec<Chord> {
    keys.split(" / ")
        .flat_map(|binding| match binding.strip_suffix("1…9") {
            Some(head) => ('1'..='9')
                .filter_map(|d| Chord::parse(&format!("{head}{d}")).ok())
                .collect(),
            None => Chord::parse(binding).ok().into_iter().collect::<Vec<_>>(),
        })
        .collect()
}

/// The chord a key press is, or `None` for a lone modifier or a key no chord
/// can name.
pub(crate) fn chord_of(
    key: &floem::keyboard::Key,
    physical: &floem::keyboard::PhysicalKey,
    mods: Modifiers,
) -> Option<Chord> {
    chord_from(key, physical, primary_held(mods), mods.alt(), mods.shift())
}

/// [`chord_of`] with the primary modifier already decided.
///
/// **A letter is read by its character and anything else by its place.** A
/// letter follows the layout, so Ctrl+Z is where the Z is on AZERTY. A key
/// that is not a Latin letter — a digit, `/`, `` ` ``, or a letter Shift or
/// AltGr has changed (`!` for Shift+1, `ł` for AltGr+L) — is read by the key
/// it sits on, as a US keyboard names it, so `Ctrl+Shift+/` is one chord
/// whatever Shift turned the `/` into, and Ctrl+Alt+L works on Windows, which
/// delivers Ctrl+Alt as AltGr.
fn chord_from(
    key: &floem::keyboard::Key,
    physical: &floem::keyboard::PhysicalKey,
    primary: bool,
    alt: bool,
    shift: bool,
) -> Option<Chord> {
    use floem::keyboard::{Key, NamedKey, PhysicalKey};
    let named = |n: &NamedKey| {
        Some(match n {
            NamedKey::Tab => ChordKey::Tab,
            NamedKey::Enter => ChordKey::Enter,
            NamedKey::Space => ChordKey::Space,
            NamedKey::Escape => ChordKey::Escape,
            NamedKey::Backspace => ChordKey::Backspace,
            NamedKey::Delete => ChordKey::Delete,
            NamedKey::Insert => ChordKey::Insert,
            NamedKey::Home => ChordKey::Home,
            NamedKey::End => ChordKey::End,
            NamedKey::PageUp => ChordKey::PageUp,
            NamedKey::PageDown => ChordKey::PageDown,
            NamedKey::ArrowUp => ChordKey::Up,
            NamedKey::ArrowDown => ChordKey::Down,
            NamedKey::ArrowLeft => ChordKey::Left,
            NamedKey::ArrowRight => ChordKey::Right,
            n => ChordKey::F(function_key(n)?),
        })
    };
    let at = match physical {
        PhysicalKey::Code(code) => us_char(*code),
        _ => None,
    };
    let key = match key {
        Key::Named(n) => named(n)?,
        Key::Character(c) => {
            let c = c.as_str();
            let mut chars = c.chars();
            match (chars.next(), chars.next()) {
                (Some(' '), None) => ChordKey::Space,
                (Some(ch), None) if ch.is_ascii_alphabetic() => {
                    ChordKey::Char(ch.to_ascii_lowercase())
                }
                (Some(ch), _) => ChordKey::Char(at.unwrap_or(ch.to_lowercase().next()?)),
                (None, _) => ChordKey::Char(at?),
            }
        }
        // A dead key, or one the layout gives no character: its place.
        _ => ChordKey::Char(at?),
    };
    Some(Chord {
        primary,
        alt,
        shift,
        key,
    })
}

/// `F1`…`F24` as a number.
fn function_key(n: &floem::keyboard::NamedKey) -> Option<u8> {
    use floem::keyboard::NamedKey as N;
    const KEYS: [N; 24] = [
        N::F1,
        N::F2,
        N::F3,
        N::F4,
        N::F5,
        N::F6,
        N::F7,
        N::F8,
        N::F9,
        N::F10,
        N::F11,
        N::F12,
        N::F13,
        N::F14,
        N::F15,
        N::F16,
        N::F17,
        N::F18,
        N::F19,
        N::F20,
        N::F21,
        N::F22,
        N::F23,
        N::F24,
    ];
    KEYS.iter().position(|k| k == n).map(|i| i as u8 + 1)
}

/// The character a US keyboard prints on the key at `code`, unshifted.
fn us_char(code: floem::keyboard::KeyCode) -> Option<char> {
    use floem::keyboard::KeyCode as K;
    const LETTERS: [K; 26] = [
        K::KeyA,
        K::KeyB,
        K::KeyC,
        K::KeyD,
        K::KeyE,
        K::KeyF,
        K::KeyG,
        K::KeyH,
        K::KeyI,
        K::KeyJ,
        K::KeyK,
        K::KeyL,
        K::KeyM,
        K::KeyN,
        K::KeyO,
        K::KeyP,
        K::KeyQ,
        K::KeyR,
        K::KeyS,
        K::KeyT,
        K::KeyU,
        K::KeyV,
        K::KeyW,
        K::KeyX,
        K::KeyY,
        K::KeyZ,
    ];
    const DIGITS: [K; 10] = [
        K::Digit0,
        K::Digit1,
        K::Digit2,
        K::Digit3,
        K::Digit4,
        K::Digit5,
        K::Digit6,
        K::Digit7,
        K::Digit8,
        K::Digit9,
    ];
    if let Some(i) = LETTERS.iter().position(|k| *k == code) {
        return Some((b'a' + i as u8) as char);
    }
    if let Some(i) = DIGITS.iter().position(|k| *k == code) {
        return Some((b'0' + i as u8) as char);
    }
    Some(match code {
        K::Backquote => '`',
        K::Minus => '-',
        K::Equal => '=',
        K::BracketLeft => '[',
        K::BracketRight => ']',
        K::Backslash => '\\',
        K::Semicolon => ';',
        K::Quote => '\'',
        K::Comma => ',',
        K::Period => '.',
        K::Slash => '/',
        _ => return None,
    })
}

/// What a row does, in the interface's language: a function rather than the
/// text, so the table stays a `const` while each message id is still checked
/// by `tr!` at compile time. The key string beside it is never translated.
pub(crate) type Describe = fn() -> String;

/// One group of the Shortcuts modal: a heading and its `(keys, description)` rows.
pub(crate) type ShortcutGroup = (Group, &'static [(&'static str, Describe)]);

/// Every shortcut the modal shows, in display order.
///
/// **What earns a row:** every Ctrl/Alt-modified binding, plus the plain keys that
/// do something you would not guess — Delete marks a row for deletion, Enter
/// edits a cell, Tab hops between cells. Plain arrow keys and "Escape closes this"
/// are left out on purpose: they are what everyone already tries, and listing them
/// would bury the handful of bindings that genuinely cannot be discovered.
pub(crate) const SHORTCUTS: &[ShortcutGroup] = &[
    (
        Group::Global,
        &[("Ctrl+1…9", || tr!("shortcuts-jump-to-tab"))],
    ),
    (
        Group::Editor,
        &[
            ("Ctrl+Enter", || tr!("shortcuts-run-query")),
            ("Ctrl+Space", || tr!("shortcuts-autocomplete")),
            ("Ctrl+K", || tr!("shortcuts-inline-ai")),
            ("Ctrl+F", || tr!("shortcuts-find-in-editor")),
            // Ctrl+F collapses the replace row, so nothing in the UI reveals
            // Ctrl+H — this modal is its only affordance.
            ("Ctrl+H", || tr!("shortcuts-find-replace")),
            ("Ctrl+Shift+\\", || tr!("shortcuts-matching-bracket")),
            ("Ctrl+/", || tr!("shortcuts-line-comment")),
            ("Ctrl+Shift+/", || tr!("shortcuts-block-comment")),
            ("Ctrl+Shift+U", || tr!("shortcuts-toggle-case")),
            ("Ctrl+Shift+J", || tr!("shortcuts-join-lines")),
            ("Ctrl+D", || tr!("shortcuts-duplicate")),
            ("Ctrl+X", || tr!("shortcuts-cut")),
            ("Alt+↑ / Alt+↓", || tr!("shortcuts-move-line")),
            ("Alt+Enter", || tr!("shortcuts-quick-fix")),
            ("Tab / Shift+Tab", || tr!("shortcuts-indent")),
            ("Tab / Ctrl+→", || tr!("shortcuts-take-prediction")),
        ],
    ),
    (
        Group::ResultsGrid,
        &[
            ("Ctrl+F", || tr!("shortcuts-find-in-results")),
            // Ctrl+G in the grid has no affordance anywhere else either.
            ("Ctrl+G", || tr!("shortcuts-go-to-row")),
            ("Ctrl+C", || tr!("shortcuts-copy")),
            ("Ctrl+V", || tr!("shortcuts-paste-staged")),
            ("Ctrl+A", || tr!("shortcuts-select-all")),
            ("Ctrl+Home / Ctrl+End", || tr!("shortcuts-first-last-cell")),
            // "Edit cell" only. Enter on a **read-only** cell does nothing —
            // viewing one is the right-click menu's View — and a row promising
            // otherwise teaches a key that isn't there.
            ("Enter", || tr!("shortcuts-edit-cell")),
            ("Tab / Shift+Tab", || tr!("shortcuts-next-cell")),
            ("Ctrl+Enter", || tr!("shortcuts-commit-edits")),
            ("Del", || tr!("shortcuts-mark-deletion")),
            // The toolbar strip is the app's one ring outside an overlay, and F6
            // is its only affordance — nothing on screen says the icons can be
            // reached at all.
            ("F6", || tr!("shortcuts-results-toolbar")),
            ("← / →", || tr!("shortcuts-along-toolbar")),
        ],
    ),
    (
        Group::SchemaTree,
        &[
            ("Enter", || tr!("shortcuts-open-selected")),
            // The same menu the right-click builds, opened at the row rather
            // than at the pointer.
            ("Shift+F10", || tr!("shortcuts-context-menu")),
        ],
    ),
    (
        Group::ServerActivity,
        &[
            // The row's right-click menu — *Cancel query*, *Kill session* —
            // at the cursor row; Enter too, since a row has no other action.
            ("Shift+F10", || tr!("shortcuts-context-menu")),
            ("Enter", || tr!("shortcuts-context-menu")),
        ],
    ),
    (
        Group::AiPanel,
        &[
            ("Enter", || tr!("shortcuts-send-message")),
            ("Shift+Enter", || tr!("shortcuts-new-line")),
            ("Ctrl+↑ / Ctrl+↓", || tr!("shortcuts-recall-prompt")),
        ],
    ),
    (
        Group::Terminal,
        &[("Ctrl+Shift+C / Ctrl+Shift+V", || {
            tr!("shortcuts-copy-paste")
        })],
    ),
    (
        Group::ErDiagram,
        // Ctrl+F is the find bar's only opener — there is no button for it — so
        // this modal is the one place it is written down.
        &[("Ctrl+F", || tr!("shortcuts-find-in-diagram"))],
    ),
];

/// The keys a **command palette** entry should display, by the command's `name`.
///
/// Only where the command does *the same thing* the key does, which is a
/// narrower set than it looks. `Run` runs all statements while Ctrl+Enter runs
/// the one under the caret; `Terminal` and `Ask AI` take an argument and act on
/// it, where Ctrl+` and Ctrl+Shift+A only toggle a panel; `Toggle Panel` names
/// its panel as an argument, so it has three bindings and therefore none.
/// Showing a nearly-right binding on a row is worse than showing none — it
/// teaches a key that does something else.
///
/// Each palette command is mapped to the keymap's command that does the same
/// thing, so its keycap is the key the user has bound to it now, and a command
/// they cleared shows none.
pub(crate) const COMMAND_KEYS: &[(&str, Command)] = &[
    ("new tab", Command::NewTab),
    ("close tab", Command::CloseTab),
    ("open file", Command::OpenFile),
    ("save file", Command::SaveFile),
    ("save file as", Command::SaveFileAs),
    ("next tab", Command::NextTab),
    ("previous tab", Command::PreviousTab),
    ("format code", Command::FormatCode),
    ("format statement", Command::FormatStatement),
    ("make single line", Command::MakeSingleLine),
    ("go to line", Command::GoToLine),
];

/// The keys to show on a palette row for the command called `name`, respelled
/// for the platform and language — `None` when it maps to no keymap command
/// or the user cleared that command's key.
pub(crate) fn command_keys(name: &str) -> Option<Cow<'static, str>> {
    COMMAND_KEYS
        .iter()
        .find(|(n, _)| *n == name)
        .and_then(|(_, c)| command_chord_label(*c))
        .map(Cow::Owned)
}

/// A context-menu entry that does **exactly** what a key does — the menus'
/// half of [`COMMAND_KEYS`], and held to the same rule: a nearly-right keycap
/// teaches a key that does something else, so an entry gets one only when the
/// key and the click are the same act on the same thing. The grid cell menu's
/// *Delete row* toggles that one row while Del marks the whole selection, so it
/// has none; the gutter menu's *Delete* acts on the selection as Del does, so
/// it has one.
///
/// An enum rather than a string key, so a menu cannot name an entry that is not
/// here; `tests::every_menu_key_is_the_shortcut_row_it_names` holds each fixed
/// one to its `SHORTCUTS` row. One that runs a keymap command shows that
/// command's key as it stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuKey {
    /// The grid's cell and gutter *Copy*: the selection, as Ctrl+C copies it.
    GridCopy,
    /// The grid cell menu's *Paste*: onto the selection, as Ctrl+V pastes.
    GridPaste,
    /// The grid gutter menu's *Delete* / *Undo delete* over the selected rows.
    GridDeleteRows,
    /// The editor menu's *Format code*.
    EditorFormat,
}

/// What a [`MenuKey`] shows: a fixed `SHORTCUTS` row, or a keymap command.
enum MenuRow {
    /// The group is what the menu-key test finds the row by.
    #[cfg_attr(not(test), allow(dead_code))]
    Fixed(Group, &'static str, Describe),
    Command(Command),
}

impl MenuKey {
    /// The row this entry shows.
    fn row(self) -> MenuRow {
        match self {
            MenuKey::GridCopy => {
                MenuRow::Fixed(Group::ResultsGrid, "Ctrl+C", || tr!("shortcuts-copy"))
            }
            MenuKey::GridPaste => MenuRow::Fixed(Group::ResultsGrid, "Ctrl+V", || {
                tr!("shortcuts-paste-staged")
            }),
            MenuKey::GridDeleteRows => {
                MenuRow::Fixed(Group::ResultsGrid, "Del", || tr!("shortcuts-mark-deletion"))
            }
            MenuKey::EditorFormat => MenuRow::Command(Command::FormatCode),
        }
    }

    /// The keys as this platform reads them, for the menu's keycap — `None`
    /// for a command the user left without a key.
    pub(crate) fn keys(self) -> Option<Cow<'static, str>> {
        match self.row() {
            MenuRow::Fixed(_, keys, _) => Some(keys_label(keys)),
            MenuRow::Command(c) => command_chord_label(c).map(Cow::Owned),
        }
    }

    /// What the Shortcuts modal calls the action — the menu entry's own label
    /// where the menu takes it from here, so the two say the same thing.
    pub(crate) fn label(self) -> String {
        match self.row() {
            MenuRow::Fixed(_, _, describe) => describe(),
            MenuRow::Command(c) => command_label(c),
        }
    }
}

// ── The primary modifier ─────────────────────────────────────────────────────
// macOS spells it Cmd and every other desktop spells it Ctrl. Both halves of
// that — what the modal *says* and what the handlers *accept* — hang off the one
// capability below, so they cannot drift into disagreeing: a modal that showed
// Cmd while the handlers only took Ctrl would be documentation of a key that
// does nothing, which is worse than the Ctrl label it replaced.

/// Whether this platform's primary shortcut modifier is Cmd rather than Ctrl.
///
/// The single `cfg!(target_os = …)` for this concern. Callers ask the
/// capability, per *Ask the capability, never `cfg!(target_os = …)` at the use
/// site* in `docs/architecture.md` — the functions below take it as an argument
/// precisely so both platforms stay testable from whichever one runs the suite.
pub(crate) const PRIMARY_IS_CMD: bool = cfg!(target_os = "macos");

/// A [`SHORTCUTS`] key string as this platform and language should read it:
/// `Ctrl+K` on Windows and Linux, `Cmd+K` on macOS, `Strg+K` in German.
///
/// The tables stay Ctrl-spelled in English and are rewritten here on the way to
/// the screen. A second, Cmd-spelled table would be the obvious alternative and
/// is the wrong one — two tables drift, and the tests that keep the modal and
/// the palette honest work by comparing them literally. The same holds for a
/// language: its keyboard's legends (`Strg`, `Umschalt`, `Entf`) come from the
/// catalogue key by key ([`key_name`]), never from a translated table.
pub(crate) fn keys_label(keys: &'static str) -> Cow<'static, str> {
    keys_label_on(keys, PRIMARY_IS_CMD, key_name)
}

/// A named key's legend in the interface's language, or `None` for a key
/// written the same everywhere (a letter, a digit, an arrow, `Tab`, `Esc`,
/// `F6`). `Cmd` is not here: it is macOS's name in every language.
fn key_name(key: &str) -> Option<String> {
    Some(match key {
        "Ctrl" => tr!("key-ctrl"),
        "Shift" => tr!("key-shift"),
        "Alt" => tr!("key-alt"),
        "Enter" => tr!("key-enter"),
        "Del" => tr!("key-del"),
        "Home" => tr!("key-home"),
        "End" => tr!("key-end"),
        "Space" => tr!("key-space"),
        _ => return None,
    })
}

/// [`keys_label`] with the platform and the namer passed in, so both are
/// testable from whichever machine runs the suite. A row is bindings joined by
/// ` / `, each a chord of keys joined by `+` — so the `/` in `Ctrl+/`, with no
/// spaces around it, is a key and not a separator.
fn keys_label_on<'a>(
    keys: &'a str,
    primary_is_cmd: bool,
    name: impl Fn(&str) -> Option<String>,
) -> Cow<'a, str> {
    let named = keys
        .split(" / ")
        .map(|chord| {
            chord
                .split('+')
                .map(|key| match key {
                    "Ctrl" if primary_is_cmd => Cow::Borrowed("Cmd"),
                    _ => name(key).map_or(Cow::Borrowed(key), Cow::Owned),
                })
                .collect::<Vec<_>>()
                .join("+")
        })
        .collect::<Vec<_>>()
        .join(" / ");
    if named == keys {
        Cow::Borrowed(keys)
    } else {
        Cow::Owned(named)
    }
}

/// Is the platform's primary shortcut modifier held?
///
/// Ctrl everywhere, and Cmd *as well* on macOS — additive on purpose, so the
/// Ctrl bindings a macOS user already learned keep working. Off macOS this
/// deliberately ignores Meta: there it is the Windows/Super key, and answering
/// to it would take over shortcuts that belong to the desktop.
pub(crate) fn primary_held(mods: Modifiers) -> bool {
    primary_from(mods.control(), mods.meta(), PRIMARY_IS_CMD)
}

fn primary_from(control: bool, meta: bool, primary_is_cmd: bool) -> bool {
    control || (primary_is_cmd && meta)
}

/// Is `key` itself the platform's primary modifier — the key [`primary_held`]
/// answers for, pressed or released on its own?
///
/// A lone Ctrl's own key-down does not reliably carry Ctrl in its modifiers
/// yet, so a view that reacts to the modifier going down or up (the editor's
/// Ctrl+hover underline) asks about the key, not the state.
pub(crate) fn is_primary_key(key: &floem::keyboard::Key) -> bool {
    primary_key_from(key, PRIMARY_IS_CMD)
}

fn primary_key_from(key: &floem::keyboard::Key, primary_is_cmd: bool) -> bool {
    use floem::keyboard::{Key, NamedKey};
    match key {
        Key::Named(NamedKey::Control) => true,
        Key::Named(NamedKey::Meta) => primary_is_cmd,
        _ => false,
    }
}

thread_local! {
    static HELD_KEYS: std::cell::RefCell<
        Option<(floem::reactive::RwSignal<HeldModifiers>, floem::reactive::Scope)>,
    > = const { std::cell::RefCell::new(None) };
}

/// Does a press with these modifiers open what the name under the pointer
/// names? The primary modifier alone: Ctrl+Shift and Ctrl+Alt are other
/// shortcuts' chords. **The one rule** the Ctrl+click, the pointer-move
/// underline and the key-driven underline all ask, so the line is never drawn
/// on a word a click with the same keys would not open.
pub(crate) fn opens_link(primary: bool, shift: bool, alt: bool) -> bool {
    primary && !shift && !alt
}

/// The modifier keys held, as their own key events last said.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct HeldModifiers {
    pub primary: bool,
    pub shift: bool,
    pub alt: bool,
}

impl HeldModifiers {
    /// [`opens_link`] over what is held.
    pub(crate) fn opens_link(self) -> bool {
        opens_link(self.primary, self.shift, self.alt)
    }

    /// After `key` went `down` (or up): the primary modifier, Shift or Alt
    /// recorded, any other key left alone.
    fn noted(self, key: &floem::keyboard::Key, down: bool) -> Self {
        use floem::keyboard::{Key, NamedKey};
        let mut next = self;
        if is_primary_key(key) {
            next.primary = down;
        }
        match key {
            Key::Named(NamedKey::Shift) => next.shift = down,
            Key::Named(NamedKey::Alt | NamedKey::AltGraph) => next.alt = down,
            _ => {}
        }
        next
    }

    /// Corrected by a pointer event's modifiers — the repair for a key-up that
    /// went to another window. The primary modifier is only ever cleared here
    /// (a lone Ctrl's state is the key events'); Shift and Alt are taken as the
    /// pointer reports them.
    fn seen_by_pointer(self, mods: Modifiers) -> Self {
        HeldModifiers {
            primary: self.primary && primary_held(mods),
            shift: mods.shift(),
            alt: mods.alt(),
        }
    }
}

/// The modifier keys held, as their own key events last said — window-wide, in
/// a detached scope like `widgets::window_size`.
///
/// **Its own signal because floem delivers a key only to the focused view and
/// then to the root view's listeners**, never to the ancestors between: a
/// lone Ctrl pressed over the SQL editor reaches the editor's key handler (which
/// consumes it), and its release, which nothing focused answers, reaches only
/// the root. The editor's Ctrl+hover underline has to hear both, so the three
/// places a key arrives — the editor's key handler, the root's `KeyDown` and its
/// `KeyUp` — note it here ([`note_modifier_key`]) and each pane reads it. A
/// release while the window is not focused never arrives; a pointer move
/// corrects it ([`note_pointer_modifiers`]).
///
/// **Shift and Alt are held here too**, not only the primary modifier: the
/// key-driven underline drew on Ctrl's own press, and Shift-then-Ctrl — how
/// Ctrl+Shift+F starts — underlined a word a Ctrl+Shift click does not open.
pub(crate) fn modifier_keys() -> floem::reactive::RwSignal<HeldModifiers> {
    HELD_KEYS.with(|cell| {
        if cell.borrow().is_none() {
            let scope = floem::reactive::Scope::new();
            let sig = scope.create_rw_signal(HeldModifiers::default());
            *cell.borrow_mut() = Some((sig, scope));
        }
        cell.borrow().as_ref().unwrap().0
    })
}

/// Record a key event in [`modifier_keys`] if `key` is the primary modifier,
/// Shift or Alt. A held key repeats its key-down, so only a change is written.
pub(crate) fn note_modifier_key(key: &floem::keyboard::Key, down: bool) {
    use floem::reactive::{SignalGet, SignalUpdate};
    let sig = modifier_keys();
    let now = sig.get_untracked();
    let next = now.noted(key, down);
    if next != now {
        sig.set(next);
    }
}

/// Correct [`modifier_keys`] from a pointer event's modifiers
/// ([`HeldModifiers::seen_by_pointer`]).
pub(crate) fn note_pointer_modifiers(mods: Modifiers) {
    use floem::reactive::{SignalGet, SignalUpdate};
    let sig = modifier_keys();
    let now = sig.get_untracked();
    let next = now.seen_by_pointer(mods);
    if next != now {
        sig.set(next);
    }
}

#[cfg(test)]
mod primary_key_tests {
    use super::{HeldModifiers, opens_link, primary_key_from};
    use floem::keyboard::{Key, Modifiers, NamedKey};

    /// The keys noted in order, from nothing held.
    fn after(keys: &[(NamedKey, bool)]) -> HeldModifiers {
        keys.iter().fold(HeldModifiers::default(), |h, (k, down)| {
            h.noted(&Key::Named(*k), *down)
        })
    }

    /// **Shift (or Alt) then Ctrl underlines nothing**, as a click with those
    /// keys opens nothing — the key-driven underline asked the primary key
    /// alone, so starting Ctrl+Shift+F over a name drew the link line on it.
    #[test]
    fn shift_or_alt_then_ctrl_opens_no_link() {
        assert!(after(&[(NamedKey::Control, true)]).opens_link());
        assert!(!after(&[(NamedKey::Shift, true), (NamedKey::Control, true)]).opens_link());
        assert!(!after(&[(NamedKey::Alt, true), (NamedKey::Control, true)]).opens_link());
        // Shift added after Ctrl takes the line away, and letting it go
        // brings it back.
        assert!(!after(&[(NamedKey::Control, true), (NamedKey::Shift, true)]).opens_link());
        assert!(
            after(&[
                (NamedKey::Control, true),
                (NamedKey::Shift, true),
                (NamedKey::Shift, false),
            ])
            .opens_link()
        );
        assert!(!after(&[(NamedKey::Control, true), (NamedKey::Control, false)]).opens_link());
        // The rule itself, as the click asks it.
        assert!(opens_link(true, false, false));
        assert!(!opens_link(true, true, false));
        assert!(!opens_link(true, false, true));
        assert!(!opens_link(false, false, false));
    }

    /// A pointer move repairs a key-up the window never heard: Shift and Alt
    /// as it reports them, the primary key only ever cleared.
    #[test]
    fn a_pointer_move_corrects_a_missed_release() {
        let stuck = after(&[(NamedKey::Control, true), (NamedKey::Shift, true)]);
        let seen = stuck.seen_by_pointer(Modifiers::CONTROL);
        assert_eq!(
            seen,
            HeldModifiers {
                primary: true,
                shift: false,
                alt: false
            }
        );
        assert!(!seen.seen_by_pointer(Modifiers::empty()).primary);
        assert!(
            !HeldModifiers::default()
                .seen_by_pointer(Modifiers::CONTROL)
                .primary,
            "a pointer never sets the primary key"
        );
    }

    #[test]
    fn control_is_the_primary_key_everywhere() {
        assert!(primary_key_from(&Key::Named(NamedKey::Control), false));
        assert!(primary_key_from(&Key::Named(NamedKey::Control), true));
    }

    #[test]
    fn meta_is_the_primary_key_only_where_cmd_is() {
        assert!(primary_key_from(&Key::Named(NamedKey::Meta), true));
        assert!(!primary_key_from(&Key::Named(NamedKey::Meta), false));
    }

    #[test]
    fn no_other_key_is_the_primary_key() {
        for k in [
            Key::Named(NamedKey::Shift),
            Key::Named(NamedKey::Alt),
            Key::Character("c".into()),
        ] {
            assert!(!primary_key_from(&k, true), "{k:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        COMMAND_KEYS, Chord, Command, Context, Group, Keymap, MenuKey, MenuRow, SHORTCUTS,
        chord_from, command_keys, command_label, fixed_keys, keymap, keys_label, keys_label_on,
        primary_from, row_chords,
    };
    use floem::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    /// The files that bind keys, each with the [`SHORTCUTS`] groups its bindings
    /// may be documented in.
    ///
    /// Not every module with a `KeyDown` listener — only the ones that bind a
    /// *modified letter*, which is what this gate is about. A new file of that
    /// kind belongs on this list.
    ///
    /// **The groups are what makes a second binding of an already-listed letter
    /// visible.** `documented` was letter-global, so adding a Ctrl+D to the grid
    /// needed no row: Editor / "Ctrl+D — Duplicate line" vouched for it. Both
    /// letters that really do have two bindings (`Ctrl+F`, `Ctrl+G`) got their
    /// second row by hand, with nothing asking for it. Scoping the lookup to the
    /// surface that binds it is what turns that into a failure.
    ///
    /// A file lists every group it may document into, not one — `lib.rs` is the
    /// window root *and* the terminal panel. (The global *commands* are the
    /// keymap's now, and no file binds their letters; `Group::Global` stays on
    /// the surfaces that may still hand-bind a window-wide key.) When a binding
    /// legitimately belongs to a group not listed here, add the group; the
    /// failure message says so.
    const KEY_FILES: &[(&str, &[Group])] = &[
        ("editor_pane.rs", &[Group::Editor, Group::Global]),
        ("grid.rs", &[Group::ResultsGrid, Group::Global]),
        ("lib.rs", &[Group::Global, Group::Terminal]),
        ("ai_panel.rs", &[Group::Global]),
        ("schema_tree.rs", &[Group::SchemaTree, Group::Global]),
        ("overlays.rs", &[Group::Global]),
        // **Short until it wasn't.** The ER diagram binds Ctrl+F and that
        // binding is the find bar's *only* affordance — no toolbar button, no
        // menu entry — and because this array is hand-maintained and the file
        // was not on it, `bound_letters` never read the file, the `f` was never
        // seen, and the module whose doc opens "a binding missing here is a
        // feature nobody can find" passed green over exactly that.
        ("erd_view.rs", &[Group::ErDiagram]),
    ];

    /// Letters bound to something that is not a user-facing shortcut, each with
    /// the file it is exempt *in* and the reason it is not in the modal. A letter
    /// leaves this list the moment it becomes something a user would look up.
    ///
    /// Kept as a baseline in the spirit of `contrast::UI_SHORTFALL`: an unlisted
    /// letter must be documented, and a listed one carries its justification.
    ///
    /// **Scoped to one file, for the reason `KEY_FILES` is.** A crate-wide
    /// exemption goes blind everywhere at once — exempting Ctrl+Z for the
    /// masked field's sake would have vouched for a real editor undo binding
    /// added later, in a different file, doing a different thing.
    const EXEMPT: &[(&str, &str, &str)] = &[
        (
            "lib.rs",
            "z",
            "undo inside a masked secret field, *suppressed* rather than bound: the \
             document's history is a history of mask characters, so replaying one \
             writes asterisks into the password. Nothing here for a user to look up.",
        ),
        (
            "lib.rs",
            "y",
            "redo — the other half of the same suppression.",
        ),
    ];

    fn src_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
    }

    /// The file with its `#[cfg(test)]` module cut off.
    ///
    /// Not a nicety: the first version of this scan read the test modules too and
    /// reported `Ctrl+B` as an undocumented binding, which was
    /// `Some("b".to_string())` in a grid fixture. Test data is full of one-letter
    /// strings, so scanning it makes the gate cry wolf — and a gate that cries
    /// wolf gets deleted.
    ///
    /// [`crate::source_gate`] owns the cut. This used to assume "every file here
    /// keeps its tests at the bottom", which `widgets.rs` does not: its inline
    /// test-only `fn` at line 929 ended the scan 6,330 lines early.
    use crate::source_gate::production_code;

    /// Every single letter bound with Ctrl or Alt in `src`, by the five idioms
    /// the codebase has used:
    ///
    /// - `"x" | "X"` — the case pair, covering both `matches!(s.as_str(), …)` and
    ///   a plain `match` arm (the grid, the root, the terminal's copy/paste)
    /// - `eq_ignore_ascii_case("x")` — the editor's style
    /// - `Some("x") =>` — a match on the lowercased character, as `NavKeys`'
    ///   arms were, requiring the `=>` so this doesn't match every `Some("…")`
    ///   in the crate
    /// - `ch == Some("x")` — the same, spelled as an equality rather than a
    ///   pattern; it is where `NavKeys`' Ctrl+Shift+letter arms lived, and it
    ///   was once missing from this list
    /// - `KeyCode::KeyX` — a *physical* key, which is how a Ctrl+Alt letter was
    ///   matched before the keymap read one by its place (Windows delivers
    ///   Ctrl+Alt as AltGr, so the logical character need not be the letter)
    ///
    /// The last three bind nothing in production code since those keys became
    /// keymap commands; they stay scanned because they are the spellings a new
    /// hand-written binding would most likely use.
    ///
    /// Anything spelled a sixth way slips through, which is the weakness this
    /// test accepts: it exists to catch the binding nobody wrote down, not to
    /// prove the absence of one.
    fn bound_letters(src: &str) -> BTreeSet<char> {
        let src = production_code(src);
        let src = src.as_str();
        let mut out = BTreeSet::new();
        let bytes = src.as_bytes();

        // `"x" | "X"` — a lower/upper case pair, whichever construct wraps it.
        for (i, w) in bytes.windows(9).enumerate() {
            let _ = i;
            if w[0] == b'"'
                && w[2] == b'"'
                && w[3] == b' '
                && w[4] == b'|'
                && w[5] == b' '
                && w[6] == b'"'
                && w[8] == b'"'
                && w[1].is_ascii_lowercase()
                && w[7] == w[1].to_ascii_uppercase()
            {
                out.insert(w[1] as char);
            }
        }
        // `eq_ignore_ascii_case("x")`
        let mut rest = src;
        while let Some(i) = rest.find("eq_ignore_ascii_case(\"") {
            rest = &rest[i + "eq_ignore_ascii_case(\"".len()..];
            let b = rest.as_bytes();
            if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b'"' {
                out.insert(b[0].to_ascii_lowercase() as char);
            }
        }
        // `Some("x") =>` — the `=>` is what keeps this from matching every option.
        let mut rest = src;
        while let Some(i) = rest.find("Some(\"") {
            rest = &rest[i + "Some(\"".len()..];
            let b = rest.as_bytes();
            if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b'"' {
                let after = rest[2..].trim_start();
                if after.starts_with(") =>") || after.starts_with(")=>") {
                    out.insert(b[0].to_ascii_lowercase() as char);
                }
            }
        }
        // `== Some("x")` — the equality form, as the shifted arms of
        // `NavKeys::handle` were written. The `==` is what distinguishes it from
        // an ordinary `Some("…")`, exactly as the `=>` does above.
        let mut rest = src;
        while let Some(i) = rest.find("== Some(\"") {
            rest = &rest[i + "== Some(\"".len()..];
            let b = rest.as_bytes();
            if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b'"' {
                out.insert(b[0].to_ascii_lowercase() as char);
            }
        }
        // `KeyCode::KeyX` — a physical key.
        let mut rest = src;
        while let Some(i) = rest.find("KeyCode::Key") {
            rest = &rest[i + "KeyCode::Key".len()..];
            if let Some(c) = rest.chars().next()
                && c.is_ascii_alphabetic()
                // `KeyCode::KeyL` but not `KeyCode::Keyboard…`
                && !rest[1..].starts_with(|n: char| n.is_ascii_alphabetic())
            {
                out.insert(c.to_ascii_lowercase());
            }
        }
        out
    }

    /// Does the table name this letter as a modified key, in one of `groups`?
    ///
    /// Scoped rather than table-wide: a Ctrl+D added to the grid must not be
    /// vouched for by the *editor's* Ctrl+D, which does something else entirely.
    /// `groups` empty means "anywhere", which is what the whole-table questions
    /// below ask.
    fn documented_in(letter: char, groups: &[Group]) -> bool {
        let upper = letter.to_ascii_uppercase();
        SHORTCUTS
            .iter()
            .filter(|(group, _)| groups.is_empty() || groups.contains(group))
            .any(|(_, rows)| {
                rows.iter().any(|(keys, _)| {
                    keys.split(['+', ' ', '/'])
                        .any(|part| part.len() == 1 && part.starts_with(upper))
                })
            })
    }

    /// Does the table name this letter anywhere, as a modified key?
    fn documented(letter: char) -> bool {
        documented_in(letter, &[])
    }

    /// A binding with no row is a feature nobody can find. This is the whole
    /// point of the module.
    #[test]
    fn every_bound_letter_is_documented() {
        let mut found = BTreeSet::new();
        let mut missing: Vec<String> = Vec::new();
        for (name, groups) in KEY_FILES {
            let path = src_dir().join(name);
            let src = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let letters = bound_letters(&src);
            // **In one of the file's own groups.** Table-wide, a Ctrl+D added to
            // the grid was vouched for by the editor's Ctrl+D — a different key
            // doing a different thing.
            missing.extend(
                letters
                    .iter()
                    .filter(|c| {
                        !EXEMPT
                            .iter()
                            .any(|(f, e, _)| f == name && e.starts_with(**c))
                    })
                    .filter(|c| !documented_in(**c, groups))
                    .map(|c| format!("{name}: Ctrl/Alt+{}", c.to_ascii_uppercase())),
            );
            found.extend(letters);
        }
        assert!(
            !found.is_empty(),
            "found no bound letters at all — the scan idioms have changed and \
             this gate is now vacuous, which is worse than it failing"
        );
        assert!(
            missing.is_empty(),
            "these keys are bound in the handlers but appear in no SHORTCUTS row \
             of the binding file's own groups, so nothing in the app reveals \
             them: {}. Add the row in the same change as the binding; if the \
             binding belongs to a group `KEY_FILES` doesn't list for that file, \
             add the group there; or add the letter to EXEMPT with its reason.",
            missing.join(", ")
        );
    }

    /// Each of the four scanned idioms, on synthetic input.
    ///
    /// Without these, the whole gate can rot silently: `bound_letters` returning
    /// less than it should still passes [`every_bound_letter_is_documented`],
    /// because finding nothing means finding nothing missing. These pin what the
    /// scan can see, so a rewrite of the key handling breaks *here*, where the
    /// message says the scan needs widening.
    #[test]
    fn the_scan_sees_every_idiom_the_codebase_binds_keys_with() {
        let case_pair = r#"Key::Character(s) if ctrl && matches!(s.as_str(), "f" | "F") => {}"#;
        assert!(bound_letters(case_pair).contains(&'f'));
        let match_arm = r#"match s.as_str() { "c" | "C" => copy(), "v" | "V" => paste() }"#;
        let arms = bound_letters(match_arm);
        assert!(arms.contains(&'c') && arms.contains(&'v'));
        assert!(bound_letters(r#"if c.eq_ignore_ascii_case("g") {"#).contains(&'g'));
        assert!(bound_letters(r#"Some("p") => { find(); }"#).contains(&'p'));
        assert!(
            bound_letters(r#"if ch == Some("b") { bold(); }"#).contains(&'b'),
            "the equality form, where the Ctrl+Shift+letter arms lived"
        );
        assert!(
            bound_letters("PhysicalKey::Code(KeyCode::KeyL)").contains(&'l'),
            "Ctrl+Alt+L is matched physically, not as a character"
        );
    }

    /// The noise the scan must *not* pick up — each of these produced a false
    /// "undocumented binding" or would have.
    #[test]
    fn the_scan_ignores_what_is_not_a_binding() {
        // The one that actually fired: a grid test fixture, reported as Ctrl+B.
        let with_tests = "fn real() {}\n#[cfg(test)]\nmod t { let x = Some(\"b\".to_string()); }";
        assert!(
            !bound_letters(with_tests).contains(&'b'),
            "test modules must be cut off before scanning"
        );
        // An ordinary `Option<&str>` with no match arm after it.
        assert!(bound_letters(r#"let name = Some("z");"#).is_empty());
        // A multi-letter key code is not a single-letter binding.
        assert!(bound_letters("KeyCode::KeyboardLayout").is_empty());
        // Two unrelated string literals that merely sit next to each other.
        assert!(bound_letters(r#"&["a", "b"]"#).is_empty());
    }

    /// The converse: a row naming a letter nothing binds is a shortcut that has
    /// been removed or renamed, and the modal is now lying about it — **or** the
    /// scan has stopped recognising the idiom that binds it, which is why this
    /// doubles as the coverage check on `bound_letters` against the real sources.
    #[test]
    fn every_documented_letter_is_still_bound() {
        let mut found = BTreeSet::new();
        for (name, _) in KEY_FILES {
            let src = std::fs::read_to_string(src_dir().join(name)).expect("read source");
            found.extend(bound_letters(&src));
        }
        let mut stale = Vec::new();
        for (group, rows) in SHORTCUTS {
            for (keys, desc) in *rows {
                for part in keys.split(['+', ' ', '/']) {
                    let mut it = part.chars();
                    if let (Some(c), None) = (it.next(), it.next())
                        && c.is_ascii_alphabetic()
                        && !found.contains(&c.to_ascii_lowercase())
                    {
                        stale.push(format!("{group:?} / {keys} ({})", desc()));
                    }
                }
            }
        }
        assert!(
            stale.is_empty(),
            "these rows name a letter no handler binds: {}. Either the shortcut \
             was removed and the row is now a lie, or it is bound by an idiom \
             `bound_letters` doesn't scan — check which before deleting the row, \
             because widening the scan is the right fix for the second case.",
            stale.join(", ")
        );
    }

    /// **A row in one group must not vouch for a binding in another.** Adding a
    /// Ctrl+D to the grid was covered by Editor / "Ctrl+D — Duplicate line",
    /// which is a different key doing a different thing — and the two letters
    /// that really do have two bindings each got their second row by hand, with
    /// nothing asking for it.
    #[test]
    fn a_row_only_vouches_for_its_own_group() {
        // Ctrl+D exists, in the Editor group only.
        assert!(documented('d'), "table-wide");
        assert!(documented_in('d', &[Group::Editor]));
        assert!(
            !documented_in('d', &[Group::ResultsGrid]),
            "the grid has no Ctrl+D row, so a grid binding must fail the gate"
        );
        // And the letters that legitimately appear in two groups still do.
        assert!(documented_in('f', &[Group::Editor]));
        assert!(documented_in('f', &[Group::ResultsGrid]));
    }

    /// Every group a `KEY_FILES` row names has to exist, or a typo silently
    /// widens the gate to "documented nowhere" and every letter fails — or, if
    /// the file's other groups cover it, silently narrows to nothing.
    #[test]
    fn every_key_file_names_real_shortcut_groups() {
        for (file, groups) in KEY_FILES {
            for g in *groups {
                assert!(
                    SHORTCUTS.iter().any(|(group, _)| group == g),
                    "{file} names group {g:?}, which has no rows in SHORTCUTS"
                );
            }
        }
    }

    /// The other half of the gate: a letter is "documented" only when it appears
    /// as a key *on its own*, so a word that happens to start with it can't
    /// vouch for it.
    #[test]
    fn documented_reads_a_letter_only_as_a_whole_key() {
        assert!(documented('g'), "Ctrl+G");
        assert!(documented('u'), "Ctrl+Shift+U");
        assert!(documented('j'), "Ctrl+Shift+J");
        // The failure this spelling avoids: `Ctrl+Enter` must not make every
        // Ctrl+E binding look documented, nor `Del` vouch for Ctrl+D.
        assert!(
            !documented('q'),
            "nothing binds or documents Q — if this ever passes, the match is too loose"
        );
        assert!(
            !documented('n'),
            "no Ctrl+N anywhere, and `Enter` must not count"
        );
        assert!(
            !documented('r'),
            "no Ctrl+R, and `Enter`/`Recall` must not count"
        );
    }

    /// **A palette command shows the key of the keymap command that does the
    /// same thing.** Each palette name is its command's id with spaces, so a
    /// mapping to the wrong command reads as a mismatch here.
    #[test]
    fn every_palette_keycap_names_the_command_of_its_name() {
        for (name, command) in COMMAND_KEYS {
            assert_eq!(
                name.replace(' ', "-"),
                command.id(),
                "palette command {name:?} shows the key of {command:?}"
            );
        }
    }

    /// **A context menu may only show a key the modal documents, for the row
    /// that means it.** Each fixed [`MenuKey`] names its `SHORTCUTS` row by
    /// group, key string *and* description, so a row reworded or moved fails
    /// here rather than leaving a menu showing a key for something else.
    #[test]
    fn every_menu_key_is_the_shortcut_row_it_names() {
        for key in menu_keys_all() {
            let MenuRow::Fixed(group, keys, what) = key.row() else {
                continue;
            };
            let found = SHORTCUTS
                .iter()
                .filter(|(g, _)| *g == group)
                .any(|(_, rows)| rows.iter().any(|(k, d)| *k == keys && d() == what()));
            assert!(
                found,
                "{key:?} shows {keys:?} as {:?} in {group:?}, which is not a \
                 SHORTCUTS row — fix the entry to name a real one",
                what()
            );
            assert_eq!(key.keys(), Some(keys_label(keys)));
        }
    }

    /// **A menu entry that runs a command shows that command's key as it
    /// stands** — Format code's menu keycap follows the keymap, and is gone
    /// when the user clears the key.
    #[test]
    fn a_command_menu_key_follows_the_keymap() {
        use floem::reactive::SignalUpdate;
        let map = keymap();
        assert_eq!(
            MenuKey::EditorFormat.keys().as_deref(),
            Some(&*keys_label("Ctrl+Alt+L"))
        );
        map.update(|m| m.clear(Command::FormatCode));
        assert_eq!(MenuKey::EditorFormat.keys(), None);
        map.set(Keymap::defaults());
    }

    /// The format action is called the same on the shortcut sheet, the editor
    /// menu (which takes [`MenuKey::label`]) and the palette (whose command
    /// name is matched here), in each surface's own case. It was "Format SQL",
    /// "Format" and "Format Code" — three names for one key.
    #[test]
    fn the_format_action_has_one_name_on_every_surface() {
        let label = MenuKey::EditorFormat.label();
        let palette = COMMAND_KEYS
            .iter()
            .find(|(_, c)| *c == Command::FormatCode)
            .map(|(name, _)| *name)
            .expect("the palette names the format command");
        assert!(
            label.eq_ignore_ascii_case(palette),
            "{label:?} / {palette:?}"
        );
        assert_eq!(label, command_label(Command::FormatCode));
    }

    /// Every [`MenuKey`], spelled so that a new variant fails to compile here
    /// until it is listed — and so is checked above.
    fn menu_keys_all() -> [MenuKey; 4] {
        fn _exhaustive(k: MenuKey) {
            match k {
                MenuKey::GridCopy
                | MenuKey::GridPaste
                | MenuKey::GridDeleteRows
                | MenuKey::EditorFormat => {}
            }
        }
        [
            MenuKey::GridCopy,
            MenuKey::GridPaste,
            MenuKey::GridDeleteRows,
            MenuKey::EditorFormat,
        ]
    }

    #[test]
    fn command_keys_looks_up_by_name_and_misses_cleanly() {
        // Compared against `keys_label` rather than a literal: what comes back
        // is respelled for the platform, so a literal here would assert Ctrl on
        // a macOS run.
        assert_eq!(command_keys("go to line"), Some(keys_label("Ctrl+G")));
        assert_eq!(
            command_keys("previous tab"),
            Some(keys_label("Ctrl+Shift+Tab"))
        );
        // A command with no binding, and one that doesn't exist, both yield None
        // — the row simply shows no keys.
        assert_eq!(command_keys("duplicate tab"), None);
        assert_eq!(command_keys("nonexistent"), None);
        // Names are the palette's lowercased labels; a label-cased lookup is a
        // miss rather than a silent match on the wrong thing.
        assert_eq!(command_keys("Go to Line"), None);
    }

    /// **The palette's keycap is the key the user bound**, not the default:
    /// moved, it follows; cleared, it is gone.
    #[test]
    fn a_palette_keycap_follows_the_keymap() {
        use floem::reactive::SignalUpdate;
        let map = keymap();
        map.update(|m| {
            m.assign(Command::NewTab, Chord::parse("Ctrl+N").unwrap(), &[]);
        });
        assert_eq!(command_keys("new tab"), Some(keys_label("Ctrl+N")));
        map.update(|m| m.clear(Command::NewTab));
        assert_eq!(command_keys("new tab"), None);
        map.set(Keymap::defaults());
    }

    /// A command name is what the parser matches and what Tab completes, so a
    /// duplicate here would make one of the two entries dead.
    #[test]
    fn command_keys_names_are_unique() {
        let mut seen = BTreeSet::new();
        for (name, _) in COMMAND_KEYS {
            assert!(seen.insert(*name), "duplicate COMMAND_KEYS entry {name:?}");
        }
    }

    // ── The keymap against the fixed keys ────────────────────────────────

    /// **No command's default is a fixed key heard where the command is.** A
    /// default on one would leave either the command or the fixed key dead
    /// from the first launch — and `Keymap::load` would report it to every
    /// user with no `keymap.json` at all.
    #[test]
    fn no_default_is_a_fixed_key() {
        let (_, problems) = Keymap::load(&Default::default(), &fixed_keys());
        assert_eq!(problems, vec![]);
    }

    /// Every chord a fixed row names reads as a chord, so none of them can be
    /// silently missing from the keys a command may not take.
    #[test]
    fn every_fixed_row_reads_as_chords() {
        for (group, rows) in SHORTCUTS {
            for (keys, desc) in *rows {
                let want = keys.split(" / ").count() + if keys.contains("1…9") { 8 } else { 0 };
                assert_eq!(
                    row_chords(keys).len(),
                    want,
                    "{group:?} / {keys} ({}) has a binding that does not parse",
                    desc()
                );
            }
        }
    }

    #[test]
    fn ctrl_1_to_9_is_nine_fixed_keys() {
        let chords = row_chords("Ctrl+1…9");
        assert_eq!(chords.len(), 9);
        assert_eq!(chords[0], Chord::parse("Ctrl+1").unwrap());
        assert_eq!(chords[8], Chord::parse("Ctrl+9").unwrap());
    }

    /// A global command cannot be moved onto a key the editor or the grid
    /// answers itself — the editor's find is heard before nothing else.
    #[test]
    fn a_global_command_cannot_take_a_surfaces_fixed_key() {
        use schemaic_core::keymap::Verdict;
        let fixed = fixed_keys();
        let map = Keymap::defaults();
        let ctrl_f = Chord::parse("Ctrl+F").unwrap();
        assert!(matches!(
            map.check(Command::NewTab, ctrl_f, &fixed),
            Verdict::Fixed(_)
        ));
        // F6 is the grid's toolbar key, not the editor's: Go to line may have it.
        let f6 = Chord::parse("F6").unwrap();
        assert_eq!(map.check(Command::GoToLine, f6, &fixed), Verdict::Free);
    }

    // ── Reading a key press ──────────────────────────────────────────────

    fn press(logical: Key, code: KeyCode, primary: bool, alt: bool, shift: bool) -> Option<Chord> {
        chord_from(&logical, &PhysicalKey::Code(code), primary, alt, shift)
    }

    fn ch(s: &str) -> Key {
        Key::Character(s.into())
    }

    #[test]
    fn a_letter_is_read_by_its_character_whatever_shift_did_to_it() {
        assert_eq!(
            press(ch("P"), KeyCode::KeyP, true, false, true),
            Chord::parse("Ctrl+Shift+P").ok()
        );
        // AZERTY: the A is where the Q is, and Ctrl+A is still Ctrl+A.
        assert_eq!(
            press(ch("a"), KeyCode::KeyQ, true, false, false),
            Chord::parse("Ctrl+A").ok()
        );
    }

    /// **Ctrl+Alt+L on Windows**, which delivers Ctrl+Alt as AltGr: the
    /// character is whatever AltGr+L types (`ł` on a Polish layout), and the
    /// chord is still Ctrl+Alt+L — the editor's format key.
    #[test]
    fn a_letter_altgr_changed_is_read_by_its_place() {
        let chord = press(ch("ł"), KeyCode::KeyL, true, true, false);
        assert_eq!(chord, Chord::parse("Ctrl+Alt+L").ok());
        assert_eq!(
            Keymap::defaults().resolve(chord.unwrap(), Context::Editor),
            Some(Command::FormatCode)
        );
    }

    #[test]
    fn a_shifted_symbol_is_read_by_its_place() {
        assert_eq!(
            press(ch("?"), KeyCode::Slash, true, false, true),
            Chord::parse("Ctrl+Shift+/").ok()
        );
        assert_eq!(
            press(ch("!"), KeyCode::Digit1, true, false, true),
            Chord::parse("Ctrl+Shift+1").ok()
        );
        assert_eq!(
            press(ch("`"), KeyCode::Backquote, true, false, false),
            Chord::parse("Ctrl+`").ok()
        );
    }

    /// A Cyrillic layout types `п` where the G is; Ctrl on it is Ctrl+G.
    #[test]
    fn a_non_latin_letter_is_read_by_its_place() {
        assert_eq!(
            press(ch("п"), KeyCode::KeyG, true, false, false),
            Chord::parse("Ctrl+G").ok()
        );
    }

    #[test]
    fn named_and_function_keys_are_read_by_name() {
        assert_eq!(
            press(Key::Named(NamedKey::Tab), KeyCode::Tab, true, false, true),
            Chord::parse("Ctrl+Shift+Tab").ok()
        );
        assert_eq!(
            press(Key::Named(NamedKey::F6), KeyCode::F6, false, false, false),
            Chord::parse("F6").ok()
        );
        assert_eq!(
            press(ch(" "), KeyCode::Space, true, false, false),
            Chord::parse("Ctrl+Space").ok()
        );
    }

    #[test]
    fn a_lone_modifier_is_no_chord() {
        for n in [
            NamedKey::Control,
            NamedKey::Shift,
            NamedKey::Alt,
            NamedKey::Meta,
        ] {
            assert_eq!(
                press(Key::Named(n), KeyCode::ControlLeft, true, false, false),
                None,
                "{n:?}"
            );
        }
    }

    #[test]
    fn the_keys_a_command_had_by_default_resolve_as_they_did() {
        let map = Keymap::defaults();
        let cases = [
            (
                press(ch("p"), KeyCode::KeyP, true, false, false),
                Command::FindAnywhere,
            ),
            (
                press(ch("T"), KeyCode::KeyT, true, false, true),
                Command::ReopenTab,
            ),
            (
                press(ch("E"), KeyCode::KeyE, true, false, true),
                Command::ToggleSchema,
            ),
            (
                press(ch("A"), KeyCode::KeyA, true, false, true),
                Command::ToggleAi,
            ),
            (
                press(ch("`"), KeyCode::Backquote, true, false, false),
                Command::ToggleTerminal,
            ),
        ];
        for (chord, command) in cases {
            assert_eq!(map.resolve(chord.unwrap(), Context::Global), Some(command));
        }
    }

    #[test]
    fn a_chord_is_named_in_the_interface_language() {
        assert_eq!(
            keys_label_on(
                &Chord::parse("Ctrl+Shift+P").unwrap().to_string(),
                false,
                german
            ),
            "Strg+Umschalt+P"
        );
    }

    /// macOS spells the primary modifier Cmd, so the table's rows are rewritten
    /// on the way to the screen rather than duplicated per platform — one table
    /// stays the single source, and a keymap file written on one platform reads
    /// the same on the other.
    #[test]
    fn a_key_string_is_respelled_for_cmd_only_where_cmd_is_the_modifier() {
        assert_eq!(keys_label_on("Ctrl+K", true, english), "Cmd+K");
        assert_eq!(keys_label_on("Ctrl+K", false, english), "Ctrl+K");
    }

    #[test]
    fn every_ctrl_in_a_row_is_respelled_not_just_the_first() {
        // `Ctrl+Home / Ctrl+End` is one row holding two bindings; respelling the
        // head of it and leaving the tail would document a key that isn't there.
        assert_eq!(
            keys_label_on("Ctrl+Home / Ctrl+End", true, english),
            "Cmd+Home / Cmd+End"
        );
        assert_eq!(keys_label_on("Ctrl+Shift+A", true, english), "Cmd+Shift+A");
        assert_eq!(keys_label_on("Ctrl+Alt+L", true, english), "Cmd+Alt+L");
    }

    #[test]
    fn a_row_with_no_ctrl_is_left_exactly_as_it_was() {
        for row in ["Alt+↑ / Alt+↓", "Enter", "F5", "Shift+Tab"] {
            assert_eq!(keys_label_on(row, true, english), row);
            assert_eq!(keys_label_on(row, false, english), row);
        }
    }

    /// A namer that knows no key — what English amounts to.
    fn english(_: &str) -> Option<String> {
        None
    }

    /// The German keyboard's legends, as the `de` catalogue writes them.
    fn german(key: &str) -> Option<String> {
        let name = match key {
            "Ctrl" => "Strg",
            "Shift" => "Umschalt",
            "Enter" => "Eingabe",
            "Del" => "Entf",
            "Home" => "Pos1",
            "End" => "Ende",
            "Space" => "Leertaste",
            _ => return None,
        };
        Some(name.to_string())
    }

    /// **A key is named in the interface's language**, key by key: German
    /// reads Strg+Umschalt where English reads Ctrl+Shift, and a key with no
    /// name of its own there (a letter, `Tab`, `/`, an arrow) is left alone —
    /// including the `/` that is a key in `Ctrl+/` rather than the separator
    /// between two bindings.
    #[test]
    fn every_key_in_a_row_is_named_in_the_interface_language() {
        assert_eq!(
            keys_label_on("Ctrl+Shift+Tab", false, german),
            "Strg+Umschalt+Tab"
        );
        assert_eq!(
            keys_label_on("Ctrl+Home / Ctrl+End", false, german),
            "Strg+Pos1 / Strg+Ende"
        );
        assert_eq!(keys_label_on("Ctrl+/", false, german), "Strg+/");
        assert_eq!(
            keys_label_on("Ctrl+Shift+/", false, german),
            "Strg+Umschalt+/"
        );
        assert_eq!(keys_label_on("Del", false, german), "Entf");
        assert_eq!(keys_label_on("Ctrl+1…9", false, german), "Strg+1…9");
        assert_eq!(keys_label_on("← / →", false, german), "← / →");
    }

    /// Cmd is macOS's name for the primary modifier in every language, so it
    /// wins over the language's name for Ctrl — and the rest of the row is
    /// still named.
    #[test]
    fn cmd_is_named_before_the_language_is() {
        assert_eq!(
            keys_label_on("Ctrl+Shift+P", true, german),
            "Cmd+Umschalt+P"
        );
    }

    /// **English output is byte-identical**: the `en-GB` catalogue names every
    /// key exactly as the table spells it, so every row comes back as it was
    /// written — the regression net the suite's English assertions rely on.
    #[test]
    fn english_names_every_key_as_the_table_spells_it() {
        for (_, rows) in SHORTCUTS {
            for (keys, _) in *rows {
                assert_eq!(keys_label_on(keys, false, super::key_name), *keys);
            }
        }
        for key in [
            "Ctrl", "Shift", "Alt", "Enter", "Del", "Home", "End", "Space", "Tab",
        ] {
            assert_eq!(super::key_name(key).as_deref().unwrap_or(key), key);
        }
    }

    /// The half that decides what the app *does*, and the reason it takes the
    /// capability rather than reading `cfg!` itself: both platforms have to be
    /// testable from whichever one the suite happens to run on.
    #[test]
    fn cmd_counts_as_the_primary_modifier_only_where_cmd_is_the_modifier() {
        // On macOS, Cmd works and Ctrl keeps working — the change is additive,
        // so nothing a macOS user already knew stops working.
        assert!(primary_from(false, true, true), "Cmd on macOS");
        assert!(primary_from(true, false, true), "Ctrl still works on macOS");
        // Off macOS the Meta key is the Windows/Super key, and binding the app's
        // shortcuts to it would hijack keys that belong to the desktop.
        assert!(
            !primary_from(false, true, false),
            "the Windows/Super key must never stand in for Ctrl"
        );
        assert!(primary_from(true, false, false), "Ctrl off macOS");
        assert!(!primary_from(false, false, true));
        assert!(!primary_from(false, false, false));
        // The fourth off-macOS combination, so the four together say the whole
        // thing: off macOS the answer is the Ctrl bit and nothing else, which is
        // what makes this change provably macOS-only in effect.
        assert!(primary_from(true, true, false), "Ctrl+Super is still Ctrl");
    }

    /// **A key label is respelled, never spelled** — the invariant
    /// `docs/architecture.md` states in the imperative, and a source gate
    /// because the three sites that broke it were `&'static str`s in view code
    /// that no runtime test reaches.
    ///
    /// The divergence they produced: on macOS the Shortcuts modal, the command
    /// palette and the ER diagram all say `Cmd+Enter` while the new tab's own
    /// prompt — the first thing on screen, and the only place the app says how
    /// to run something — said `Ctrl+Enter`, as did the commit button's
    /// tooltip. The handlers were right either way (`primary_held` is
    /// additive); what was wrong is that the app advertised two keys for one
    /// action.
    ///
    /// The needle is a `Ctrl+` inside a string literal, in either view crate,
    /// outside this module. It is deliberately blind to `Ctrl` alone
    /// (`"control() || meta()"` prose, the terminal's `Ctrl+Shift+C` encoding
    /// note) — those are comments and code, and comments are already cut by
    /// `production_code`.
    ///
    /// Two forms are not labels and are recognised by their shape rather than
    /// by their file, so the rule stays about the thing and not about where it
    /// happens to live today:
    ///
    /// - an argument to `keys_label` itself, which is a Ctrl-spelled table
    ///   entry on its way *through* the respeller. That is the fix, not the
    ///   defect.
    /// - the trailing description of a `pair!` / `faded!` row in the contrast
    ///   audit, which names a colour pair for a report and never reaches a
    ///   view.
    #[test]
    fn no_view_code_spells_the_primary_modifier_in_a_label() {
        let mut offenders: Vec<String> = Vec::new();
        for (file, code) in crate::source_gate::crate_sources() {
            if file == "shortcuts.rs" {
                continue;
            }
            for (i, line) in code.lines().enumerate() {
                let l = line.trim();
                if l.contains("keys_label(") || l.contains("pair!(") || l.contains("faded!(") {
                    continue;
                }
                // Inside a string literal, which is what makes it a label
                // rather than a mention: `"…Ctrl+…"`.
                let quoted = l
                    .split('"')
                    .skip(1)
                    .step_by(2)
                    .any(|seg| seg.contains("Ctrl+"));
                if quoted {
                    offenders.push(format!("{file}:{}: {l}", i + 1));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "a key label must go through `shortcuts::keys_label`, or macOS reads \
             Ctrl where every other surface reads Cmd:\n{}",
            offenders.join("\n")
        );
    }

    #[test]
    fn the_table_is_not_accidentally_empty() {
        assert!(SHORTCUTS.len() >= 3);
        for (group, rows) in SHORTCUTS {
            assert!(!rows.is_empty(), "group {group:?} has no rows");
            for (keys, desc) in *rows {
                assert!(
                    !keys.is_empty() && !desc().is_empty(),
                    "blank row in {group:?}"
                );
            }
        }
    }
}
