//! The shared machinery behind this crate's **source gates** — the tests that
//! read the crate's own `.rs` files and fail on a spelling that must not appear
//! in production code (a floem `Dropdown`, a captured `Color`, a raw pixel inset,
//! an unguarded `exec_after`).
//!
//! **Compiled into the library, and it has to be.** A `#[cfg(test)] mod` is
//! invisible to a *different* crate's tests, and `schemaic-app`'s gates need
//! the cut — so `production_code` and the byte scanners under it are
//! unconditional public API of `schemaic-ui`, rendered by `cargo doc` and held
//! to `RUSTDOCFLAGS=-D warnings`. What is test-only is the corpus walkers
//! (`crate_sources`, `workspace_sources`), which read the source tree at
//! paths derived from `CARGO_MANIFEST_DIR` and mean nothing at runtime.
//!
//! Nothing here is *called* from the app: it is a handful of pure string
//! functions that a linker with `--gc-sections` drops. That is a different
//! claim from "not compiled", and this file said the second one for a while
//! after it stopped being true.
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

/// `src` with every `#[cfg(test)]` item blanked, and every `//` comment line
/// blanked.
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
///
/// **Blanked, not deleted, so a line number means something.** This used to
/// remove the lines, and the result then had no relationship at all to the
/// file's own numbering — measured over the tree, `widgets.rs` went 7,881
/// source lines → 2,990, `lib.rs` 11,402 → 5,811, `shortcuts.rs` 727 → 113. Two
/// gates print positions from this text as if they were file positions, so a
/// violation at `widgets.rs:6500` was reported at roughly `widgets.rs:2500` and
/// the reader opened the wrong function; `popup_anchor_gate`'s `EXEMPT` array
/// is *keyed* on that number, so an exemption moved whenever an unrelated
/// comment was added anywhere above it — silently re-arming the gate on the
/// exempted site, or licensing a different one. Every gate scans with
/// `contains`/`find`, so the blank lines cost nothing and the numbers are right
/// by construction.
/// **`pub`, so the one walk is reachable from `schemaic-app`.** It was
/// `pub(crate)`, and the consequence was a twelfth private copy in `app/ai.rs`
/// carrying both defects this one exists to remove — a cut at the *first*
/// literal `#[cfg(test)]`, found by a bare `str::find` over the raw text. So
/// this is unconditional public API rather than test-only, which the module doc
/// states in full; the alternative to exporting it is another copy.
pub fn production_code(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    /// Keep the newlines of a removed span so the line count does not move.
    fn blank(out: &mut String, cut: &str) {
        for _ in cut.bytes().filter(|c| *c == b'\n') {
            out.push('\n');
        }
    }
    while i < b.len() {
        let Some(at) = next_cfg_test(src, i) else {
            out.push_str(&src[i..]);
            break;
        };
        out.push_str(&src[i..at]);
        let end = match item_end(src, at + "#[cfg(test)]".len()) {
            Some(end) => end,
            // Unbalanced: refuse to guess, and let the rest be scanned. A false
            // positive fails loudly; a silent truncation is what this exists to
            // stop.
            None => at + "#[cfg(test)]".len(),
        };
        blank(&mut out, &src[at..end]);
        i = end;
    }
    // `split('\n')` rather than `lines()`, so the round trip is exact: `lines()`
    // drops a trailing empty segment, and a file whose last line is inside a cut
    // test module ends on one — which cost the whole text a line and put every
    // number after it out by one again.
    let mut kept = String::with_capacity(out.len());
    for (n, line) in out.split('\n').enumerate() {
        if n > 0 {
            kept.push('\n');
        }
        if !line.trim_start().starts_with("//") {
            kept.push_str(line);
        }
    }
    kept
}

/// The offset of the next `#[cfg(test)]` reached **as code**, from `from`.
///
/// **The one scan in this module that was not comment-aware, and it was the one
/// that decides what gets cut.** It was a plain `str::find` over the raw text,
/// while [`item_end`] three lines below carefully skips strings, chars and both
/// kinds of comment for every byte it reads. So a `///` line *mentioning* the
/// attribute was treated as a real one, `item_end` ran from inside that comment,
/// skipped the rest of the comment correctly — and then consumed the next real
/// item. Four live sites at the time it was found, including `production_code`'s
/// own body and the whole of `shortcuts.rs`'s `COMMAND_KEYS` table, which is
/// what all fourteen gates scan: a `views::dropdown(` planted in that span
/// passed the gate that exists to refuse it.
///
/// A silent under-report, which is the direction that matters. The loud
/// direction was already handled — an unbalanced item makes `item_end` return
/// `None` and the rest is re-scanned.
fn next_cfg_test(src: &str, from: usize) -> Option<usize> {
    const ATTR: &str = "#[cfg(test)]";
    let b = src.as_bytes();
    let mut i = from;
    while i < b.len() {
        match b[i] {
            b'"' => {
                i = skip_quoted(b, i, b'"');
                continue;
            }
            b'\'' => {
                i = skip_char_or_lifetime(b, i);
                continue;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = skip_block_comment(b, i);
                continue;
            }
            // To the end of the line — `///`, `//!` and `//` alike. This is the
            // case that was live: the existing `//`-line filter runs *after* the
            // cut, so a mention was reachable before anything dropped it.
            b'/' if b.get(i + 1) == Some(&b'/') => {
                i = match find_bytes(&b[i..], b"\n") {
                    Some(p) => i + p + 1,
                    None => b.len(),
                };
                continue;
            }
            b'#' if src[i..].starts_with(ATTR) => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// The offset just past the item beginning at `from` — the byte after its
/// closing `}`, or after the `;` / `,` of an item with no block of its own.
///
/// `pub(crate)` because a gate can be about **one function** rather than a whole
/// file: `ddl_preview`'s asks that two named functions never reach for the live
/// connection switcher, while their neighbours in the same file legitimately
/// do. Pointed at a `fn`'s first byte it returns the byte after that function's
/// body — the parameter list's `(` is counted, so the `)` cannot end the item
/// before the block starts.
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
pub(crate) fn item_end(src: &str, from: usize) -> Option<usize> {
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
    //
    // **`r"…"` counts, and the `hashes > 0` test used to say it did not.** A raw
    // string with no hashes fell through to the escaping path below, where the
    // `\` of `r"a\"` consumed the closing quote — and the scan then ran on to
    // the next `"` anywhere in the file, swallowing every brace between. In
    // `item_end` that means the item never balances, `production_code` takes its
    // "refuse to guess" branch, and a whole test module comes back as production
    // code. Three files were in that state (`db/lib.rs`, `core/ddl.rs`,
    // `core/schema.rs`), silently, because the fallback is by design quiet; it
    // is `no_test_module_survives_the_cut` that says so now.
    //
    // A `"` directly after an `r` is a raw string and nothing else — Rust has no
    // identifier that may abut a literal — so the `r` is the test, and the hash
    // count only picks the terminator.
    let hashes = b[..i].iter().rev().take_while(|c| **c == b'#').count();
    if i > hashes && b[..i - hashes].last() == Some(&b'r') {
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
#[cfg(test)]
pub(crate) fn crate_sources() -> Vec<(String, String)> {
    sources_of(&["", "schemaic-app/"])
}

/// [`crate_sources`] plus `schemaic-core` and `schemaic-db`.
///
/// **For a gate whose rule is not about views.** "Ask a capability, never an
/// engine" is stated in CLAUDE.md about the whole workspace, and it names
/// `ddl::supports_change`, `supports_column_reorder`, `stats::supports_table_stats`
/// and `ref_schema_is_database` — every one of them in `schemaic-core`. A gate
/// over the two view crates therefore claimed a rule it could not reach: a
/// `dialect == SqlDialect::MySql` planted in `core/ddl.rs` passed the whole
/// suite, and a live instance was already in a review ledger
/// (`core/schema.rs`'s `is_bare_default`, since removed).
///
/// Separate from [`crate_sources`] rather than replacing it, because the
/// view-shaped gates — "every `<select>` in the app", "the KeyDown listener is on
/// the view the app's view function returned" — really are about the two crates
/// that build views, and widening their corpus would only add noise they have no
/// judgement for.
#[cfg(test)]
pub(crate) fn workspace_sources() -> Vec<(String, String)> {
    sources_of(&["", "schemaic-app/", "schemaic-core/", "schemaic-db/"])
}

/// The `.rs` files of the named crates, as `(display name, production code)`.
/// `""` is this crate; every other label is a sibling directory name with its
/// trailing slash, which is also the prefix each file is reported under.
#[cfg(test)]
fn sources_of(labels: &[&str]) -> Vec<(String, String)> {
    let ui = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let crates = ui
        .parent()
        .and_then(|p| p.parent())
        .expect("the workspace's crates dir")
        .to_path_buf();
    let dirs: Vec<(&str, std::path::PathBuf)> = labels
        .iter()
        .map(|label| {
            let dir = if label.is_empty() {
                ui.clone()
            } else {
                crates.join(label.trim_end_matches('/')).join("src")
            };
            (*label, dir)
        })
        .collect();
    let mut out = Vec::new();
    for (label, dir) in dirs {
        let before = out.len();
        collect_rs(&dir, label, "", &mut out);
        // **Per directory, not only in total.** A total floor is satisfied by
        // the largest crate alone, so a `src` that moved under any of the others
        // would leave every gate over it green by finding nothing. The smallest
        // crate here has six files.
        assert!(
            out.len() - before >= 5,
            "only {} source files scanned in {}",
            out.len() - before,
            dir.display()
        );
    }
    // The scan has to still be reading something: a moved `src` would pass every
    // gate by finding no files at all.
    //
    // **Near the real count, not a token floor.** This was `> 20` against 65
    // files, so two thirds of the corpus could vanish and every gate over it
    // would still report green — the same shape as the floors those gates
    // carry, one level up. The real landmark is
    // `the_scan_reaches_both_crates_that_build_views`, which `.expect`s
    // `lib.rs` and `schemaic-app/main.rs` by name; this is the blunt half.
    //
    // The floor counts the *view* crates only — 65 today — so that it stays the
    // same number whichever entry point called in, and because
    // [`workspace_sources`]' extra two crates carry their own per-directory
    // floor above.
    assert!(out.len() >= 60, "only {} source files scanned", out.len());
    out
}

/// Every `.rs` file under `dir`, at any depth, appended as
/// `(label + relative path, production code)`.
///
/// **Recursive, and that is the fix rather than a refinement.** This was one
/// non-recursive `read_dir` per crate, with a comment saying a future
/// `src/<subdir>/*.rs` "would fall outside every gate silently — this floor is
/// what would notice". It would not: the floor is a *lower* bound on the whole
/// corpus, so moving six files out of a flat directory into a subdirectory
/// leaves the count at 65 minus 6 and every gate over those six blind, with
/// nothing red. Both view crates are flat today; the point is that they no
/// longer have to be.
///
/// The relative path is joined with `/` on every platform, so a name a gate
/// reports or matches on reads the same on Windows and Linux.
#[cfg(test)]
fn collect_rs(dir: &std::path::Path, label: &str, rel: &str, out: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        panic!("a gate's source directory is missing: {}", dir.display());
    };
    let mut paths: Vec<std::path::PathBuf> =
        entries.map(|e| e.expect("a dir entry").path()).collect();
    // Sorted, so a gate that reports the first offender names the same file on
    // every machine — `read_dir` order is the filesystem's.
    paths.sort();
    for path in paths {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if path.is_dir() {
            collect_rs(&path, label, &format!("{rel}{name}/"), out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let src = std::fs::read_to_string(&path).expect("a source file");
            out.push((format!("{label}{rel}{name}"), production_code(&src)));
        }
    }
}

/// **A `\` line continuation typed as `\n`.**
///
/// Rust's `\`-before-newline strips the newline *and* the next line's
/// indentation; `\n` inserts a newline and keeps the indentation, so a sentence
/// meant to be one line arrives with a hard break and a run of twenty-odd
/// spaces in the middle of it. Three of them shipped, in three different files,
/// each byte-verified with `cat -A`:
///
/// * `app/main.rs` — the import's one warning that read-only and the
///   environment badge are not carried over, `\n` + 17 spaces.
/// * `ui/users_view.rs` — the grant-statement cap note, `\n` + 25 spaces.
/// * `ui/lib.rs` — the pinned-results memory tooltip, `\n` + 21 spaces.
///
/// None of the three is testable on its own: each is a literal inside a view,
/// and the damage is what a renderer does with it. **Three instances is the
/// case for a lint**, and this is it — one scan closing a class that would
/// otherwise be found one screenshot at a time.
///
/// Eight spaces is the threshold because a deliberate `\n` in prose is followed
/// by the next word, and Rust source that wraps a string literal is indented
/// past eight columns by the time it is nested in a view. The only matches in
/// the tree outside these two crates' production code are a CLI-help fixture, a
/// synthetic source fixture and two live-test SQL strings, all of which mean
/// their newline.
#[cfg(test)]
mod no_continuation_typed_as_newline_gate {
    #[test]
    fn a_wrapped_sentence_is_continued_not_broken() {
        let mut offenders: Vec<String> = Vec::new();
        for (name, code) in super::crate_sources() {
            for (i, line) in code.lines().enumerate() {
                let mut from = 0usize;
                while let Some(at) = line[from..].find("\\n") {
                    let at = from + at;
                    from = at + 2;
                    let spaces = line[from..].bytes().take_while(|b| *b == b' ').count();
                    if spaces >= 8 {
                        offenders.push(format!(
                            "{name}:{} has `\\n` followed by {spaces} spaces — a `\\` line \
                             continuation typed as `\\n`. `\\` strips the newline and the next \
                             line's indentation; `\\n` keeps both, so the sentence renders \
                             broken with a long indent in the middle of it. Write `\\` (or, if \
                             the break is deliberate, put the next line at column 0).",
                            i + 1
                        ));
                    }
                }
            }
        }
        assert!(offenders.is_empty(), "{}", offenders.join("\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The stripped text has the file's own line numbering**, so a gate that
    /// reports a position reports one the reader can open.
    ///
    /// It did not. Measured before the fix: `widgets.rs` 7,881 source lines →
    /// 2,990 stripped (0.38), `lib.rs` 11,402 → 5,811, `shortcuts.rs` 727 →
    /// 113. `no_floem_dropdown_gate` and `popup_anchor_gate` both print a
    /// number from this text as if it were a file position — off by a factor
    /// that varies per file — and `popup_anchor_gate`'s `EXEMPT` array is keyed
    /// on it, so an exemption moved whenever an unrelated comment or test item
    /// was added anywhere above it.
    ///
    /// Over the whole corpus rather than a fixture, because the property is
    /// about what the gates are actually handed.
    #[test]
    fn stripping_a_file_keeps_its_line_numbering() {
        let ui = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut checked = 0usize;
        for entry in std::fs::read_dir(&ui).expect("the crate's src") {
            let path = entry.expect("a dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("a source file");
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            assert_eq!(
                production_code(&src).lines().count(),
                src.lines().count(),
                "{name}: the stripped text is a different length from the file, \
                 so every line number a gate prints from it is wrong"
            );
            checked += 1;
        }
        assert!(checked > 20, "only {checked} files checked");
    }

    /// **A doc comment that *mentions* `#[cfg(test)]` deleted the item it
    /// documents from every gate's view.**
    ///
    /// The attribute was located with a plain `str::find` over the raw text —
    /// the one scan in this module that did not use the skip helpers `item_end`
    /// applies to every other byte. So an occurrence inside a `///` line was
    /// treated as a real attribute, `item_end` ran from inside the comment,
    /// skipped the remaining comment lines correctly, and then consumed **the
    /// next real item**. Measured over the frozen tree at four live sites:
    ///
    /// * `shortcuts.rs:160` — the whole `COMMAND_KEYS` table went.
    /// * `source_gate.rs:25` — `production_code`'s **own body** went, from its
    ///   own crate's production code.
    /// * `widgets.rs:3666` — `pub menus: MenuFlags,` went.
    /// * `contrast.rs:250` — a cut of no net effect.
    ///
    /// A silent under-report, and it is what all fourteen gates scan: a
    /// `views::dropdown(` planted inside `COMMAND_KEYS`' span passed
    /// `no_floem_dropdown_gate`, and an `inset_left(12.0)` there passed
    /// `float_inset_gate`.
    ///
    /// The synthetic case is the one that pins the rule; the two real files are
    /// what say the rule was being broken.
    #[test]
    fn an_attribute_named_in_a_comment_is_not_an_attribute() {
        let synthetic = "/// a #[cfg(test)] item, so not linkable\n\
                         const KEPT: u8 = 1;\n\
                         fn after() {}\n";
        let out = production_code(synthetic);
        assert!(out.contains("KEPT"), "{out}");
        assert!(out.contains("fn after"), "{out}");

        // `//!` and a plain `//` too, and inside a block comment.
        for src in [
            "//! a #[cfg(test)] mention\nconst KEPT: u8 = 1;\n",
            "// a #[cfg(test)] mention\nconst KEPT: u8 = 1;\n",
            "/* a #[cfg(test)] mention */\nconst KEPT: u8 = 1;\n",
            "const S: &str = \"#[cfg(test)]\";\nconst KEPT: u8 = 1;\n",
        ] {
            assert!(
                production_code(src).contains("KEPT"),
                "{src:?} → {:?}",
                production_code(src)
            );
        }

        // And the two real sites, which is what makes this a regression test
        // rather than a unit test of a helper.
        assert!(
            production_code(include_str!("shortcuts.rs")).contains("COMMAND_KEYS"),
            "shortcuts.rs lost its COMMAND_KEYS table"
        );
        assert!(
            production_code(include_str!("source_gate.rs"))
                .contains("String::with_capacity(src.len())"),
            "source_gate.rs deleted its own production_code body"
        );

        // The real attribute still cuts, which is the whole job.
        let real = "#[cfg(test)]\nmod tests { fn t() {} }\nfn after() {}\n";
        let out = production_code(real);
        assert!(!out.contains("fn t()"), "{out}");
        assert!(out.contains("fn after"), "{out}");
    }

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

    /// **Every gate's corpus is only as good as this cut**, so the cut is
    /// checked against the real files rather than only against hand-written
    /// fixtures.
    ///
    /// If `item_end` mis-reads one item, the rest of that file arrives as
    /// "production code" and every gate over it then scans its test modules —
    /// which does not fail loudly, it fails as noise: assertions about the app
    /// answered from its tests. The tell is a surviving `#[cfg(test)]`, and
    /// checking for one costs a scan of text already in memory.
    #[test]
    fn no_test_module_survives_the_cut() {
        let mut leaks: Vec<String> = Vec::new();
        for (name, code) in crate::source_gate::workspace_sources() {
            // Not `contains`: the string appears in this module's own fixtures
            // and prose, which the cut is not meant to remove.
            let n = code.matches("#[cfg(test)]").count() + code.matches("#[test]").count();
            if n > 0 && name != "source_gate.rs" {
                leaks.push(format!("{name}: {n}"));
            }
        }
        assert!(
            leaks.is_empty(),
            "`production_code` left a test module in place, so every gate over \
             these files is scanning their tests:\n{}",
            leaks.join("\n")
        );
    }

    /// **A raw string with no hashes, ending in a backslash.**
    ///
    /// `r"a\\"` is `a\\`: in a raw string the backslash escapes nothing. The
    /// scanner took it for one, consumed the closing quote, and ran on to the
    /// next `"` anywhere in the file — every brace in between counted, so the
    /// enclosing item never balanced and `production_code` handed a whole test
    /// module back as production code. It found three files in that state.
    #[test]
    fn a_raw_string_ending_in_a_backslash_ends_where_it_ends() {
        let src = concat!(
            "#[cfg(test)]\nmod t {\n",
            "    fn a() { assert_eq!(f(r\"a\\\"), 1); }\n",
            "}\n",
            "fn after() { views::dropdown(); }\n",
        );
        let code = production_code(src);
        assert!(!code.contains("assert_eq"), "the module survived:\n{code}");
        assert!(
            code.contains("fn after()"),
            "the cut ran past the module:\n{code}"
        );
        // …and the hashed forms still work, in both directions.
        for lit in [
            "r\"plain\"",
            "r#\"has \"quotes\"\"#",
            "\"ordinary \\\" escaped\"",
        ] {
            let src = format!(
                "#[cfg(test)]\nmod t {{\n    fn a() {{ g({lit}); }}\n}}\nfn after() {{}}\n"
            );
            let code = production_code(&src);
            assert!(code.contains("fn after()"), "{lit}:\n{code}");
            assert!(!code.contains("fn a()"), "{lit}:\n{code}");
        }
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

    /// **Nothing joins `ChangeSet::emit()`'s statements into a script outside
    /// `ddl::client_script`.**
    ///
    /// `client_script` terminates every statement and wraps a compound MySQL
    /// body in `DELIMITER $$`. A builder that joins them itself hands the reader
    /// a `CREATE TRIGGER … BEGIN SET NEW.a = 1; SET NEW.b = 2; END` that the
    /// app's own splitter cuts at the internal semicolons — the ERROR 1064
    /// fragment the wrapping exists to prevent. Only what the user is handed to
    /// read is affected; the apply path sends each statement whole, which is
    /// what makes it easy to reintroduce and hard to notice.
    ///
    /// **Third spelling of this guard, and the first that can see the whole
    /// workspace.** `9fe049d` deleted the first extra builder and wrote the rule
    /// down in prose; `d316d35` deleted the second and added a ratchet that read
    /// the production half of `ddl.rs` and `compare.rs` only and matched the
    /// single literal `emit().join(`. `schemaic-app/src/mcp.rs` was a live fifth
    /// member the whole time, in a crate the corpus did not include, under a
    /// commit message declaring the class closed at four.
    ///
    /// Here rather than in `schemaic-core/tests/` — where it first landed —
    /// because a workspace-wide scan there needs a second copy of
    /// [`production_code`] (`schemaic-ui` depends on `schemaic-core`, so the
    /// dev-dependency back is a cycle), and a second copy of this walk is the
    /// thing this module exists to prevent. The rule is about `schemaic-core`'s
    /// API and the corpus is every crate, so it belongs with the walk.
    #[test]
    fn nothing_joins_the_emitted_statements_outside_client_script() {
        // Assembled, or this test's own source is the hit.
        let needle = format!(".{}()", "emit");
        let mut seen = 0usize;
        let mut offenders: Vec<String> = Vec::new();
        for (name, code) in workspace_sources() {
            let mut from = 0usize;
            while let Some(rel) = code[from..].find(&needle) {
                let at = from + rel + needle.len();
                from = at;
                seen += 1;
                // The rest of this statement: a join into one script is inside
                // the same expression, so it lands before the `;`.
                let stmt = &code[at..];
                let end = stmt.find(';').unwrap_or(stmt.len()).min(400);
                if stmt[..end].contains(".join(") || stmt[..end].contains(".concat(") {
                    let line = 1 + code[..at].bytes().filter(|c| *c == b'\n').count();
                    offenders.push(format!("{name}:{line}"));
                }
            }
        }
        assert!(
            seen >= 5,
            "the needle stopped matching: {seen} `.emit()` calls across the \
             workspace's production code — a gate that scans nothing reports \
             success"
        );
        assert!(
            offenders.is_empty(),
            "these join emitted statements without going through \
             `ddl::client_script`, which terminates every statement and wraps a \
             compound MySQL body in `DELIMITER $$`:\n{}",
            offenders.join("\n")
        );
    }

    /// Apply Rust's line-continuation rule to a literal's raw source text: a
    /// `\` at end of line removes the newline **and the whole of the next
    /// line's leading whitespace**.
    ///
    /// This is the whole point of the gate below. A literal that *is* continued
    /// properly still holds `\`, a newline and an indent in its source bytes, so
    /// a scan of the raw text reports every correctly-written multi-line string
    /// in the workspace. What is left after this is what the program will hold.
    fn unescape_continuations(raw: &str) -> String {
        let mut out = String::with_capacity(raw.len());
        let mut chars = raw.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.peek() {
                Some('\n') => {
                    chars.next();
                    while chars.peek().is_some_and(|c| c.is_whitespace()) {
                        chars.next();
                    }
                }
                // Any other escape: keep both characters. `\"` must not be read
                // as the end of anything, and `\\` must not eat the next one.
                Some(_) => {
                    out.push(c);
                    out.push(chars.next().expect("peeked"));
                }
                None => out.push(c),
            }
        }
        out
    }

    /// Every `"…"` literal in `src`, as `(line, contents)`, skipping raw
    /// strings, char literals and both kinds of comment.
    ///
    /// A char literal is skipped **whole**: `'a` is one byte to step over, but
    /// `'"'` is not — treating its `"` as the opening of a string
    /// desynchronises the scan for the rest of the file, and the run of
    /// "literals" that follows is source code.
    fn string_literals(src: &str) -> Vec<(usize, String)> {
        let b = src.as_bytes();
        let mut out = Vec::new();
        let mut i = 0usize;
        let mut line = 1usize;
        while i < b.len() {
            match b[i] {
                b'\n' => {
                    line += 1;
                    i += 1;
                }
                b'/' if b.get(i + 1) == Some(&b'/') => {
                    while i < b.len() && b[i] != b'\n' {
                        i += 1;
                    }
                }
                b'/' if b.get(i + 1) == Some(&b'*') => {
                    i += 2;
                    while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                        if b[i] == b'\n' {
                            line += 1;
                        }
                        i += 1;
                    }
                    i = (i + 2).min(b.len());
                }
                // A raw string keeps whatever it holds on purpose. Compared as
                // bytes: `i` walks past every byte of the body, so slicing here
                // lands inside a multi-byte character the moment one appears.
                b'r' if matches!(b.get(i + 1), Some(b'"' | b'#')) => {
                    i += 1;
                    let mut hashes = 0usize;
                    while b.get(i) == Some(&b'#') {
                        hashes += 1;
                        i += 1;
                    }
                    if b.get(i) != Some(&b'"') {
                        continue;
                    }
                    i += 1;
                    let close = format!("\"{}", "#".repeat(hashes));
                    let close = close.as_bytes();
                    while i < b.len() && !b[i..].starts_with(close) {
                        if b[i] == b'\n' {
                            line += 1;
                        }
                        i += 1;
                    }
                    i = (i + close.len()).min(b.len());
                }
                b'\'' => {
                    let body = if b.get(i + 1) == Some(&b'\\') { 3 } else { 2 };
                    i += if b.get(i + body) == Some(&b'\'') {
                        body + 1
                    } else {
                        1
                    };
                }
                b'"' => {
                    let at = line;
                    i += 1;
                    let start = i;
                    while i < b.len() {
                        if b[i] == b'\\' {
                            if b.get(i + 1) == Some(&b'\n') {
                                line += 1;
                            }
                            i += 2;
                            continue;
                        }
                        if b[i] == b'"' {
                            break;
                        }
                        if b[i] == b'\n' {
                            line += 1;
                        }
                        i += 1;
                    }
                    if start <= i && i <= src.len() {
                        out.push((at, unescape_continuations(&src[start..i])));
                    }
                    i += 1;
                }
                _ => i += 1,
            }
        }
        out
    }

    /// **A string literal wrapped across source lines carries a `\` at the
    /// break**, or the indentation becomes content.
    ///
    /// Rust does not join adjacent lines inside a `"…"`: without the backslash,
    /// the newline *and the whole of the next line's indent* are part of the
    /// string. The result is a sentence with a fourteen- or eighteen-space run
    /// in the middle of it, and it reaches wherever that string goes.
    ///
    /// The instances differed only in who saw them: `app/antigravity.rs`'s
    /// `blocked_reason` reaches the AI panel's no-tools note verbatim, so a user
    /// read "already using Antigravity.⎵×14 Antigravity keeps one
    /// machine-wide…"; `core/ddl.rs`'s generated-column refusal shows in the DDL
    /// preview; `ui/blob_view.rs`'s is a cell-preview message; `core/launch.rs`'s
    /// is latent, because its one consumer collapses space runs — a copy of the
    /// mistake rather than a live one, which is exactly how a class survives
    /// being fixed at its noisy sites.
    ///
    /// One test over the workspace rather than one per site, because the failure
    /// is a typing habit and not a bug in any of them. Production code only: a
    /// test's expected output lines columns up on purpose, and so does a SQL
    /// fixture — and a literal that spells its own `\n` or `\t` is left alone,
    /// that being the mark of a block whose spacing is deliberate.
    #[test]
    fn no_string_literal_carries_a_wrapped_lines_indentation() {
        let files = workspace_sources();
        assert!(files.len() >= 40, "the corpus collapsed: {}", files.len());
        let mut scanned = 0usize;
        let mut offenders: Vec<String> = Vec::new();
        for (name, code) in &files {
            for (line, lit) in string_literals(code) {
                scanned += 1;
                if lit.contains("\\n") || lit.contains("\\t") {
                    continue;
                }
                // Six is past any legitimate run inside a sentence and well
                // under the shortest indent that produced one of the originals.
                let Some(at) = lit.find("      ") else {
                    continue;
                };
                // Leading indentation of a literal that *starts* on its own line
                // is the author's, not a wrap.
                if lit[..at].trim().is_empty() {
                    continue;
                }
                let from = lit[..at].char_indices().rev().nth(30).map_or(0, |(i, _)| i);
                offenders.push(format!("{name}:{line}: …{}…", &lit[from..]));
            }
        }
        assert!(
            scanned >= 500,
            "the scanner stopped finding literals: {scanned} — a gate that scans \
             nothing reports success"
        );
        assert!(
            offenders.is_empty(),
            "a string wrapped across source lines with no `\\` at the break \
             carries the next line's indentation as content, and it reaches \
             wherever the string goes:\n{}",
            offenders.join("\n")
        );
    }

    /// **A deferred hand-back to the SQL editor either claims the keyboard or
    /// stands down.**
    ///
    /// Focus is handed back a frame late in several places — the run menu,
    /// Ctrl+K twice, the find close, the goto close and submit, the completion
    /// row's click — because the focus floem takes is cleared during the frame's
    /// own dispatch, so asking for it back inside the handler is undone by the
    /// frame that stole it. Deferring makes the request *land*; it does not make
    /// it *win*. Two immediate timers queued in one pass, and the one that lands
    /// last takes the keyboard.
    ///
    /// So each of these has to say which it is. A **mover** — the user did
    /// something whose whole point is that the editor ends up focused — calls
    /// `claim_keyboard`, and a hand-back scheduled behind it reads the
    /// generation and stands down. A **hand-back** does the reading instead,
    /// through `keyboard_claim_unchanged` or, for the tab-mount autofocus,
    /// `innermost_focus_root` (an overlay owns the keyboard while it is up). A
    /// block that does neither is the bug: it wins or loses by timing.
    ///
    /// One gate over the workspace rather than a note at each site, because the
    /// failure is a hand-back written without looking at its five siblings —
    /// two of the five claimed and three did not.
    ///
    /// Scoped to the **editor's** `editor_view_id`, deliberately. The grid's
    /// keyboard home is the other side of the same protocol and reads the
    /// generation rather than claiming it, and a picker's mount autofocus is a
    /// different question again; a rule wide enough to cover all three would
    /// have to be "or does something else", which is not a rule.
    #[test]
    fn a_deferred_editor_hand_back_claims_or_stands_down() {
        /// The byte after the `(` opened at `open`, by paren balance.
        fn call_end(b: &[u8], open: usize) -> Option<usize> {
            let mut depth = 0usize;
            for (i, c) in b.iter().enumerate().skip(open) {
                match c {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(i);
                        }
                    }
                    _ => {}
                }
            }
            None
        }

        let files = workspace_sources();
        assert!(files.len() >= 40, "the corpus collapsed: {}", files.len());
        let mut deferred = 0usize;
        let mut offenders: Vec<String> = Vec::new();
        for (name, code) in &files {
            let b = code.as_bytes();
            let mut from = 0usize;
            while let Some(rel) = code[from..].find("exec_after(") {
                let at = from + rel;
                from = at + "exec_after(".len();
                let Some(end) = call_end(b, at + "exec_after".len()) else {
                    continue;
                };
                let block = &code[at..=end];
                if !block.contains("request_focus()") || !block.contains("editor_view_id") {
                    continue;
                }
                deferred += 1;
                let decided = block.contains("claim_keyboard()")
                    || block.contains("keyboard_claim_unchanged")
                    || block.contains("innermost_focus_root()");
                if !decided {
                    let line = code[..at].matches('\n').count() + 1;
                    offenders.push(format!("{name}:{line}"));
                }
            }
        }
        assert!(
            deferred >= 5,
            "only {deferred} deferred editor hand-backs found; this gate has gone \
             blind — it reports success by matching nothing"
        );
        assert!(
            offenders.is_empty(),
            "a deferred hand-back to the editor that neither claims the keyboard \
             nor stands down: it wins or loses by which timer lands last, and the \
             keyboard ends up somewhere the user did not put it:\n{}",
            offenders.join("\n")
        );
    }
}
