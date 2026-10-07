//! Pure editor text operations (no UI): the query editor's line and selection
//! edits (Ctrl+/, Ctrl+D, Ctrl+X, Alt+↑↓, Tab/Shift+Tab, Enter, Ctrl+Shift+U,
//! Ctrl+Shift+J, Ctrl+Shift+/), the search helpers the find surfaces share, and line/column
//! arithmetic. Each edit is computed with the selection the caret should occupy
//! afterwards, which the pane applies as one edit through `edit_untyped` (so
//! it's one undo step).

use crate::intel::SqlDialect;
use crate::pairs::{Region, region_at};
use crate::sql::{is_word_byte, skip_noncode};

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
    //
    // **One walk of the text for the whole span**, not one per line.
    // `region_at` scans from the start of the document, so asking it per line
    // is quadratic: measured 223 ms over a 3,200-line selection, quadrupling
    // with every doubling, on the UI thread, for one Ctrl+/. The probes are
    // the span's own start followed by each line's first non-blank byte, which
    // is already in order — see `pairs::regions_at`.
    let probe_of = |i: usize| {
        let content: &str = lines[i];
        starts[i] + (content.len() - content.trim_start().len())
    };
    let mut probes: Vec<usize> = Vec::with_capacity(last - first + 2);
    probes.push(starts[first]);
    probes.extend((first..=last).map(probe_of));
    let regions = crate::pairs::regions_at(text, &probes, dialect);
    let span_starts_in_string = regions[0] == crate::pairs::Region::Str;
    let in_string: Vec<bool> = (first..=last)
        .map(|i| span_starts_in_string && regions[i - first + 1] == crate::pairs::Region::Str)
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

/// Ctrl+Shift+U: upper-case the selection — or, on a bare caret, the word it
/// touches ([`is_word_byte`]) — unless it has no lower-case letter, in which
/// case lower-case it — so presses alternate between the two cases, though
/// mixed case does not come back (`Name` → `NAME` → `name`). A selection stays
/// selected over its new extent (a character can change length: `ß` → `SS`);
/// a bare caret stays where it was. `None` when there is nothing to change.
pub fn toggle_case(full: &str, sel_a: usize, sel_b: usize) -> Option<IndentEdit> {
    let b = full.as_bytes();
    let lo = sel_a.min(sel_b).min(full.len());
    let hi = sel_a.max(sel_b).min(full.len());
    let (start, end) = if lo == hi {
        let mut s = lo;
        while s > 0 && is_word_byte(b[s - 1]) {
            s -= 1;
        }
        let mut e = lo;
        while e < b.len() && is_word_byte(b[e]) {
            e += 1;
        }
        (s, e)
    } else {
        (lo, hi)
    };
    let old = full.get(start..end)?;
    let text = if old.chars().any(char::is_lowercase) {
        old.to_uppercase()
    } else {
        old.to_lowercase()
    };
    if text == old {
        return None;
    }
    let sel = if lo == hi {
        let at = lo.min(start + text.len());
        (at, at)
    } else {
        (start, start + text.len())
    };
    Some(IndentEdit {
        start,
        end,
        text,
        sel,
    })
}

/// Ctrl+Shift+J: join the caret's line with the next — or every line the
/// selection spans — each line break and the blanks around it becoming one
/// space. No space where one would only pad a bracket or a separator (after
/// `(`/`[`, before `)`/`]`/`,`/`;`) or where either side is empty.
///
/// **Only a break in code joins.** One inside a string is the string's data,
/// and one ending a `--`/`#` comment is what ends the comment — joined, the
/// next line's code would be commented out; both stay where they are. A
/// selection ending at a line's start does not pull that line in. The caret
/// lands at the first join; a selection is kept over the joined text. `None`
/// when no break joins.
pub fn join_lines(
    full: &str,
    sel_a: usize,
    sel_b: usize,
    dialect: SqlDialect,
) -> Option<IndentEdit> {
    let b = full.as_bytes();
    let len = full.len();
    let lo = floor_char(full, sel_a.min(sel_b));
    let hi = floor_char(full, sel_a.max(sel_b));
    let last = if hi > lo && b[hi - 1] == b'\n' {
        hi - 1
    } else {
        hi
    };
    let first_ls = full[..lo].rfind('\n').map_or(0, |i| i + 1);
    let last_le = full[last..].find('\n').map_or(len, |i| last + i);
    let mut breaks: Vec<usize> = (first_ls..last_le).filter(|&i| b[i] == b'\n').collect();
    if breaks.is_empty() {
        if last_le == len {
            return None;
        }
        breaks.push(last_le);
    }

    // Which breaks the lexer says are not code: inside a span, or the end of
    // a line comment (its span stops at the `\n`, `\r` included).
    let mut kept = vec![false; breaks.len()];
    let (mut i, mut k) = (0, 0);
    let scan_end = breaks[breaks.len() - 1];
    while i <= scan_end && k < breaks.len() {
        let Some(j) = skip_noncode(b, i, dialect) else {
            i += 1;
            continue;
        };
        let j = j.max(i + 1);
        while k < breaks.len() && breaks[k] < i {
            k += 1;
        }
        while k < breaks.len() && breaks[k] < j {
            kept[k] = true;
            k += 1;
        }
        if matches!(b[i], b'-' | b'#') && k < breaks.len() && breaks[k] == j {
            kept[k] = true;
            k += 1;
        }
        i = j;
    }

    let blank = |c: u8| c == b' ' || c == b'\t';
    let mut out = String::new();
    let mut span: Option<(usize, usize)> = None; // (start, end) replaced
    for (&nl, _) in breaks.iter().zip(&kept).filter(|(_, kept)| !**kept) {
        let floor = span.map_or(0, |(_, e)| e);
        let mut s = nl;
        while s > floor && (blank(b[s - 1]) || b[s - 1] == b'\r') {
            s -= 1;
        }
        let mut e = nl + 1;
        while e < len && blank(b[e]) {
            e += 1;
        }
        let start = span.map_or(s, |(st, _)| st);
        out.push_str(&full[span.map_or(s, |(_, pe)| pe)..s]);
        let left = out.chars().last().or_else(|| full[..start].chars().last());
        let right = full[e..].chars().next();
        let tight = matches!(left, None | Some('\n' | '(' | '['))
            || matches!(right, None | Some('\n' | '\r' | ')' | ']' | ',' | ';'));
        if !tight {
            out.push(' ');
        }
        span = Some((start, e));
    }
    let (start, end) = span?;
    let map = |off: usize| {
        if off <= start {
            off
        } else if off >= end {
            off - (end - start) + out.len()
        } else {
            start + out.len()
        }
    };
    let sel = if lo == hi {
        (start, start)
    } else {
        (map(lo), map(hi))
    };
    Some(IndentEdit {
        start,
        end,
        text: out,
        sel,
    })
}

/// `i`, clamped to `full`'s length and taken back to the start of the
/// character it falls inside.
fn floor_char(full: &str, i: usize) -> usize {
    let mut i = i.min(full.len());
    while !full.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Ctrl+Shift+/: wrap the selection — or, on a bare caret, its line's text —
/// in `/* … */`, or unwrap the block comment it already sits in.
///
/// - **Unwrap** when the caret is inside a block comment, or at either end of
///   a line that is only one, or the selection (its outer blanks aside) lies
///   within one: the `/*` and `*/` go, with one
///   space inside each if there is one.
/// - **Wrap** otherwise, the selection's outer blanks left outside. The result
///   is lexed before it is offered: it has to be one comment ending at its own
///   `*/`. An inner `*/` ends a comment early on MySQL and SQLite — wrapping
///   would leave the rest as code — and is fine where comments nest; a range
///   starting or ending inside a string would put the comment in the data.
///   Either is `None`, as is a range with nothing in it.
///
/// A selection stays over the wrapped comment or the unwrapped text, so a
/// second press undoes the first; a bare caret keeps its place in the text.
pub fn toggle_block_comment(
    full: &str,
    sel_a: usize,
    sel_b: usize,
    dialect: SqlDialect,
) -> Option<IndentEdit> {
    let b = full.as_bytes();
    let len = full.len();
    let lo = floor_char(full, sel_a.min(sel_b));
    let hi = floor_char(full, sel_a.max(sel_b));
    let bare = lo == hi;
    // What would be wrapped: the selection, or the caret's line, unpadded.
    let (from, to) = if bare {
        let ls = full[..lo].rfind('\n').map_or(0, |i| i + 1);
        let le = full[lo..].find('\n').map_or(len, |i| lo + i);
        (ls, le)
    } else {
        (lo, hi)
    };
    let s = &full[from..to];
    let rlo = from + (s.len() - s.trim_start().len());
    let rhi = rlo + s.trim().len();

    // Unwrap: the block comment the caret, or the whole selection, sits in.
    let limit = if bare { lo } else { rlo };
    let mut i = 0;
    while i < len && i <= limit {
        let Some(j) = skip_noncode(b, i, dialect) else {
            i += 1;
            continue;
        };
        let j = j.max(i + 1);
        let block = j - i >= 4 && full[i..j].starts_with("/*") && full[..j].ends_with("*/");
        // A bare caret also unwraps a line that is only the comment, from
        // either end of it.
        let inside = if bare {
            (i < lo && lo < j) || (i == rlo && j == rhi)
        } else {
            rlo < rhi && i <= rlo && rhi <= j
        };
        if block && inside {
            let inner = &full[i + 2..j - 2];
            let lead = usize::from(inner.starts_with(' '));
            let inner = &inner[lead..];
            let text = inner.strip_suffix(' ').unwrap_or(inner).to_string();
            let sel = if bare {
                let at = i + (lo.saturating_sub(i + 2 + lead)).min(text.len());
                (at, at)
            } else {
                (i, i + text.len())
            };
            return Some(IndentEdit {
                start: i,
                end: j,
                text,
                sel,
            });
        }
        i = j;
    }

    // Wrap — only what lexes back as one comment, ending at its own `*/`,
    // and only a range whose ends are both in code.
    if rlo == rhi
        || region_at(full, rlo, dialect) != Region::Code
        || region_at(full, rhi, dialect) != Region::Code
    {
        return None;
    }
    let text = format!("/* {} */", &full[rlo..rhi]);
    if !crate::sql::is_one_comment(&text, dialect) {
        return None;
    }
    let sel = if !bare {
        (rlo, rlo + text.len())
    } else if lo < rlo {
        (lo, lo)
    } else if lo <= rhi {
        (lo + 3, lo + 3)
    } else {
        (lo + 6, lo + 6)
    };
    Some(IndentEdit {
        start: rlo,
        end: rhi,
        text,
        sel,
    })
}

/// Ctrl+D's duplicate: the line(s) spanned by `[sel_start, sel_end]` copied onto
/// fresh lines below them. Returns `(at, insert, sel)` — insert `insert` at
/// `at` (the end of the last spanned line, before its newline) and reapply
/// `sel`, the same selection or caret on the copy.
///
/// The copy is `\n` + the lines rather than the lines + `\n`, so the newline-less
/// last line duplicates onto a line of its own. Floem's `DuplicateLineDown`
/// slices `line_start..next_line_start`, which on that line holds no `\n`, and
/// the copy landed on the same line — `SELECT 1SELECT 1`. A selection ending
/// exactly at a line's start does not pull that line in, as in [`move_line`].
pub fn duplicate_lines(
    text: &str,
    sel_start: usize,
    sel_end: usize,
) -> (usize, String, (usize, usize)) {
    let len = text.len();
    let lo = sel_start.min(sel_end).min(len);
    let hi = sel_start.max(sel_end).min(len);
    let starts = line_start_offsets(text);
    let first = line_index_of(&starts, lo);
    let mut last = line_index_of(&starts, hi);
    if last > first && hi == starts[last] {
        last -= 1;
    }
    let ls = starts[first];
    let le = starts.get(last + 1).map_or(len, |&next| next - 1);
    let shift = le - ls + 1;
    (le, format!("\n{}", &text[ls..le]), (lo + shift, hi + shift))
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

/// What Ctrl+F / Ctrl+H put in the find field: the selected text, when there
/// is some and it sits on one line. A multi-line selection is a block being
/// worked on, not a needle — the field is one line — so it seeds nothing and
/// the field keeps what it held. `sel` is `(start, end)` in either order.
pub fn find_seed(text: &str, sel: (usize, usize)) -> Option<&str> {
    let (a, b) = (sel.0.min(sel.1), sel.0.max(sel.1));
    let s = text.get(a..b)?;
    (!s.is_empty() && !s.contains(['\n', '\r'])).then_some(s)
}

/// The hit a new find query starts at: the first at or after `from` (the
/// selection's start, or the caret), wrapping to the first in the document —
/// so a needle seeded from the selection, or typed one character at a time,
/// stays on the match the user is looking at rather than leaping to the top.
/// `0` for no hits, which the caller reads as "none" by the list's length.
pub fn first_hit_from(hits: &[usize], from: usize) -> usize {
    let i = hits.partition_point(|&h| h < from);
    if i < hits.len() { i } else { 0 }
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

/// [`contains_ignore_ascii_case`] against `hay` **as if its whitespace runs were
/// collapsed to single spaces**, without building the collapsed string.
///
/// The two search boxes that read SQL — Query History and Server Activity — both
/// want this: a statement is stored as the user typed it, over several lines,
/// and is drawn as one collapsed line, so a phrase visibly on screen has to find
/// the row it is on. Both got there by allocating the collapsed form
/// (`history`'s since-deleted `full_preview`: a `Vec<&str>` and a `String`, both the size of the
/// statement) **per candidate, per keystroke**. The activity panel is the one
/// where that bites: `information_schema.PROCESSLIST.INFO` is the *untruncated*
/// statement, unlike `SHOW PROCESSLIST`'s 100 characters, it holds up to
/// `MAX_SESSIONS` of them, the statement test is last in the `||` chain so the
/// common while-typing case (a partial word matching nothing yet) pays for every
/// row, and the filter re-runs on every two-second poll besides.
///
/// Leading and trailing whitespace is dropped and each internal run counts as
/// one space — exactly `split_whitespace().collect::<Vec<_>>().join(" ")`, which
/// is what this replaces. The **needle** is used as given, which is also what it
/// replaces: it comes from a single-line search box.
///
/// `O(n·m)` like its uncollapsed sibling and for the same reason — these are
/// short needles over a bounded list, and a two-way search would be a different
/// function's job.
pub fn contains_collapsed_ignore_ascii_case(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut cur = Collapsed::new(hay.as_bytes());
    loop {
        let mut probe = cur;
        if needle
            .bytes()
            .all(|n| probe.next().is_some_and(|h| h.eq_ignore_ascii_case(&n)))
        {
            return true;
        }
        if cur.next().is_none() {
            return false;
        }
    }
}

/// The bytes of a string with whitespace runs collapsed, produced on the fly.
///
/// `Copy`, which is what lets the search above restart from a position without
/// re-deriving it.
#[derive(Clone, Copy)]
struct Collapsed<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Collapsed<'a> {
    /// Positioned past any **leading** whitespace, which the collapsed form does
    /// not have.
    fn new(b: &'a [u8]) -> Self {
        let mut i = 0;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        Self { b, i }
    }
}

impl Iterator for Collapsed<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<u8> {
        let c = *self.b.get(self.i)?;
        if !c.is_ascii_whitespace() {
            self.i += 1;
            return Some(c);
        }
        // A run. It becomes one space — unless nothing follows it, in which case
        // it is trailing whitespace and the collapsed form ends here.
        let mut j = self.i;
        while j < self.b.len() && self.b[j].is_ascii_whitespace() {
            j += 1;
        }
        if j == self.b.len() {
            self.i = j;
            return None;
        }
        self.i = j;
        Some(b' ')
    }
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
/// of a Tab, Shift+Tab, Enter, Ctrl+Shift+U, Ctrl+Shift+J or Ctrl+Shift+/
/// ([`soft_tab_indent`], [`hard_tab_indent`], [`soft_tab_outdent`],
/// [`newline_indent`], [`toggle_case`], [`join_lines`],
/// [`toggle_block_comment`]).
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

    indent_lines(full, lo, hi, &" ".repeat(tw))
}

/// Compute pressing Enter over the selection `[sel_a, sel_b]` (either order):
/// the selection becomes a line break plus an indent, with `unit` — one level
/// as the tab settings spell it, spaces or `\t` — standing for a level.
///
/// - The new line keeps the caret's line's indent, cut at the caret, so Enter
///   inside the indent does not add to it.
/// - **After an open `(` or `[` in code** it goes one `unit` deeper; and when
///   the matching closer follows the caret (`(|)`), the closer moves to a line
///   of its own at the old indent, the caret on the deeper line between them.
///   A bracket in a string or comment opens nothing.
/// - **After a block-opening word** — `BEGIN`, `CASE`, `THEN`, `ELSE`,
///   `LOOP`, `DO`, `REPEAT` ([`opens_block`]) — the same, with an `END`
///   following the caret in the closer's place.
/// - At the end of a line whose next line is deeper, it takes that line's
///   indent — the way floem's own Enter answered, kept so a list being
///   extended stays in step.
///
/// Floem's Enter never added a level, and its auto-indent, had it been on,
/// adds the buffer's own unit — four spaces, which nothing here can set —
/// rather than the configured one.
///
/// An offset inside a character is taken back to that character's start,
/// rather than panicking on the slice: the caller reads the text from the
/// pane's `query` mirror, not the buffer the offsets came from.
pub fn newline_indent(
    full: &str,
    sel_a: usize,
    sel_b: usize,
    unit: &str,
    dialect: SqlDialect,
) -> IndentEdit {
    let len = full.len();
    let lo = floor_char(full, sel_a.min(sel_b));
    let hi = floor_char(full, sel_a.max(sel_b));
    let is_blank = |c: char| c == ' ' || c == '\t';
    let ls = full[..lo].rfind('\n').map_or(0, |i| i + 1);
    let indent_of = |from: usize| {
        let line = &full[from..];
        &line[..line.len() - line.trim_start_matches(is_blank).len()]
    };
    let mut base = indent_of(ls);
    base = &base[..base.len().min(lo - ls)];
    let line_end = full[hi..].find('\n').map_or(len, |i| hi + i);
    let rest = full[hi..line_end]
        .trim_end_matches('\r')
        .trim_start_matches(is_blank);

    // The position *before* the bracket is asked, not after: a `(` closing a
    // line comment would read as code one byte later, at the comment's end.
    let before = full[ls..lo].trim_end_matches(is_blank);
    let in_code = |at: usize| region_at(full, at, dialect) == Region::Code;
    let opener = before
        .bytes()
        .last()
        .filter(|&b| b == b'(' || b == b'[')
        .filter(|_| in_code(ls + before.len() - 1));
    let edit = |text: String, caret_in_text: usize| IndentEdit {
        start: lo,
        end: hi,
        text,
        sel: (lo + caret_in_text, lo + caret_in_text),
    };
    if let Some(open) = opener {
        let inner = format!("\n{base}{unit}");
        let closer = if open == b'(' { ')' } else { ']' };
        if rest.starts_with(closer) {
            let caret = inner.len();
            return edit(format!("{inner}\n{base}"), caret);
        }
        let caret = inner.len();
        return edit(inner, caret);
    }
    // A block-opening word ends the line: one level deeper, and its `END`, if
    // it follows the caret, to a line of its own — the bracket's answer.
    if opens_block(before, |at| in_code(ls + at)) {
        let inner = format!("\n{base}{unit}");
        let caret = inner.len();
        let end_follows = rest.get(..3).is_some_and(|w| w.eq_ignore_ascii_case("END"))
            && rest.as_bytes().get(3).is_none_or(|&c| !is_word_byte(c));
        return if end_follows {
            edit(format!("{inner}\n{base}"), caret)
        } else {
            edit(inner, caret)
        };
    }
    // At a line's end, a deeper next line lends its indent (floem's answer).
    if rest.is_empty() && line_end < len {
        let next = indent_of(line_end + 1);
        if next.len() > base.len() {
            base = next;
        }
    }
    let text = format!("\n{base}");
    let caret = text.len();
    edit(text, caret)
}

/// Does `before` — a line up to the caret, trailing blanks off — end with a
/// word that opens a block: `BEGIN` (T-SQL's `BEGIN TRY`/`BEGIN CATCH` too),
/// `CASE`, `THEN`, `ELSE`, `LOOP`, `DO`, `REPEAT`? A bare word in code
/// (`in_code`, asked of an offset into `before`), not a member (`t.begin`),
/// a variable or a quoted name. `BEGIN TRAN` is a transaction, and its last
/// word is not one of these.
fn opens_block(before: &str, in_code: impl Fn(usize) -> bool) -> bool {
    let last_word = |s: &str| {
        let at = s
            .bytes()
            .rposition(|c| !is_word_byte(c))
            .map_or(0, |i| i + 1);
        (at < s.len()).then_some(at)
    };
    let Some(at) = last_word(before) else {
        return false;
    };
    if at > 0 && matches!(before.as_bytes()[at - 1], b'.' | b'@' | b'$' | b'#') || !in_code(at) {
        return false;
    }
    let word = before[at..].to_ascii_uppercase();
    match word.as_str() {
        "BEGIN" | "CASE" | "THEN" | "ELSE" | "LOOP" | "DO" | "REPEAT" => true,
        "TRY" | "CATCH" => {
            let prev = before[..at].trim_end_matches([' ', '\t']);
            last_word(prev).is_some_and(|p| prev[p..].eq_ignore_ascii_case("BEGIN"))
        }
        _ => false,
    }
}

/// Typing `ch` at a bare caret (`sel_a == sel_b`) when it completes `END` as
/// the only word on its line: the edit that puts the line at the indent of
/// the line whose `BEGIN` or `CASE` it closes ([`crate::pairs::match_block`]),
/// `ch` included. `None` — the character types as usual — when it does not
/// complete such an `END`, there is more on the line, nothing matches, or the
/// line is already at that indent.
pub fn dedent_typed_end(
    full: &str,
    sel_a: usize,
    sel_b: usize,
    ch: char,
    dialect: SqlDialect,
) -> Option<IndentEdit> {
    if sel_a != sel_b || !ch.eq_ignore_ascii_case(&'d') {
        return None;
    }
    let caret = floor_char(full, sel_a);
    let ls = full[..caret].rfind('\n').map_or(0, |i| i + 1);
    let line_end = full[caret..].find('\n').map_or(full.len(), |i| caret + i);
    if !full[caret..line_end].trim().is_empty() {
        return None;
    }
    let before = &full[ls..caret];
    let word = before.trim_start_matches([' ', '\t']);
    if !word.eq_ignore_ascii_case("EN") {
        return None;
    }
    // Asked of the text as it will be, with `ch` typed.
    let typed = format!("{}{ch}{}", &full[..caret], &full[caret..]);
    let end_at = caret - word.len();
    let (_, (opener, _)) = crate::pairs::match_block(&typed, end_at, dialect)?;
    let ols = typed[..opener].rfind('\n').map_or(0, |i| i + 1);
    let line = &typed[ols..];
    let indent = &line[..line.len() - line.trim_start_matches([' ', '\t']).len()];
    if indent == &before[..before.len() - word.len()] {
        return None;
    }
    let text = format!("{indent}{word}{ch}");
    let at = ls + text.len();
    Some(IndentEdit {
        start: ls,
        end: caret,
        text,
        sel: (at, at),
    })
}

/// [`soft_tab_indent`] with **hard tabs**: a bare caret inserts one `\t`, a
/// selection indents each spanned line by one, as the soft-tab one does.
///
/// Floem's `InsertTab`, which hard tabs used to fall through to, replaces a
/// selection with a single `\t` — Tab over three selected lines left one tab
/// character where they had been.
pub fn hard_tab_indent(full: &str, sel_a: usize, sel_b: usize) -> IndentEdit {
    let len = full.len();
    let lo = sel_a.min(sel_b).min(len);
    let hi = sel_a.max(sel_b).min(len);
    if lo == hi {
        return IndentEdit {
            start: lo,
            end: lo,
            text: "\t".to_string(),
            sel: (lo + 1, lo + 1),
        };
    }
    indent_lines(full, lo, hi, "\t")
}

/// Indent every line `[lo, hi)` spans by `pad` at its first non-blank column
/// (blank lines untouched), and re-select the block. A selection ending exactly
/// at a line start doesn't pull that line in.
fn indent_lines(full: &str, lo: usize, hi: usize, pad: &str) -> IndentEdit {
    let len = full.len();
    let line_start = |off: usize| full[..off].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let mut eff_hi = hi;
    if eff_hi > lo && full.as_bytes()[eff_hi - 1] == b'\n' {
        eff_hi -= 1;
    }
    let region_start = line_start(lo);
    let region_end = full[eff_hi..].find('\n').map(|i| eff_hi + i).unwrap_or(len);
    let block = &full[region_start..region_end];
    let mut out = String::with_capacity(block.len() + pad.len() * 4);
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
        out.push_str(pad);
        out.push_str(&line[fnb..]);
        added += pad.len();
    }
    IndentEdit {
        start: region_start,
        end: region_end,
        text: out,
        sel: (region_start, region_end + added),
    }
}

/// What Ctrl+X on bare carets does — see [`cut_lines`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineCut {
    /// The lines cut, each with its line end, in document order.
    pub clipboard: String,
    /// The whole document after the cut.
    pub text: String,
    /// Where the caret goes: the start of the line that followed the first cut
    /// — or, when that cut was the last line, of the line before it.
    pub caret: usize,
}

/// Ctrl+X with no selection: cut the line under each caret in `carets`, line
/// end and all, onto the clipboard — and only those lines.
///
/// It was floem's `DeleteLine`, which **never copies**, so the paste after it
/// brought back whatever the clipboard held before; and with several carets
/// it deleted from the first to the last, every line between included. A last
/// line without a line end of its own takes the one before it, so no blank
/// line is left behind, and goes onto the clipboard with a `\n` so a paste puts
/// it back as a line.
pub fn cut_lines(full: &str, carets: &[usize]) -> LineCut {
    let len = full.len();
    let line_start = |off: usize| full[..off].rfind('\n').map_or(0, |i| i + 1);
    let line_end = |off: usize| full[off..].find('\n').map_or(len, |i| off + i + 1);
    let mut lines: Vec<(usize, usize)> = carets
        .iter()
        .map(|&c| {
            let ls = line_start(c.min(len));
            (ls, line_end(ls))
        })
        .collect();
    lines.sort_unstable();
    lines.dedup();
    let mut clipboard = String::new();
    for &(a, b) in &lines {
        clipboard.push_str(&full[a..b]);
        if !full[a..b].ends_with('\n') {
            clipboard.push('\n');
        }
    }
    // What goes: each line, the last one's preceding line end in its place.
    let mut text = String::with_capacity(len);
    let mut at = 0;
    let mut first_cut = None;
    for &(a, b) in &lines {
        let a = if b == len && !full[a..b].ends_with('\n') && a > 0 {
            a - 1
        } else {
            a
        };
        let a = a.max(at);
        text.push_str(&full[at..a]);
        first_cut.get_or_insert(text.len());
        at = b;
    }
    text.push_str(&full[at..]);
    // The start of the line the first cut is on — the line after it, or the
    // one before when the last line took the line end before it.
    let cut = first_cut.unwrap_or(0).min(text.len());
    let caret = text[..cut].rfind('\n').map_or(0, |i| i + 1);
    LineCut {
        clipboard,
        text,
        caret,
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

/// Compute pressing Shift+Tab in `full`, soft tabs or hard — the inverse of
/// [`soft_tab_indent`] and [`hard_tab_indent`]. Removes one indent level (a
/// leading tab, or up to `tw` leading spaces) from each affected line:
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

    #[test]
    fn find_seeds_from_a_one_line_selection_only() {
        let t = "SELECT id\nFROM users";
        assert_eq!(find_seed(t, (7, 9)), Some("id"));
        assert_eq!(find_seed(t, (9, 7)), Some("id"), "a backwards selection");
        assert_eq!(find_seed(t, (7, 7)), None, "a bare caret");
        assert_eq!(find_seed(t, (7, 14)), None, "across a line break");
        assert_eq!(find_seed("a\r\nb", (0, 3)), None);
        assert_eq!(find_seed(t, (15, 99)), None, "past the end");
        assert_eq!(find_seed("café", (3, 4)), None, "inside a character");
    }

    #[test]
    fn a_new_find_query_starts_at_the_selection_and_wraps() {
        let hits = [3, 10, 20];
        assert_eq!(first_hit_from(&hits, 10), 1, "the selection's own match");
        assert_eq!(first_hit_from(&hits, 11), 2);
        assert_eq!(first_hit_from(&hits, 0), 0);
        assert_eq!(first_hit_from(&hits, 21), 0, "wraps to the top");
        assert_eq!(first_hit_from(&[], 5), 0);
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

    /// **Tab over a selection indents it with hard tabs too.** The editor
    /// handled soft tabs itself and left hard ones to floem's `InsertTab`,
    /// which replaces the selection with one `\t` — three selected lines
    /// became a single tab character.
    #[test]
    fn hard_tab_indents_a_selection_rather_than_replacing_it() {
        let full = "select a\nfrom t\nwhere x = 1";
        let e = hard_tab_indent(full, 0, full.len());
        assert_eq!((e.start, e.end), (0, full.len()));
        assert_eq!(e.text, "\tselect a\n\tfrom t\n\twhere x = 1");
        assert_eq!(e.sel, (0, e.text.len()));
        // A selection inside one line indents that line.
        let e = hard_tab_indent("  select a", 4, 8);
        assert_eq!(e.text, "  \tselect a");
        // Blank lines stay blank; a selection ending at a line start leaves it.
        let e = hard_tab_indent("a\n\nb\nc", 0, 5);
        assert_eq!(e.text, "\ta\n\n\tb");
        // A bare caret inserts one tab there.
        let e = hard_tab_indent("ab", 1, 1);
        assert_eq!(
            (e.start, e.end, e.text.as_str(), e.sel),
            (1, 1, "\t", (2, 2))
        );
    }

    /// **Ctrl+X on a bare caret cuts the caret's line — every caret's, and
    /// only theirs.** It ran floem's `DeleteLine`, which never copies, so the
    /// Ctrl+V after it pasted whatever was on the clipboard before; and with a
    /// caret on line 1 and another on line 50 it deleted lines 1 through 50.
    #[test]
    fn cutting_lines_takes_each_carets_line_onto_the_clipboard() {
        let full = "one\ntwo\nthree\nfour";
        let c = cut_lines(full, &[1]);
        assert_eq!(c.clipboard, "one\n");
        assert_eq!(c.text, "two\nthree\nfour");
        assert_eq!(c.caret, 0);
        // Two carets: their two lines, nothing between.
        let c = cut_lines(full, &[1, 10]);
        assert_eq!(c.clipboard, "one\nthree\n");
        assert_eq!(c.text, "two\nfour");
        // Two carets on one line cut it once.
        let c = cut_lines(full, &[4, 6]);
        assert_eq!(c.clipboard, "two\n");
        assert_eq!(c.text, "one\nthree\nfour");
        assert_eq!(c.caret, 4);
        // The last line, with no newline of its own, takes the one before it.
        let c = cut_lines(full, &[full.len()]);
        assert_eq!(c.clipboard, "four\n");
        assert_eq!(c.text, "one\ntwo\nthree");
        assert_eq!(c.caret, "one\ntwo\n".len());
        // The only line.
        let c = cut_lines("solo", &[2]);
        assert_eq!(
            (c.clipboard.as_str(), c.text.as_str(), c.caret),
            ("solo\n", "", 0)
        );
        // CRLF lines are cut whole.
        let c = cut_lines("a\r\nb\r\n", &[0]);
        assert_eq!((c.clipboard.as_str(), c.text.as_str()), ("a\r\n", "b\r\n"));
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

    /// Apply `e` to `src` and render its selection as `{…}` (a caret as `|`).
    fn applied(src: &str, e: &IndentEdit) -> String {
        let mut out = format!("{}{}{}", &src[..e.start], e.text, &src[e.end..]);
        if e.sel.0 == e.sel.1 {
            out.insert(e.sel.0, '|');
        } else {
            out.insert(e.sel.1, '}');
            out.insert(e.sel.0, '{');
        }
        out
    }

    #[test]
    fn toggle_case_upper_cases_then_lower_cases_a_selection() {
        let src = "select Name from t";
        let up = toggle_case(src, 0, 11).unwrap();
        assert_eq!(applied(src, &up), "{SELECT NAME} from t");
        let again = format!("{}{}", up.text, &src[11..]);
        let down = toggle_case(&again, 11, 0).unwrap();
        assert_eq!(applied(&again, &down), "{select name} from t");
        // A length change keeps the whole result selected.
        let e = toggle_case("straße", 0, 7).unwrap();
        assert_eq!(applied("straße", &e), "{STRASSE}");
    }

    #[test]
    fn toggle_case_on_a_bare_caret_takes_its_word() {
        let src = "SELECT user_id FROM t";
        let e = toggle_case(src, 10, 10).unwrap();
        assert_eq!(applied(src, &e), "SELECT USE|R_ID FROM t");
        // At a word's end, that word.
        let e = toggle_case(src, 6, 6).unwrap();
        assert_eq!(applied(src, &e), "select| user_id FROM t");
    }

    #[test]
    fn toggle_case_with_nothing_to_change_is_none() {
        assert_eq!(toggle_case("a  b", 2, 2), None, "no word at the caret");
        assert_eq!(toggle_case("12 + 3", 0, 6), None, "no letters");
        assert_eq!(toggle_case("", 0, 0), None);
    }

    /// Join at the `|` (or over `{`…`}`) in `src` and render the result with
    /// the selection as `{…}` (a caret as `|`); `None` when nothing joins.
    fn join(src: &str, dialect: SqlDialect) -> Option<String> {
        let (sel, clean) = match (src.find('{'), src.find('}')) {
            (Some(lo), Some(hi)) => ((lo, hi - 1), src.replacen(['{', '}'], "", 2)),
            _ => {
                let at = src.find('|').expect("a caret");
                ((at, at), src.replacen('|', "", 1))
            }
        };
        join_lines(&clean, sel.0, sel.1, dialect).map(|e| applied(&clean, &e))
    }

    #[test]
    fn join_lines_joins_the_caret_line_with_the_next() {
        use SqlDialect::MySql;
        assert_eq!(
            join("SELECT a,|\n    b\nFROM t", MySql).unwrap(),
            "SELECT a,| b\nFROM t"
        );
        assert_eq!(join("|a  \r\n\tb", MySql).unwrap(), "a| b");
        // An empty side takes no space.
        assert_eq!(join("a|\n\nb", MySql).unwrap(), "a|\nb");
        assert_eq!(join("a\nb|", MySql), None, "the last line has no next");
    }

    #[test]
    fn join_lines_joins_every_spanned_line_and_keeps_the_selection() {
        use SqlDialect::MySql;
        assert_eq!(join("{a\n  b\n  c}\nd", MySql).unwrap(), "{a b c}\nd");
        // A blank line between two joined lines leaves one space, not none:
        // the left side is what has been joined so far.
        assert_eq!(join("{a\n\nb}", MySql).unwrap(), "{a b}");
        // A selection ending at a line start does not pull that line in.
        assert_eq!(join("{a\n  b\n}c", MySql).unwrap(), "{a b\n}c");
        // No padding inside brackets or before a separator.
        assert_eq!(join("{IN (\n  1\n  , 2\n)}", MySql).unwrap(), "{IN (1, 2)}");
    }

    #[test]
    fn join_lines_leaves_a_break_in_a_string_or_ending_a_line_comment() {
        use SqlDialect::{MySql, Postgres};
        assert_eq!(join("SELECT 'x|\ny'", MySql), None);
        assert_eq!(join("a -- note|\nb", MySql), None);
        assert_eq!(join("a # note|\nb", MySql), None);
        // `#` is no comment on PostgreSQL, so that break joins.
        assert_eq!(join("a # b|\nc", Postgres).unwrap(), "a # b| c");
        // The joinable breaks around a kept one still join.
        assert_eq!(
            join("{a,\nb -- x\nc,\nd}", MySql).unwrap(),
            "{a, b -- x\nc, d}"
        );
    }

    /// Toggle a block comment at the `|` (or over `{`…`}`) in `src`, rendered
    /// as [`join`] renders.
    fn block(src: &str, dialect: SqlDialect) -> Option<String> {
        let (sel, clean) = match (src.find('{'), src.find('}')) {
            (Some(lo), Some(hi)) => ((lo, hi - 1), src.replacen(['{', '}'], "", 2)),
            _ => {
                let at = src.find('|').expect("a caret");
                ((at, at), src.replacen('|', "", 1))
            }
        };
        toggle_block_comment(&clean, sel.0, sel.1, dialect).map(|e| applied(&clean, &e))
    }

    #[test]
    fn block_comment_wraps_a_selection_and_unwraps_it_again() {
        use SqlDialect::MySql;
        let wrapped = block("SELECT {a, b} FROM t", MySql).unwrap();
        assert_eq!(wrapped, "SELECT {/* a, b */} FROM t");
        assert_eq!(block(&wrapped, MySql).unwrap(), "SELECT {a, b} FROM t");
        // Outer blanks stay outside the comment.
        assert_eq!(
            block("SELECT{ a }FROM", MySql).unwrap(),
            "SELECT {/* a */} FROM"
        );
        // A selection inside a comment unwraps the whole of it.
        assert_eq!(block("x /*a {b} c*/ y", MySql).unwrap(), "x {a b c} y");
    }

    #[test]
    fn block_comment_on_a_bare_caret_takes_the_line_or_its_comment() {
        use SqlDialect::MySql;
        assert_eq!(
            block("  SEL|ECT 1\nx", MySql).unwrap(),
            "  /* SEL|ECT 1 */\nx"
        );
        assert_eq!(
            block("SELECT /* n|ote */ 1", MySql).unwrap(),
            "SELECT n|ote 1"
        );
        assert_eq!(
            block("  |/* SELECT 1 */\nx", MySql).unwrap(),
            "  |SELECT 1\nx"
        );
        assert_eq!(block("/* a */|", MySql).unwrap(), "a|");
        assert_eq!(block("a\n  |  \nb", MySql), None, "a blank line");
    }

    #[test]
    fn block_comment_is_refused_where_the_wrap_would_not_be_one_comment() {
        use SqlDialect::{MySql, Postgres};
        // An inner `*/` ends the comment early on MySQL; PostgreSQL nests.
        assert_eq!(block("{a /* b */ c}", MySql), None);
        assert_eq!(
            block("{a /* b */ c}", Postgres).unwrap(),
            "{/* a /* b */ c */}"
        );
        // …and there an unclosed comment the range ends on stays open inside.
        assert_eq!(block("{a /* b}", Postgres), None);
        // A range that starts or ends inside a string.
        assert_eq!(block("SELECT 'a{bc' x}", MySql), None);
        assert_eq!(block("SELECT {x 'ab}c'", MySql), None);
        // An executable comment is code, not a comment to unwrap.
        assert_eq!(block("SELECT /*!50000 1| */", MySql), None);
    }

    /// Press Enter at the `|` (or over `{`…`}`) in `src` and render the result
    /// with the caret as `|`.
    fn enter(src: &str, unit: &str, dialect: SqlDialect) -> String {
        let (sel, clean) = match (src.find('{'), src.find('}')) {
            (Some(lo), Some(hi)) => ((lo, hi - 1), src.replacen(['{', '}'], "", 2)),
            _ => {
                let at = src.find('|').expect("a caret");
                ((at, at), src.replacen('|', "", 1))
            }
        };
        let e = newline_indent(&clean, sel.0, sel.1, unit, dialect);
        assert_eq!(e.sel.0, e.sel.1, "Enter leaves a bare caret");
        let mut out = format!("{}{}{}", &clean[..e.start], e.text, &clean[e.end..]);
        out.insert(e.sel.0, '|');
        out
    }

    #[test]
    fn enter_keeps_the_lines_indent_cut_at_the_caret() {
        use SqlDialect::MySql;
        assert_eq!(enter("  SELECT a|", "    ", MySql), "  SELECT a\n  |");
        assert_eq!(enter("\tSELECT a,| b", "\t", MySql), "\tSELECT a,\n\t| b");
        // Inside the indent, the new line takes only what is left of the caret.
        assert_eq!(enter("  |  SELECT", "  ", MySql), "  \n  |  SELECT");
        assert_eq!(enter("|", "  ", MySql), "\n|");
        // A selection is replaced.
        assert_eq!(enter("  a {xyz}b", "  ", MySql), "  a \n  |b");
    }

    #[test]
    fn enter_after_an_open_bracket_goes_one_unit_deeper() {
        use SqlDialect::{MySql, Postgres};
        assert_eq!(enter("  IN (|", "    ", MySql), "  IN (\n      |");
        assert_eq!(enter("IN (|", "\t", MySql), "IN (\n\t|");
        // Trailing blanks after the bracket still count as after it.
        assert_eq!(enter("f(  |", "  ", MySql), "f(  \n  |");
        assert_eq!(enter("a[|", "  ", Postgres), "a[\n  |");
    }

    #[test]
    fn enter_between_a_pair_puts_the_closer_on_its_own_line() {
        use SqlDialect::MySql;
        assert_eq!(enter("  f(|)", "  ", MySql), "  f(\n    |\n  )");
        assert_eq!(enter("f(|  )", "\t", MySql), "f(\n\t|\n  )");
        // A closer of the other kind is not the pair's.
        assert_eq!(enter("f(|]", "  ", MySql), "f(\n  |]");
    }

    /// A word that opens a block opens a level, as a bracket does: `BEGIN`
    /// (and T-SQL's `BEGIN TRY`/`BEGIN CATCH`), `CASE`, `THEN`, `ELSE`,
    /// `LOOP`, `DO`, `REPEAT`. With its `END` straight after the caret the
    /// `END` goes to a line of its own at the old indent.
    #[test]
    fn enter_after_a_block_opening_word_goes_one_unit_deeper() {
        use SqlDialect::{MsSql, MySql, Postgres};
        assert_eq!(enter("  BEGIN|", "  ", MsSql), "  BEGIN\n    |");
        assert_eq!(enter("begin  |", "\t", MySql), "begin  \n\t|");
        assert_eq!(enter("BEGIN TRY|", "  ", MsSql), "BEGIN TRY\n  |");
        assert_eq!(enter("  BEGIN CATCH|", "  ", MsSql), "  BEGIN CATCH\n    |");
        assert_eq!(enter("SELECT CASE|", "  ", MySql), "SELECT CASE\n  |");
        assert_eq!(
            enter("  IF x > 1 THEN|", "  ", MySql),
            "  IF x > 1 THEN\n    |"
        );
        assert_eq!(enter("  ELSE|", "  ", Postgres), "  ELSE\n    |");
        assert_eq!(enter("LOOP|", "  ", Postgres), "LOOP\n  |");
        assert_eq!(enter("WHILE i < 3 DO|", "  ", MySql), "WHILE i < 3 DO\n  |");
        assert_eq!(enter("REPEAT|", "  ", MySql), "REPEAT\n  |");
        // Its END after the caret takes a line of its own.
        assert_eq!(enter("  BEGIN|END", "  ", MsSql), "  BEGIN\n    |\n  END");
        // The blanks before it stay with it, as they do before a bracket.
        assert_eq!(enter("CASE| END", "  ", MySql), "CASE\n  |\n END");
        // What follows the word is the block's body, and goes in with it.
        assert_eq!(enter("BEGIN| x", "  ", MsSql), "BEGIN\n  | x");
        assert_eq!(
            enter("BEGIN TRY|END TRY", "  ", MsSql),
            "BEGIN TRY\n  |\nEND TRY"
        );
    }

    /// Typing the `D` that makes a line's only word `END` puts the line at
    /// the indent of the `BEGIN`'s or `CASE`'s line it closes; `None` leaves
    /// the character to type as usual.
    #[test]
    fn typing_an_end_dedents_it_to_its_opener() {
        use SqlDialect::{MsSql, MySql};
        let typed = |src: &str, ch: char, d| {
            let at = src.find('|').unwrap();
            let clean = src.replacen('|', "", 1);
            dedent_typed_end(&clean, at, at, ch, d).map(|e| {
                let mut out = format!("{}{}{}", &clean[..e.start], e.text, &clean[e.end..]);
                out.insert(e.sel.0, '|');
                out
            })
        };
        assert_eq!(
            typed("BEGIN\n  SELECT 1;\n  EN|", 'D', MsSql).as_deref(),
            Some("BEGIN\n  SELECT 1;\nEND|")
        );
        assert_eq!(
            typed("  begin\n    x;\n      en|", 'd', MsSql).as_deref(),
            Some("  begin\n    x;\n  end|")
        );
        assert_eq!(
            typed("SELECT CASE\n    WHEN a THEN 1\n    EN|", 'D', MySql).as_deref(),
            Some("SELECT CASE\n    WHEN a THEN 1\nEND|")
        );
        // Already where it belongs, not the line's only word, nothing to
        // close, or more on the line: the character types as usual.
        assert_eq!(typed("BEGIN\n  x;\nEN|", 'D', MsSql), None);
        assert_eq!(typed("BEGIN\n  SET a = EN|", 'D', MsSql), None);
        assert_eq!(typed("-- BEGIN\n  EN|", 'D', MsSql), None);
        assert_eq!(typed("BEGIN\n  x;\n  EN| -- c", 'D', MsSql), None);
        assert_eq!(typed("BEGIN\n  x;\n  EN|", 'X', MsSql), None);
    }

    /// Not a block: the word is part of a name, in a comment or string, or a
    /// transaction (`BEGIN TRAN`, `BEGIN;`).
    #[test]
    fn enter_after_a_word_that_opens_no_block_adds_nothing() {
        use SqlDialect::{MsSql, MySql};
        assert_eq!(enter("SELECT t.begin|", "  ", MySql), "SELECT t.begin\n|");
        assert_eq!(enter("SELECT my_case|", "  ", MySql), "SELECT my_case\n|");
        assert_eq!(enter("  -- BEGIN|", "  ", MySql), "  -- BEGIN\n  |");
        assert_eq!(enter("SELECT 'then|", "  ", MySql), "SELECT 'then\n|");
        assert_eq!(enter("BEGIN TRAN|", "  ", MsSql), "BEGIN TRAN\n|");
        assert_eq!(enter("BEGIN;|", "  ", MySql), "BEGIN;\n|");
        assert_eq!(enter("SELECT `case`|", "  ", MySql), "SELECT `case`\n|");
    }

    #[test]
    fn enter_after_a_bracket_in_a_string_or_comment_adds_nothing() {
        use SqlDialect::MySql;
        assert_eq!(enter("  SELECT '(|", "  ", MySql), "  SELECT '(\n  |");
        assert_eq!(enter("  -- note (|", "  ", MySql), "  -- note (\n  |");
        assert_eq!(enter("  /* ( */|", "  ", MySql), "  /* ( */\n  |");
    }

    #[test]
    fn enter_at_a_line_end_takes_a_deeper_next_lines_indent() {
        use SqlDialect::MySql;
        assert_eq!(enter("SELECT|\n    a", "  ", MySql), "SELECT\n    |\n    a");
        // Not a shallower one, and not when the line goes on past the caret.
        assert_eq!(enter("  x|\ny", "  ", MySql), "  x\n  |\ny");
        assert_eq!(enter("SELECT| b\n    a", "  ", MySql), "SELECT\n| b\n    a");
        // A CRLF line's `\r` is its end, not text after the caret.
        assert_eq!(enter("f(|)\r\n", "  ", MySql), "f(\n  |\n)\r\n");
        assert_eq!(enter("SELECT|\r\n  a", "  ", MySql), "SELECT\n  |\r\n  a");
        // An offset inside a character is answered, not panicked on.
        let e = newline_indent("é", 1, 1, "  ", MySql);
        assert_eq!((e.start, e.end), (0, 0));
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

    // --- duplicate_lines ---------------------------------------------------

    /// Apply a [`duplicate_lines`] result: the new text, and the text the
    /// returned selection covers in it.
    fn duplicated(text: &str, a: usize, b: usize) -> (String, String) {
        let (at, insert, (lo, hi)) = duplicate_lines(text, a, b);
        let mut out = text.to_string();
        out.insert_str(at, &insert);
        let sel = out[lo..hi].to_string();
        (out, sel)
    }

    /// The newline-less last line duplicates onto a line of its own, with a
    /// selection or a caret, and the selection follows onto the copy.
    #[test]
    fn duplicating_the_last_line_makes_a_new_line() {
        let text = "SELECT 1;\nSELECT 2";
        // `SELECT` selected on the last line.
        assert_eq!(
            duplicated(text, 10, 16),
            (
                "SELECT 1;\nSELECT 2\nSELECT 2".to_string(),
                "SELECT".to_string()
            )
        );
        // A caret keeps its column on the copy.
        let (at, insert, sel) = duplicate_lines(text, 13, 13);
        assert_eq!((at, insert.as_str(), sel), (18, "\nSELECT 2", (22, 22)));
        // A selection over both lines copies both, below the second.
        assert_eq!(
            duplicated(text, 2, 12).0,
            "SELECT 1;\nSELECT 2\nSELECT 1;\nSELECT 2"
        );
        // One ending at a line's start leaves that line out.
        assert_eq!(duplicated(text, 0, 10).0, "SELECT 1;\nSELECT 1;\nSELECT 2");
        assert_eq!(duplicated("", 0, 0).0, "\n");
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

#[cfg(test)]
mod collapsed_search_tests {
    use super::contains_collapsed_ignore_ascii_case as find;

    /// The property both search boxes are built on: the phrase as it is *drawn*
    /// (one line) finds the statement as it is *stored* (several).
    #[test]
    fn a_phrase_reads_across_the_statements_own_newlines() {
        let sql = "SELECT *\n  FROM   orders\n WHERE id = 1";
        assert!(find(sql, "select * from orders"));
        assert!(find(sql, "FROM ORDERS WHERE"));
        assert!(find(sql, "orders where id = 1"));
    }

    /// It must agree with the allocating form it replaces, run by run, on the
    /// shapes that distinguish them — leading, trailing, tabs, newlines, and a
    /// run in the middle of the needle's span.
    #[test]
    fn it_agrees_with_the_allocating_collapse_it_replaces() {
        let collapse = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        let hays = [
            "",
            "   ",
            "a",
            "  a  ",
            "a  b",
            "a\n\tb\r\nc",
            "\n\n select  1 \t\n",
            "one",
            "  ",
        ];
        let needles = ["", " ", "a", "a b", "ab", "select 1", "one two", "c"];
        for h in hays {
            let flat = collapse(h);
            for n in needles {
                assert_eq!(
                    find(h, n),
                    super::contains_ignore_ascii_case(&flat, n),
                    "hay {h:?} needle {n:?} (collapsed {flat:?})"
                );
            }
        }
    }

    /// Leading and trailing whitespace is not matchable, because the collapsed
    /// form does not have it. A needle that is only a space finds a *run*.
    #[test]
    fn the_edges_collapse_away_and_an_internal_run_is_one_space() {
        assert!(!find("  a", " a"), "the leading run is gone");
        assert!(!find("a  ", "a "), "and so is the trailing one");
        assert!(find("a \t\n b", "a b"), "however long the internal run");
        assert!(!find("a b", "a  b"), "which is one space, not two");
        assert!(find("a b", " "));
        assert!(!find("ab", " "));
    }

    /// An empty needle matches anything, as the uncollapsed sibling has it, and
    /// case is ignored on both sides.
    #[test]
    fn an_empty_needle_matches_and_case_is_ignored() {
        assert!(find("", ""));
        assert!(find("anything", ""));
        assert!(find("SeLeCt\n1", "select 1"));
        assert!(find("select\n1", "SELECT 1"));
        assert!(!find("", "a"));
    }
}
