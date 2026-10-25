//! Guard: the user guide (`docs/guides/`) names only labels the interface has,
//! links only pages that exist, and publishes every page it holds.
//!
//! The guide is written and kept up to date by agents, and the failure that
//! matters in documentation written that way is not style but a confident
//! sentence about a menu entry that does not exist — or did, until a rename.
//! So the guide writes every interface label in **bold** and nothing else in
//! bold, and this test reads each bold span back against the `en-GB` catalogue
//! (`crates/schemaic-ui/i18n/en-GB/schemaic.ftl`) — or, for a label not yet
//! moved into it, a string literal of the same text in the UI's source. A
//! catalogue message with a placeable (`{ $name }`) matches any text in its
//! place.
//!
//! A match says the label *exists*, not that the sentence around it is true:
//! it catches a renamed or invented label, never a wrong description of a
//! real one. That part is the guide writer's to check against the code.
//!
//! It reads the filesystem because the thing under test *is* a file — the
//! exemption AGENTS.md grants the repository's own source and docs.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn guide_src() -> PathBuf {
    repo_root().join("docs/guides/src")
}

/// Every `.md` file under `dir`, recursively.
fn markdown_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).expect("a guide folder") {
        let path = entry.expect("a guide entry").path();
        if path.is_dir() {
            out.extend(markdown_files(&path));
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// Every `.rs` file under `dir`, recursively, as text — up to its first
/// `#[cfg(test)]`, since a literal in a test module is a fixture, not
/// something the interface shows.
fn rust_sources(dir: &Path, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("a source folder") {
        let path = entry.expect("a source entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = fs::read_to_string(&path).expect("a source file");
            let end = text.find("#[cfg(test)]").unwrap_or(text.len());
            out.push(text[..end].to_string());
        }
    }
}

/// The page's prose, one entry per block with the line it starts on: fenced
/// blocks (```` ``` ```` or `~~~`, of any length) dropped, consecutive lines of
/// a paragraph joined so a bold label wrapped over two lines is read whole, and
/// code spans removed — `**` inside code is SQL or a glob, not a label. A table
/// row or a heading is a block of its own.
fn prose_blocks(text: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut fence: Option<String> = None;
    let mut open = false;
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some(marker) = &fence {
            if trimmed.starts_with(marker.as_str())
                && trimmed.trim_end().chars().all(|c| marker.starts_with(c))
            {
                fence = None;
            }
            continue;
        }
        let run: String = trimmed.chars().take_while(|c| *c == '`').collect();
        let run = if run.len() >= 3 {
            run
        } else {
            trimmed.chars().take_while(|c| *c == '~').collect()
        };
        if run.len() >= 3 {
            fence = Some(run);
            open = false;
            continue;
        }
        if trimmed.is_empty() {
            open = false;
            continue;
        }
        let standalone = trimmed.starts_with('|') || trimmed.starts_with('#');
        match out.last_mut() {
            Some((_, block)) if open && !standalone => {
                block.push(' ');
                block.push_str(trimmed);
            }
            _ => out.push((i + 1, trimmed.to_string())),
        }
        open = !standalone;
    }
    out.into_iter()
        .map(|(line, block)| (line, strip_code_spans(&block)))
        .collect()
}

/// `text` without its code spans. A span opens on a run of backticks and
/// closes on the next run of the *same* length (CommonMark's rule), so
/// ``` `` a`b `` ``` is one span; a run nothing closes is literal text.
fn strip_code_spans(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('`') {
        out.push_str(&rest[..at]);
        let run = rest[at..].chars().take_while(|c| *c == '`').count();
        let after = &rest[at + run..];
        let mut search = after;
        let mut consumed = 0;
        let close = loop {
            let Some(next) = search.find('`') else {
                break None;
            };
            let len = search[next..].chars().take_while(|c| *c == '`').count();
            if len == run {
                break Some(consumed + next);
            }
            consumed += next + len;
            search = &search[next + len..];
        };
        match close {
            Some(end) => rest = &after[end + run..],
            None => {
                out.push_str(&rest[at..at + run]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The text of each `**…**` span in a block.
fn bold_spans(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find("**") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("**") else { break };
        out.push(after[..end].to_string());
        rest = &after[end + 2..];
    }
    out
}

/// Does `label` read as catalogue message `message`, with each `{ … }`
/// placeable standing for any non-empty text?
fn matches_message(label: &str, message: &str) -> bool {
    let mut parts = Vec::new();
    let mut rest = message;
    while let Some(open) = rest.find('{') {
        parts.push(&rest[..open]);
        match rest[open..].find('}') {
            Some(close) => rest = &rest[open + close + 1..],
            None => return false,
        }
    }
    parts.push(rest);
    if parts.len() == 1 {
        return label == message;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !label.starts_with(first) || !label.ends_with(last) {
        return false;
    }
    if label.len() < first.len() + last.len() {
        return false;
    }
    let mut cursor = first.len();
    let limit = label.len() - last.len();
    // Each placeable stands for some text, so every gap is non-empty: the
    // search for the next literal starts one *character* on, not one byte.
    for part in &parts[1..parts.len() - 1] {
        let Some(step) = label[cursor..limit].chars().next().map(char::len_utf8) else {
            return false;
        };
        match label[cursor + step..limit].find(part) {
            Some(at) => cursor += step + at + part.len(),
            None => return false,
        }
    }
    cursor < limit
}

/// Is `label` this catalogue message, with its placeables filled in — and is
/// the message's own wording at least half of what the guide wrote? Without
/// the second half, `{ $n } rows` accepted *Delete all rows* and
/// `Drop { $what } { $name }` *Drop everything now*: a message mostly made of
/// placeables vouches for nearly any text, and the guard passed invented
/// labels.
fn label_matches(label: &str, message: &str) -> bool {
    if !message.contains('{') {
        return label == message;
    }
    let label_letters = label.chars().filter(|c| c.is_alphabetic()).count();
    2 * literal_letters(message) >= label_letters && matches_message(label, message)
}

/// The `en-GB` catalogue's messages and attributes, each by its first line,
/// plus every variant of a select (`[one] …`, `*[other] …`) — the plural form
/// of a label like *Commit { $n } changes* lives only there.
fn catalogue_messages() -> Vec<String> {
    let path = repo_root().join("crates/schemaic-ui/i18n/en-GB/schemaic.ftl");
    let text = fs::read_to_string(path).expect("the en-GB catalogue");
    text.lines()
        .map(str::trim_start)
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            l.split_once(" = ").map(|(_, value)| value).or_else(|| {
                let variant = l.strip_prefix('*').unwrap_or(l).strip_prefix('[')?;
                variant.split_once(']').map(|(_, value)| value)
            })
        })
        .map(|value| value.trim().to_string())
        // A message that is mostly placeables (`{ $name }`, `{ $a } · { $b }`)
        // would match nearly any text, and pass whatever the guide wrote.
        .filter(|value| literal_letters(value) >= 2)
        .collect()
}

/// The letters a message keeps outside its placeables.
fn literal_letters(message: &str) -> usize {
    let mut depth = 0usize;
    let mut letters = 0;
    for c in message.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            c if depth == 0 && c.is_alphabetic() => letters += 1,
            _ => {}
        }
    }
    letters
}

#[test]
fn every_bold_span_in_the_guide_is_an_interface_label() {
    let messages = catalogue_messages();
    let mut sources = Vec::new();
    for krate in ["schemaic-ui", "schemaic-app"] {
        rust_sources(
            &repo_root().join("crates").join(krate).join("src"),
            &mut sources,
        );
    }
    // Labels are sentence case, so a lower-case literal is a key, a column
    // name or SQL — never a label the catalogue hasn't taken in yet.
    let in_source = |label: &str| {
        let literal = format!("\"{label}\"");
        label.starts_with(char::is_uppercase) && sources.iter().any(|s| s.contains(&literal))
    };

    let mut offenders = Vec::new();
    for page in markdown_files(&guide_src()) {
        let text = fs::read_to_string(&page).expect("a guide page");
        let name = page
            .strip_prefix(guide_src())
            .unwrap()
            .display()
            .to_string();
        for (line, prose) in prose_blocks(&text) {
            for label in bold_spans(&prose) {
                let known = messages.iter().any(|m| label_matches(&label, m)) || in_source(&label);
                if !known {
                    offenders.push(format!("{name}:{line}: **{label}**"));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "the guide names a label the interface does not have. Bold is reserved for \
         interface labels, spelled exactly as the en-GB catalogue spells them — fix the \
         label, or un-bold it if it was emphasis:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn every_relative_link_in_the_guide_resolves() {
    let mut offenders = Vec::new();
    for page in markdown_files(&guide_src()) {
        let text = fs::read_to_string(&page).expect("a guide page");
        let name = page
            .strip_prefix(guide_src())
            .unwrap()
            .display()
            .to_string();
        let dir = page.parent().unwrap();
        let root = guide_src()
            .canonicalize()
            .expect("the guide's source folder");
        for (line, prose) in prose_blocks(&text) {
            let mut rest = prose.as_str();
            while let Some(at) = rest.find("](") {
                let after = &rest[at + 2..];
                let Some(end) = after.find(')') else { break };
                let target = &after[..end];
                rest = &after[end + 1..];
                let path = target.split('#').next().unwrap_or("");
                // `[Title]()` is a draft chapter in SUMMARY.md; an absolute URL
                // or a bare anchor is not a file.
                if path.is_empty() || path.contains("://") || path.starts_with("mailto:") {
                    continue;
                }
                // mdBook publishes only what is under `src/`: a link out to the
                // repository's README resolves here and 404s on the site.
                let published = dir
                    .join(path)
                    .canonicalize()
                    .is_ok_and(|p| p.is_file() && p.starts_with(&root));
                if path.starts_with('/') || !published {
                    offenders.push(format!("{name}:{line}: {target}"));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a guide link points at a page that does not exist, or one outside docs/guides/src \
         that the published guide will not have:\n{}",
        offenders.join("\n")
    );
}

/// **A backtick key is written `&#96;`.** A literal one inside `<kbd>` opens a
/// code span that runs to the next backtick on the page, swallowing the prose
/// between — *Toggle terminal*'s <kbd>Ctrl</kbd>+<kbd>`</kbd> turned the rest of
/// its paragraph into code. It also hides that prose from the label check, so
/// this one has to be caught on its own.
#[test]
fn no_kbd_holds_a_literal_backtick() {
    let mut offenders = Vec::new();
    for page in markdown_files(&guide_src()) {
        let text = fs::read_to_string(&page).expect("a guide page");
        let name = page
            .strip_prefix(guide_src())
            .unwrap()
            .display()
            .to_string();
        for (i, line) in text.lines().enumerate() {
            if line.contains("<kbd>`") {
                offenders.push(format!("{name}:{}", i + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a <kbd> holds a literal backtick; write it <kbd>&#96;</kbd>:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn every_guide_page_is_in_the_table_of_contents() {
    let summary = fs::read_to_string(guide_src().join("SUMMARY.md")).expect("SUMMARY.md");
    // A chapter commented out is not published, so it does not count as listed.
    let mut listed = String::new();
    let mut rest = summary.as_str();
    while let Some(open) = rest.find("<!--") {
        listed.push_str(&rest[..open]);
        rest = rest[open..]
            .find("-->")
            .map_or("", |close| &rest[open + close + 3..]);
    }
    listed.push_str(rest);
    let listed = listed.replace("](./", "](");
    let orphans: Vec<_> = markdown_files(&guide_src())
        .into_iter()
        .map(|p| {
            p.strip_prefix(guide_src())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .filter(|p| p != "SUMMARY.md")
        .filter(|p| !listed.contains(&format!("]({p})")))
        .collect();
    assert!(
        orphans.is_empty(),
        "a guide page SUMMARY.md does not list, which mdBook will not publish:\n{}",
        orphans.join("\n")
    );
}

#[test]
fn a_placeable_matches_any_text_in_its_place() {
    assert!(matches_message("Claude Code path", "{ $cli } path"));
    assert!(matches_message(
        "System (English)",
        "System ({ $language })"
    ));
    assert!(!matches_message(" path", "{ $cli } path"));
    assert!(!matches_message("Claude Code paths", "{ $cli } path"));
    assert!(matches_message("Edit table", "Edit table"));
    assert!(!matches_message("Edit tables", "Edit table"));
}

#[test]
fn a_placeable_may_stand_for_text_that_is_not_ascii() {
    // Sliced by byte, a gap opening on `·` panicked the whole test, which then
    // reported no offenders at all.
    assert!(matches_message(
        "· kept — read-only",
        "{ $a } kept — { $b }"
    ));
    assert!(matches_message(
        "Rows 1–100 of 250",
        "Rows { $from }–{ $to } of { $total }"
    ));
    assert!(!matches_message("· kept", "{ $a } kept — { $b }"));
}

#[test]
fn a_message_that_is_all_placeables_is_no_label() {
    assert_eq!(literal_letters("{ $name }"), 0);
    assert_eq!(literal_letters("{ $a } · { $b }"), 0);
    assert_eq!(literal_letters("{ $cli } path"), 4);
}

#[test]
fn code_is_not_prose() {
    let spans = spans_of("a `**x**` b\n```\n**y**\n```\n\n**z**");
    assert_eq!(spans, ["z"]);
}

fn spans_of(text: &str) -> Vec<String> {
    prose_blocks(text)
        .iter()
        .flat_map(|(_, block)| bold_spans(block))
        .collect()
}

#[test]
fn a_bold_label_wrapped_over_two_lines_is_read_whole() {
    // Read line by line, `**Windows (this` and `account)**` paired with the
    // *next* bold on the page and checked the prose between as a label.
    let spans =
        spans_of("Sign in with **Windows (this\naccount)** or **Microsoft Entra (Azure\nCLI)**.");
    assert_eq!(
        spans,
        ["Windows (this account)", "Microsoft Entra (Azure CLI)"]
    );
}

#[test]
fn every_fence_and_code_span_shape_is_code() {
    assert_eq!(spans_of("~~~sql\nSELECT 2 ** 3 ** 2\n~~~\n\n**a**"), ["a"]);
    assert_eq!(spans_of("````\n```\n**x**\n```\n````\n\n**b**"), ["b"]);
    assert_eq!(spans_of("`` a`b `` then **c**"), ["c"]);
    assert_eq!(spans_of("a lone ` then **d**"), ["d"]);
}

#[test]
fn a_table_row_and_a_heading_are_blocks_of_their_own() {
    let blocks = prose_blocks("# T\n| **a** | b |\n| **c** | d |\ntext\nmore");
    let lines: Vec<_> = blocks.iter().map(|(line, _)| *line).collect();
    assert_eq!(lines, [1, 2, 3, 4]);
}

#[test]
fn a_message_mostly_placeables_vouches_for_little() {
    assert!(!label_matches("Delete all rows", "{ $n } rows"));
    assert!(!label_matches(
        "Drop everything now",
        "Drop { $what } { $name }"
    ));
    assert!(label_matches(
        "Also a user in database",
        "Also a user in { $database }"
    ));
    assert!(label_matches("Edit table", "Edit table"));
}
