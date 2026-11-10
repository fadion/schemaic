//! The rebindable shortcuts: which commands a key can be bound to, what each
//! is bound to by default, what the user changed, and which command a pressed
//! chord runs.
//!
//! **One table.** Every handler that runs one of these commands asks
//! [`Keymap::resolve`], and the Shortcuts modal and the palette show
//! [`Keymap::chord`]. A handler with its own `match` arm for a command's key
//! would keep answering the default after the user moved it, and the modal
//! would document a key that no longer does what it says.
//!
//! **Only what the user changed is stored** ([`KeymapFile`], `keymap.json`): a
//! command they never touched follows its default, so a default changed in a
//! later release still reaches them. A command they cleared is stored as
//! `null`, which is a different thing from absent.
//!
//! **Conflicts are reported, never resolved silently.** [`Keymap::check`]
//! answers whether a chord may be given to a command and, when it may not,
//! names what holds it. [`Keymap::load`] runs the same check over a file, keeps
//! what it can and reports each entry it could not keep.
//!
//! What is *not* here: the keys that stay fixed — grid cell movement, editing
//! inside the editor, Tab, Enter and Escape, terminal input. The caller passes
//! the fixed keys it binds ([`FixedKey`]) so a command cannot be moved onto one
//! of them, which would leave either the command or the fixed key dead.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// The file the user's changes to the default keymap are kept in.
pub const FILE: &str = "keymap.json";

/// Where a key is listened for: the whole window, or one surface of it.
///
/// A `Global` key is answered whatever has focus, so it overlaps every other
/// context; two surfaces never overlap each other, which is why the editor's
/// Ctrl+G and the grid's Ctrl+G can mean two things.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Context {
    Global,
    Editor,
    ResultsGrid,
    SchemaTree,
    AiPanel,
    Terminal,
    ErDiagram,
    ServerActivity,
}

impl Context {
    /// Could one press be heard by a key in both contexts?
    pub fn overlaps(self, other: Context) -> bool {
        self == other || self == Context::Global || other == Context::Global
    }
}

/// A command a key can be bound to.
///
/// The [`id`](Command::id) is what `keymap.json` stores, so it is identity: add
/// a command, never rename one's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Command {
    FindAnywhere,
    CommandPalette,
    NewTab,
    CloseTab,
    NextTab,
    PreviousTab,
    ReopenTab,
    OpenFile,
    SaveFile,
    SaveFileAs,
    ToggleSchema,
    ToggleAi,
    ToggleTerminal,
    FormatCode,
    FormatStatement,
    MakeSingleLine,
    GoToLine,
}

impl Command {
    /// Every command, in the order the Shortcuts modal lists them.
    pub const ALL: &'static [Command] = &[
        Command::FindAnywhere,
        Command::CommandPalette,
        Command::NewTab,
        Command::CloseTab,
        Command::NextTab,
        Command::PreviousTab,
        Command::ReopenTab,
        Command::OpenFile,
        Command::SaveFile,
        Command::SaveFileAs,
        Command::ToggleSchema,
        Command::ToggleAi,
        Command::ToggleTerminal,
        Command::FormatCode,
        Command::FormatStatement,
        Command::MakeSingleLine,
        Command::GoToLine,
    ];

    /// The stable name `keymap.json` stores.
    pub fn id(self) -> &'static str {
        match self {
            Command::FindAnywhere => "find-anywhere",
            Command::CommandPalette => "command-palette",
            Command::NewTab => "new-tab",
            Command::CloseTab => "close-tab",
            Command::NextTab => "next-tab",
            Command::PreviousTab => "previous-tab",
            Command::ReopenTab => "reopen-tab",
            Command::OpenFile => "open-file",
            Command::SaveFile => "save-file",
            Command::SaveFileAs => "save-file-as",
            Command::ToggleSchema => "toggle-schema",
            Command::ToggleAi => "toggle-ai",
            Command::ToggleTerminal => "toggle-terminal",
            Command::FormatCode => "format-code",
            Command::FormatStatement => "format-statement",
            Command::MakeSingleLine => "make-single-line",
            Command::GoToLine => "go-to-line",
        }
    }

    /// The command `keymap.json` calls `id`.
    pub fn from_id(id: &str) -> Option<Command> {
        Command::ALL.iter().copied().find(|c| c.id() == id)
    }

    /// Where the command's key is listened for.
    pub fn context(self) -> Context {
        match self {
            Command::FormatCode
            | Command::FormatStatement
            | Command::MakeSingleLine
            | Command::GoToLine => Context::Editor,
            _ => Context::Global,
        }
    }

    /// The key the command has until the user changes it.
    pub fn default_chord(self) -> Chord {
        let (primary, alt, shift, key) = match self {
            Command::FindAnywhere => (true, false, false, Key::Char('p')),
            Command::CommandPalette => (true, false, true, Key::Char('p')),
            Command::NewTab => (true, false, false, Key::Char('t')),
            Command::CloseTab => (true, false, false, Key::Char('w')),
            Command::NextTab => (true, false, false, Key::Tab),
            Command::PreviousTab => (true, false, true, Key::Tab),
            Command::ReopenTab => (true, false, true, Key::Char('t')),
            Command::OpenFile => (true, false, false, Key::Char('o')),
            Command::SaveFile => (true, false, false, Key::Char('s')),
            Command::SaveFileAs => (true, false, true, Key::Char('s')),
            Command::ToggleSchema => (true, false, true, Key::Char('e')),
            Command::ToggleAi => (true, false, true, Key::Char('a')),
            Command::ToggleTerminal => (true, false, false, Key::Char('`')),
            Command::FormatCode => (true, true, false, Key::Char('l')),
            Command::FormatStatement => (true, true, true, Key::Char('l')),
            Command::MakeSingleLine => (true, true, false, Key::Char('j')),
            Command::GoToLine => (true, false, false, Key::Char('g')),
        };
        Chord {
            primary,
            alt,
            shift,
            key,
        }
    }
}

/// The key a chord ends on.
///
/// A character is kept lower-case and unshifted — `Ctrl+Shift+P` is
/// `Char('p')` with `shift` — so a chord does not depend on what Shift makes of
/// a key on one layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    Char(char),
    Tab,
    Enter,
    Space,
    Escape,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    /// `F1` to `F24`.
    F(u8),
}

/// The named keys, as a chord is written: `(spelling, key)`. The spelling is
/// the Shortcuts table's own, so a written chord goes through the modal's
/// respeller (`Ctrl` → `Cmd`, `Del` → `Entf`) unchanged.
const NAMED: &[(&str, Key)] = &[
    ("Tab", Key::Tab),
    ("Enter", Key::Enter),
    ("Space", Key::Space),
    ("Esc", Key::Escape),
    ("Backspace", Key::Backspace),
    ("Del", Key::Delete),
    ("Ins", Key::Insert),
    ("Home", Key::Home),
    ("End", Key::End),
    ("PgUp", Key::PageUp),
    ("PgDn", Key::PageDown),
    ("↑", Key::Up),
    ("↓", Key::Down),
    ("←", Key::Left),
    ("→", Key::Right),
];

/// Other spellings a hand-written `keymap.json` may use for a named key, read
/// case-insensitively. Never written.
const NAMED_ALIASES: &[(&str, Key)] = &[
    ("return", Key::Enter),
    ("escape", Key::Escape),
    ("delete", Key::Delete),
    ("insert", Key::Insert),
    ("pageup", Key::PageUp),
    ("pagedown", Key::PageDown),
    ("up", Key::Up),
    ("down", Key::Down),
    ("left", Key::Left),
    ("right", Key::Right),
];

/// A key pressed with modifiers. `primary` is Ctrl, or Cmd on macOS — written
/// `Ctrl` in every file, so one keymap serves every platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Chord {
    pub primary: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Key,
}

impl Chord {
    /// A chord of `key` with no modifiers.
    pub fn plain(key: Key) -> Chord {
        Chord {
            primary: false,
            alt: false,
            shift: false,
            key,
        }
    }

    /// Read a chord as written: `Ctrl+Shift+P`, `Ctrl+Alt+L`, `F2`. Case is
    /// ignored, `Cmd` and `Control` read as `Ctrl`, `Option` as `Alt`.
    pub fn parse(text: &str) -> Result<Chord, ChordError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(ChordError::Empty);
        }
        let mut chord = Chord::plain(Key::Space);
        let mut key = None;
        // `Ctrl++` binds the plus key: an empty part after a `+` is the key.
        let parts: Vec<&str> = if let Some(head) = text.strip_suffix("++") {
            head.split('+').chain(["+"]).collect()
        } else {
            text.split('+').collect()
        };
        let last = parts.len() - 1;
        for (i, part) in parts.iter().enumerate() {
            let part = part.trim();
            let lower = part.to_lowercase();
            let modifier = match lower.as_str() {
                "ctrl" | "control" | "cmd" | "command" | "primary" => Some(&mut chord.primary),
                "alt" | "option" => Some(&mut chord.alt),
                "shift" => Some(&mut chord.shift),
                _ => None,
            };
            if i < last {
                let Some(flag) = modifier else {
                    return Err(ChordError::UnknownKey(part.to_string()));
                };
                if *flag {
                    return Err(ChordError::Repeated(part.to_string()));
                }
                *flag = true;
            } else {
                if modifier.is_some() {
                    return Err(ChordError::NoKey);
                }
                key =
                    Some(parse_key(part).ok_or_else(|| ChordError::UnknownKey(part.to_string()))?);
            }
        }
        chord.key = key.ok_or(ChordError::NoKey)?;
        Ok(chord)
    }

    /// Can a command be bound to this chord at all, whatever else is bound?
    ///
    /// A character needs the primary modifier: without it the key types, and
    /// Alt alone is how macOS types accents (Option+E) and many layouts type
    /// `@` or `{`. Escape is never a command's key — it closes what is open
    /// everywhere, and the key capture's own cancel. The editing keys every
    /// text field answers (Ctrl+C/V/X/A/Z/Y, Ctrl with an arrow, Home, End,
    /// Backspace or Delete) are reserved, since a command on one would take it
    /// from every field in the app.
    ///
    /// Two more depend on the platform ([`Platform`]). Where AltGr arrives as
    /// Ctrl+Alt, Ctrl+Alt with a character is refused — the editor would run
    /// the command instead of typing the `@` — except a default's own chord,
    /// which has always been there. On macOS, Option with an arrow, Backspace
    /// or Delete is a text field's word movement, so it is reserved as an
    /// editing key.
    pub fn refusal(self) -> Option<Refusal> {
        self.refusal_on(Platform::CURRENT)
    }

    /// [`refusal`](Chord::refusal) on `platform`.
    pub fn refusal_on(self, platform: Platform) -> Option<Refusal> {
        let any = self.primary || self.alt;
        match self.key {
            Key::Escape => return Some(Refusal::Escape),
            Key::Char(_) if !self.primary => return Some(Refusal::NeedsPrimary),
            Key::Tab if !self.primary => return Some(Refusal::NeedsPrimary),
            Key::F(_) => {}
            Key::Char(_) | Key::Tab => {}
            _ if !any => return Some(Refusal::NeedsModifier),
            _ => {}
        }
        let editing = match self.key {
            // Shifted, only Ctrl+Shift+Z is a field's (redo): Ctrl+Shift+A is
            // the AI panel's default.
            Key::Char(c) if self.shift => c == 'z',
            Key::Char(c) => matches!(c, 'c' | 'v' | 'x' | 'a' | 'z' | 'y'),
            // Shift with these extends the selection, so it is reserved too.
            Key::Up
            | Key::Down
            | Key::Left
            | Key::Right
            | Key::Home
            | Key::End
            | Key::Backspace
            | Key::Delete => true,
            _ => false,
        };
        if self.primary && !self.alt && editing {
            return Some(Refusal::Editing);
        }
        let word_editing = matches!(
            self.key,
            Key::Up | Key::Down | Key::Left | Key::Right | Key::Backspace | Key::Delete
        );
        if platform.option_edits_text && self.alt && !self.primary && word_editing {
            return Some(Refusal::Editing);
        }
        if self.alt && !self.primary && self.key == Key::F(4) {
            return Some(Refusal::System);
        }
        if platform.altgr_is_ctrl_alt
            && self.primary
            && self.alt
            && matches!(self.key, Key::Char(_))
            && !Command::ALL.iter().any(|c| c.default_chord() == self)
        {
            return Some(Refusal::AltGr);
        }
        None
    }
}

/// Why a chord can never be a command's key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A character or Tab with no Ctrl: it would type, or move focus.
    NeedsPrimary,
    /// A named key with neither Ctrl nor Alt: Enter, Space, an arrow.
    NeedsModifier,
    /// Escape, which closes and cancels everywhere.
    Escape,
    /// An editing key every text field answers.
    Editing,
    /// A key the desktop owns (Alt+F4).
    System,
    /// Ctrl+Alt with a character, where that is how AltGr arrives: on many
    /// layouts it types `@`, `{` or `€`.
    AltGr,
}

/// What the desktop does with the modifiers, as far as it decides which
/// chords a command may take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform {
    /// AltGr is reported as Ctrl+Alt (Windows), so Ctrl+Alt with a character
    /// is how a layout types `@` or `{`.
    pub altgr_is_ctrl_alt: bool,
    /// Option with an arrow, Backspace or Delete moves or deletes by word in
    /// every text field (macOS).
    pub option_edits_text: bool,
}

impl Platform {
    /// The platform this build runs on.
    pub const CURRENT: Platform = Platform {
        altgr_is_ctrl_alt: cfg!(windows),
        option_edits_text: cfg!(target_os = "macos"),
    };
}

fn parse_key(part: &str) -> Option<Key> {
    if let Some((_, k)) = NAMED.iter().find(|(n, _)| n.eq_ignore_ascii_case(part)) {
        return Some(*k);
    }
    let lower = part.to_lowercase();
    if let Some((_, k)) = NAMED_ALIASES.iter().find(|(n, _)| *n == lower) {
        return Some(*k);
    }
    if let Some(n) = lower.strip_prefix('f')
        && let Ok(n) = n.parse::<u8>()
        && (1..=24).contains(&n)
    {
        return Some(Key::F(n));
    }
    let mut chars = part.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if !c.is_whitespace() => Some(Key::Char(c.to_lowercase().next()?)),
        _ => None,
    }
}

impl fmt::Display for Chord {
    /// The table's spelling: modifiers in the order Ctrl, Alt, Shift, then the
    /// key — `Ctrl+Alt+Shift+L`, as the Shortcuts modal has always written it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.primary {
            f.write_str("Ctrl+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        match self.key {
            Key::Char(c) => write!(f, "{}", c.to_uppercase()),
            Key::F(n) => write!(f, "F{n}"),
            k => f.write_str(
                NAMED
                    .iter()
                    .find(|(_, named)| *named == k)
                    .map(|(n, _)| *n)
                    .unwrap_or("?"),
            ),
        }
    }
}

/// Why a written chord could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChordError {
    Empty,
    /// Only modifiers: `Ctrl+Shift`.
    NoKey,
    /// A part that is no key this app knows.
    UnknownKey(String),
    /// The same modifier twice.
    Repeated(String),
}

impl fmt::Display for ChordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChordError::Empty => f.write_str("empty"),
            ChordError::NoKey => f.write_str("no key after the modifiers"),
            ChordError::UnknownKey(k) => write!(f, "unknown key {k:?}"),
            ChordError::Repeated(k) => write!(f, "{k} is written twice"),
        }
    }
}

/// A key the app binds that the keymap cannot move: the caller's list, so a
/// command is never put on one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedKey {
    pub context: Context,
    pub chord: Chord,
}

/// What [`Keymap::check`] found for a chord a command is to be given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing else answers it.
    Free,
    /// The command already has it.
    Unchanged,
    /// Another command answers it where this one would: assigning it means
    /// taking it from that one.
    Taken(Command),
    /// A key the app binds and the keymap cannot move — the index into the
    /// caller's fixed list, which knows what it does.
    Fixed(usize),
    /// No command may have it.
    Refused(Refusal),
}

/// `keymap.json`: the user's changes from the defaults, by command id. `null`
/// is a command they cleared.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeymapFile {
    #[serde(default)]
    pub bindings: BTreeMap<String, Option<String>>,
    /// Every other top-level key, read and written back untouched — entries
    /// written by hand without the `bindings` wrapper, or a later build's
    /// field — and reported ([`Problem::UnknownKey`]). Dropped, they went
    /// unnoticed and the next save wrote over them.
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

/// An entry of `keymap.json` that could not be kept, and why. The entry is
/// left in the file — a later build may know the command — but the session
/// runs without it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    UnknownCommand(String),
    /// A top-level key beside `bindings` this build does not read.
    UnknownKey(String),
    Unreadable {
        command: Command,
        text: String,
        error: ChordError,
    },
    Refused {
        command: Command,
        chord: Chord,
        refusal: Refusal,
    },
    /// The chord belongs to a fixed key (its index in the caller's list).
    Fixed {
        command: Command,
        chord: Chord,
        fixed: usize,
    },
    /// The chord was already `holder`'s; `command` was left without one.
    Conflict {
        command: Command,
        chord: Chord,
        holder: Command,
    },
}

/// Every command's binding as it stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keymap {
    bindings: BTreeMap<Command, Option<Chord>>,
    /// Entries of the file this build could not keep, carried back out on
    /// save so a newer build's commands, or a typo the user means to fix, are
    /// not lost by a save from this one. A known command's entry leaves once
    /// the user gives that command a key ([`assign`](Keymap::assign),
    /// [`clear`](Keymap::clear), [`reset`](Keymap::reset)): what they said
    /// since replaces what the file said.
    kept: BTreeMap<String, Option<String>>,
    /// The file's top-level keys beside `bindings`, written back as read
    /// ([`KeymapFile::other`]).
    other: BTreeMap<String, serde_json::Value>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::defaults()
    }
}

impl Keymap {
    /// Every command on its default key.
    pub fn defaults() -> Keymap {
        Keymap {
            bindings: Command::ALL
                .iter()
                .map(|&c| (c, Some(c.default_chord())))
                .collect(),
            kept: BTreeMap::new(),
            other: BTreeMap::new(),
        }
    }

    /// The defaults with the file's changes applied, and every entry that
    /// could not be applied.
    ///
    /// An entry is applied when it reads, may be a command's key at all, is no
    /// fixed key, and no command already answers it. **What the file names
    /// wins over a default**: a file that moves New tab to Ctrl+P takes Ctrl+P
    /// from Find anywhere, which is reported, rather than leaving New tab
    /// unbound. Between two entries of the file, the first in [`Command::ALL`]
    /// order keeps the chord.
    pub fn load(file: &KeymapFile, fixed: &[FixedKey]) -> (Keymap, Vec<Problem>) {
        let mut map = Keymap {
            bindings: BTreeMap::new(),
            kept: BTreeMap::new(),
            other: file.other.clone(),
        };
        let mut problems: Vec<Problem> = file
            .other
            .keys()
            .cloned()
            .map(Problem::UnknownKey)
            .collect();
        let mut named: BTreeMap<Command, Option<Chord>> = BTreeMap::new();
        for (id, text) in &file.bindings {
            let Some(command) = Command::from_id(id) else {
                problems.push(Problem::UnknownCommand(id.clone()));
                map.kept.insert(id.clone(), text.clone());
                continue;
            };
            let Some(text) = text else {
                named.insert(command, None);
                continue;
            };
            let chord = match Chord::parse(text) {
                Ok(c) => c,
                Err(error) => {
                    problems.push(Problem::Unreadable {
                        command,
                        text: text.clone(),
                        error,
                    });
                    map.kept.insert(id.clone(), Some(text.clone()));
                    continue;
                }
            };
            if let Some(refusal) = chord.refusal() {
                problems.push(Problem::Refused {
                    command,
                    chord,
                    refusal,
                });
                map.kept.insert(id.clone(), Some(text.clone()));
                continue;
            }
            if let Some(i) = fixed_holding(fixed, command.context(), chord) {
                problems.push(Problem::Fixed {
                    command,
                    chord,
                    fixed: i,
                });
                map.kept.insert(id.clone(), Some(text.clone()));
                continue;
            }
            named.insert(command, Some(chord));
        }
        // The file's own entries first, in command order, then the defaults
        // around them.
        for &command in Command::ALL {
            if let Some(&chord) = named.get(&command) {
                let chord = chord.filter(|&c| match map.holder(c, command.context()) {
                    Some(holder) => {
                        problems.push(Problem::Conflict {
                            command,
                            chord: c,
                            holder,
                        });
                        // The loser's text goes back out on save, as every
                        // other entry that could not be applied does.
                        if let Some(text) = file.bindings.get(command.id()) {
                            map.kept.insert(command.id().to_string(), text.clone());
                        }
                        false
                    }
                    None => true,
                });
                map.bindings.insert(command, chord);
            }
        }
        for &command in Command::ALL {
            if named.contains_key(&command) {
                continue;
            }
            let chord = command.default_chord();
            let chord = match map.holder(chord, command.context()) {
                Some(holder) => {
                    problems.push(Problem::Conflict {
                        command,
                        chord,
                        holder,
                    });
                    None
                }
                None => Some(chord),
            };
            map.bindings.insert(command, chord);
        }
        (map, problems)
    }

    /// The file that reproduces this keymap: every command off its default,
    /// and whatever the loaded file held that this build could not keep. A
    /// kept entry of a known command stands for that command: it is only
    /// still kept while the user has not given the command a key since.
    pub fn to_file(&self) -> KeymapFile {
        let mut bindings = self.kept.clone();
        for &command in Command::ALL {
            if self.kept.contains_key(command.id()) {
                continue;
            }
            let chord = self.chord(command);
            if chord != Some(command.default_chord()) {
                bindings.insert(command.id().to_string(), chord.map(|c| c.to_string()));
            }
        }
        KeymapFile {
            bindings,
            other: self.other.clone(),
        }
    }

    /// How many commands `other` binds differently — what an import of it
    /// would change.
    pub fn changes_from(&self, other: &Keymap) -> usize {
        Command::ALL
            .iter()
            .filter(|&&c| self.chord(c) != other.chord(c))
            .count()
    }

    /// The command's key, or `None` when it has none.
    pub fn chord(&self, command: Command) -> Option<Chord> {
        self.bindings.get(&command).copied().flatten()
    }

    /// Is the command on its default key?
    pub fn is_default(&self, command: Command) -> bool {
        self.chord(command) == Some(command.default_chord())
    }

    /// The command a press of `chord` runs in `context`. A handler asks for
    /// its own context; the editor, which consumes every key, asks for
    /// [`Context::Global`] first and then for its own.
    pub fn resolve(&self, chord: Chord, context: Context) -> Option<Command> {
        self.bindings
            .iter()
            .find(|(c, b)| c.context() == context && **b == Some(chord))
            .map(|(c, _)| *c)
    }

    /// The command that answers `chord` anywhere `context` is heard.
    fn holder(&self, chord: Chord, context: Context) -> Option<Command> {
        self.bindings
            .iter()
            .find(|(c, b)| c.context().overlaps(context) && **b == Some(chord))
            .map(|(c, _)| *c)
    }

    /// May `command` be given `chord`? The answer the modal shows before it
    /// changes anything.
    pub fn check(&self, command: Command, chord: Chord, fixed: &[FixedKey]) -> Verdict {
        if let Some(refusal) = chord.refusal() {
            return Verdict::Refused(refusal);
        }
        if self.chord(command) == Some(chord) {
            return Verdict::Unchanged;
        }
        if let Some(i) = fixed_holding(fixed, command.context(), chord) {
            return Verdict::Fixed(i);
        }
        match self.holder(chord, command.context()) {
            Some(holder) => Verdict::Taken(holder),
            None => Verdict::Free,
        }
    }

    /// Give `command` the chord, taking it from any command that answers it
    /// where this one would — the *Replace* of a [`Verdict::Taken`]. The
    /// caller has asked [`check`](Keymap::check) first; a refused or fixed
    /// chord is not assigned.
    pub fn assign(&mut self, command: Command, chord: Chord, fixed: &[FixedKey]) -> bool {
        match self.check(command, chord, fixed) {
            Verdict::Refused(_) | Verdict::Fixed(_) => return false,
            Verdict::Unchanged => {
                self.set(command, Some(chord));
                return true;
            }
            Verdict::Taken(_) | Verdict::Free => {}
        }
        while let Some(holder) = self.holder(chord, command.context()) {
            self.set(holder, None);
        }
        self.set(command, Some(chord));
        true
    }

    /// Leave `command` with no key.
    pub fn clear(&mut self, command: Command) {
        self.set(command, None);
    }

    /// Bind `command` as the user has now said, dropping whatever the file
    /// held for it that could not be applied.
    fn set(&mut self, command: Command, chord: Option<Chord>) {
        self.kept.remove(command.id());
        self.bindings.insert(command, chord);
    }

    /// What [`reset`](Keymap::reset) would find in the default's way: a
    /// command holding it, or a fixed key. `Free` or `Unchanged` when nothing
    /// stands in the way.
    pub fn check_reset(&self, command: Command, fixed: &[FixedKey]) -> Verdict {
        self.check(command, command.default_chord(), fixed)
    }

    /// Put `command` back on its default, taking the key from any command the
    /// user has since put on it.
    pub fn reset(&mut self, command: Command, fixed: &[FixedKey]) -> bool {
        self.assign(command, command.default_chord(), fixed)
    }
}

/// The fixed key that answers `chord` where `context` is heard.
fn fixed_holding(fixed: &[FixedKey], context: Context, chord: Chord) -> Option<usize> {
    fixed
        .iter()
        .position(|f| f.chord == chord && f.context.overlaps(context))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(s: &str) -> Chord {
        Chord::parse(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    fn file(entries: &[(&str, Option<&str>)]) -> KeymapFile {
        KeymapFile {
            bindings: entries
                .iter()
                .map(|(k, v)| (k.to_string(), v.map(str::to_string)))
                .collect(),
            ..Default::default()
        }
    }

    fn fixed(context: Context, s: &str) -> FixedKey {
        FixedKey {
            context,
            chord: chord(s),
        }
    }

    // ── Chords ───────────────────────────────────────────────────────────

    #[test]
    fn a_chord_reads_back_as_the_table_writes_it() {
        for s in [
            "Ctrl+P",
            "Ctrl+Shift+P",
            "Ctrl+Alt+Shift+L",
            "Ctrl+Tab",
            "Ctrl+`",
            "Ctrl+/",
            "Ctrl+Shift+\\",
            "F2",
            "Shift+F10",
            "Alt+Enter",
            "Ctrl+Del",
            "Ctrl+PgDn",
            "Alt+↑",
            "Ctrl++",
        ] {
            assert_eq!(chord(s).to_string(), s);
        }
    }

    #[test]
    fn modifiers_are_written_in_one_order_whatever_order_they_were_read_in() {
        assert_eq!(chord("shift+alt+ctrl+l").to_string(), "Ctrl+Alt+Shift+L");
    }

    #[test]
    fn cmd_and_option_read_as_the_primary_modifier_and_alt() {
        assert_eq!(chord("Cmd+Option+L"), chord("Ctrl+Alt+L"));
        assert_eq!(chord("Control+P"), chord("Ctrl+P"));
    }

    #[test]
    fn a_letter_is_held_lower_case_so_shift_is_only_the_flag() {
        assert_eq!(chord("Ctrl+Shift+p").key, Key::Char('p'));
        assert_eq!(chord("Ctrl+Shift+P"), chord("ctrl+shift+p"));
    }

    #[test]
    fn hand_written_key_names_are_accepted() {
        assert_eq!(chord("Ctrl+Return"), chord("Ctrl+Enter"));
        assert_eq!(chord("Ctrl+PageDown"), chord("Ctrl+PgDn"));
        assert_eq!(chord("Alt+Up"), chord("Alt+↑"));
        assert_eq!(chord("Ctrl+Delete"), chord("Ctrl+Del"));
    }

    #[test]
    fn what_is_not_a_chord_is_refused_with_its_reason() {
        assert_eq!(Chord::parse(""), Err(ChordError::Empty));
        assert_eq!(Chord::parse("  "), Err(ChordError::Empty));
        assert_eq!(Chord::parse("Ctrl+Shift"), Err(ChordError::NoKey));
        assert_eq!(
            Chord::parse("Hyper+P"),
            Err(ChordError::UnknownKey("Hyper".into()))
        );
        assert_eq!(
            Chord::parse("Ctrl+Banana"),
            Err(ChordError::UnknownKey("Banana".into()))
        );
        assert_eq!(
            Chord::parse("Ctrl+Ctrl+P"),
            Err(ChordError::Repeated("Ctrl".into()))
        );
        assert_eq!(
            Chord::parse("Ctrl+F25"),
            Err(ChordError::UnknownKey("F25".into()))
        );
    }

    #[test]
    fn a_character_needs_the_primary_modifier() {
        assert_eq!(chord("P").refusal(), Some(Refusal::NeedsPrimary));
        assert_eq!(chord("Shift+P").refusal(), Some(Refusal::NeedsPrimary));
        // Option+E is how macOS types an accent.
        assert_eq!(chord("Alt+E").refusal(), Some(Refusal::NeedsPrimary));
        assert_eq!(chord("Ctrl+P").refusal(), None);
        assert_eq!(chord("Ctrl+Alt+P").refusal_on(MAC), None);
        assert_eq!(chord("Ctrl+Alt+P").refusal_on(LINUX), None);
    }

    const WINDOWS: Platform = Platform {
        altgr_is_ctrl_alt: true,
        option_edits_text: false,
    };
    const MAC: Platform = Platform {
        altgr_is_ctrl_alt: false,
        option_edits_text: true,
    };
    const LINUX: Platform = Platform {
        altgr_is_ctrl_alt: false,
        option_edits_text: false,
    };

    #[test]
    fn where_altgr_arrives_as_ctrl_alt_a_character_with_both_would_type() {
        // AltGr+Q is `@` on a German keyboard, AltGr+E `€`, AltGr+7 `{`.
        for s in ["Ctrl+Alt+Q", "Ctrl+Alt+E", "Ctrl+Alt+7", "Ctrl+Alt+Shift+Q"] {
            assert_eq!(chord(s).refusal_on(WINDOWS), Some(Refusal::AltGr), "{s}");
            assert_eq!(chord(s).refusal_on(LINUX), None, "{s}");
        }
        // A named or function key types nothing, whatever the modifiers.
        assert_eq!(chord("Ctrl+Alt+PgDn").refusal_on(WINDOWS), None);
        assert_eq!(chord("Ctrl+Alt+F2").refusal_on(WINDOWS), None);
    }

    #[test]
    fn the_defaults_on_ctrl_alt_stay_usable_where_altgr_is_ctrl_alt() {
        for s in ["Ctrl+Alt+L", "Ctrl+Alt+Shift+L", "Ctrl+Alt+J"] {
            assert_eq!(chord(s).refusal_on(WINDOWS), None, "{s}");
        }
    }

    #[test]
    fn on_macos_option_with_an_arrow_or_a_delete_key_edits_text() {
        for s in [
            "Alt+←",
            "Alt+→",
            "Alt+↑",
            "Alt+↓",
            "Alt+Shift+←",
            "Alt+Backspace",
            "Alt+Del",
        ] {
            assert_eq!(chord(s).refusal_on(MAC), Some(Refusal::Editing), "{s}");
            assert_eq!(chord(s).refusal_on(WINDOWS), None, "{s}");
        }
        assert_eq!(chord("Alt+Enter").refusal_on(MAC), None);
        assert_eq!(chord("Alt+Home").refusal_on(MAC), None);
    }

    #[test]
    fn tab_needs_the_primary_modifier_and_escape_is_never_a_key() {
        assert_eq!(chord("Tab").refusal(), Some(Refusal::NeedsPrimary));
        assert_eq!(chord("Shift+Tab").refusal(), Some(Refusal::NeedsPrimary));
        assert_eq!(chord("Alt+Tab").refusal(), Some(Refusal::NeedsPrimary));
        assert_eq!(chord("Ctrl+Tab").refusal(), None);
        assert_eq!(chord("Esc").refusal(), Some(Refusal::Escape));
        assert_eq!(chord("Ctrl+Esc").refusal(), Some(Refusal::Escape));
    }

    #[test]
    fn a_named_key_needs_ctrl_or_alt_and_a_function_key_needs_neither() {
        for s in ["Enter", "Space", "↑", "Home", "PgDn", "Shift+Enter"] {
            assert_eq!(chord(s).refusal(), Some(Refusal::NeedsModifier), "{s}");
        }
        assert_eq!(chord("Alt+Enter").refusal(), None);
        assert_eq!(chord("Ctrl+Enter").refusal(), None);
        assert_eq!(chord("F2").refusal(), None);
        assert_eq!(chord("Shift+F10").refusal(), None);
    }

    #[test]
    fn the_editing_keys_every_field_answers_are_reserved() {
        for s in [
            "Ctrl+C",
            "Ctrl+V",
            "Ctrl+X",
            "Ctrl+A",
            "Ctrl+Z",
            "Ctrl+Shift+Z",
            "Ctrl+Y",
            "Ctrl+←",
            "Ctrl+Shift+→",
            "Ctrl+Home",
            "Ctrl+End",
            "Ctrl+Backspace",
            "Ctrl+Del",
        ] {
            assert_eq!(chord(s).refusal(), Some(Refusal::Editing), "{s}");
        }
        // With Alt as well they are no field's keys, and shifted only redo is.
        assert_eq!(chord("Ctrl+Alt+C").refusal_on(LINUX), None);
        assert_eq!(chord("Ctrl+Shift+A").refusal(), None);
        assert_eq!(chord("Ctrl+Shift+C").refusal(), None);
        assert_eq!(chord("Alt+F4").refusal(), Some(Refusal::System));
        assert_eq!(chord("Ctrl+Alt+F4").refusal(), None);
    }

    #[test]
    fn every_default_is_a_chord_a_command_may_have() {
        for &c in Command::ALL {
            assert_eq!(c.default_chord().refusal(), None, "{c:?}");
        }
    }

    // ── Commands ─────────────────────────────────────────────────────────

    #[test]
    fn every_command_is_listed_once_and_its_id_reads_back() {
        let mut ids = std::collections::BTreeSet::new();
        for &c in Command::ALL {
            assert!(ids.insert(c.id()), "duplicate id {}", c.id());
            assert_eq!(Command::from_id(c.id()), Some(c));
        }
        assert_eq!(Command::from_id("New-Tab"), None, "ids are exact");
        assert_eq!(Command::from_id(""), None);
    }

    /// Spelled so a new variant fails to compile until it is in `ALL`.
    #[test]
    fn all_lists_every_variant() {
        fn _exhaustive(c: Command) -> usize {
            match c {
                Command::FindAnywhere
                | Command::CommandPalette
                | Command::NewTab
                | Command::CloseTab
                | Command::NextTab
                | Command::PreviousTab
                | Command::ReopenTab
                | Command::OpenFile
                | Command::SaveFile
                | Command::SaveFileAs
                | Command::ToggleSchema
                | Command::ToggleAi
                | Command::ToggleTerminal
                | Command::FormatCode
                | Command::FormatStatement
                | Command::MakeSingleLine
                | Command::GoToLine => 17,
            }
        }
        assert_eq!(Command::ALL.len(), _exhaustive(Command::NewTab));
    }

    #[test]
    fn no_two_defaults_collide_where_both_are_heard() {
        let (_, problems) = Keymap::load(&KeymapFile::default(), &[]);
        assert_eq!(problems, vec![]);
    }

    #[test]
    fn contexts_overlap_only_through_global() {
        assert!(Context::Global.overlaps(Context::Editor));
        assert!(Context::Editor.overlaps(Context::Global));
        assert!(Context::Editor.overlaps(Context::Editor));
        assert!(!Context::Editor.overlaps(Context::ResultsGrid));
    }

    // ── Resolving ────────────────────────────────────────────────────────

    #[test]
    fn a_press_resolves_to_its_command_in_its_own_context_only() {
        let map = Keymap::defaults();
        assert_eq!(
            map.resolve(chord("Ctrl+P"), Context::Global),
            Some(Command::FindAnywhere)
        );
        assert_eq!(
            map.resolve(chord("Ctrl+Shift+P"), Context::Global),
            Some(Command::CommandPalette)
        );
        assert_eq!(
            map.resolve(chord("Ctrl+G"), Context::Editor),
            Some(Command::GoToLine)
        );
        assert_eq!(
            map.resolve(chord("Ctrl+G"), Context::Global),
            None,
            "the editor's key is not the window's"
        );
        assert_eq!(map.resolve(chord("Ctrl+Q"), Context::Global), None);
    }

    /// Shift is part of the chord: Ctrl+Alt+L formats the code, Shift added
    /// narrows it to the statement, and Shift+J is no format key at all.
    #[test]
    fn the_format_keys_differ_only_by_shift_and_resolve_apart() {
        let map = Keymap::defaults();
        let at = |s: &str| map.resolve(chord(s), Context::Editor);
        assert_eq!(at("Ctrl+Alt+L"), Some(Command::FormatCode));
        assert_eq!(at("Ctrl+Alt+Shift+L"), Some(Command::FormatStatement));
        assert_eq!(at("Ctrl+Alt+J"), Some(Command::MakeSingleLine));
        assert_eq!(at("Ctrl+Alt+Shift+J"), None);
        assert_eq!(at("Ctrl+L"), None);
    }

    #[test]
    fn a_moved_command_answers_its_new_key_and_not_its_old_one() {
        let mut map = Keymap::defaults();
        assert!(map.assign(Command::NewTab, chord("Ctrl+N"), &[]));
        assert_eq!(
            map.resolve(chord("Ctrl+N"), Context::Global),
            Some(Command::NewTab)
        );
        assert_eq!(map.resolve(chord("Ctrl+T"), Context::Global), None);
    }

    // ── Checking and assigning ───────────────────────────────────────────

    #[test]
    fn a_key_another_command_holds_is_reported_as_taken() {
        let map = Keymap::defaults();
        assert_eq!(
            map.check(Command::NewTab, chord("Ctrl+P"), &[]),
            Verdict::Taken(Command::FindAnywhere)
        );
        // A global command over an editor one: both are heard in the editor.
        assert_eq!(
            map.check(Command::NewTab, chord("Ctrl+G"), &[]),
            Verdict::Taken(Command::GoToLine)
        );
        assert_eq!(
            map.check(Command::NewTab, chord("Ctrl+N"), &[]),
            Verdict::Free
        );
        assert_eq!(
            map.check(Command::NewTab, chord("Ctrl+T"), &[]),
            Verdict::Unchanged
        );
    }

    #[test]
    fn a_fixed_key_is_refused_where_it_is_heard_and_free_elsewhere() {
        let fixed = [
            fixed(Context::Editor, "Ctrl+F"),
            fixed(Context::ResultsGrid, "Ctrl+D"),
        ];
        let map = Keymap::defaults();
        assert_eq!(
            map.check(Command::NewTab, chord("Ctrl+F"), &fixed),
            Verdict::Fixed(0),
            "a global key would shadow the editor's find"
        );
        assert_eq!(
            map.check(Command::GoToLine, chord("Ctrl+D"), &fixed),
            Verdict::Free,
            "the grid's Ctrl+D is not heard in the editor"
        );
    }

    #[test]
    fn a_refused_chord_is_refused_before_anything_is_looked_up() {
        let map = Keymap::defaults();
        assert_eq!(
            map.check(Command::NewTab, chord("Ctrl+C"), &[]),
            Verdict::Refused(Refusal::Editing)
        );
    }

    #[test]
    fn replacing_takes_the_key_from_its_holder() {
        let mut map = Keymap::defaults();
        assert!(map.assign(Command::NewTab, chord("Ctrl+P"), &[]));
        assert_eq!(map.chord(Command::NewTab), Some(chord("Ctrl+P")));
        assert_eq!(map.chord(Command::FindAnywhere), None);
    }

    #[test]
    fn a_fixed_or_refused_key_is_never_assigned() {
        let fixed = [fixed(Context::Editor, "Ctrl+F")];
        let mut map = Keymap::defaults();
        assert!(!map.assign(Command::NewTab, chord("Ctrl+F"), &fixed));
        assert!(!map.assign(Command::NewTab, chord("Ctrl+C"), &fixed));
        assert_eq!(map, Keymap::defaults());
    }

    #[test]
    fn clearing_and_resetting() {
        let mut map = Keymap::defaults();
        map.clear(Command::CloseTab);
        assert_eq!(map.chord(Command::CloseTab), None);
        assert!(!map.is_default(Command::CloseTab));
        assert_eq!(map.check_reset(Command::CloseTab, &[]), Verdict::Free);
        assert!(map.reset(Command::CloseTab, &[]));
        assert!(map.is_default(Command::CloseTab));
    }

    #[test]
    fn resetting_onto_a_key_since_given_away_takes_it_back() {
        let mut map = Keymap::defaults();
        map.assign(Command::NewTab, chord("Ctrl+W"), &[]);
        assert_eq!(map.chord(Command::CloseTab), None);
        assert_eq!(
            map.check_reset(Command::CloseTab, &[]),
            Verdict::Taken(Command::NewTab)
        );
        assert!(map.reset(Command::CloseTab, &[]));
        assert_eq!(map.chord(Command::CloseTab), Some(chord("Ctrl+W")));
        assert_eq!(map.chord(Command::NewTab), None);
    }

    // ── The file ─────────────────────────────────────────────────────────

    #[test]
    fn an_untouched_keymap_writes_an_empty_file() {
        assert_eq!(Keymap::defaults().to_file(), KeymapFile::default());
    }

    #[test]
    fn the_file_holds_only_what_moved_and_a_cleared_command_as_null() {
        let mut map = Keymap::defaults();
        map.assign(Command::NewTab, chord("Ctrl+N"), &[]);
        map.clear(Command::CloseTab);
        assert_eq!(
            map.to_file(),
            file(&[("new-tab", Some("Ctrl+N")), ("close-tab", None)])
        );
    }

    #[test]
    fn a_command_moved_back_to_its_default_leaves_the_file() {
        let mut map = Keymap::defaults();
        map.assign(Command::NewTab, chord("Ctrl+N"), &[]);
        map.assign(Command::NewTab, chord("Ctrl+T"), &[]);
        assert_eq!(map.to_file(), KeymapFile::default());
    }

    #[test]
    fn a_file_reads_back_into_the_keymap_that_wrote_it() {
        let mut map = Keymap::defaults();
        map.assign(Command::NewTab, chord("Ctrl+P"), &[]);
        map.clear(Command::SaveFileAs);
        map.assign(Command::GoToLine, chord("Ctrl+Shift+G"), &[]);
        let (read, problems) = Keymap::load(&map.to_file(), &[]);
        assert_eq!(problems, vec![]);
        assert_eq!(read, map);
    }

    #[test]
    fn the_file_is_the_json_the_user_would_write() {
        let json = r#"{ "bindings": { "new-tab": "Ctrl+N", "close-tab": null } }"#;
        let read: KeymapFile = serde_json::from_str(json).unwrap();
        assert_eq!(
            read,
            file(&[("new-tab", Some("Ctrl+N")), ("close-tab", None)])
        );
        let empty: KeymapFile = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, KeymapFile::default());
    }

    #[test]
    fn every_entry_that_cannot_be_kept_is_reported_and_kept_in_the_file() {
        let fixed = [fixed(Context::Editor, "Ctrl+F")];
        let f = file(&[
            ("open-sesame", Some("Ctrl+Q")),
            ("new-tab", Some("Ctrl+Banana")),
            ("close-tab", Some("Ctrl+C")),
            ("open-file", Some("Ctrl+F")),
        ]);
        let (map, problems) = Keymap::load(&f, &fixed);
        assert_eq!(
            problems,
            vec![
                Problem::Refused {
                    command: Command::CloseTab,
                    chord: chord("Ctrl+C"),
                    refusal: Refusal::Editing,
                },
                Problem::Unreadable {
                    command: Command::NewTab,
                    text: "Ctrl+Banana".into(),
                    error: ChordError::UnknownKey("Banana".into()),
                },
                Problem::Fixed {
                    command: Command::OpenFile,
                    chord: chord("Ctrl+F"),
                    fixed: 0,
                },
                Problem::UnknownCommand("open-sesame".into()),
            ]
        );
        // What could not be kept runs on its default…
        assert!(map.is_default(Command::NewTab));
        assert!(map.is_default(Command::CloseTab));
        assert!(map.is_default(Command::OpenFile));
        // …and goes back out on save, for the user (or a later build) to fix.
        assert_eq!(map.to_file(), f);
    }

    /// **A key beside `bindings` is reported and kept, not dropped.** A file
    /// written by hand without the `bindings` wrapper parsed as an empty
    /// keymap with no notice, and the next save wrote `{"bindings":{…}}` over
    /// it; a later build's field beside `bindings` went the same way.
    #[test]
    fn a_top_level_key_this_build_does_not_read_is_reported_and_kept() {
        let text = r#"{"new-tab": "Ctrl+N", "close-tab": null, "bindings": {"open-file": "Ctrl+Shift+O"}}"#;
        let f: KeymapFile = serde_json::from_str(text).unwrap();
        let (map, problems) = Keymap::load(&f, &[]);
        assert_eq!(
            problems,
            [
                Problem::UnknownKey("close-tab".into()),
                Problem::UnknownKey("new-tab".into()),
            ]
        );
        // The bindings it does read still apply…
        assert_eq!(map.chord(Command::OpenFile), Some(chord("Ctrl+Shift+O")));
        // …and a save writes the unread keys back as they were.
        let out = map.to_file();
        assert_eq!(out, f);
        let back: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&out).unwrap()).unwrap();
        assert_eq!(back["new-tab"], "Ctrl+N");
        assert_eq!(back["close-tab"], serde_json::Value::Null);
    }

    #[test]
    fn a_kept_entry_leaves_the_file_once_the_user_sets_that_commands_key() {
        let f = file(&[("open-file", Some("Ctrl+Banana"))]);
        let chosen = chord("Ctrl+Shift+O");
        // Assign then Reset: the command is back on its default.
        let (mut map, _) = Keymap::load(&f, &[]);
        assert!(map.assign(Command::OpenFile, chosen, &[]));
        assert!(map.reset(Command::OpenFile, &[]));
        assert_eq!(map.to_file(), KeymapFile::default());
        // Clear then Reset.
        let (mut map, _) = Keymap::load(&f, &[]);
        map.clear(Command::OpenFile);
        assert!(map.reset(Command::OpenFile, &[]));
        assert_eq!(map.to_file(), KeymapFile::default());
        // Giving the command the key it already runs on says so too.
        let (mut map, _) = Keymap::load(&f, &[]);
        assert!(map.assign(Command::OpenFile, chord("Ctrl+O"), &[]));
        assert_eq!(map.to_file(), KeymapFile::default());
        // A command whose key another assignment takes has been given an
        // answer too: no key, written as null.
        let (mut map, _) = Keymap::load(&f, &[]);
        assert!(map.assign(Command::NewTab, chord("Ctrl+O"), &[]));
        assert_eq!(
            map.to_file(),
            file(&[("new-tab", Some("Ctrl+O")), ("open-file", None)])
        );
    }

    #[test]
    fn a_file_entry_that_loses_a_conflict_is_kept_in_the_file() {
        let f = file(&[("new-tab", Some("Ctrl+N")), ("close-tab", Some("Ctrl+N"))]);
        let (map, _) = Keymap::load(&f, &[]);
        assert_eq!(map.chord(Command::CloseTab), None);
        assert_eq!(map.to_file(), f);
    }

    #[test]
    fn what_the_file_names_wins_over_a_default_and_the_loss_is_reported() {
        let (map, problems) = Keymap::load(&file(&[("new-tab", Some("Ctrl+P"))]), &[]);
        assert_eq!(map.chord(Command::NewTab), Some(chord("Ctrl+P")));
        assert_eq!(map.chord(Command::FindAnywhere), None);
        assert_eq!(
            problems,
            vec![Problem::Conflict {
                command: Command::FindAnywhere,
                chord: chord("Ctrl+P"),
                holder: Command::NewTab,
            }]
        );
    }

    #[test]
    fn between_two_entries_of_the_file_the_first_command_keeps_the_key() {
        let (map, problems) = Keymap::load(
            &file(&[("new-tab", Some("Ctrl+N")), ("close-tab", Some("Ctrl+N"))]),
            &[],
        );
        assert_eq!(map.chord(Command::NewTab), Some(chord("Ctrl+N")));
        assert_eq!(map.chord(Command::CloseTab), None);
        assert_eq!(
            problems,
            vec![Problem::Conflict {
                command: Command::CloseTab,
                chord: chord("Ctrl+N"),
                holder: Command::NewTab,
            }]
        );
    }

    #[test]
    fn a_cleared_default_frees_its_key_for_another_entry() {
        let (map, problems) = Keymap::load(
            &file(&[("find-anywhere", None), ("new-tab", Some("Ctrl+P"))]),
            &[],
        );
        assert_eq!(problems, vec![]);
        assert_eq!(map.chord(Command::NewTab), Some(chord("Ctrl+P")));
    }

    #[test]
    fn changes_count_the_commands_bound_differently() {
        let mine = Keymap::defaults();
        assert_eq!(mine.changes_from(&Keymap::defaults()), 0);
        let mut theirs = Keymap::defaults();
        // Taking Ctrl+P moves two commands: New tab gains it, Find anywhere
        // loses it.
        theirs.assign(Command::NewTab, chord("Ctrl+P"), &[]);
        theirs.clear(Command::SaveFile);
        assert_eq!(mine.changes_from(&theirs), 3);
        assert_eq!(theirs.changes_from(&mine), 3);
    }

    #[test]
    fn the_same_key_in_two_surfaces_is_no_conflict() {
        // Go to line is the editor's; a grid-only fixed Ctrl+L does not touch it.
        let fixed = [fixed(Context::ResultsGrid, "Ctrl+L")];
        let (map, problems) = Keymap::load(&file(&[("go-to-line", Some("Ctrl+L"))]), &fixed);
        assert_eq!(problems, vec![]);
        assert_eq!(map.chord(Command::GoToLine), Some(chord("Ctrl+L")));
    }
}
