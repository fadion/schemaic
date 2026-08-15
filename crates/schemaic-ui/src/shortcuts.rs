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
//! `Key::Character` spread across four files, and rewriting them to dispatch
//! through a table would be a far larger change than the problem warrants. So the
//! guarantee is enforced from the other side: the tests below scan those files
//! for the four idioms the codebase actually uses to bind a **Ctrl/Alt + letter**
//! key, and fail when one isn't named here.
//!
//! That scope is the point. Modified-letter keys are where every undiscoverable
//! binding lives, because they are the ones with no menu item, no button and no
//! hover text. Plain keys (arrows, Enter, Escape, Delete) are deliberately *not*
//! gated: they are bound in dozens of places for ordinary navigation, so gating
//! them would be all noise, and a user guesses them anyway.
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
            ("Ctrl+Tab", "Cycle tabs (Shift = reverse)"),
            ("Ctrl+1…9", "Jump to tab"),
            ("Ctrl+Shift+T", "Reopen last closed tab"),
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
            ("Ctrl+A", "Select all"),
            ("Ctrl+Home / Ctrl+End", "First / last cell"),
            ("Enter", "Edit cell / open value"),
            ("Tab / Shift+Tab", "Next / previous cell while editing"),
            ("Ctrl+Enter", "Commit edits"),
            ("Del", "Mark row for deletion"),
        ],
    ),
    (
        "Schema tree",
        &[("Enter", "Open the selected table, column or object")],
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

#[cfg(test)]
mod tests {
    use super::SHORTCUTS;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    /// The files that bind keys. Not every module with a `KeyDown` listener —
    /// only the ones that bind a *modified letter*, which is what this gate is
    /// about. A new file of that kind belongs on this list.
    const KEY_FILES: &[&str] = &[
        "editor_pane.rs",
        "grid.rs",
        "lib.rs",
        "ai_panel.rs",
        "schema_tree.rs",
        "overlays.rs",
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

    /// Every single letter bound with Ctrl or Alt in `src`, by the four idioms the
    /// codebase actually uses:
    ///
    /// - `"x" | "X"` — the case pair, covering both `matches!(s.as_str(), …)` and
    ///   a plain `match` arm (the grid, the root, the terminal's copy/paste)
    /// - `eq_ignore_ascii_case("x")` — the editor's style
    /// - `Some("x") =>` — `NavKeys`' match arms, requiring the `=>` so this
    ///   doesn't match every `Some("…")` in the crate
    /// - `KeyCode::KeyX` — a *physical* key, which is how Ctrl+Alt+L has to be
    ///   matched (Windows delivers Ctrl+Alt as AltGr, so the logical character
    ///   isn't "l")
    ///
    /// Anything spelled a fifth way slips through, which is the weakness this
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

    /// Does the table name this letter anywhere, as a modified key?
    fn documented(letter: char) -> bool {
        let upper = letter.to_ascii_uppercase();
        SHORTCUTS.iter().any(|(_, rows)| {
            rows.iter().any(|(keys, _)| {
                keys.split(['+', ' ', '/'])
                    .any(|part| part.len() == 1 && part.starts_with(upper))
            })
        })
    }

    /// A binding with no row is a feature nobody can find. This is the whole
    /// point of the module.
    #[test]
    fn every_bound_letter_is_documented() {
        let mut found = BTreeSet::new();
        for name in KEY_FILES {
            let path = src_dir().join(name);
            let src = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            found.extend(bound_letters(&src));
        }
        assert!(
            !found.is_empty(),
            "found no bound letters at all — the scan idioms have changed and \
             this gate is now vacuous, which is worse than it failing"
        );

        let missing: Vec<String> = found
            .iter()
            .filter(|c| !EXEMPT.iter().any(|(e, _)| e.starts_with(**c)))
            .filter(|c| !documented(**c))
            .map(|c| format!("Ctrl/Alt+{}", c.to_ascii_uppercase()))
            .collect();

        assert!(
            missing.is_empty(),
            "these keys are bound in the handlers but appear in no SHORTCUTS row, \
             so nothing in the app reveals them: {}. Add the row in the same \
             change as the binding, or add the letter to EXEMPT with its reason.",
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
        for name in KEY_FILES {
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
