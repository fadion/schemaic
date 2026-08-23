//! The app's keyboard shortcuts, as **one table** — and the test that keeps it
//! honest.
//!
//! This list is the app's only keyboard documentation: the Shortcuts modal
//! ([`crate::settings::help_overlay`]) renders straight from [`SHORTCUTS`], and
//! for a binding like Ctrl+H or Ctrl+G there is no other affordance anywhere in
//! the UI. A binding missing here is a feature nobody can find.
//!
//! It used to be a literal inside the modal, hand-maintained against `match` arms
//! in three files, and it drifted exactly as its own comment predicted it would:
//! Alt+↑/↓ (move line up/down), Ctrl+↑/↓ (recall an AI prompt), Ctrl+Shift+C/V
//! (terminal copy/paste) and Ctrl+Home/End (first/last cell) were all implemented
//! and documented nowhere.
//!
//! # What the test does, and what it deliberately doesn't
//!
//! The handlers can't render *from* this table — they are `match` arms on
//! `Key::Character` spread across several files, and rewriting them to dispatch
//! through a table would be a far larger change than the problem warrants. So the
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

/// One group of the Shortcuts modal: a heading and its `(keys, description)` rows.
pub(crate) type ShortcutGroup = (&'static str, &'static [(&'static str, &'static str)]);

/// Every shortcut the modal shows, in display order.
///
/// **What earns a row:** every Ctrl/Alt-modified binding, plus the plain keys that
/// do something you would not guess — Delete marks a row for deletion, Enter
/// edits a cell, Tab hops between cells. Plain arrow keys and "Escape closes this"
/// are left out on purpose: they are what everyone already tries, and listing them
/// would bury the handful of bindings that genuinely cannot be discovered.
///
/// Escape is also not uniformly true, which is a second reason not to claim it
/// here: `tx_prompt_overlay` swallows it deliberately, because there is no safe
/// "never mind" for uncommitted writes.
pub(crate) const SHORTCUTS: &[ShortcutGroup] = &[
    (
        "Global",
        &[
            ("Ctrl+P", "Find Anywhere"),
            ("Ctrl+Shift+P", "Command palette"),
            ("Ctrl+T", "New query tab"),
            ("Ctrl+W", "Close query tab"),
            // Split rather than "Ctrl+Tab (Shift = reverse)": the palette shows a
            // command's own keys, and Previous Tab needs a row of its own to name.
            ("Ctrl+Tab", "Next tab"),
            ("Ctrl+Shift+Tab", "Previous tab"),
            ("Ctrl+1…9", "Jump to tab"),
            ("Ctrl+Shift+T", "Reopen last closed tab"),
            ("Ctrl+O", "Open SQL file"),
            ("Ctrl+S", "Save SQL file"),
            ("Ctrl+Shift+S", "Save SQL file as"),
            ("Ctrl+Shift+E", "Toggle schema panel"),
            ("Ctrl+Shift+A", "Toggle AI panel"),
            ("Ctrl+`", "Toggle terminal"),
        ],
    ),
    (
        "Editor",
        &[
            ("Ctrl+Enter", "Run query"),
            ("Ctrl+Space", "Autocomplete"),
            ("Ctrl+K", "Inline AI edit"),
            ("Ctrl+F", "Find in editor"),
            // Ctrl+F collapses the replace row, so nothing in the UI reveals
            // Ctrl+H — this modal is its only affordance.
            ("Ctrl+H", "Find and replace"),
            ("Ctrl+G", "Go to line"),
            ("Ctrl+/", "Toggle line comment"),
            ("Ctrl+D", "Duplicate line / selection"),
            ("Ctrl+X", "Delete line"),
            ("Ctrl+Alt+L", "Format SQL"),
            ("Alt+↑ / Alt+↓", "Move line up / down"),
            ("Tab / Shift+Tab", "Indent / outdent"),
        ],
    ),
    (
        "Results grid",
        &[
            ("Ctrl+F", "Find in results"),
            // Ctrl+G in the grid has no affordance anywhere else either.
            ("Ctrl+G", "Go to row"),
            ("Ctrl+C", "Copy"),
            ("Ctrl+V", "Paste as staged edits"),
            ("Ctrl+A", "Select all"),
            ("Ctrl+Home / Ctrl+End", "First / last cell"),
            // "Edit cell" only. Enter on a **read-only** cell does nothing —
            // viewing one is the right-click menu's View — and a row promising
            // otherwise teaches a key that isn't there.
            ("Enter", "Edit cell"),
            ("Tab / Shift+Tab", "Next / previous cell while editing"),
            ("Ctrl+Enter", "Commit edits"),
            ("Del", "Mark row for deletion"),
            // The toolbar strip is the app's one ring outside an overlay, and F6
            // is its only affordance — nothing on screen says the icons can be
            // reached at all.
            ("F6", "Go to the results toolbar"),
            ("← / →", "Move along the toolbar (Esc returns to the grid)"),
        ],
    ),
    (
        "Schema tree",
        &[
            ("Enter", "Open the selected table, column or object"),
            // The same menu the right-click builds, opened at the row rather
            // than at the pointer.
            ("Shift+F10", "Context menu for the selected row"),
        ],
    ),
    (
        "AI panel",
        &[
            ("Enter", "Send message"),
            ("Shift+Enter", "New line"),
            ("Ctrl+↑ / Ctrl+↓", "Recall previous / next prompt"),
        ],
    ),
    (
        "Terminal",
        &[("Ctrl+Shift+C / Ctrl+Shift+V", "Copy / paste")],
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
/// The keys string must be *byte-identical* to a [`SHORTCUTS`] row, which
/// `tests::every_command_key_is_a_real_shortcut` enforces (a `#[cfg(test)]`
/// item, so not linkable), and the palette can therefore never advertise a
/// binding the modal doesn't document or the app doesn't have.
///
/// **Each entry names its row's group**, not just the key string. The table has
/// two `Ctrl+G` rows meaning different things — the editor's Go to Line and the
/// grid's Go to Row — so matching on the string alone let *either* vouch for the
/// palette's keycap: deleting the Editor row would have left the palette
/// advertising `Ctrl+G` for "Go to Line" while the surviving row means the
/// grid's row jump. A nearly-right keycap is worse than none, because it teaches
/// a key that does something else.
pub(crate) const COMMAND_KEYS: &[(&str, &str, &str)] = &[
    ("new tab", "Global", "Ctrl+T"),
    ("close tab", "Global", "Ctrl+W"),
    ("open file", "Global", "Ctrl+O"),
    ("save file", "Global", "Ctrl+S"),
    ("save file as", "Global", "Ctrl+Shift+S"),
    ("next tab", "Global", "Ctrl+Tab"),
    ("previous tab", "Global", "Ctrl+Shift+Tab"),
    ("format code", "Editor", "Ctrl+Alt+L"),
    ("go to line", "Editor", "Ctrl+G"),
];

/// The keys to show on a palette row for the command called `name`.
pub(crate) fn command_keys(name: &str) -> Option<&'static str> {
    COMMAND_KEYS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, _, k)| *k)
}

#[cfg(test)]
mod tests {
    use super::{COMMAND_KEYS, SHORTCUTS, command_keys};
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
    /// window root *and* the terminal panel, and the editor re-binds the global
    /// navigation keys because it consumes every KeyDown itself. When a binding
    /// legitimately belongs to a group not listed here, add the group; the
    /// failure message says so.
    const KEY_FILES: &[(&str, &[&str])] = &[
        ("editor_pane.rs", &["Editor", "Global"]),
        ("grid.rs", &["Results grid", "Global"]),
        ("lib.rs", &["Global", "Terminal"]),
        ("ai_panel.rs", &["Global"]),
        ("schema_tree.rs", &["Schema tree", "Global"]),
        ("overlays.rs", &["Global"]),
    ];

    /// Letters bound to something that is not a user-facing shortcut, each with
    /// the reason it is not in the modal. A letter leaves this list the moment it
    /// becomes something a user would look up.
    ///
    /// Kept as a baseline in the spirit of `contrast::UI_SHORTFALL`: an unlisted
    /// letter must be documented, and a listed one carries its justification.
    /// Empty today — every modified letter the app binds is in the table — and an
    /// empty list is the healthy state, not a sign the mechanism is unused.
    const EXEMPT: &[(&str, &str)] = &[];

    fn src_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
    }

    /// The file with its `#[cfg(test)]` module cut off.
    ///
    /// Not a nicety: the first version of this scan read the test modules too and
    /// reported `Ctrl+B` as an undocumented binding, which was
    /// `Some("b".to_string())` in a grid fixture. Test data is full of one-letter
    /// strings, so scanning it makes the gate cry wolf — and a gate that cries
    /// wolf gets deleted. Every file here keeps its tests at the bottom.
    fn production_code(src: &str) -> &str {
        match src.find("#[cfg(test)]") {
            Some(i) => &src[..i],
            None => src,
        }
    }

    /// Every single letter bound with Ctrl or Alt in `src`, by the five idioms
    /// the codebase actually uses:
    ///
    /// - `"x" | "X"` — the case pair, covering both `matches!(s.as_str(), …)` and
    ///   a plain `match` arm (the grid, the root, the terminal's copy/paste)
    /// - `eq_ignore_ascii_case("x")` — the editor's style
    /// - `Some("x") =>` — `NavKeys`' match arms, requiring the `=>` so this
    ///   doesn't match every `Some("…")` in the crate
    /// - `ch == Some("x")` — the *same* function's shifted arms, spelled as an
    ///   equality rather than a pattern. This one was missing, and it is where
    ///   **both** of the app's Ctrl+Shift+letter bindings live: the gate was
    ///   green only because `p` and `t` happen to be bound a second time in the
    ///   unshifted arms below, so adding a third Ctrl+Shift+letter in the
    ///   file's own established style would have needed no `SHORTCUTS` row
    /// - `KeyCode::KeyX` — a *physical* key, which is how Ctrl+Alt+L has to be
    ///   matched (Windows delivers Ctrl+Alt as AltGr, so the logical character
    ///   isn't "l")
    ///
    /// Anything spelled a sixth way slips through, which is the weakness this
    /// test accepts: it exists to catch the binding nobody wrote down, not to
    /// prove the absence of one.
    fn bound_letters(src: &str) -> BTreeSet<char> {
        let src = production_code(src);
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
        // `== Some("x")` — the equality form, which is how the shifted arms of
        // `NavKeys::handle` are written. The `==` is what distinguishes it from
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
    fn documented_in(letter: char, groups: &[&str]) -> bool {
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
                    .filter(|c| !EXEMPT.iter().any(|(e, _)| e.starts_with(**c)))
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
            "the equality form, where both Ctrl+Shift+letter bindings live"
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
                        stale.push(format!("{group} / {keys} ({desc})"));
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
        assert!(documented_in('d', &["Editor"]));
        assert!(
            !documented_in('d', &["Results grid"]),
            "the grid has no Ctrl+D row, so a grid binding must fail the gate"
        );
        // And the letters that legitimately appear in two groups still do.
        assert!(documented_in('f', &["Editor"]));
        assert!(documented_in('f', &["Results grid"]));
    }

    /// Every group a `KEY_FILES` row names has to exist, or a typo silently
    /// widens the gate to "documented nowhere" and every letter fails — or, if
    /// the file's other groups cover it, silently narrows to nothing.
    #[test]
    fn every_key_file_names_real_shortcut_groups() {
        for (file, groups) in KEY_FILES {
            for g in *groups {
                assert!(
                    SHORTCUTS.iter().any(|(name, _)| name == g),
                    "{file} names group {g:?}, which is not in SHORTCUTS"
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
        assert!(documented('l'), "Ctrl+Alt+L");
        assert!(documented('e'), "Ctrl+Shift+E");
        // The failure this spelling avoids: `Ctrl+Enter` must not make every
        // Ctrl+E binding look documented, nor `Del` vouch for Ctrl+D.
        assert!(
            !documented('j'),
            "nothing binds or documents J — if this ever passes, the match is too loose"
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

    /// The palette may only advertise a binding the modal documents, spelled
    /// identically. Without this the two lists drift into disagreeing about the
    /// same key, which is worse than either being merely incomplete.
    #[test]
    fn every_command_key_is_a_real_shortcut() {
        for (name, group, keys) in COMMAND_KEYS {
            // The row that *means* it, not any row with the same key string:
            // the table has two `Ctrl+G` rows for two different things.
            let found = SHORTCUTS
                .iter()
                .filter(|(g, _)| g == group)
                .any(|(_, rows)| rows.iter().any(|(k, _)| k == keys));
            assert!(
                found,
                "palette command {name:?} shows {keys:?} from group {group:?}, which is \
                 not a SHORTCUTS row there — add the row, fix the spelling to match one \
                 exactly, or name the group the binding really comes from"
            );
        }
    }

    /// Two rows can share a key string across groups (`Ctrl+G` is Go to Line in
    /// the editor and Go to Row in the grid), which is exactly why the check
    /// above names the group — this pins that the ambiguity is real.
    #[test]
    fn a_key_string_can_mean_two_things_in_two_groups() {
        let with_ctrl_g: Vec<&str> = SHORTCUTS
            .iter()
            .filter(|(_, rows)| rows.iter().any(|(k, _)| *k == "Ctrl+G"))
            .map(|(g, _)| *g)
            .collect();
        assert!(
            with_ctrl_g.len() > 1,
            "expected Ctrl+G in more than one group, got {with_ctrl_g:?}"
        );
    }

    #[test]
    fn command_keys_looks_up_by_name_and_misses_cleanly() {
        assert_eq!(command_keys("go to line"), Some("Ctrl+G"));
        assert_eq!(command_keys("previous tab"), Some("Ctrl+Shift+Tab"));
        // A command with no binding, and one that doesn't exist, both yield None
        // — the row simply shows no keys.
        assert_eq!(command_keys("duplicate tab"), None);
        assert_eq!(command_keys("nonexistent"), None);
        // Names are the palette's lowercased labels; a label-cased lookup is a
        // miss rather than a silent match on the wrong thing.
        assert_eq!(command_keys("Go to Line"), None);
    }

    /// A command name is what the parser matches and what Tab completes, so a
    /// duplicate here would make one of the two entries dead.
    #[test]
    fn command_keys_names_are_unique() {
        let mut seen = BTreeSet::new();
        for (name, _, _) in COMMAND_KEYS {
            assert!(seen.insert(*name), "duplicate COMMAND_KEYS entry {name:?}");
        }
    }

    #[test]
    fn the_table_is_not_accidentally_empty() {
        assert!(SHORTCUTS.len() >= 3);
        for (group, rows) in SHORTCUTS {
            assert!(!rows.is_empty(), "group {group:?} has no rows");
            for (keys, desc) in *rows {
                assert!(
                    !keys.is_empty() && !desc.is_empty(),
                    "blank row in {group:?}"
                );
            }
        }
    }
}
