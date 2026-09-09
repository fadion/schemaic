//! Guard: every source module is named somewhere in `docs/architecture.md`.
//!
//! That file is this project's architecture document — every contributor and
//! every AI session reads it as the map. A review found 19 modules missing from
//! it, ~12% of the codebase, including the whole ER-diagram subsystem: an
//! undocumented module is where the invariants quietly stop applying, which is
//! exactly what happened to `core/filter.rs` (it builds SQL that reaches the
//! server, and the quoting invariants never mentioned it).
//!
//! A basename match is a deliberately weak test — it says a module was *thought
//! about*, not that the entry is accurate. It only has to catch the one failure
//! that keeps recurring: a new module added and never written down.
//!
//! This is the one test in the workspace that reads the filesystem, because the
//! thing under test *is* a file.

use std::fs;
use std::path::{Path, PathBuf};

/// Modules whose name carries no information in a map organised by crate, plus
/// build scripts (not modules at all).
const EXEMPT: &[&str] = &["lib.rs", "mod.rs", "main.rs", "build.rs"];

/// Is `needle` in `hay` as a **whole name** rather than as the tail of a longer
/// one?
///
/// **A bare `doc.contains` could not fail for twelve of the modules this test
/// governs.** A basename that is a suffix of another module's basename was
/// satisfied by that other module's entry, and the other module is itself
/// guaranteed to be named — so deleting every mention of `core/date.rs` left the
/// guard green on `update.rs`'s entry. Seven such pairs existed:
/// `date.rs` ⊂ `update.rs`, `edit.rs` ⊂ `celledit.rs`/`snippet_edit.rs`,
/// `diff.rs` ⊂ `inline_diff.rs`, `script.rs` ⊂ `transcript.rs`,
/// `import.rs` ⊂ `conn_import.rs`, `export.rs` ⊂ `erd_export.rs`,
/// `history.rs` ⊂ `search_history.rs`. The one failure this test says it exists
/// to catch was the one it could not catch.
///
/// A left boundary is the whole fix for those: `/`, a letter, a digit and `_`
/// all disqualify a match, so `up|date.rs` and `conn_|import.rs` no longer
/// answer. The right side needs nothing — `.rs` ends the name — beyond refusing
/// a longer extension.
fn names_module(hay: &str, needle: &str) -> bool {
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'/';
    let bytes = hay.as_bytes();
    hay.match_indices(needle).any(|(at, _)| {
        let left_ok = at == 0 || !word(bytes[at - 1]);
        let after = at + needle.len();
        let right_ok = bytes.get(after).is_none_or(|b| !b.is_ascii_alphanumeric());
        left_ok && right_ok
    })
}

/// Is the module `name` of crate `short` named in the doc?
///
/// **Crate-qualified when the basename is ambiguous.** The doc writes both forms
/// — `core/stats.rs` and a bare `contrast.rs` — and a bare key means one entry
/// answers for two files: `dump.rs`, `script.rs`, `secrets.rs`, `update.rs` and
/// `window_chrome.rs` each exist in two crates, so documenting
/// `app/secrets.rs` covered `core/secrets.rs` for free, while the assertion
/// message told the reader to add them *per crate*, which the check could not
/// verify.
///
/// So a basename that exists in only one crate may be named bare; one that
/// exists in two must be named with its crate. That is the weakest rule that
/// cannot answer for the wrong file, and it leaves the doc's existing prose
/// alone wherever a bare mention is unambiguous.
fn covered(doc: &str, short: &str, name: &str, ambiguous: bool) -> bool {
    names_module(doc, &format!("{short}/{name}")) || (!ambiguous && names_module(doc, name))
}

/// The crate's short name as the doc writes it — `schemaic-core` → `core`.
fn crate_short(dir: &Path) -> String {
    dir.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.trim_start_matches("schemaic-").to_string())
        .unwrap_or_default()
}

fn repo_root() -> PathBuf {
    // `crates/schemaic-core` → the workspace root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("workspace root")
}

/// Every `.rs` file under `dir`, recursively.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn every_module_is_named_in_the_architecture_doc() {
    let root = repo_root();
    let Ok(doc) = fs::read_to_string(root.join("docs").join("architecture.md")) else {
        // Checked out without the doc (or built from a package) — nothing to
        // guard, and failing here would be about the checkout, not the code.
        return;
    };

    // `src/` only — a test file documents itself, and the map is about the
    // shipped modules. `(crate short name, basename)` rather than the basename
    // alone: the doc is organised by crate and two crates can hold one name.
    let mut modules: Vec<(String, String)> = Vec::new();
    for crate_dir in fs::read_dir(root.join("crates"))
        .expect("crates/")
        .flatten()
    {
        let short = crate_short(&crate_dir.path());
        let mut files = Vec::new();
        sources(&crate_dir.path().join("src"), &mut files);
        for p in &files {
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if EXEMPT.contains(&name) {
                continue;
            }
            modules.push((short.clone(), name.to_string()));
        }
    }
    assert!(!modules.is_empty(), "found no sources to check under crates/");

    // Which basenames exist in more than one crate — the ones a bare mention
    // cannot answer for.
    let ambiguous = |name: &str| {
        modules
            .iter()
            .filter(|(_, n)| n == name)
            .map(|(c, _)| c)
            .collect::<std::collections::HashSet<_>>()
            .len()
            > 1
    };

    let mut missing: Vec<String> = modules
        .iter()
        .filter(|(short, name)| !covered(&doc, short, name, ambiguous(name)))
        .map(|(short, name)| format!("{short}/{name}"))
        .collect();
    missing.sort_unstable();
    missing.dedup();

    assert!(
        missing.is_empty(),
        "these modules are not named anywhere in docs/architecture.md — add them \
         to its Crates section at the same altitude as their peers: {}",
        missing.join(", ")
    );
}

/// **The check's own decision, asserted without the filesystem.** The guard
/// above reads two real files, so it can only be run against the tree as it
/// stands; this is what says the rule is the right one, and it is red against
/// the `doc.contains` it replaced.
#[test]
fn a_module_is_not_covered_by_a_longer_name_that_ends_with_it() {
    // The seven suffix pairs that existed, in the shape the doc writes them.
    for (name, other) in [
        ("date.rs", "core/update.rs"),
        ("edit.rs", "ui/celledit.rs"),
        ("edit.rs", "ui/snippet_edit.rs"),
        ("diff.rs", "ui/inline_diff.rs"),
        ("script.rs", "core/transcript.rs"),
        ("import.rs", "core/conn_import.rs"),
        ("export.rs", "core/erd_export.rs"),
        ("history.rs", "core/search_history.rs"),
    ] {
        let doc = format!("- `{other}` — something else entirely.");
        assert!(
            !covered(&doc, "core", name, false),
            "{name} was covered by {other}"
        );
    }
    // And its own entry does cover it, in either spelling.
    assert!(covered("- `core/date.rs` — dates.", "core", "date.rs", false));
    assert!(covered("- `date.rs` — dates.", "core", "date.rs", false));

    // An ambiguous basename needs its crate. `secrets.rs` exists in
    // `schemaic-app` and `schemaic-core`, so one entry answered for both.
    let app_only = "- `app/secrets.rs` — the OS keyring.";
    assert!(covered(app_only, "app", "secrets.rs", true));
    assert!(
        !covered(app_only, "core", "secrets.rs", true),
        "app/secrets.rs must not answer for core/secrets.rs"
    );

    // A crate name is not a word boundary either way round.
    assert!(!covered("- `xcore/date.rs`", "core", "date.rs", true));
    assert!(!names_module("`update.rs`", "date.rs"));
    assert!(!names_module("`date.rst`", "date.rs"));
    assert!(names_module("(`date.rs`)", "date.rs"));
    assert!(names_module("date.rs at the very start", "date.rs"));
}
