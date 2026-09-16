//! Guard: **nothing joins `ChangeSet::emit()`'s statements into a script
//! outside `ddl::client_script`.**
//!
//! `client_script` is what terminates every statement and wraps a compound
//! MySQL body in `DELIMITER $$`. A builder that joins the statements itself
//! hands the reader a `CREATE TRIGGER … BEGIN SET NEW.a = 1; SET NEW.b = 2; END`
//! that the app's own splitter cuts at the internal semicolons — the ERROR 1064
//! fragment the wrapping exists to prevent. Only what the user is handed to
//! read is affected; the apply path sends each statement whole. That is what
//! makes it easy to reintroduce and hard to notice.
//!
//! **This is the third spelling of this guard, and the first that can see the
//! whole workspace.** `9fe049d` deleted the first extra builder and wrote the
//! rule down in prose; `d316d35` deleted the second and added a ratchet — which
//! read the production half of `ddl.rs` and `compare.rs` only, and matched the
//! single literal `emit().join(`. `schemaic-app/src/mcp.rs` was a live fifth
//! member the whole time, in a crate the ratchet's corpus did not include, and
//! the commit that added the ratchet declared the class closed at four.
//!
//! So the corpus is every crate's `src`, and the question is asked about the
//! *expression* rather than about one spelling of it: a `.emit()` whose
//! statement goes on to `.join(` is a builder. A floor guards the needle, so a
//! scan that stops matching cannot read as a clean tree.
//!
//! Reading the repository's own source is the sanctioned kind of filesystem
//! access here, for the same reason `doc_coverage.rs` gives: the thing under
//! test *is* a file.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
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
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// `src` with every `#[cfg(test)]` item blanked.
///
/// A local brace walk rather than `schemaic_ui::source_gate::production_code`:
/// this crate is below `schemaic-ui` in the dependency graph, and inverting that
/// for a test would be the wrong trade. It is the same rule — follow the
/// attribute to the `{` that opens its item and to the matching `}` — and it is
/// deliberately *conservative* about what it cuts: an item it cannot balance is
/// left in, so the failure is a false positive somebody reads rather than a
/// silent truncation.
fn production_code(src: &str) -> String {
    const ATTR: &str = "#[cfg(test)]";
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(at) = rest.find(ATTR) {
        out.push_str(&rest[..at]);
        let after = &rest[at + ATTR.len()..];
        let Some(open) = after.find('{') else {
            // No block: a `use` or a field. It ends at the next `;`.
            let end = after.find(';').map(|i| i + 1).unwrap_or(after.len());
            rest = &after[end..];
            continue;
        };
        let bytes = after.as_bytes();
        let mut depth = 0usize;
        let mut i = open;
        let mut end = None;
        while i < bytes.len() {
            match bytes[i] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i + 1);
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        match end {
            Some(e) => rest = &after[e..],
            // Unbalanced (a brace inside a string or comment): do not guess.
            None => rest = after,
        }
    }
    out.push_str(rest);
    out
}

#[test]
fn nothing_joins_the_emitted_statements_outside_client_script() {
    let root = repo_root();
    let mut files = Vec::new();
    for crate_dir in fs::read_dir(root.join("crates"))
        .expect("crates/")
        .flatten()
    {
        sources(&crate_dir.path().join("src"), &mut files);
    }
    assert!(
        files.len() >= 40,
        "the corpus collapsed: {} source files found",
        files.len()
    );

    // Assembled, or this file's own source is the hit and the gate passes on
    // itself.
    let needle = format!(".{}()", "emit");
    let mut seen = 0usize;
    let mut offenders: Vec<String> = Vec::new();
    for path in &files {
        let Ok(src) = fs::read_to_string(path) else {
            continue;
        };
        let code = production_code(&src);
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string();
        let mut from = 0usize;
        while let Some(rel_at) = code[from..].find(&needle) {
            let at = from + rel_at + needle.len();
            from = at;
            seen += 1;
            // The rest of this statement. A join into one script is inside the
            // same expression, so it lands before the `;`.
            let stmt = &code[at..];
            let end = stmt.find(';').unwrap_or(stmt.len()).min(400);
            if stmt[..end].contains(".join(") || stmt[..end].contains(".concat(") {
                let line = 1 + code[..at].bytes().filter(|c| *c == b'\n').count();
                offenders.push(format!("{rel}:{line}"));
            }
            from = from.max(at);
        }
    }
    assert!(
        seen >= 5,
        "the needle stopped matching: {seen} `.emit()` calls found across the \
         workspace's production code — a gate that scans nothing reports success"
    );
    assert!(
        offenders.is_empty(),
        "these join emitted statements without going through `ddl::client_script`, \
         which is what terminates every statement and wraps a compound MySQL body \
         in `DELIMITER $$`:\n{}",
        offenders.join("\n")
    );
}
