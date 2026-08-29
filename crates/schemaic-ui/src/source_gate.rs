//! The shared machinery behind this crate's **source gates** — the tests that
//! read the crate's own `.rs` files and fail on a spelling that must not appear
//! in production code (a floem `Dropdown`, a captured `Color`, a raw pixel inset,
//! an unguarded `exec_after`).
//!
//! Test-only: nothing here is compiled into the app.
//!
//! # Why this is one module and not eleven copies
//!
//! The idiom — "read the file, cut the tests off, scan what is left" — was
//! written out eleven times across nine files, five of them wrapped in a private
//! `production_code` and four byte-identical. Every one of them cut the file at
//! the **first** `#[cfg(test)]`, which is right only for a file whose tests are
//! all at the bottom. `widgets.rs` has an inline test-only `fn` at line 929, so
//! its gate read 929 of 7,259 lines and the entire replacement menu system it
//! exists to protect — 87% of the file — was never scanned at all. A planted
//! `Dropdown` at line 2000 passed.
//!
//! A copy each also meant a fix reached one of eleven. So the cut lives here,
//! once, and it is brace-aware rather than positional.

/// `src` with every `#[cfg(test)]` item removed, and every `//` comment line
/// dropped.
///
/// **Brace-aware, not positional.** Each `#[cfg(test)]` attribute is followed to
/// the `{` that opens the item it applies to and the matching `}` that closes it;
/// only that span goes. Anything after it is production code again — which is the
/// whole difference from cutting at the first occurrence.
///
/// Braces inside strings, chars and comments are skipped, or a test module
/// containing `"{"` would eat the rest of the file and hand back a gate that
/// scans nothing while reporting success. An item with no block of its own — a
/// `use`, a struct field, an enum variant, a match arm — ends at its `;` or `,`
/// instead; see [`item_end`].
pub(crate) fn production_code(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    while i < b.len() {
        let Some(rel) = src[i..].find("#[cfg(test)]") else {
            out.push_str(&src[i..]);
            break;
        };
        let at = i + rel;
        out.push_str(&src[i..at]);
        i = match item_end(src, at + "#[cfg(test)]".len()) {
            Some(end) => end,
            // Unbalanced: refuse to guess, and let the rest be scanned. A false
            // positive fails loudly; a silent truncation is what this exists to
            // stop.
            None => at + "#[cfg(test)]".len(),
        };
    }
    out.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The offset just past the item beginning at `from` — the byte after its
/// closing `}`, or after the `;` / `,` of an item with no block of its own.
///
/// **Not every `#[cfg(test)]` is on a block.** It can sit on a `use` (ends at
/// `;`), on a struct field, an enum variant or a match arm (ends at the `,`, or
/// at the enclosing `}` when it is the last one). Reading only `{`/`}` made the
/// last of those decrement past zero and panic with `attempt to subtract with
/// overflow` — taking all eleven gates, and so the whole suite, down with a
/// message naming neither the file nor the construct.
///
/// `(` and `[` are counted alongside `{` for one reason: without them a `,` at
/// "depth 0" would land in the middle of `fn f(a: u32, b: u32)` and hand the
/// body of a test-only function back as production code.
fn item_end(src: &str, from: usize) -> Option<usize> {
    let b = src.as_bytes();
    let mut i = from;
    let mut depth = 0usize;
    // A block item ends at the `}` that closes its own block; everything else
    // ends at a separator. Without this, `fn f()`'s `)` — which also returns the
    // depth to 0 — would end the item before its body.
    let mut saw_block = false;
    while i < b.len() {
        match b[i] {
            b'"' => i = skip_quoted(b, i, b'"'),
            b'\'' => i = skip_char_or_lifetime(b, i),
            b'/' if b.get(i + 1) == Some(&b'/') => {
                i = src[i..].find('\n').map_or(b.len(), |n| i + n + 1);
            }
            b'/' if b.get(i + 1) == Some(&b'*') => i = skip_block_comment(b, i),
            b'{' => {
                saw_block = true;
                depth += 1;
                i += 1;
            }
            b'(' | b'[' => {
                depth += 1;
                i += 1;
            }
            // A closer at depth 0 belongs to whatever *encloses* the item — the
            // struct, enum or match the attribute's item is the last member of.
            // The item ends here, and the closer is not ours to consume.
            b'}' | b')' | b']' if depth == 0 => return Some(i),
            b'}' => {
                depth -= 1;
                i += 1;
                if depth == 0 && saw_block {
                    return Some(i);
                }
            }
            b')' | b']' => {
                depth -= 1;
                i += 1;
            }
            b';' | b',' if depth == 0 => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

/// Past a `"…"` (or `r"…"`/`r#"…"#`) literal starting at `i`.
fn skip_quoted(b: &[u8], i: usize, q: u8) -> usize {
    // Raw strings: count the `#`s that opened it and look for the same close.
    let hashes = b[..i].iter().rev().take_while(|c| **c == b'#').count();
    if hashes > 0 && b[..i - hashes].last() == Some(&b'r') {
        let close = format!("\"{}", "#".repeat(hashes));
        let rest = &b[i + 1..];
        return match find_bytes(rest, close.as_bytes()) {
            Some(p) => i + 1 + p + close.len(),
            None => b.len(),
        };
    }
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            c if c == q => return j + 1,
            _ => j += 1,
        }
    }
    b.len()
}

/// Past a `'x'` literal — or past nothing at all, for a lifetime like `'a`,
/// which has no closing quote to find.
fn skip_char_or_lifetime(b: &[u8], i: usize) -> usize {
    let mut j = i + 1;
    if b.get(j) == Some(&b'\\') {
        j += 2;
    } else {
        j += 1;
    }
    if b.get(j) == Some(&b'\'') {
        j + 1
    } else {
        i + 1
    }
}

/// Past a `/* … */`, nesting as Rust's do.
fn skip_block_comment(b: &[u8], i: usize) -> usize {
    let mut j = i + 2;
    let mut depth = 1usize;
    while j + 1 < b.len() {
        if b[j] == b'/' && b[j + 1] == b'*' {
            depth += 1;
            j += 2;
        } else if b[j] == b'*' && b[j + 1] == b'/' {
            depth -= 1;
            j += 2;
            if depth == 0 {
                return j;
            }
        } else {
            j += 1;
        }
    }
    b.len()
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Every `.rs` file this crate's gates scan, as `(display name, production
/// code)`.
///
/// **Both crates that build views, not just this one.** The invariants these
/// gates enforce are stated app-wide — "every `<select>` in the app", "the
/// KeyDown listener is on the view the app's view function returned" — while the
/// scan walked `env!("CARGO_MANIFEST_DIR")/src`, which is `schemaic-ui` alone.
/// `schemaic-app` builds views too (`app_view`), so a violation added there
/// passed the whole suite.
pub(crate) fn crate_sources() -> Vec<(String, String)> {
    let ui = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let app = ui
        .parent()
        .and_then(|p| p.parent())
        .expect("the workspace's crates dir")
        .join("schemaic-app")
        .join("src");
    let mut out = Vec::new();
    for (label, dir) in [("", ui), ("schemaic-app/", app)] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            panic!("a gate's source directory is missing: {}", dir.display());
        };
        for entry in entries {
            let path = entry.expect("a dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let src = std::fs::read_to_string(&path).expect("a source file");
            out.push((format!("{label}{name}"), production_code(&src)));
        }
    }
    // The scan has to still be reading something: a moved `src` would pass every
    // gate by finding no files at all.
    assert!(out.len() > 20, "only {} source files scanned", out.len());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_after_an_inline_test_item_is_still_production_code() {
        // The failure this module exists for: `widgets.rs` has a test-only `fn`
        // at line 929, and cutting at the first `#[cfg(test)]` threw away the
        // 6,330 lines after it — the whole menu system the gate protects.
        let src = "fn a() {}\n\
                   #[cfg(test)]\n\
                   fn only_in_tests() { let x = 1; }\n\
                   fn b() { views::dropdown(); }\n";
        let code = production_code(src);
        assert!(code.contains("fn a()"), "{code}");
        assert!(code.contains("views::dropdown()"), "{code}");
        assert!(!code.contains("only_in_tests"), "{code}");
    }

    #[test]
    fn a_test_module_is_removed_whole() {
        let src = "fn a() {}\n\
                   #[cfg(test)]\n\
                   mod tests {\n    fn inner() { if true { } }\n}\n\
                   fn b() {}\n";
        let code = production_code(src);
        assert!(!code.contains("inner"), "{code}");
        assert!(code.contains("fn b()"), "{code}");
    }

    #[test]
    fn a_brace_inside_a_string_does_not_end_the_item() {
        // Without this, a test module containing `"{"` swallowed the rest of the
        // file and the gate reported success over nothing.
        let src = "#[cfg(test)]\nmod tests {\n    const S: &str = \"{\";\n}\nfn after() {}\n";
        let code = production_code(src);
        assert!(code.contains("fn after()"), "{code}");
        assert!(!code.contains("const S"), "{code}");
    }

    #[test]
    fn a_cfg_test_use_takes_only_its_own_line() {
        let src = "#[cfg(test)]\nuse std::fmt;\nfn after() {}\n";
        let code = production_code(src);
        assert!(code.contains("fn after()"), "{code}");
        assert!(!code.contains("std::fmt"), "{code}");
    }

    /// **An attribute on something that has no block of its own.** A struct
    /// field, an enum variant, a match arm: each ends at its `,`, or at the
    /// enclosing `}` when it is the last member. Reading only braces made the
    /// last case decrement past zero and panic — every gate, on every file, the
    /// moment anyone wrote one.
    #[test]
    fn an_attribute_on_a_blockless_item_ends_at_its_separator() {
        let cases = [
            (
                "struct S {\n    #[cfg(test)]\n    only_in_tests: u32,\n    kept: u32,\n}\n\
                 fn after() {}\n",
                "only_in_tests",
            ),
            (
                "enum E {\n    Kept,\n    #[cfg(test)]\n    OnlyInTests,\n}\nfn after() {}\n",
                "OnlyInTests",
            ),
            (
                "fn f() {\n    match x {\n        A => 1,\n        #[cfg(test)]\n        \
                 B => 2,\n    }\n}\nfn after() {}\n",
                "B => 2",
            ),
        ];
        for (src, gone) in cases {
            let code = production_code(src);
            assert!(!code.contains(gone), "{gone} survived: {code}");
            assert!(
                code.contains("fn after()"),
                "the rest of the file went with it: {code}"
            );
        }
        // The field's *neighbours* stay: an item that ends at its own separator
        // must not take the members after it.
        let code = production_code(
            "struct S {\n    #[cfg(test)]\n    only_in_tests: u32,\n    kept: u32,\n}\n",
        );
        assert!(code.contains("kept: u32"), "{code}");
    }

    /// A `,` only ends an item at depth 0 — otherwise the first parameter of a
    /// test-only `fn` would end it, and the body would come back as production
    /// code for every gate to scan.
    #[test]
    fn a_comma_inside_a_signature_does_not_end_a_test_only_function() {
        let src = "#[cfg(test)]\nfn helper(a: u32, b: u32) { views::dropdown(); }\nfn after() {}\n";
        let code = production_code(src);
        assert!(!code.contains("dropdown"), "{code}");
        assert!(!code.contains("helper"), "{code}");
        assert!(code.contains("fn after()"), "{code}");
    }

    #[test]
    fn comment_lines_are_dropped() {
        let src = "// views::dropdown in a comment\nfn a() {}\n";
        assert!(!production_code(src).contains("dropdown"));
    }

    #[test]
    fn the_scan_reaches_both_crates_that_build_views() {
        let files = crate_sources();
        assert!(
            files.iter().any(|(n, _)| n == "lib.rs"),
            "schemaic-ui is not being read"
        );
        assert!(
            files.iter().any(|(n, _)| n == "schemaic-app/main.rs"),
            "schemaic-app builds views too, and the invariants are stated app-wide"
        );
    }
}
