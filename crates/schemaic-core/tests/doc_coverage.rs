//! Guard: every source module is named somewhere in `CLAUDE.md`.
//!
//! CLAUDE.md is this project's architecture document *and* its agent-instruction
//! file — every contributor and every AI session reads it as the map. A review
//! found 19 modules missing from it, ~12% of the codebase, including the whole
//! ER-diagram subsystem: an undocumented module is where the invariants quietly
//! stop applying, which is exactly what happened to `core/filter.rs` (it builds
//! SQL that reaches the server, and the quoting invariants never mentioned it).
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
fn every_module_is_named_in_claude_md() {
    let root = repo_root();
    let Ok(doc) = fs::read_to_string(root.join("CLAUDE.md")) else {
        // Checked out without the doc (or built from a package) — nothing to
        // guard, and failing here would be about the checkout, not the code.
        return;
    };

    // `src/` only — a test file documents itself, and the map is about the
    // shipped modules.
    let mut files = Vec::new();
    for crate_dir in fs::read_dir(root.join("crates"))
        .expect("crates/")
        .flatten()
    {
        sources(&crate_dir.path().join("src"), &mut files);
    }
    assert!(!files.is_empty(), "found no sources to check under crates/");

    let mut missing: Vec<String> = files
        .iter()
        .filter_map(|p| p.file_name()?.to_str())
        .filter(|name| !EXEMPT.contains(name))
        .filter(|name| !doc.contains(*name))
        .map(str::to_string)
        .collect();
    missing.sort_unstable();
    missing.dedup();

    assert!(
        missing.is_empty(),
        "these modules are not named anywhere in CLAUDE.md — add them to the \
         Crates section at the same altitude as their peers: {}",
        missing.join(", ")
    );
}
