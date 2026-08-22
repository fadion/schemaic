//! Pure editor text operations (no UI). Currently: SQL line-comment toggling for
//! the editor's Ctrl+/. The function computes the full edited buffer plus the
//! selection the caret should occupy, which the UI applies in a single
//! `edit_single` (so it's one undo step).

use crate::intel::SqlDialect;

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
/// - **A line whose first non-whitespace column is inside a string literal is
///   left untouched**, and is not counted when deciding comment-vs-uncomment.
///   Inserting `-- ` there doesn't comment anything out: it edits the *data* the
///   statement writes, silently, and the statement still parses and still runs.
///   That is what `dialect` is for — `$$…$$` is a string on PostgreSQL and two
///   operators on MySQL, so the same buffer has two different answers.
///
/// A line inside a **block comment** does toggle. A nested `--` there is inert,
/// and refusing would make Ctrl+/ do nothing on a commented-out block, which is
/// where people reach for it most.
pub fn toggle_line_comment(
    text: &str,
    sel_start: usize,
    sel_end: usize,
    dialect: SqlDialect,
) -> LineEdit {
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

    // Which lines in the span are ours to touch.
    //
    // Only a string that opened *before* the span protects its lines. If the
    // span starts in code, every literal inside it is being commented out whole,
    // token and all — so commenting each of its lines is exactly what was asked
    // and it round-trips. If the span starts inside a literal, the opening quote
    // is staying put and a `-- ` would land in the data, so those lines are left
    // alone. That is the difference between "comment out this statement" and
    // "comment out the middle of the string it writes".
    let span_starts_in_string =
        crate::pairs::region_at(text, starts[first], dialect) == crate::pairs::Region::Str;
    let in_string: Vec<bool> = (first..=last)
        .map(|i| {
            if !span_starts_in_string {
                return false;
            }
            let content = lines[i];
            let indent_len = content.len() - content.trim_start().len();
            crate::pairs::region_at(text, starts[i] + indent_len, dialect)
                == crate::pairs::Region::Str
        })
        .collect();
    let editable = |i: usize| !in_string[i - first];

    // Decide comment vs uncomment from the non-blank lines in the span.
    let mut all_commented = true;
    let mut any_nonblank = false;
    for (i, line) in lines[first..=last].iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || in_string[i] {
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
        // Leave blank lines untouched when toggling a real (has-content) block,
        // and lines that are string contents rather than code.
        if (trimmed.is_empty() && any_nonblank) || !editable(i) {
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

/// Move the line(s) spanned by the byte range `[sel_start, sel_end]` up (`up =
/// true`) or down by one line, returning the new full document text plus the
/// selection to reapply (the moved block, shifted by the swapped line). Returns
/// `None` when the move isn't possible — the block is already at the top (`up`)
/// or the bottom (`down`).
///
/// Reorders whole line *segments* (`split('\n')`) and rejoins with `\n`, so the
/// last line having no trailing newline is handled correctly. (Floem's built-in
/// `MoveLineUp`/`MoveLineDown` slices `line_start..next_line_start` assuming a
/// trailing `\n`, so moving the newline-less last line merges it into its
/// neighbour.) Applied by the UI as one full-buffer edit (a single undo step).
pub fn move_line(text: &str, sel_start: usize, sel_end: usize, up: bool) -> Option<LineEdit> {
    let len = text.len();
    let lo = sel_start.min(sel_end).min(len);
    let hi = sel_start.max(sel_end).min(len);

    let starts = line_start_offsets(text);
    let first = line_index_of(&starts, lo);
    let mut last = line_index_of(&starts, hi);
    // A selection ending exactly at a line's start doesn't pull that line in.
    if last > first && hi == starts[last] {
        last -= 1;
    }

    let mut lines: Vec<&str> = text.split('\n').collect();
    let n = lines.len();

    // Shift = the swapped neighbour segment's length + its one `\n`. Total text
    // length is invariant under reorder+rejoin, so the moved block just slides by
    // this amount; the selection follows.
    let (new_lo, new_hi) = if up {
        if first == 0 {
            return None;
        }
        let shift = lines[first - 1].len() + 1;
        let prev = lines.remove(first - 1);
        lines.insert(last, prev); // block is now at [first-1..=last-1]
        (lo - shift, hi - shift)
    } else {
        if last + 1 >= n {
            return None;
        }
        let shift = lines[last + 1].len() + 1;
        let next = lines.remove(last + 1);
        lines.insert(first, next);
        (lo + shift, hi + shift)
    };

    Some(LineEdit {
        text: lines.join("\n"),
        sel: (new_lo, new_hi),
    })
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

/// Is `needle` still exactly at byte offset `off` in `hay`, by the same
/// ASCII-case-insensitive rule [`find_matches`] uses?
///
/// The question a *stored* match offset has to answer before anything edits it.
/// The find bar keeps its hit list in a signal, and an edit elsewhere in the
/// document moves every later match: replacing at a remembered offset then
/// rewrites whatever now occupies those bytes. It really happened — inserting
/// `-- ` at the head of a two-line query turned `SELECT a FROM t;` into
/// `-- SELECT a FRx t;`, destroying the `OM` of `FROM` while the `t;` the user
/// searched for was left alone.
///
/// `false` for an out-of-range offset or one that isn't a char boundary, so the
/// caller can treat "not a match any more" and "not addressable any more" the
/// same way: re-derive.
pub fn matches_at(hay: &str, off: usize, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let end = off + needle.len();
    end <= hay.len()
        && hay.is_char_boundary(off)
        && hay.is_char_boundary(end)
        && hay.as_bytes()[off..end].eq_ignore_ascii_case(needle.as_bytes())
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

/// 1-based `(line, column)` of a byte offset within `text`, for the status-bar
/// cursor readout. Columns count *characters* from the line start (a tab is one
/// column), matching how editors display "Ln/Col". An offset past the end clamps
/// to the end; an offset landing mid-codepoint rounds down to a char boundary.
pub fn line_col_of_offset(text: &str, offset: usize) -> (usize, usize) {
    let mut end = offset.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut line = 1usize;
    let mut col = 1usize;
    for ch in text[..end].chars() {
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// A single text replacement plus the selection to apply afterward — the result
/// of a soft-tab indent ([`soft_tab_indent`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndentEdit {
    /// Byte range in the original text to replace.
    pub start: usize,
    pub end: usize,
    /// Replacement text.
    pub text: String,
    /// Selection (byte offsets into the *new* document) to apply after the edit.
    pub sel: (usize, usize),
}

/// Compute pressing Tab with **soft tabs** (spaces) in `full`, given the current
/// selection `[sel_a, sel_b]` (either order) and tab width `tw` (clamped ≥ 1).
///
/// Floem's built-in `InsertTab` uses the document buffer's own fixed indent width
/// and ignores the configured tab width, so the editor computes the edit here and
/// applies it directly. Behaviour mirrors a typical editor:
/// - A bare caret inserts spaces to the next `tw` tab stop.
/// - A selection indents every spanned line by `tw` spaces at its first non-blank
///   column (blank lines untouched) and re-selects the indented block. A selection
///   ending exactly at a line start doesn't pull that line in.
pub fn soft_tab_indent(full: &str, sel_a: usize, sel_b: usize, tw: usize) -> IndentEdit {
    let tw = tw.max(1);
    let len = full.len();
    let lo = sel_a.min(sel_b).min(len);
    let hi = sel_a.max(sel_b).min(len);
    let line_start = |off: usize| full[..off].rfind('\n').map(|i| i + 1).unwrap_or(0);

    if lo == hi {
        // Bare caret: fill to the next tab stop from the caret's column.
        let ls = line_start(lo);
        let col = full[ls..lo].chars().count();
        let n = tw - (col % tw);
        let caret = lo + n;
        return IndentEdit {
            start: lo,
            end: lo,
            text: " ".repeat(n),
            sel: (caret, caret),
        };
    }

    // Selection: re-indent each spanned line. A selection ending exactly at a line
    // start shouldn't include that (otherwise-untouched) line.
    let mut eff_hi = hi;
    if eff_hi > lo && full.as_bytes()[eff_hi - 1] == b'\n' {
        eff_hi -= 1;
    }
    let region_start = line_start(lo);
    let region_end = full[eff_hi..].find('\n').map(|i| eff_hi + i).unwrap_or(len);
    let pad = " ".repeat(tw);
    let block = &full[region_start..region_end];
    let mut out = String::with_capacity(block.len() + tw * 4);
    let mut added = 0usize;
    for (i, line) in block.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if line.is_empty() {
            continue; // leave blank lines unindented
        }
        let fnb = line.len() - line.trim_start().len(); // leading-whitespace bytes
        out.push_str(&line[..fnb]);
        out.push_str(&pad);
        out.push_str(&line[fnb..]);
        added += tw;
    }
    IndentEdit {
        start: region_start,
        end: region_end,
        text: out,
        sel: (region_start, region_end + added),
    }
}

/// Leading indentation to strip for one outdent step: a single leading tab, else
/// up to `tw` leading spaces.
fn outdent_strip_len(line: &str, tw: usize) -> usize {
    if line.starts_with('\t') {
        1
    } else {
        line.bytes().take(tw).take_while(|&b| b == b' ').count()
    }
}

/// Compute pressing Shift+Tab with **soft tabs** in `full` — the inverse of
/// [`soft_tab_indent`]. Removes one indent level (a leading tab, or up to `tw`
/// leading spaces) from each affected line:
/// - A bare caret outdents the caret's line and shifts the caret left by however
///   much was removed before it.
/// - A selection outdents every spanned line and re-selects the block. A selection
///   ending exactly at a line start doesn't pull that line in.
///
/// Lines with no leading whitespace are unchanged (so an all-unindented block is a
/// no-op — `text == full[start..end]`).
pub fn soft_tab_outdent(full: &str, sel_a: usize, sel_b: usize, tw: usize) -> IndentEdit {
    let tw = tw.max(1);
    let len = full.len();
    let lo = sel_a.min(sel_b).min(len);
    let hi = sel_a.max(sel_b).min(len);
    let caret_mode = lo == hi;
    let line_start = |off: usize| full[..off].rfind('\n').map(|i| i + 1).unwrap_or(0);

    let mut eff_hi = hi;
    if !caret_mode && eff_hi > lo && full.as_bytes()[eff_hi - 1] == b'\n' {
        eff_hi -= 1;
    }
    let region_start = line_start(lo);
    let region_end = full[eff_hi..].find('\n').map(|i| eff_hi + i).unwrap_or(len);
    let block = &full[region_start..region_end];

    let mut out = String::with_capacity(block.len());
    let mut removed_total = 0usize;
    let mut caret_new = lo;
    let mut abs = region_start;
    for (i, line) in block.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
            abs += 1; // the '\n' separator
        }
        let r = outdent_strip_len(line, tw);
        if caret_mode && abs <= lo && lo <= abs + line.len() {
            // Shift the caret left by whatever was removed before it on its line.
            caret_new = lo - r.min(lo - abs);
        }
        out.push_str(&line[r..]);
        removed_total += r;
        abs += line.len();
    }
    let sel = if caret_mode {
        (caret_new, caret_new)
    } else {
        (region_start, region_end - removed_total)
    };
    IndentEdit {
        start: region_start,
        end: region_end,
        text: out,
        sel,
    }
}

/// Byte offset of the start of 1-based `line` in `text`, or `None` if the line
/// doesn't exist (line 0, or past the last line). Used by the editor's Go-to-line
/// popup. Line 1 is offset 0; a trailing newline yields a valid final empty line.
pub fn offset_of_line(text: &str, line: usize) -> Option<usize> {
    if line == 0 {
        return None;
    }
    if line == 1 {
        return Some(0);
    }
    let mut newlines = 0usize;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            newlines += 1;
            if newlines == line - 1 {
                return Some(i + 1);
            }
        }
    }
    None
}

/// The text `range` covers in `text`, or `None` when there is no usable
/// selection — an empty or reversed range, one reaching past the end, or one
/// whose ends fall inside a multi-byte character.
///
/// Every caller holds a byte range mirrored out of the mounted editor while the
/// text comes from the tab's own signal, so the two can disagree by a keystroke.
/// A disagreement must degrade to "no selection", never to a panic: `SELECT
/// 'città'` with a stale offset is a slice through the middle of `à`.
pub fn selected_text(text: &str, range: Option<(usize, usize)>) -> Option<&str> {
    let (a, b) = range?;
    if a >= b {
        return None;
    }
    text.get(a..b).filter(|s| !s.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_is_the_text_between_its_ends() {
        assert_eq!(selected_text("SELECT a FROM t", Some((7, 8))), Some("a"));
    }

    #[test]
    fn a_caret_is_not_a_selection() {
        assert_eq!(selected_text("SELECT 1", Some((3, 3))), None);
        assert_eq!(selected_text("SELECT 1", None), None);
        // Whitespace only: the user has selected nothing worth sending.
        assert_eq!(selected_text("a   b", Some((1, 4))), None);
    }

    #[test]
    fn a_stale_or_split_range_yields_nothing_rather_than_panicking() {
        // Past the end (the buffer shrank since the mirror).
        assert_eq!(selected_text("SELECT 1", Some((0, 99))), None);
        // Reversed.
        assert_eq!(selected_text("SELECT 1", Some((5, 2))), None);
        // Mid-character: `à` is two bytes, so 5 lands inside it and 4 doesn't.
        assert_eq!(selected_text("città", Some((0, 5))), None);
        assert_eq!(selected_text("città", Some((0, 4))), Some("citt"));
    }

    fn toggled(text: &str, a: usize, b: usize) -> String {
        toggle_line_comment(text, a, b, SqlDialect::MySql).text
    }

    // ── A comment token inside a string literal is data, not a comment ────

    #[test]
    fn a_line_inside_a_string_literal_is_left_alone() {
        // The whole defect: this still parses and still runs, and writes
        // `alpha\n-- beta` where the user wrote `alpha\nbeta`. Nothing on screen
        // says the token landed in data rather than in code.
        let src = "INSERT INTO t VALUES ('alpha\nbeta');";
        let line2 = src.find("beta").unwrap();
        assert_eq!(toggled(src, line2, line2), src);
    }

    #[test]
    fn the_line_that_opens_the_string_still_comments() {
        // Only the *continuation* lines are inside the literal. Line 1 starts in
        // code, so commenting it out is exactly what the user asked for.
        let src = "INSERT INTO t VALUES ('alpha\nbeta');";
        assert_eq!(
            toggled(src, 0, 0),
            "-- INSERT INTO t VALUES ('alpha\nbeta');"
        );
    }

    #[test]
    fn commenting_a_whole_statement_takes_its_string_with_it_and_round_trips() {
        // The span starts in code, so the literal is being commented out whole —
        // opening quote included. Every line gets the token, which is what the
        // user asked for and is inert, and toggling back restores the original.
        // Protecting the continuation line here would be *worse* than not: the
        // block would half-comment, and the uncomment pass would then see a line
        // that is no longer inside any string and comment everything again.
        let src = "SELECT 1;\nINSERT INTO t VALUES ('alpha\nbeta');\nSELECT 2;";
        let out = toggled(src, 0, src.len());
        assert_eq!(
            out,
            "-- SELECT 1;\n-- INSERT INTO t VALUES ('alpha\n-- beta');\n-- SELECT 2;"
        );
        assert_eq!(toggled(&out, 0, out.len()), src);
    }

    #[test]
    fn a_selection_starting_inside_a_string_protects_it_and_still_comments_the_code_after() {
        // Drag from inside the literal past its end. The opening quote isn't
        // moving, so the lines still inside it are data; the statement after it
        // is code and comments normally.
        let src = "INSERT INTO t VALUES ('alpha\nbeta');\nSELECT 2;";
        let from = src.find("beta").unwrap();
        assert_eq!(
            toggled(src, from, src.len()),
            "INSERT INTO t VALUES ('alpha\nbeta');\n-- SELECT 2;"
        );
    }

    #[test]
    fn a_dollar_quoted_body_is_protected_on_postgres_only() {
        // `$$…$$` is a string on PG and not on MySQL, so the same buffer answers
        // differently — which is why this takes a dialect rather than assuming.
        let src = "CREATE FUNCTION f() RETURNS int AS $$\nSELECT 1;\n$$ LANGUAGE sql;";
        let line2 = src.find("SELECT 1").unwrap();
        assert_eq!(
            toggle_line_comment(src, line2, line2, SqlDialect::Postgres).text,
            src
        );
        assert_ne!(
            toggle_line_comment(src, line2, line2, SqlDialect::MySql).text,
            src
        );
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
        let sel = toggle_line_comment(src, 0, 0, SqlDialect::MySql).sel;
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

    /// The corruption this exists to prevent, as bytes: hits taken over one
    /// document, then used against the next one.
    #[test]
    fn a_hit_offset_does_not_survive_an_edit_before_it() {
        let before = "SELECT a FROM t;\nSELECT b FROM t;";
        let hits = find_matches(before, "t;");
        assert_eq!(hits, vec![14, 31]);
        let after = format!("-- {before}");
        assert!(
            !matches_at(&after, hits[0], "t;"),
            "offset 14 is now the OM of FROM"
        );
        assert!(
            matches_at(&after, hits[0] + 3, "t;"),
            "it moved by the edit"
        );
    }

    #[test]
    fn matches_at_agrees_with_find_matches_everywhere_it_reports_one() {
        let hay = "SELECT select SeLeCt";
        for off in find_matches(hay, "select") {
            assert!(matches_at(hay, off, "select"), "at {off}");
        }
        assert!(!matches_at(hay, 1, "select"), "one byte off is not a match");
    }

    #[test]
    fn matches_at_refuses_an_unaddressable_offset() {
        assert!(!matches_at("abc", 2, "bc"), "past the end");
        assert!(!matches_at("abc", 9, "a"), "beyond the string");
        assert!(!matches_at("café", 4, "é"), "not a char boundary");
        assert!(!matches_at("abc", 0, ""), "an empty needle matches nothing");
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
        let ed = toggle_line_comment(src, 0, src.len(), SqlDialect::MySql);
        // Whole new text is two commented lines; selection covers both.
        assert_eq!(ed.sel, (0, ed.text.len()));
    }

    #[test]
    fn line_col_start_of_empty() {
        assert_eq!(line_col_of_offset("", 0), (1, 1));
    }

    #[test]
    fn line_col_within_first_line() {
        assert_eq!(line_col_of_offset("abc", 0), (1, 1));
        assert_eq!(line_col_of_offset("abc", 2), (1, 3));
        assert_eq!(line_col_of_offset("abc", 3), (1, 4));
    }

    #[test]
    fn line_col_after_newline_resets_column() {
        let src = "ab\ncd";
        assert_eq!(line_col_of_offset(src, 3), (2, 1)); // start of line 2
        assert_eq!(line_col_of_offset(src, 5), (2, 3)); // end of "cd"
    }

    #[test]
    fn line_col_offset_at_newline_stays_on_first_line() {
        // Offset points at the '\n' itself → still end of line 1.
        assert_eq!(line_col_of_offset("ab\ncd", 2), (1, 3));
    }

    #[test]
    fn line_col_offset_past_end_clamps() {
        assert_eq!(line_col_of_offset("ab\ncd", 999), (2, 3));
    }

    #[test]
    fn line_col_counts_characters_not_bytes() {
        // "á" is two UTF-8 bytes; after it the column is 2 (one character).
        assert_eq!(line_col_of_offset("áb", 2), (1, 2));
        // A mid-codepoint offset rounds down to the char boundary at 0.
        assert_eq!(line_col_of_offset("áb", 1), (1, 1));
    }

    #[test]
    fn offset_of_line_basics() {
        let src = "ab\ncd\nef";
        assert_eq!(offset_of_line(src, 1), Some(0));
        assert_eq!(offset_of_line(src, 2), Some(3));
        assert_eq!(offset_of_line(src, 3), Some(6));
    }

    #[test]
    fn offset_of_line_zero_and_past_end() {
        let src = "ab\ncd";
        assert_eq!(offset_of_line(src, 0), None);
        assert_eq!(offset_of_line(src, 3), None); // only 2 lines
        assert_eq!(offset_of_line(src, 99), None);
    }

    #[test]
    fn offset_of_line_trailing_newline_has_final_empty_line() {
        let src = "ab\n";
        assert_eq!(offset_of_line(src, 2), Some(3)); // empty line 2 at end
        assert_eq!(offset_of_line(src, 3), None);
    }

    #[test]
    fn offset_of_line_empty_text() {
        assert_eq!(offset_of_line("", 1), Some(0));
        assert_eq!(offset_of_line("", 2), None);
    }

    #[test]
    fn offset_of_line_roundtrips_with_line_col() {
        let src = "one\ntwo\nthree";
        for line in 1..=3 {
            let off = offset_of_line(src, line).unwrap();
            assert_eq!(line_col_of_offset(src, off), (line, 1));
        }
    }

    #[test]
    fn soft_tab_caret_at_line_start_inserts_full_width() {
        let e = soft_tab_indent("abc", 0, 0, 4);
        assert_eq!(
            e,
            IndentEdit {
                start: 0,
                end: 0,
                text: "    ".into(),
                sel: (4, 4)
            }
        );
        let e2 = soft_tab_indent("abc", 0, 0, 2);
        assert_eq!(e2.text, "  ");
        assert_eq!(e2.sel, (2, 2));
    }

    #[test]
    fn soft_tab_caret_fills_to_next_stop() {
        // Caret after 1 char, width 4 → 3 spaces to reach column 4.
        let e = soft_tab_indent("a", 1, 1, 4);
        assert_eq!(e.text, "   ");
        assert_eq!(e.sel, (4, 4));
        // Caret after 2 chars, width 2 → already at a stop → a full 2 spaces.
        let e2 = soft_tab_indent("ab", 2, 2, 2);
        assert_eq!(e2.text, "  ");
    }

    #[test]
    fn soft_tab_caret_column_counts_from_line_start() {
        // Second line, caret after 1 char → 3 spaces (width 4).
        let src = "xxxx\na";
        let e = soft_tab_indent(src, 6, 6, 4);
        assert_eq!(e.text, "   ");
    }

    #[test]
    fn soft_tab_clamps_zero_width() {
        let e = soft_tab_indent("a", 0, 0, 0);
        assert_eq!(e.text, " "); // tw clamped to 1
    }

    #[test]
    fn soft_tab_single_line_selection_indents_the_line() {
        // Selecting "bc" in "abc" indents the whole line by 2 at its start.
        let e = soft_tab_indent("abc", 1, 3, 2);
        assert_eq!(e.start, 0);
        assert_eq!(e.end, 3);
        assert_eq!(e.text, "  abc");
        assert_eq!(e.sel, (0, 5));
    }

    #[test]
    fn soft_tab_indents_after_existing_indentation() {
        // A line already indented by 2 gets 2 more (inserted after the leading ws).
        let e = soft_tab_indent("  x", 3, 3, 2);
        // caret case (col 3 → next stop at 4 → 1 space)
        assert_eq!(e.text, " ");
    }

    #[test]
    fn soft_tab_multiline_selection_indents_each_nonblank_line() {
        let src = "a\nb\nc";
        // Select from start of "a" to end of "c".
        let e = soft_tab_indent(src, 0, 5, 2);
        assert_eq!(e.start, 0);
        assert_eq!(e.end, 5);
        assert_eq!(e.text, "  a\n  b\n  c");
        assert_eq!(e.sel, (0, 5 + 6)); // 3 lines × 2 spaces
    }

    #[test]
    fn soft_tab_multiline_skips_blank_lines() {
        let src = "a\n\nb";
        let e = soft_tab_indent(src, 0, 4, 2);
        assert_eq!(e.text, "  a\n\n  b"); // middle blank line untouched
        assert_eq!(e.sel, (0, 4 + 4)); // only 2 lines indented
    }

    #[test]
    fn soft_tab_selection_ending_at_line_start_excludes_that_line() {
        let src = "a\nb\nc";
        // Select "a\n" (ends at start of line 2) → only line 1 indented.
        let e = soft_tab_indent(src, 0, 2, 2);
        assert_eq!(e.end, 1); // region ends at end of line 1, not into line 2
        assert_eq!(e.text, "  a");
    }

    #[test]
    fn outdent_caret_removes_up_to_width_and_shifts_caret() {
        // "    x", caret after the 4 spaces (offset 4), width 4 → strip 4, caret→0.
        let e = soft_tab_outdent("    x", 4, 4, 4);
        assert_eq!((e.start, e.end), (0, 5));
        assert_eq!(e.text, "x");
        assert_eq!(e.sel, (0, 0));
    }

    #[test]
    fn outdent_caret_partial_indent() {
        // Only 2 leading spaces though width is 4 → remove the 2 present.
        let e = soft_tab_outdent("  x", 3, 3, 4);
        assert_eq!(e.text, "x");
        assert_eq!(e.sel, (1, 1)); // caret 3 shifted left by 2
    }

    #[test]
    fn outdent_caret_inside_indentation_clamps() {
        // Caret at offset 1 inside 4 leading spaces → all 4 removed, caret clamps to 0.
        let e = soft_tab_outdent("    x", 1, 1, 4);
        assert_eq!(e.text, "x");
        assert_eq!(e.sel, (0, 0));
    }

    #[test]
    fn outdent_no_leading_whitespace_is_noop() {
        let e = soft_tab_outdent("x", 1, 1, 4);
        assert_eq!(e.text, "x"); // unchanged
        assert_eq!(e.text, "x".get(e.start..e.end).unwrap_or(""));
    }

    #[test]
    fn outdent_removes_a_leading_tab() {
        let e = soft_tab_outdent("\tx", 2, 2, 4);
        assert_eq!(e.text, "x");
        assert_eq!(e.sel, (1, 1));
    }

    #[test]
    fn outdent_multiline_selection() {
        let src = "  a\n    b\nc";
        // Select all three lines.
        let e = soft_tab_outdent(src, 0, src.len(), 2);
        assert_eq!(e.text, "a\n  b\nc"); // -2 from line1, -2 from line2, none from line3
        assert_eq!(e.sel, (0, src.len() - 4));
    }

    #[test]
    fn outdent_roundtrips_indent() {
        // Indent then outdent a caret line → back to the original caret + text.
        let src = "x";
        let ind = soft_tab_indent(src, 0, 0, 4); // "    " inserted, caret 4
        let indented = format!("{}{}", ind.text, src); // "    x"
        let out = soft_tab_outdent(&indented, ind.sel.0, ind.sel.1, 4);
        assert_eq!(out.text, "x");
        assert_eq!(out.sel, (0, 0));
    }

    // --- move_line ---------------------------------------------------------

    #[test]
    fn move_line_up_swaps_with_previous() {
        // caret on line "b" (offset 2) → moves above "a"
        let e = move_line("a\nb\nc", 2, 2, true).unwrap();
        assert_eq!(e.text, "b\na\nc");
        assert_eq!(e.sel, (0, 0)); // caret followed "b" up by len("a")+1 = 2
    }

    #[test]
    fn move_line_down_swaps_with_next() {
        // caret on line "a" (offset 0) → moves below "b"
        let e = move_line("a\nb\nc", 0, 0, false).unwrap();
        assert_eq!(e.text, "b\na\nc");
        assert_eq!(e.sel, (2, 2));
    }

    #[test]
    fn move_last_line_up_keeps_newline() {
        // The reported bug: last line has no trailing newline; moving it up must
        // NOT merge the previous line into it.
        let e = move_line("s1;\ns2;\ns3;", 8, 8, true).unwrap();
        assert_eq!(e.text, "s1;\ns3;\ns2;");
        // caret was at start of "s3;" (offset 8) → follows up by len("s2;")+1 = 4
        assert_eq!(e.sel, (4, 4));
    }

    #[test]
    fn move_second_last_line_down_keeps_newline() {
        // The mirror case: moving the second-to-last line down past the
        // newline-less last line must not merge them either.
        let e = move_line("s1;\ns2;\ns3;", 4, 4, false).unwrap();
        assert_eq!(e.text, "s1;\ns3;\ns2;");
        assert_eq!(e.sel, (8, 8));
    }

    #[test]
    fn move_line_at_edges_is_noop() {
        assert!(move_line("a\nb", 0, 0, true).is_none()); // first line up
        assert!(move_line("a\nb", 2, 2, false).is_none()); // last line down
    }

    #[test]
    fn move_multiline_selection_up() {
        // Select lines "b" and "c" (offsets 2..5) → whole block moves above "a".
        let e = move_line("a\nb\nc\nd", 2, 5, true).unwrap();
        assert_eq!(e.text, "b\nc\na\nd");
        assert_eq!(e.sel, (0, 3)); // block shifted up by len("a")+1 = 2
    }

    #[test]
    fn move_selection_ending_at_line_start_excludes_that_line() {
        // Selection covers "a" and ends exactly at the start of "b" → only "a"
        // moves down (not "b").
        let e = move_line("a\nb\nc", 0, 2, false).unwrap();
        assert_eq!(e.text, "b\na\nc");
        assert_eq!(e.sel, (2, 4));
    }
}
