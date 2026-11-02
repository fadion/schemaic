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

/// Every `.rs` file under `dir`, recursively, as [`label_source`] reads it.
fn rust_sources(dir: &Path, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("a source folder") {
        let path = entry.expect("a source entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = fs::read_to_string(&path).expect("a source file");
            out.push(label_source(&text));
        }
    }
}

/// A source file's **code** — where a literal could be something the
/// interface shows — up to its test module, with every comment dropped.
///
/// **A comment vouches for no label**: a renamed label's old spelling lives
/// on in the comments that describe it (*AI Seed Table* did, in two doc
/// comments), and the guard accepted a guide page still writing it. Cut at
/// `#[cfg(test)] mod`, not the first `#[cfg(test)]`, which can be a helper
/// halfway down a file with real code after it.
fn label_source(text: &str) -> String {
    let end = text
        .match_indices("#[cfg(test)]")
        .find(|(at, m)| text[at + m.len()..].trim_start().starts_with("mod "))
        .map_or(text.len(), |(at, _)| at);
    let text = &text[..end];
    let mut out = String::with_capacity(text.len());
    let (mut in_string, mut in_block) = (false, false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block = false;
            }
            continue;
        }
        if in_string {
            out.push(c);
            match c {
                '\\' => out.extend(chars.next()),
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match (c, chars.peek()) {
            ('/', Some('/')) => {
                // To the end of the line, which is kept.
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                in_block = true;
            }
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            // A char literal — `'"'`, `'\''` — copied whole, so its quote
            // opens no string; a lifetime (`'a`) is just copied.
            ('\'', _) => {
                out.push(c);
                let mut ahead = chars.clone();
                let literal = matches!(
                    (ahead.next(), ahead.next()),
                    (Some('\\'), _) | (Some(_), Some('\''))
                );
                if literal {
                    while let Some(c) = chars.next() {
                        out.push(c);
                        if c == '\\' {
                            out.extend(chars.next());
                        } else if c == '\'' {
                            break;
                        }
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
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
        // A backtick fence's info string holds no backtick (CommonMark):
        // ```` ```x``` then **a** ```` is a code span and prose after it.
        let run = if run.len() >= 3 && !trimmed[run.len()..].contains('`') {
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

/// The text of each bold span in a block — `**…**`, and `__…__` where it
/// opens at a word's start, as Markdown reads it (`snake__case` is no bold).
/// Every spelling, since one the guard does not read is a label it never
/// checks.
fn bold_spans(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for marker in ["**", "__"] {
        let mut rest = line;
        while let Some(start) = rest.find(marker) {
            let opens_word = rest[..start]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric());
            let after = &rest[start + 2..];
            let Some(end) = after.find(marker) else { break };
            if marker == "**" || opens_word {
                out.push(after[..end].to_string());
            }
            rest = &after[end + 2..];
        }
    }
    out
}

/// Every link target in a page's prose: inline `[text](target)` and a
/// reference definition's `[name]: target`, which the guard read none of.
fn link_targets(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (_, prose) in prose_blocks(text) {
        let mut rest = prose.as_str();
        while let Some(at) = rest.find("](") {
            let after = &rest[at + 2..];
            let Some(end) = after.find(')') else { break };
            out.push(after[..end].to_string());
            rest = &after[end + 1..];
        }
    }
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(def) = t.strip_prefix('[')
            && let Some((label, target)) = def.split_once("]:")
            && !label.is_empty()
            && let Some(target) = target.split_whitespace().next()
        {
            out.push(target.to_string());
        }
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
        for target in link_targets(&text) {
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
                offenders.push(format!("{name}: {target}"));
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

/// **A comment vouches for no label.** A renamed label's old spelling lives
/// on in the comments that describe it — *AI Seed Table* did — and a guide
/// page still writing it passed. And a test module is cut where it starts,
/// not at the first `#[cfg(test)]`, which can be a helper's mid-file.
#[test]
fn a_label_quoted_in_a_comment_is_not_in_the_source() {
    let src = "/// Open the \"AI Seed Table\" popover\n\
               fn f() { show(\"Seed rows\"); } // the \"Old caption\" was here\n\
               /* \"Block caption\" */\n\
               #[cfg(test)]\nfn helper() { show(\"Later label\"); }\n\
               #[cfg(test)]\nmod tests { fn t() { show(\"Fixture\"); } }";
    let code = label_source(src);
    assert!(!code.contains("\"AI Seed Table\""), "{code}");
    assert!(!code.contains("\"Old caption\""), "{code}");
    assert!(!code.contains("\"Block caption\""), "{code}");
    assert!(
        code.contains("\"Seed rows\"") && code.contains("\"Later label\""),
        "{code}"
    );
    assert!(!code.contains("\"Fixture\""), "{code}");
    // A `//` inside a string is the string's.
    assert!(label_source("f(\"a//b\", \"Next\");").contains("\"Next\""));
}

/// **Every spelling of bold is read, and a backtick run with more after it
/// on its line is a code span, not a fence** — `__a__` was not read at all,
/// and a line opening ```` ```x``` ```` hid the rest of the page.
#[test]
fn every_bold_spelling_is_read_and_an_inline_run_is_no_fence() {
    assert_eq!(spans_of("```x``` then **a**\n\nmore **b**"), ["a", "b"]);
    let mut both = spans_of("__a__ and **b**");
    both.sort();
    assert_eq!(both, ["a", "b"]);
    assert_eq!(spans_of("snake_case_name and **b**"), ["b"]);
}

/// **A reference-style link's target is a link too.**
#[test]
fn a_reference_definition_is_a_link() {
    assert_eq!(
        link_targets("See [the page][p].\n\n[p]: ../other.md#x\n"),
        ["../other.md#x"]
    );
    assert_eq!(link_targets("[a](b.md) and [c](d.md)"), ["b.md", "d.md"]);
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
