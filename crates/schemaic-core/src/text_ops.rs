//! Pure editor text operations (no UI). Currently: SQL line-comment toggling for
//! the editor's Ctrl+/. The function computes the full edited buffer plus the
//! selection the caret should occupy, which the UI applies in a single
//! `edit_single` (so it's one undo step).

/// SQL line-comment token.
const TOKEN: &str = "--";

/// Result of a line-level edit: the new full document text and the byte-offset
/// selection (`start..end`, into the new `text`) to apply afterward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEdit {
    pub text: String,
    pub sel: (usize, usize),
}

/// Toggle `-- ` line comments across the lines spanned by the byte range
/// `[sel_start, sel_end]` in `text`.
///
/// - If *every* non-blank line in the span is already commented, all are
///   uncommented (the `--` plus one following space, if present, are stripped at
///   the line's indent). Otherwise each non-blank line is commented by inserting
///   `-- ` at its first non-whitespace column.
/// - Blank / whitespace-only lines inside a multi-line block are left untouched.
/// - A selection that ends exactly at the start of a line does not pull that line
///   into the span (matches typical editor behaviour when shift-selecting down).
/// - The returned selection spans the affected lines' new extent, so a repeated
///   Ctrl+/ keeps toggling the same block.
pub fn toggle_line_comment(text: &str, sel_start: usize, sel_end: usize) -> LineEdit {
    let len = text.len();
    let lo = sel_start.min(sel_end).min(len);
    let hi = sel_start.max(sel_end).min(len);

    let starts = line_start_offsets(text);
    let lines: Vec<&str> = text.split('\n').collect();
    let first = line_index_of(&starts, lo);
    let mut last = line_index_of(&starts, hi);
    // Selection ending exactly at a line's start shouldn't include that line.
    if last > first && hi == starts[last] {
        last -= 1;
    }

    // Decide comment vs uncomment from the non-blank lines in the span.
    let mut all_commented = true;
    let mut any_nonblank = false;
    for line in &lines[first..=last] {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        any_nonblank = true;
        if !trimmed.starts_with(TOKEN) {
            all_commented = false;
        }
    }
    let uncomment = any_nonblank && all_commented;

    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    for (i, &content) in lines.iter().enumerate() {
        if i < first || i > last {
            out.push(content.to_string());
            continue;
        }
        let trimmed = content.trim_start();
        // Leave blank lines untouched when toggling a real (has-content) block.
        if trimmed.is_empty() && any_nonblank {
            out.push(content.to_string());
            continue;
        }
        let indent_len = content.len() - trimmed.len();
        let (indent, rest) = content.split_at(indent_len);
        if uncomment {
            let after = &rest[TOKEN.len()..];
            let after = after.strip_prefix(' ').unwrap_or(after);
            out.push(format!("{indent}{after}"));
        } else {
            out.push(format!("{indent}{TOKEN} {rest}"));
        }
    }
    let joined = out.join("\n");
    let new_text = if text.ends_with('\n') && !joined.ends_with('\n') {
        // `split('\n')` on a trailing-newline string yields a final "" element, so
        // the join already reproduces the trailing newline; guard just in case.
        format!("{joined}\n")
    } else {
        joined
    };

    // Select the affected lines' new extent.
    let new_starts = line_start_offsets(&new_text);
    let sel_lo = new_starts[first];
    let sel_hi = if last + 1 < new_starts.len() {
        new_starts[last + 1].saturating_sub(1) // exclude the newline
    } else {
        new_text.len()
    };
    LineEdit {
        text: new_text,
        sel: (sel_lo, sel_hi),
    }
}

/// Byte offsets of every (non-overlapping) ASCII-case-insensitive occurrence of
/// `needle` in `hay`. Offsets index into `hay` directly (the search is byte-wise
/// and boundary-checked, so no `to_lowercase` reallocation shifts them). Empty
/// needle → no matches.
pub fn find_matches(hay: &str, needle: &str) -> Vec<usize> {
    let n = needle.len();
    if n == 0 || hay.len() < n {
        return Vec::new();
    }
    let (hb, nb) = (hay.as_bytes(), needle.as_bytes());
    let mut out = Vec::new();
    let mut i = 0;
    while i + n <= hb.len() {
        if hb[i..i + n].eq_ignore_ascii_case(nb)
            && hay.is_char_boundary(i)
            && hay.is_char_boundary(i + n)
        {
            out.push(i);
            i += n; // non-overlapping
        } else {
            i += 1;
        }
    }
    out
}

/// Replace every non-overlapping ASCII-case-insensitive occurrence of `needle`
/// in `hay` with `replacement`, returning the new string and the number of
/// replacements. Matches are the same ones [`find_matches`] reports (so the UI's
/// count and replace-all agree). Empty needle → unchanged, zero replacements.
/// Left-to-right, non-overlapping: the search resumes *after* each original
/// match (not inside the inserted text), so a replacement that itself contains
/// the needle won't be re-replaced in the same pass.
pub fn replace_all(hay: &str, needle: &str, replacement: &str) -> (String, usize) {
    let hits = find_matches(hay, needle);
    if hits.is_empty() {
        return (hay.to_string(), 0);
    }
    let n = needle.len();
    let mut out = String::with_capacity(hay.len());
    let mut prev = 0;
    for &off in &hits {
        out.push_str(&hay[prev..off]);
        out.push_str(replacement);
        prev = off + n;
    }
    out.push_str(&hay[prev..]);
    (out, hits.len())
}

/// Whether `hay` contains `needle`, ASCII-case-insensitively. Allocation-free
/// (unlike `find_matches`), so it's cheap to call per grid cell. Empty needle
/// matches anything.
pub fn contains_ignore_ascii_case(hay: &str, needle: &str) -> bool {
    let n = needle.len();
    if n == 0 {
        return true;
    }
    if hay.len() < n {
        return false;
    }
    let (hb, nb) = (hay.as_bytes(), needle.as_bytes());
    (0..=hb.len() - n).any(|i| hb[i..i + n].eq_ignore_ascii_case(nb))
}

/// Byte offset where each line begins (line 0 at 0, then one past each `\n`).
fn line_start_offsets(text: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Index of the line containing byte offset `off` (largest start ≤ off).
fn line_index_of(starts: &[usize], off: usize) -> usize {
    match starts.binary_search(&off) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toggled(text: &str, a: usize, b: usize) -> String {
        toggle_line_comment(text, a, b).text
    }

    #[test]
    fn comments_a_single_line() {
        assert_eq!(toggled("SELECT 1", 0, 0), "-- SELECT 1");
    }

    #[test]
    fn uncomments_a_single_line() {
        assert_eq!(toggled("-- SELECT 1", 0, 0), "SELECT 1");
    }

    #[test]
    fn roundtrips() {
        let src = "SELECT 1";
        let once = toggled(src, 0, 0);
        assert_eq!(once, "-- SELECT 1");
        // Re-toggle the same line (whole-line selection returned by the first call).
        let sel = toggle_line_comment(src, 0, 0).sel;
        assert_eq!(toggled(&once, sel.0, sel.1), "SELECT 1");
    }

    #[test]
    fn preserves_indent() {
        assert_eq!(toggled("    WHERE x = 1", 0, 0), "    -- WHERE x = 1");
        assert_eq!(toggled("    -- WHERE x = 1", 0, 0), "    WHERE x = 1");
    }

    #[test]
    fn comments_whole_multiline_block() {
        let src = "SELECT a\nFROM t\nWHERE x";
        // Span all three lines.
        let out = toggled(src, 0, src.len());
        assert_eq!(out, "-- SELECT a\n-- FROM t\n-- WHERE x");
    }

    #[test]
    fn mixed_block_comments_all() {
        // One line already commented, one not → not all-commented → comment all.
        let src = "-- SELECT a\nFROM t";
        let out = toggled(src, 0, src.len());
        assert_eq!(out, "-- -- SELECT a\n-- FROM t");
    }

    #[test]
    fn fully_commented_block_uncomments() {
        let src = "-- SELECT a\n-- FROM t";
        let out = toggled(src, 0, src.len());
        assert_eq!(out, "SELECT a\nFROM t");
    }

    #[test]
    fn blank_lines_untouched_in_block() {
        let src = "SELECT a\n\nFROM t";
        let out = toggled(src, 0, src.len());
        assert_eq!(out, "-- SELECT a\n\n-- FROM t");
    }

    #[test]
    fn selection_ending_at_line_start_excludes_it() {
        let src = "SELECT a\nFROM t";
        // Select from 0 to the start of line 1 (offset 9) → only line 0.
        let out = toggled(src, 0, 9);
        assert_eq!(out, "-- SELECT a\nFROM t");
    }

    #[test]
    fn uncomment_strips_only_one_space() {
        assert_eq!(toggled("--  x", 0, 0), " x");
    }

    #[test]
    fn preserves_trailing_newline() {
        assert_eq!(toggled("SELECT 1\n", 0, 0), "-- SELECT 1\n");
    }

    #[test]
    fn find_matches_case_insensitive_nonoverlapping() {
        assert_eq!(
            find_matches("SELECT select SeLeCt", "select"),
            vec![0, 7, 14]
        );
        assert_eq!(find_matches("aaaa", "aa"), vec![0, 2]); // non-overlapping
        assert_eq!(find_matches("abc", ""), Vec::<usize>::new());
        assert_eq!(find_matches("abc", "xyz"), Vec::<usize>::new());
        // Offsets index into the original string (é is 2 bytes → bar at byte 6).
        assert_eq!(find_matches("café bar", "bar"), vec![6]);
    }

    #[test]
    fn replace_all_matches_find() {
        // Case-insensitive, replaces every occurrence, reports the count.
        assert_eq!(
            replace_all("SELECT select SeLeCt", "select", "x"),
            ("x x x".to_string(), 3)
        );
        // Non-overlapping, resumes after each original match.
        assert_eq!(replace_all("aaaa", "aa", "b"), ("bb".to_string(), 2));
        // A replacement that contains the needle isn't re-replaced this pass.
        assert_eq!(replace_all("a", "a", "aa"), ("aa".to_string(), 1));
        // Empty needle / no match → unchanged, zero replacements.
        assert_eq!(replace_all("abc", "", "x"), ("abc".to_string(), 0));
        assert_eq!(replace_all("abc", "z", "x"), ("abc".to_string(), 0));
        // Multibyte-safe (é is 2 bytes).
        assert_eq!(
            replace_all("café bar", "bar", "pub"),
            ("café pub".to_string(), 1)
        );
    }

    #[test]
    fn contains_ci() {
        assert!(contains_ignore_ascii_case("Hello World", "world"));
        assert!(contains_ignore_ascii_case("abc", ""));
        assert!(!contains_ignore_ascii_case("abc", "xyz"));
        assert!(!contains_ignore_ascii_case("ab", "abc"));
    }

    #[test]
    fn selection_spans_affected_lines() {
        let src = "SELECT a\nFROM t";
        let ed = toggle_line_comment(src, 0, src.len());
        // Whole new text is two commented lines; selection covers both.
        assert_eq!(ed.sel, (0, ed.text.len()));
    }
}
