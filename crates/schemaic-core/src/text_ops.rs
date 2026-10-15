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
/// a bare caret stays after the same characters, which may have moved it (`İ`
/// → `i̇` is a byte longer). `None` when there is nothing to change.
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
    let upper = old.chars().any(char::is_lowercase);
    let recase = |s: &str| {
        if upper {
            s.to_uppercase()
        } else {
            s.to_lowercase()
        }
    };
    let text = recase(old);
    if text == old {
        return None;
    }
    let sel = if lo == hi {
        // The characters before the caret can change byte length (`İ` → `i̇`
        // grows, `ı` → `I` shrinks), so the caret's old offset may now fall
        // inside a character: re-case just that prefix and stand after it.
        // A case mapping is per character, so the prefix's length is the
        // length of the same prefix of `text`.
        let before = &old[..floor_char(old, lo - start)];
        let at = start + recase(before).len();
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
    let lo = floor_char(full, sel_a.min(sel_b));
    let hi = floor_char(full, sel_a.max(sel_b));
    let plan = join_breaks(full, join_candidates(full, lo, hi)?, dialect)?;
    let sel = if lo == hi {
        (plan.start, plan.start)
    } else {
        (plan.coarse(lo), plan.coarse(hi))
    };
    Some(IndentEdit {
        start: plan.start,
        end: plan.end,
        text: plan.out,
        sel,
    })
}

/// The line breaks [`join_lines`] would join for the selection `sel_a..sel_b`
/// before asking which are code: every break inside the lines it spans, or —
/// within one line — the one ending it. `None` on the last line alone.
fn join_candidates(full: &str, sel_a: usize, sel_b: usize) -> Option<Vec<usize>> {
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
    Some(breaks)
}

/// A join worked out ([`join_breaks`]): `[start, end)` of the text becomes
/// `out`, made of one replacement per joined break — `segs`, each its old
/// `(start, end)`, its start in the new text and its length there.
struct JoinPlan {
    start: usize,
    end: usize,
    out: String,
    segs: Vec<(usize, usize, usize, usize)>,
}

impl JoinPlan {
    /// `off` in the joined text, as [`join_lines`] has always mapped a
    /// selection's ends: anything inside the replaced span goes to its end.
    fn coarse(&self, off: usize) -> usize {
        if off <= self.start {
            off
        } else if off >= self.end {
            off - (self.end - self.start) + self.out.len()
        } else {
            self.start + self.out.len()
        }
    }

    /// `off` in the joined text, break by break: text between two joins
    /// keeps its place, and an offset inside a joined break goes to where
    /// that break's replacement starts.
    fn map(&self, off: usize) -> usize {
        let mut delta = 0isize;
        for &(s, e, new_s, new_len) in &self.segs {
            if off <= s {
                break;
            }
            if off < e {
                return new_s;
            }
            delta = (new_s + new_len) as isize - e as isize;
        }
        (off as isize + delta) as usize
    }

    /// Where the replacement of the joined break covering `nl` starts.
    fn seg_start(&self, nl: usize) -> Option<usize> {
        self.segs
            .iter()
            .find(|&&(s, e, _, _)| s <= nl && nl < e)
            .map(|&(_, _, new_s, _)| new_s)
    }

    fn edit(self) -> RegionEdit {
        RegionEdit {
            start: self.start,
            end: self.end,
            insert: self.out,
            sel: (self.start, self.start),
        }
    }
}

/// Join the candidate `breaks` (ascending, every one a `\n`) that are code,
/// each with the blanks around it, as [`join_lines`] describes. `None` when
/// none joins.
fn join_breaks(full: &str, breaks: Vec<usize>, dialect: SqlDialect) -> Option<JoinPlan> {
    let b = full.as_bytes();
    let len = full.len();
    if breaks.is_empty() {
        return None;
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
    let mut segs = Vec::new();
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
        let new_s = start + out.len();
        if !tight {
            out.push(' ');
        }
        segs.push((s, e, new_s, start + out.len() - new_s));
        span = Some((start, e));
    }
    let (start, end) = span?;
    Some(JoinPlan {
        start,
        end,
        out,
        segs,
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

/// Byte offsets of every (non-overlapping) case-insensitive occurrence of
/// `needle` in `hay`. Offsets index into `hay` directly (the search walks `hay`
/// itself, so no `to_lowercase` reallocation shifts them). Empty needle → no
/// matches.
///
/// **Every hit spans exactly `needle.len()` bytes**, which is what the find
/// bar selects, highlights and replaces by. So the folding is per character,
/// by a character's lowercase when that is one character ([`fold_char`]), and
/// two characters are the same only when they are as wide in UTF-8 as well:
/// `É`/`é`, `Ö`/`ö`, Cyrillic and Greek fold, while the few pairs whose cases
/// differ in width — the Kelvin sign `K` and `k`, `ſ` and `s` — stay
/// case-sensitive rather than report a span that cuts a character. An ASCII
/// needle takes the byte-wise path, and matches exactly what it would by the
/// character walk: no other character is as narrow as an ASCII one.
pub fn find_matches(hay: &str, needle: &str) -> Vec<usize> {
    find_matches_in(hay, needle, FindOptions::default())
}

/// The find bar's options. **Match case** and **whole word** keep every hit
/// exactly `needle.len()` bytes long — one compares byte for byte, the other
/// only refuses a hit — which is why they can share [`find_matches`]' callers
/// unchanged. **Regex** cannot, and is read only by the hit-based entry
/// points ([`find_hits`], [`hit_holds`], [`replacement_for`],
/// [`replace_hits`]), whose hits carry their own length; the plain-text
/// functions ignore it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub match_case: bool,
    /// The needle is a regular expression (the `regex` crate's syntax, `^`/`$`
    /// at line ends), and a replacement may name its groups as `$1`/`${name}`.
    pub regex: bool,
    /// No name byte either side of the hit, by [`crate::sql::continues_name`]
    /// on [`Self::dialect`] — the one definition, so `_`, `$` and a non-ASCII
    /// letter continue a word as they do an identifier, and `@`/`#` do on SQL
    /// Server. Asked only on a side where the needle's own edge is such a
    /// byte: `f(` has a boundary to want on its left alone.
    pub whole_word: bool,
    /// The tab's dialect, which is where a name ends for
    /// [`Self::whole_word`]; nothing else reads it.
    pub dialect: SqlDialect,
}

/// [`find_matches`] under `opts`.
pub fn find_matches_in(hay: &str, needle: &str, opts: FindOptions) -> Vec<usize> {
    find_matches_between(hay, needle, opts, 0, hay.len(), usize::MAX)
}

/// [`find_matches_in`] over the hits lying wholly in `hay[lo..hi]`, scanning
/// from `lo` — the boundaries whole word asks about are still read from all of
/// `hay` — and stopping after `cap` of them.
fn find_matches_between(
    hay: &str,
    needle: &str,
    opts: FindOptions,
    lo: usize,
    hi: usize,
    cap: usize,
) -> Vec<usize> {
    let n = needle.len();
    let hi = hi.min(hay.len());
    if n == 0 || hi < n {
        return Vec::new();
    }
    let ascii = needle.is_ascii();
    let mut out = Vec::new();
    let mut i = lo;
    while i + n <= hi && out.len() < cap {
        if hit_at(hay, i, needle, ascii, opts) {
            out.push(i);
            i += n; // non-overlapping
        } else {
            i += 1;
        }
    }
    out
}

/// A character's lowercase, when it is a single character; the character
/// itself otherwise (`İ` lowercases to two). [`find_matches`]' folding.
///
/// The word-final sigma `ς` folds to `σ`: both have the capital `Σ`, and a
/// per-character lowercase cannot tell which one `Σ` stands for, so without
/// it every Greek word ending in sigma was found only in its typed case. Both
/// are two bytes, so a hit's width is unchanged.
fn fold_char(c: char) -> char {
    if c == 'ς' {
        return 'σ';
    }
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// `s` with every character replaced by its [`fold_char`] where that is as
/// wide in UTF-8, and kept where it is not — **so every byte offset in the
/// result is the same character's offset in `s`.**
///
/// A plain substring search of one folded string in another then finds what
/// [`matches_at`]'s case-insensitive rule finds, at the same offsets: that
/// rule pairs two characters when they are equal, or as wide as each other
/// and fold alike, and folding only where the width holds keeps exactly those
/// pairs equal. What it buys is the search: a folded haystack built once can
/// be scanned with `str::find` rather than by asking [`matches_at`] at every
/// character, which is what made Find Anywhere's body tier ten times slower
/// on a term that starts with a non-ASCII letter (`core::body_search`).
pub fn fold_for_search(s: &str) -> String {
    if s.is_ascii() {
        return s.to_ascii_lowercase();
    }
    s.chars()
        .map(|c| {
            let f = fold_char(c);
            if f.len_utf8() == c.len_utf8() { f } else { c }
        })
        .collect()
}

/// [`matches_at`] with the needle's ASCII-ness worked out once by the caller,
/// since [`find_matches`] asks it at every byte of the document.
fn folded_match_at(hay: &str, off: usize, needle: &str, ascii: bool) -> bool {
    // `get` refuses an offset out of range or off a char boundary.
    let Some(span) = off
        .checked_add(needle.len())
        .and_then(|end| hay.get(off..end))
    else {
        return false;
    };
    if ascii {
        return span.as_bytes().eq_ignore_ascii_case(needle.as_bytes());
    }
    // Equal byte lengths, and every pair as wide as each other, so the two
    // run out of characters together.
    span.chars()
        .zip(needle.chars())
        .all(|(a, b)| a == b || (a.len_utf8() == b.len_utf8() && fold_char(a) == fold_char(b)))
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

/// The *in selection* scope Ctrl+F / Ctrl+H take from the selection: the
/// selection itself when it spans more than one line — the block
/// [`find_seed`] declines to make a needle of, being one to search *in* — and
/// `None` otherwise. `sel` is `(start, end)` in either order.
pub fn find_scope_seed(text: &str, sel: (usize, usize)) -> Option<(usize, usize)> {
    let (a, b) = (sel.0.min(sel.1), sel.0.max(sel.1));
    text.get(a..b)
        .is_some_and(|s| s.contains(['\n', '\r']))
        .then_some((a, b))
}

/// Whether Ctrl+F / Ctrl+H read the selection at all — as the needle
/// ([`find_seed`]) or the scope ([`find_scope_seed`]). Always on a bar being
/// opened; on one already open, not when the selection is `current`, the hit a
/// step revealed: a regex hit can span lines, and seeding from it would make
/// that one hit the *in selection* scope (or its text the needle, in place of
/// the pattern that found it). `sel` is `(start, end)` in either order; a hit
/// is `(offset, len)`.
pub fn selection_seeds_find(open: bool, sel: (usize, usize), current: Option<Hit>) -> bool {
    let (a, b) = (sel.0.min(sel.1), sel.0.max(sel.1));
    !open || current != Some((a, b - a))
}

/// The hit a new find query starts at: the first at or after `from` (the
/// selection's start, or the caret), wrapping to the first in the document —
/// so a needle seeded from the selection, or typed one character at a time,
/// stays on the match the user is looking at rather than leaping to the top.
/// `0` for no hits, which the caller reads as "none" by the list's length.
pub fn first_hit_from(hits: &[Hit], from: usize) -> usize {
    let i = hits.partition_point(|h| h.0 < from);
    if i < hits.len() { i } else { 0 }
}

/// Is `needle` still exactly at byte offset `off` in `hay`, by the same
/// case-insensitive rule [`find_matches`] uses?
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
    matches_at_in(hay, off, needle, FindOptions::default())
}

/// [`matches_at`] under `opts` — the same rule [`find_matches_in`] uses.
pub fn matches_at_in(hay: &str, off: usize, needle: &str, opts: FindOptions) -> bool {
    !needle.is_empty() && hit_at(hay, off, needle, needle.is_ascii(), opts)
}

/// Is `needle` a hit at `off` under `opts`? The one test every entry point
/// above asks, so the count, the step and Replace All agree on what a hit is.
fn hit_at(hay: &str, off: usize, needle: &str, ascii: bool, opts: FindOptions) -> bool {
    let end = off + needle.len();
    let spelled = if opts.match_case {
        hay.get(off..end) == Some(needle)
    } else {
        folded_match_at(hay, off, needle, ascii)
    };
    if !spelled || !opts.whole_word {
        return spelled;
    }
    word_bounded(hay, off, end, opts)
}

/// Does `hay[off..end]` stand as a whole word — no name byte either side of
/// it, by [`crate::sql::continues_name`] on the options' dialect, asked only
/// on a side where the span's own edge is a name byte?
fn word_bounded(hay: &str, off: usize, end: usize, opts: FindOptions) -> bool {
    let name = |b: u8| crate::sql::continues_name(b, opts.dialect);
    let hb = hay.as_bytes();
    let open_left = !name(hb[off]) || off == 0 || !name(hb[off - 1]);
    let open_right = !name(hb[end - 1]) || end == hb.len() || !name(hb[end]);
    open_left && open_right
}

/// The find bar's pattern, compiled — case-insensitive unless match case says
/// otherwise, `^`/`$` at line ends, a `\r\n` one included (`crlf`: a paste
/// from the Windows clipboard keeps its `\r`, and `.` must not swallow it) —
/// or the one-line sentence the bar shows.
fn find_regex(needle: &str, opts: FindOptions) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(needle)
        .case_insensitive(!opts.match_case)
        .multi_line(true)
        .crlf(true)
        .build()
        .map_err(|e| {
            // The crate's message draws a caret under the pattern over several
            // lines; its last line is the sentence.
            let msg = e.to_string();
            let last = msg
                .lines()
                .rev()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or("")
                .trim_start_matches("error:")
                .trim()
                .to_string();
            if last.is_empty() {
                "Not a valid pattern".to_string()
            } else {
                format!("Not a valid pattern: {last}")
            }
        })
}

/// Every non-empty match of `re` lying wholly in `hay[lo..hi]` that whole
/// word (if on) accepts, at most `cap` of them. `find_at` over `hay[..hi]`, so
/// `\b` and `^` at `lo` see the text before it.
fn regex_hits(
    hay: &str,
    re: &regex::Regex,
    opts: FindOptions,
    lo: usize,
    hi: usize,
    cap: usize,
) -> Vec<Hit> {
    let hi = hi.min(hay.len());
    let text = &hay[..hi];
    let mut out = Vec::new();
    let mut at = lo;
    while at <= hi && out.len() < cap {
        let Some(m) = re.find_at(text, at) else {
            break;
        };
        if m.start() == m.end() {
            // An empty match: step past one character and look again.
            at = m.end() + text[m.end()..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        if !opts.whole_word || word_bounded(hay, m.start(), m.end(), opts) {
            out.push((m.start(), m.end() - m.start()));
        }
        at = m.end();
    }
    out
}

/// Replace every non-overlapping case-insensitive occurrence of `needle`
/// in `hay` with `replacement`, returning the new string and the number of
/// replacements. Matches are the same ones [`find_matches`] reports (so the UI's
/// count and replace-all agree). Empty needle → unchanged, zero replacements.
/// Left-to-right, non-overlapping: the search resumes *after* each original
/// match (not inside the inserted text), so a replacement that itself contains
/// the needle won't be re-replaced in the same pass.
pub fn replace_all(hay: &str, needle: &str, replacement: &str) -> (String, usize) {
    replace_all_in(hay, needle, replacement, FindOptions::default())
}

/// [`replace_all`] under `opts`, over exactly the hits [`find_matches_in`]
/// reports with them.
pub fn replace_all_in(
    hay: &str,
    needle: &str,
    replacement: &str,
    opts: FindOptions,
) -> (String, usize) {
    let hits = find_matches_in(hay, needle, opts);
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

/// One find-bar hit: `(start, len)` in bytes. Under [`FindOptions::regex`] a
/// hit is as long as what the pattern matched; otherwise always the needle's
/// length.
pub type Hit = (usize, usize);

/// The most hits [`find_hits`] reports. A pattern like `.` over a large script
/// matches every character, and a list of millions is a counter nobody reads
/// and a Replace All nobody meant.
pub const FIND_HIT_CAP: usize = 100_000;

/// Every hit of `needle` in `hay` under `opts`, in order and non-overlapping —
/// within `scope` (`(start, end)`, a selection the search is confined to) when
/// there is one. `Err` carries the sentence the bar shows for a pattern that
/// does not compile; plain text never fails.
///
/// A regex hit that matches nothing (`a*` between two letters) is not a hit:
/// there is nothing to select or replace.
pub fn find_hits(
    hay: &str,
    needle: &str,
    opts: FindOptions,
    scope: Option<(usize, usize)>,
) -> Result<Vec<Hit>, String> {
    let Some((lo, hi)) = scope_bounds(hay, scope) else {
        return Ok(Vec::new());
    };
    if needle.is_empty() {
        return Ok(Vec::new());
    }
    if opts.regex {
        let re = find_regex(needle, opts)?;
        return Ok(regex_hits(hay, &re, opts, lo, hi, FIND_HIT_CAP));
    }
    let n = needle.len();
    Ok(
        find_matches_between(hay, needle, opts, lo, hi, FIND_HIT_CAP)
            .into_iter()
            .map(|off| (off, n))
            .collect(),
    )
}

/// `scope` as `(lo, hi)` within `hay` — all of it for none — or `None` when a
/// bound is not a character boundary any more.
fn scope_bounds(hay: &str, scope: Option<(usize, usize)>) -> Option<(usize, usize)> {
    let (lo, hi) = scope.map_or((0, hay.len()), |(a, b)| {
        (a.min(b).min(hay.len()), a.max(b).min(hay.len()))
    });
    (hay.is_char_boundary(lo) && hay.is_char_boundary(hi)).then_some((lo, hi))
}

/// Is `hit` still a hit of `needle` in `hay` under `opts`, within `scope` —
/// the same span, by the same rule [`find_hits`] used? What a stored hit asks
/// before anything edits it ([`matches_at`] explains why that question
/// exists).
///
/// **Against the text the search read**: a regex is matched over `hay` up to
/// the scope's end, as [`find_hits`] matches it, so a greedy hit the scope cut
/// short still holds — matched over the whole document it would be longer,
/// and no longer the hit.
pub fn hit_holds(
    hay: &str,
    hit: Hit,
    needle: &str,
    opts: FindOptions,
    scope: Option<(usize, usize)>,
) -> bool {
    let (off, len) = hit;
    let Some((lo, hi)) = scope_bounds(hay, scope) else {
        return false;
    };
    if len == 0 || off < lo || off + len > hi {
        return false;
    }
    if !opts.regex {
        return len == needle.len() && matches_at_in(hay, off, needle, opts);
    }
    if !hay.is_char_boundary(off) {
        return false;
    }
    let Ok(re) = find_regex(needle, opts) else {
        return false;
    };
    // The match a scan would make starting here — `find_at`, so the text
    // before `off` is context, as it was when the hit was found.
    re.find_at(&hay[..hi], off)
        .is_some_and(|m| m.start() == off && m.end() == off + len)
        && (!opts.whole_word || word_bounded(hay, off, off + len, opts))
}

/// What `hit` is replaced with: `replacement` as written, or under
/// [`FindOptions::regex`] with its `$1`/`${name}` groups filled from the match
/// at `hit` (and `$$` for a dollar sign) — matched over the text up to the
/// scope's end, as [`hit_holds`] matches it.
pub fn replacement_for(
    hay: &str,
    hit: Hit,
    needle: &str,
    replacement: &str,
    opts: FindOptions,
    scope: Option<(usize, usize)>,
) -> String {
    if !opts.regex {
        return replacement.to_string();
    }
    let (Ok(re), Some((_, hi))) = (find_regex(needle, opts), scope_bounds(hay, scope)) else {
        return replacement.to_string();
    };
    let (off, len) = hit;
    if off > hi {
        return replacement.to_string();
    }
    match re.captures_at(&hay[..hi], off) {
        Some(caps)
            if caps
                .get(0)
                .is_some_and(|m| m.start() == off && m.len() == len) =>
        {
            let mut out = String::new();
            caps.expand(replacement, &mut out);
            out
        }
        _ => replacement.to_string(),
    }
}

/// Replace every hit [`find_hits`] reports — the new text and how many there
/// were. Left to right over the original text, so a replacement that itself
/// matches is not replaced again.
pub fn replace_hits(
    hay: &str,
    needle: &str,
    replacement: &str,
    opts: FindOptions,
    scope: Option<(usize, usize)>,
) -> Result<(String, usize), String> {
    let hits = find_hits(hay, needle, opts, scope)?;
    if hits.is_empty() {
        return Ok((hay.to_string(), 0));
    }
    let re = if opts.regex {
        Some(find_regex(needle, opts)?)
    } else {
        None
    };
    // The groups are read from the text the search read — up to the scope's
    // end — so a hit the scope cut short fills `$1` with what it covers.
    let (_, hi) = scope_bounds(hay, scope).unwrap_or((0, hay.len()));
    let searched = &hay[..hi];
    let mut out = String::with_capacity(hay.len());
    let mut prev = 0;
    for &(off, len) in &hits {
        out.push_str(&hay[prev..off]);
        match re.as_ref().and_then(|re| re.captures_at(searched, off)) {
            Some(caps) => caps.expand(replacement, &mut out),
            None => out.push_str(replacement),
        }
        prev = off + len;
    }
    out.push_str(&hay[prev..]);
    Ok((out, hits.len()))
}

/// Carry a byte range — the find bar's *in selection* scope — through one
/// edit, `start..end` of the old text replaced by `inserted` bytes: a bound
/// before the edit stays, one after it moves by the edit's change in length,
/// and one inside the replaced text lands at the start of what replaced it
/// (the range's start) or its end (the range's end), so an edit inside the
/// scope grows or shrinks it rather than losing it.
///
/// `None` when the edit **encloses** the range — replaces all of it and more
/// (a whole-document Format, one delta spanning carets either side of it):
/// where the range sat inside what replaced it is gone, and mapping its
/// bounds to the replacement's edges would widen it to the whole edit — the
/// find scope to the document, a tab stop to every line. An edit over
/// exactly the range is what replaced it, and a point at the edit's edge is
/// where the edit began or ended; neither is lost.
pub fn shift_range(
    range: (usize, usize),
    start: usize,
    end: usize,
    inserted: usize,
) -> Option<(usize, usize)> {
    let moved = |p: usize| p - end + start + inserted;
    let (a, b) = range;
    let encloses = start < end && start <= a && b <= end && (start < a || b < end);
    let at_an_edge = a == b && (a == start || a == end);
    if encloses && !at_an_edge {
        return None;
    }
    // Typed *at* the start is typed inside the scope, so that one insertion
    // leaves the start where it is.
    let a = if a < start || (a == start && end == start) {
        a
    } else if a >= end {
        moved(a)
    } else {
        start
    };
    let b = if b < start {
        b
    } else if b >= end {
        moved(b)
    } else {
        start + inserted
    };
    Some((a, b))
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
    // The lines eight bytes at a time, and the column over the last line only:
    // the status bar asks this on every caret move.
    let before = &text[..end];
    let line = crate::text::count_newlines(before.as_bytes()) + 1;
    let line_start = before.rfind('\n').map_or(0, |p| p + 1);
    (line, before[line_start..].chars().count() + 1)
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

/// Typing `ch` at a bare caret (`sel_a == sel_b`) when it completes a block
/// word as the only word on its line: the edit that moves the line where the
/// block puts it, `ch` included —
///
/// - `END`: to the indent of the line whose `BEGIN` or `CASE` it closes
///   ([`crate::pairs::match_block`]);
/// - `WHEN`, `ELSE`: one `unit` under the line of the `CASE` they are in
///   ([`crate::pairs::enclosing_block`]); an `ELSE` in any other block is not
///   a `CASE`'s and is left alone.
///
/// `None` — the character types as usual — when it completes no such word,
/// there is more on the line, there is no block, or the line is already where
/// it belongs.
pub fn dedent_typed_word(
    full: &str,
    sel_a: usize,
    sel_b: usize,
    ch: char,
    unit: &str,
    dialect: SqlDialect,
) -> Option<IndentEdit> {
    if sel_a != sel_b {
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
    let completes = |w: &str| {
        word.len() + 1 == w.len()
            && w[..word.len()].eq_ignore_ascii_case(word)
            && ch.eq_ignore_ascii_case(&(w.as_bytes()[word.len()] as char))
    };
    // Before the copy below: a `d`, `n` or `e` is a large share of all typing,
    // and the line almost never spells the rest of one of these words.
    if !(completes("END") || completes("WHEN") || completes("ELSE")) {
        return None;
    }
    // Asked of the text as it will be, with `ch` typed.
    let typed = format!("{}{ch}{}", &full[..caret], &full[caret..]);
    let word_at = caret - word.len();
    let indent_of = |at: usize| {
        let line = &typed[typed[..at].rfind('\n').map_or(0, |i| i + 1)..];
        line[..line.len() - line.trim_start_matches([' ', '\t']).len()].to_string()
    };
    let indent = if completes("END") {
        let (_, (opener, _)) = crate::pairs::match_block(&typed, word_at, dialect)?;
        indent_of(opener)
    } else if completes("WHEN") || completes("ELSE") {
        let (case, _, is_case) = crate::pairs::enclosing_block(&typed, word_at, dialect)?;
        if !is_case {
            return None;
        }
        format!("{}{unit}", indent_of(case))
    } else {
        return None;
    };
    if indent == before[..before.len() - word.len()] {
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
        if a < at {
            // The cut before this one ends where this line starts, and took
            // that line end with it: the run of cuts ending the text takes
            // the line end before its first line instead — the last thing
            // kept, since that first line starts right after it.
            if text.ends_with('\n') {
                text.pop();
            }
        } else {
            text.push_str(&full[at..a]);
        }
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

/// One caret's share of an edit made at every caret: replace `[start, end)` of
/// the original text with `insert`, leaving the selection `sel` — offsets in
/// the text **this edit alone** would produce. A caret that only moves is an
/// empty edit at its own spot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionEdit {
    pub start: usize,
    pub end: usize,
    pub insert: String,
    pub sel: (usize, usize),
}

impl RegionEdit {
    /// A whole-text rewrite (`old` → `new`, as [`toggle_line_comment`] and
    /// [`move_line`] answer) narrowed to the span that changed, so it can sit
    /// beside the other carets' edits.
    pub fn between(old: &str, new: &str, sel: (usize, usize)) -> RegionEdit {
        let (a, b) = (old.as_bytes(), new.as_bytes());
        let mut pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
        while !old.is_char_boundary(pre) {
            pre -= 1;
        }
        let room = a.len().min(b.len()) - pre;
        let mut suf = a
            .iter()
            .rev()
            .zip(b.iter().rev())
            .take(room)
            .take_while(|(x, y)| x == y)
            .count();
        while !old.is_char_boundary(a.len() - suf) || !new.is_char_boundary(b.len() - suf) {
            suf -= 1;
        }
        RegionEdit {
            start: pre,
            end: a.len() - suf,
            insert: new[pre..b.len() - suf].to_string(),
            sel,
        }
    }
}

impl From<IndentEdit> for RegionEdit {
    /// The single-caret operations' answer — a span, its replacement, the
    /// selection after — as one caret's share.
    fn from(e: IndentEdit) -> Self {
        RegionEdit {
            start: e.start,
            end: e.end,
            insert: e.text,
            sel: e.sel,
        }
    }
}

/// Every caret's edit made as one: the replacements in the **original**
/// text's offsets, ascending — what one multi-region document edit takes —
/// and each caret's selection in the text they leave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiEdit {
    pub edits: Vec<(usize, usize, String)>,
    pub sels: Vec<(usize, usize)>,
}

/// Put the carets' edits together ([`MultiEdit`]): each selection moved by
/// what the edits before it added or removed. `None` when two of them overlap,
/// which no single edit can make — [`per_caret`] and [`indent_all`] merge the
/// overlaps a keystroke can mean before they get here.
pub fn compose(mut edits: Vec<RegionEdit>) -> Option<MultiEdit> {
    edits.sort_by_key(|e| (e.start, e.end));
    let mut out = MultiEdit {
        edits: Vec::new(),
        sels: Vec::new(),
    };
    let mut shift = 0isize;
    let mut last_end = 0usize;
    for (i, e) in edits.into_iter().enumerate() {
        if i > 0 && e.start < last_end {
            return None;
        }
        last_end = e.end;
        let at = |p: usize| (p as isize + shift).max(0) as usize;
        out.sels.push((at(e.sel.0), at(e.sel.1)));
        shift += e.insert.len() as isize - (e.end - e.start) as isize;
        if e.start != e.end || !e.insert.is_empty() {
            out.edits.push((e.start, e.end, e.insert));
        }
    }
    Some(out)
}

/// One [`line_blocks`] block: the carets whose lines touch or overlap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineBlock {
    /// The members' extent, `(lowest start, highest end)`.
    pub lo: usize,
    pub hi: usize,
    /// The block's whole lines as a selection no line operation misreads:
    /// from its first line's start to the next line's start (which, being
    /// a line start, does not pull that line in), or to the text's end. The
    /// members' extent can end at a line start that one of them *does* take
    /// — a bare caret there.
    pub lines: (usize, usize),
    /// Each caret's own `(anchor, head)`, in order, as it was given.
    pub members: Vec<(usize, usize)>,
}

/// The carets' selections gathered into **line blocks**: those whose lines
/// touch or overlap merged into one block, ascending — what a line operation
/// (comment, move, duplicate, outdent, join) acts on once per block, so two
/// carets on one line comment it once, and two on neighbouring lines move
/// together. Each block keeps its members, so every caret can follow the edit.
pub fn line_blocks(text: &str, regions: &[(usize, usize)]) -> Vec<LineBlock> {
    let len = text.len();
    let starts = line_start_offsets(text);
    // (first line, last line, lo, hi, the member as given)
    let mut spans: Vec<_> = regions
        .iter()
        .map(|&(a, b)| {
            let (a, b) = (a.min(len), b.min(len));
            let (lo, hi) = (a.min(b), a.max(b));
            let first = line_index_of(&starts, lo);
            let mut last = line_index_of(&starts, hi);
            // A selection ending at a line's start does not take that line.
            if hi > lo && last > first && starts[last] == hi {
                last -= 1;
            }
            (first, last, lo, hi, (a, b))
        })
        .collect();
    spans.sort();
    let mut out: Vec<(usize, usize, LineBlock)> = Vec::new();
    for (first, last, lo, hi, member) in spans {
        match out.last_mut() {
            Some((_, m_last, m)) if first <= *m_last + 1 => {
                *m_last = (*m_last).max(last);
                m.lo = m.lo.min(lo);
                m.hi = m.hi.max(hi);
                m.members.push(member);
            }
            _ => out.push((
                first,
                last,
                LineBlock {
                    lo,
                    hi,
                    lines: (0, 0),
                    members: vec![member],
                },
            )),
        }
    }
    out.into_iter()
        .map(|(first, last, mut block)| {
            block.lines = (starts[first], starts.get(last + 1).copied().unwrap_or(len));
            block
        })
        .collect()
}

/// A **box selection** dragged from `anchor` to `head` (byte offsets): one
/// region a line, from the top line of the two to the bottom, each spanning
/// the columns between theirs — counted in characters, the editor's font being
/// monospaced — with the caret on the head's side. A line with nothing inside
/// the box is left out; a box of no width puts a caret on every line, at its
/// end where the line is shorter. Never empty: the head alone at least.
pub fn box_regions(text: &str, anchor: usize, head: usize) -> Vec<(usize, usize)> {
    let len = text.len();
    let (anchor, head) = (anchor.min(len), head.min(len));
    // The starts of the box's own lines only, from the upper end's line to
    // the lower end: this runs on every pointer move of the drag, and a table
    // of the whole document was a full pass of a 16 MiB script per move.
    let (top, bottom) = (anchor.min(head), anchor.max(head));
    let first = text[..top].rfind('\n').map_or(0, |i| i + 1);
    let mut starts = vec![first];
    starts.extend(
        text.as_bytes()[first..bottom]
            .iter()
            .enumerate()
            .filter(|&(_, &c)| c == b'\n')
            .map(|(i, _)| first + i + 1),
    );
    let line_end = |l: usize| {
        let end = starts.get(l + 1).map_or_else(
            || text[starts[l]..].find('\n').map_or(len, |i| starts[l] + i),
            |&s| s - 1,
        );
        // A CRLF line's `\r` is not a column.
        if text[..end].ends_with('\r') {
            end - 1
        } else {
            end
        }
    };
    let col = |off: usize, l: usize| text[starts[l]..off].chars().count();
    let (la, lh) = (line_index_of(&starts, anchor), line_index_of(&starts, head));
    let (ca, ch) = (col(anchor, la), col(head, lh));
    let (c0, c1) = (ca.min(ch), ca.max(ch));
    let mut out = Vec::new();
    for l in la.min(lh)..=la.max(lh) {
        let line = &text[starts[l]..line_end(l)];
        let chars = line.chars().count();
        if chars <= c0 && c0 != c1 {
            continue;
        }
        let at = |c: usize| starts[l] + line.char_indices().nth(c).map_or(line.len(), |(i, _)| i);
        let (s, e) = (at(c0.min(chars)), at(c1.min(chars)));
        out.push(if ch < ca { (e, s) } else { (s, e) });
    }
    if out.is_empty() {
        out.push((head, head));
    }
    out
}

/// A line operation — comment, move, duplicate — at every caret: once per
/// [`line_blocks`] block, each asked of the original text (`op(text, lo,
/// hi)`), then put together. `None` when any block refuses (a move past the
/// first or last line moves nothing at all) or the edits would overlap.
pub fn per_line_block(
    text: &str,
    regions: &[(usize, usize)],
    op: impl Fn(&str, usize, usize) -> Option<RegionEdit>,
) -> Option<MultiEdit> {
    let edits = line_blocks(text, regions)
        .into_iter()
        .map(|b| op(text, b.lo, b.hi))
        .collect::<Option<Vec<_>>>()?;
    compose(edits)
}

/// A line operation at every caret that **keeps every caret**: once per
/// [`line_blocks`] block, `op` answering the block's edit and each member's
/// selection in the text that edit alone leaves, then put together as
/// [`compose`] does. `None` when any block refuses or the edits overlap.
fn per_line_block_each(
    text: &str,
    regions: &[(usize, usize)],
    op: impl Fn(&LineBlock) -> Option<(RegionEdit, Vec<(usize, usize)>)>,
) -> Option<MultiEdit> {
    let parts = line_blocks(text, regions)
        .iter()
        .map(op)
        .collect::<Option<Vec<_>>>()?;
    compose_members(parts)
}

/// [`compose`] for edits each carrying **several** carets' selections, in
/// the text that edit alone leaves: every selection moved by what the edits
/// before it added or removed. `None` when two of the edits overlap.
fn compose_members(mut parts: Vec<(RegionEdit, Vec<(usize, usize)>)>) -> Option<MultiEdit> {
    // `compose` sorts its edits the same way, so its answer lines up.
    parts.sort_by_key(|(e, _)| (e.start, e.end));
    let mut sels = Vec::new();
    let mut shift = 0isize;
    for (e, members) in &parts {
        let at = |p: usize| (p as isize + shift).max(0) as usize;
        sels.extend(members.iter().map(|&(a, b)| (at(a), at(b))));
        shift += e.insert.len() as isize - (e.end - e.start) as isize;
    }
    let mut m = compose(parts.into_iter().map(|(e, _)| e).collect())?;
    m.sels = sels;
    Some(m)
}

/// A block whose lines move as one ([`move_line`], [`duplicate_lines`]):
/// a lone caret keeps the operation's own answer, several each slide by the
/// distance the block's first line went.
fn shifted_members(
    block: &LineBlock,
    edit: RegionEdit,
    from: usize,
) -> (RegionEdit, Vec<(usize, usize)>) {
    let sels = if block.members.len() == 1 {
        vec![edit.sel]
    } else {
        let shift = edit.sel.0 as isize - from as isize;
        let at = |p: usize| (p as isize + shift) as usize;
        block.members.iter().map(|&(a, b)| (at(a), at(b))).collect()
    };
    (edit, sels)
}

/// Shift+Tab at every caret ([`soft_tab_outdent`]): each block's lines lose
/// one level once, and each caret moves left by what was removed before it —
/// on its own line no further than the line's new start.
pub fn outdent_all(text: &str, regions: &[(usize, usize)], tw: usize) -> Option<MultiEdit> {
    let tw = tw.max(1);
    per_line_block_each(text, regions, |block| {
        if let [(a, b)] = block.members[..] {
            let e = soft_tab_outdent(text, a, b, tw);
            let sel = e.sel;
            return Some((e.into(), vec![sel]));
        }
        let (lo, hi) = block.lines;
        let e = soft_tab_outdent(text, lo, hi, tw);
        // Each line's start, and the bytes removed from the lines before it.
        let mut cuts = Vec::new();
        let (mut at, mut removed) = (e.start, 0usize);
        for line in text[e.start..e.end].split('\n') {
            let r = outdent_strip_len(line, tw);
            cuts.push((at, r, removed));
            removed += r;
            at += line.len() + 1;
        }
        let map = |p: usize| {
            let i = cuts.partition_point(|&(s, _, _)| s <= p).saturating_sub(1);
            let (s, r, before) = cuts[i];
            p - before - r.min(p - s)
        };
        let sels = block
            .members
            .iter()
            .map(|&(a, b)| (map(a), map(b)))
            .collect();
        Some((e.into(), sels))
    })
}

/// An edit made **at each caret** — Ctrl+Shift+U ([`toggle_case`]),
/// Ctrl+Shift+/ ([`toggle_block_comment`]) — each asked of the original
/// text (`op(text, a, b)`), then put together.
///
/// A caret `op` answers `None` for has nothing to change there (a word with
/// no letters, a blank line): it only keeps its place, moved by the others'
/// edits, rather than refusing the keystroke at every caret. Carets asking
/// for the **same** edit — two in one word, two on one line — make it once,
/// each keeping its own selection. `None` when no caret changes anything, or
/// when two different edits overlap.
pub fn per_caret(
    text: &str,
    regions: &[(usize, usize)],
    op: impl Fn(&str, usize, usize) -> Option<RegionEdit>,
) -> Option<MultiEdit> {
    let mut parts: Vec<(RegionEdit, Vec<(usize, usize)>)> = Vec::new();
    let mut idle = Vec::new();
    for &(a, b) in regions {
        let Some(e) = op(text, a, b) else {
            idle.push((a, b));
            continue;
        };
        let same = parts
            .iter_mut()
            .find(|(p, _)| (p.start, p.end, &p.insert) == (e.start, e.end, &e.insert));
        match same {
            Some((_, sels)) => sels.push(e.sel),
            None => {
                let sel = e.sel;
                parts.push((e, vec![sel]));
            }
        }
    }
    if parts.is_empty() {
        return None;
    }
    let mut m = compose_members(parts)?;
    let idle: Vec<_> = idle
        .into_iter()
        .map(|(a, b)| (moved_by(&m.edits, a), moved_by(&m.edits, b)))
        .collect();
    m.sels.extend(idle);
    Some(m)
}

/// Where offset `p` of the original text lands after `edits` (ascending, in
/// the original's offsets): moved by every edit ending at or before it, and
/// one it falls inside clamped into that edit's replacement.
fn moved_by(edits: &[(usize, usize, String)], p: usize) -> usize {
    let mut shift = 0isize;
    for (s, e, t) in edits {
        if *e <= p {
            shift += t.len() as isize - (e - s) as isize;
        } else if *s < p {
            return (*s as isize + shift) as usize + (p - s).min(t.len());
        } else {
            break;
        }
    }
    (p as isize + shift) as usize
}

/// Tab at every caret ([`soft_tab_indent`] with `soft`, else
/// [`hard_tab_indent`]): each caret indents where it is, in one edit.
///
/// Carets whose edits would overlap — two selections on one line, a bare
/// caret inside another's selected lines — indent those lines **once**
/// together, and each keeps its own place, moved by the indent added before
/// it. A bare caret at a line's first non-blank column goes with the text;
/// a selection's end at a line start with no indent keeps the indent inside.
pub fn indent_all(text: &str, regions: &[(usize, usize)], tw: usize, soft: bool) -> MultiEdit {
    let tw = tw.max(1);
    let pad = if soft {
        " ".repeat(tw)
    } else {
        "\t".to_string()
    };
    let mut edits: Vec<((usize, usize), IndentEdit)> = regions
        .iter()
        .map(|&(a, b)| {
            let e = if soft {
                soft_tab_indent(text, a, b, tw)
            } else {
                hard_tab_indent(text, a, b)
            };
            ((a, b), e)
        })
        .collect();
    // Widest first at one start, so a bare caret at a selected line's start
    // falls inside that selection's edit rather than before it.
    edits.sort_by_key(|(_, e)| (e.start, std::cmp::Reverse(e.end)));
    let mut clusters: Vec<Vec<((usize, usize), IndentEdit)>> = Vec::new();
    let mut last_end = 0usize;
    for item in edits {
        match clusters.last_mut() {
            Some(c) if item.1.start < last_end => {
                last_end = last_end.max(item.1.end);
                c.push(item);
            }
            _ => {
                last_end = item.1.end;
                clusters.push(vec![item]);
            }
        }
    }
    let parts = clusters
        .into_iter()
        .map(|mut c| {
            if c.len() == 1 {
                let (_, e) = c.remove(0);
                let sel = e.sel;
                return (e.into(), vec![sel]);
            }
            // A cluster holds a selection's edit, which spans whole lines.
            let lo = c.iter().map(|(_, e)| e.start).min().unwrap_or(0);
            let hi = c.iter().map(|(_, e)| e.end).max().unwrap_or(lo);
            let e = indent_lines(text, lo, hi, &pad);
            // Each indented line's insertion point, and whether it has an
            // indent of its own already.
            let mut ins = Vec::new();
            let mut at = e.start;
            for line in text[e.start..e.end].split('\n') {
                if !line.is_empty() {
                    let fnb = line.len() - line.trim_start().len();
                    ins.push((at + fnb, fnb > 0));
                }
                at += line.len() + 1;
            }
            let map = |p: usize, bare: bool| {
                let n = ins
                    .iter()
                    .filter(|&&(i, indented)| i < p || (i == p && (indented || bare)))
                    .count();
                p + pad.len() * n
            };
            let sels = c
                .iter()
                .map(|&((a, b), _)| (map(a, a == b), map(b, a == b)))
                .collect();
            (e.into(), sels)
        })
        .collect();
    compose_members(parts).unwrap_or_else(|| MultiEdit {
        edits: Vec::new(),
        sels: regions.to_vec(),
    })
}

/// Alt+Up/Down at every caret ([`move_line`]): every block moves, or — one
/// at the edge — none does, each caret with its own line.
pub fn move_lines_all(text: &str, regions: &[(usize, usize)], up: bool) -> Option<MultiEdit> {
    per_line_block_each(text, regions, |block| {
        let (lo, hi) = match block.members[..] {
            [(a, b)] => (a.min(b), a.max(b)),
            _ => block.lines,
        };
        let e = move_line(text, lo, hi, up)?;
        Some(shifted_members(
            block,
            RegionEdit::between(text, &e.text, e.sel),
            lo,
        ))
    })
}

/// Ctrl+D at every caret ([`duplicate_lines`]): each block's lines copied
/// once below them, each caret onto its own line's copy.
pub fn duplicate_lines_all(text: &str, regions: &[(usize, usize)]) -> Option<MultiEdit> {
    per_line_block_each(text, regions, |block| {
        let (lo, hi) = match block.members[..] {
            [(a, b)] => (a.min(b), a.max(b)),
            _ => block.lines,
        };
        let (at, insert, sel) = duplicate_lines(text, lo, hi);
        let edit = RegionEdit {
            start: at,
            end: at,
            insert,
            sel,
        };
        Some(shifted_members(block, edit, lo))
    })
}

/// Ctrl+Shift+J at every caret ([`join_lines`]): the breaks each caret would
/// join alone, joined once a block — so carets on two touching lines join
/// three lines — with each caret at its own line's join and a selection kept
/// over the same text. A lone caret is exactly [`join_lines`]'s. `None` when
/// nothing joins.
pub fn join_lines_all(
    text: &str,
    regions: &[(usize, usize)],
    dialect: SqlDialect,
) -> Option<MultiEdit> {
    per_line_block_each(text, regions, |block| {
        if let [(a, b)] = block.members[..] {
            let e = join_lines(text, a, b, dialect)?;
            let sel = e.sel;
            return Some((e.into(), vec![sel]));
        }
        let mut breaks: Vec<usize> = block
            .members
            .iter()
            .flat_map(|&(a, b)| join_candidates(text, a, b).unwrap_or_default())
            .collect();
        breaks.sort_unstable();
        breaks.dedup();
        let plan = join_breaks(text, breaks, dialect)?;
        let sels = block
            .members
            .iter()
            .map(|&(a, b)| {
                if a == b {
                    // Where this caret's own line joins, as a lone caret
                    // lands at its join; on its spot when that break stays.
                    let p = floor_char(text, a);
                    let nl = text[p..].find('\n').map_or(text.len(), |i| p + i);
                    let at = plan.seg_start(nl).unwrap_or_else(|| plan.map(p));
                    (at, at)
                } else {
                    (plan.map(a), plan.map(b))
                }
            })
            .collect();
        Some((plan.edit(), sels))
    })
}

/// [`crate::pairs::auto_pair`] at every caret: each decided against the
/// original text, a caret with no pair to make typing `ch` plainly. `None`
/// when no caret pairs — the editor's own insert then does it everywhere —
/// or when the edits would overlap.
pub fn auto_pair_all(
    text: &str,
    regions: &[(usize, usize)],
    ch: char,
    dialect: SqlDialect,
) -> Option<MultiEdit> {
    // **Every caret's region from one walk** (`pairs::regions_at`), not a
    // `region_at` walk from byte 0 per caret: a zero-width box down a
    // 3,200-line script made typing a quote there quadratic — 600 ms a
    // keystroke, measured, where one walk is a couple of milliseconds.
    let mut order: Vec<usize> = (0..regions.len()).collect();
    order.sort_by_key(|&k| regions[k].0.min(regions[k].1));
    let offsets: Vec<usize> = order
        .iter()
        .map(|&k| regions[k].0.min(regions[k].1))
        .collect();
    let mut region = vec![crate::pairs::Region::Code; regions.len()];
    for (&k, r) in order
        .iter()
        .zip(crate::pairs::regions_at(text, &offsets, dialect))
    {
        region[k] = r;
    }
    let mut paired = false;
    let edits = regions
        .iter()
        .zip(&region)
        .map(|(&(a, b), &r)| {
            let (lo, hi) = (a.min(b), a.max(b));
            match crate::pairs::auto_pair_in(text, a, b, ch, dialect, || r) {
                Some(crate::pairs::PairAction::Insert {
                    start,
                    end,
                    insert,
                    sel,
                }) => {
                    paired = true;
                    RegionEdit {
                        start,
                        end,
                        insert,
                        sel,
                    }
                }
                Some(crate::pairs::PairAction::Skip { caret }) => {
                    paired = true;
                    RegionEdit {
                        start: lo,
                        end: lo,
                        insert: String::new(),
                        sel: (caret, caret),
                    }
                }
                None => {
                    let at = lo + ch.len_utf8();
                    RegionEdit {
                        start: lo,
                        end: hi,
                        insert: ch.to_string(),
                        sel: (at, at),
                    }
                }
            }
        })
        .collect();
    if !paired {
        return None;
    }
    compose(edits)
}

/// Backspace at every bare caret, an empty pair (`(|)`, `'|'`) deleted whole
/// as [`crate::pairs::backspace_pair`] does it and anything else one
/// character. `None` when no caret sits in a pair — the editor's own
/// Backspace then serves them all.
pub fn backspace_pair_all(text: &str, carets: &[usize], dialect: SqlDialect) -> Option<MultiEdit> {
    // The spans the lexer answers, from one walk for every caret that looks
    // like it sits in a pair, rather than one walk from byte 0 each.
    let carets: Vec<usize> = carets.iter().map(|&c| c.min(text.len())).collect();
    let mut asked: Vec<usize> = carets
        .iter()
        .copied()
        .filter(|&c| crate::pairs::may_be_empty_pair(text, c, dialect))
        .map(|c| c - 1)
        .collect();
    asked.sort_unstable();
    asked.dedup();
    let spans = crate::pairs::noncode_spans_at(text.as_bytes(), &asked, dialect);
    let span_of = |c: usize| asked.binary_search(&(c - 1)).ok().and_then(|k| spans[k]);
    let mut paired = false;
    let edits = carets
        .iter()
        .map(|&c| {
            if let Some((s, e)) = crate::pairs::backspace_pair_in(text, c, dialect, || span_of(c)) {
                paired = true;
                return RegionEdit {
                    start: s,
                    end: e,
                    insert: String::new(),
                    sel: (s, s),
                };
            }
            let prev = text[..c].char_indices().next_back().map_or(c, |(i, _)| i);
            RegionEdit {
                start: prev,
                end: c,
                insert: String::new(),
                sel: (prev, prev),
            }
        })
        .collect();
    if !paired {
        return None;
    }
    compose(edits)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Apply a [`MultiEdit`] as the editor would: every replacement in the
    /// original offsets, at once.
    fn apply(text: &str, m: &MultiEdit) -> String {
        let mut out = String::new();
        let mut at = 0;
        for (s, e, ins) in &m.edits {
            out.push_str(&text[at..*s]);
            out.push_str(ins);
            at = *e;
        }
        out.push_str(&text[at..]);
        out
    }

    #[test]
    fn each_caret_pairs_where_it_would_alone() {
        let my = SqlDialect::MySql;
        let text = "a \nb \nc";
        let carets: Vec<(usize, usize)> = [2, 5].iter().map(|&c| (c, c)).collect();
        let m = auto_pair_all(text, &carets, '(', my).unwrap();
        assert_eq!(apply(text, &m), "a ()\nb ()\nc");
        assert_eq!(m.sels, vec![(3, 3), (8, 8)]);
        // Typing the closer steps over each one.
        let after = apply(text, &m);
        let m2 = auto_pair_all(&after, &m.sels, ')', my).unwrap();
        assert!(m2.edits.is_empty());
        assert_eq!(m2.sels, vec![(4, 4), (9, 9)]);
        // A caret before a word types plainly beside one that pairs.
        let mixed = "x\n ";
        let m3 = auto_pair_all(mixed, &[(0, 0), (3, 3)], '(', my).unwrap();
        assert_eq!(apply(mixed, &m3), "(x\n ()");
        assert_eq!(m3.sels, vec![(1, 1), (5, 5)]);
        // A selection at each caret is wrapped.
        let words = "ab cd";
        let m4 = auto_pair_all(words, &[(0, 2), (3, 5)], '\'', my).unwrap();
        assert_eq!(apply(words, &m4), "'ab' 'cd'");
        // No caret pairs: the editor's own insert serves them all.
        assert_eq!(auto_pair_all("xy", &[(0, 0), (1, 1)], '(', my), None);
    }

    #[test]
    fn backspace_deletes_each_carets_empty_pair_and_one_char_elsewhere() {
        let my = SqlDialect::MySql;
        let text = "f()\ng('')\nhi";
        let m = backspace_pair_all(text, &[2, 7, 12], my).unwrap();
        assert_eq!(apply(text, &m), "f\ng()\nh");
        assert_eq!(m.sels, vec![(1, 1), (4, 4), (7, 7)]);
        assert_eq!(backspace_pair_all("ab", &[1, 2], my), None);
        // At the very start, nothing to delete.
        let m = backspace_pair_all("()", &[0, 1], my).unwrap();
        assert_eq!(apply("()", &m), "");
        assert_eq!(m.sels, vec![(0, 0), (0, 0)]);
    }

    /// The batched lexer walk behind `auto_pair_all` and `backspace_pair_all`
    /// answers what asking each caret alone does — every caret position of a
    /// text with strings, comments and pairs, carets given out of order and
    /// two at one spot included.
    #[test]
    fn many_carets_pair_exactly_as_each_would_alone() {
        let texts = [
            "f() x '' \"\" `` y\n('a') /* ( */ 'it''s' -- ''\nz $$ () $$ []",
            "select (1, 'two', 3);\nselect '' , (), x;\n",
        ];
        for text in texts {
            for d in [
                SqlDialect::MySql,
                SqlDialect::Postgres,
                SqlDialect::MsSql,
                SqlDialect::Sqlite,
            ] {
                // Every third position, from each of three starting points,
                // so neighbouring carets' edits never overlap and the
                // composed answers are worth comparing.
                for phase in 0..3 {
                    let mut carets: Vec<usize> = (0..=text.len())
                        .filter(|&c| c % 3 == phase && text.is_char_boundary(c))
                        .rev()
                        .collect();
                    carets.push(carets[3]);
                    for ch in ['(', ')', '\'', '"', '`', '[', 'x'] {
                        let regions: Vec<(usize, usize)> = carets.iter().map(|&c| (c, c)).collect();
                        let one =
                            |&(a, b): &(usize, usize)| crate::pairs::auto_pair(text, a, b, ch, d);
                        let alone: Vec<_> = regions.iter().map(one).collect();
                        let together = auto_pair_all(text, &regions, ch, d);
                        // Rebuilt from the lone answers, as auto_pair_all did.
                        let expect = alone.iter().any(Option::is_some).then(|| {
                            let edits = regions
                                .iter()
                                .zip(&alone)
                                .map(|(&(c, _), a)| match a {
                                    Some(crate::pairs::PairAction::Insert {
                                        start,
                                        end,
                                        insert,
                                        sel,
                                    }) => RegionEdit {
                                        start: *start,
                                        end: *end,
                                        insert: insert.clone(),
                                        sel: *sel,
                                    },
                                    Some(crate::pairs::PairAction::Skip { caret }) => RegionEdit {
                                        start: c,
                                        end: c,
                                        insert: String::new(),
                                        sel: (*caret, *caret),
                                    },
                                    None => RegionEdit {
                                        start: c,
                                        end: c,
                                        insert: ch.to_string(),
                                        sel: (c + ch.len_utf8(), c + ch.len_utf8()),
                                    },
                                })
                                .collect();
                            compose(edits)
                        });
                        assert_eq!(together, expect.flatten(), "{d:?} {ch:?} {text:?}");
                    }
                    carets.pop();
                    carets.retain(|&c| c > 0);
                    let alone: Vec<_> = carets
                        .iter()
                        .map(|&c| crate::pairs::backspace_pair(text, c, d))
                        .collect();
                    let expect = alone.iter().any(Option::is_some).then(|| {
                        let edits = carets
                            .iter()
                            .zip(&alone)
                            .map(|(&c, a)| {
                                let (s, e) = a.unwrap_or_else(|| {
                                    (text[..c].char_indices().next_back().unwrap().0, c)
                                });
                                RegionEdit {
                                    start: s,
                                    end: e,
                                    insert: String::new(),
                                    sel: (s, s),
                                }
                            })
                            .collect();
                        compose(edits)
                    });
                    assert_eq!(
                        backspace_pair_all(text, &carets, d),
                        expect.flatten(),
                        "{d:?} {text:?} phase {phase}"
                    );
                }
            }
        }
    }

    #[test]
    fn carets_on_touching_lines_make_one_block() {
        let text = "a\nb\nc\nd\ne\nf";
        // Lines 0 and 1 touch, 3 and 4 touch, line 2 parts them; two carets
        // on line 5, apart from line 4 by nothing, join it too.
        let spans = |rs: &[(usize, usize)]| -> Vec<(usize, usize)> {
            line_blocks(text, rs).iter().map(|b| (b.lo, b.hi)).collect()
        };
        assert_eq!(
            spans(&[(0, 0), (2, 2), (6, 6), (8, 8)]),
            vec![(0, 2), (6, 8)]
        );
        assert_eq!(
            spans(&[(0, 0), (6, 6), (10, 10), (11, 11)]),
            vec![(0, 0), (6, 6), (10, 11)]
        );
        // A selection ending at a line's start does not reach that line.
        assert_eq!(spans(&[(0, 2), (6, 6)]), vec![(0, 2), (6, 6)]);
        // Each block keeps its members, as given, and its whole lines —
        // through the next line's start, or the text's end.
        let blocks = line_blocks(text, &[(8, 8), (3, 2), (0, 0)]);
        assert_eq!(blocks[0].members, vec![(0, 0), (3, 2)]);
        assert_eq!(blocks[0].lines, (0, 4));
        assert_eq!(blocks[1].members, vec![(8, 8)]);
        assert_eq!(line_blocks(text, &[(10, 10)])[0].lines, (10, 11));
    }

    #[test]
    fn a_box_selection_takes_the_same_columns_on_every_line() {
        let text = "select a,\nb\n  from t\r\nwhere";
        let off = |line: usize, col: usize| {
            text.split('\n')
                .take(line)
                .map(|l| l.len() + 1)
                .sum::<usize>()
                + col
        };
        let pick = |rs: Vec<(usize, usize)>| -> Vec<String> {
            rs.into_iter()
                .map(|(a, b)| text[a.min(b)..a.max(b)].to_string())
                .collect()
        };
        // Columns 2..6 of lines 0–2; line 1 is too short and is left out.
        let rs = box_regions(text, off(0, 2), off(2, 6));
        assert_eq!(pick(rs.clone()), vec!["lect", "from"]);
        // Through the last line, which ends inside the box.
        let rs = box_regions(text, off(0, 2), off(3, 4));
        assert_eq!(pick(rs), vec!["le", "fr", "er"]);
        // Starting below the first line, and ending above the last.
        let rs = box_regions(text, off(3, 4), off(2, 2));
        assert_eq!(pick(rs), vec!["fr", "er"]);
        let rs = box_regions(text, off(1, 0), off(2, 4));
        assert_eq!(pick(rs), vec!["b", "  fr"]);
        // The caret on the head's side: dragged leftwards, each region ends
        // at its left column.
        let back = box_regions(text, off(2, 6), off(0, 2));
        assert_eq!(back.len(), 2);
        assert!(back.iter().all(|&(a, b)| b < a), "{back:?}");
        // No width: a caret on every line, at the end of a shorter one, and
        // the `\r` of a CRLF line is no column.
        let carets = box_regions(text, off(0, 8), off(2, 8));
        assert_eq!(carets, vec![(8, 8), (11, 11), (20, 20)]);
        // Upwards, the lines are still top to bottom.
        let up = box_regions(text, off(2, 4), off(0, 2));
        assert!(up.windows(2).all(|w| w[0].0 < w[1].0), "{up:?}");
        // Over nothing reachable, the head alone.
        assert_eq!(box_regions("a\nb", 2, 2), vec![(2, 2)]);
        let wide = "abcdef\nx";
        // `x` ends where the box begins: nothing of it is inside.
        assert_eq!(box_regions(wide, 3, 8), vec![(3, 1)]);
        // Columns count characters, not bytes.
        let utf = "città x\nabcde x";
        let r = box_regions(utf, 0, utf.find("abcd").unwrap() + 4);
        assert_eq!(pick_in(utf, &r), vec!["citt", "abcd"]);
    }

    fn pick_in(text: &str, rs: &[(usize, usize)]) -> Vec<String> {
        rs.iter()
            .map(|&(a, b)| text[a.min(b)..a.max(b)].to_string())
            .collect()
    }

    #[test]
    fn a_line_operation_acts_once_on_each_carets_block() {
        let my = SqlDialect::MySql;
        let text = "select 1\nselect 2\nselect 3\nselect 4";
        // Carets on lines 0 and 2 (and a second on line 2): two blocks.
        let comment = |t: &str, lo: usize, hi: usize| {
            let e = toggle_line_comment(t, lo, hi, my);
            Some(RegionEdit::between(t, &e.text, e.sel))
        };
        let m = per_line_block(text, &[(3, 3), (20, 20), (22, 22)], comment).unwrap();
        assert_eq!(
            apply(text, &m),
            "-- select 1\nselect 2\n-- select 3\nselect 4"
        );
        assert_eq!(m.sels.len(), 2);
        // Moving up: the first block cannot, so nothing moves.
        let up = |t: &str, lo: usize, hi: usize| {
            move_line(t, lo, hi, true).map(|e| RegionEdit::between(t, &e.text, e.sel))
        };
        assert_eq!(per_line_block(text, &[(3, 3), (20, 20)], up), None);
        let m = per_line_block(text, &[(12, 12), (30, 30)], up).unwrap();
        assert_eq!(apply(text, &m), "select 2\nselect 1\nselect 4\nselect 3");
        // Each caret stays on its own line, now one up.
        assert_eq!(m.sels, vec![(3, 3), (21, 21)]);
        // Duplicate each.
        let dup = |t: &str, lo: usize, hi: usize| {
            let (at, insert, sel) = duplicate_lines(t, lo, hi);
            Some(RegionEdit {
                start: at,
                end: at,
                insert,
                sel,
            })
        };
        let m = per_line_block(text, &[(0, 0), (27, 27)], dup).unwrap();
        assert_eq!(
            apply(text, &m),
            "select 1\nselect 1\nselect 2\nselect 3\nselect 4\nselect 4"
        );
    }

    #[test]
    fn outdent_join_and_case_act_at_every_caret() {
        let my = SqlDialect::MySql;
        // Two carets on one indented line outdent it once, not twice.
        let text = "        a\nb\n    c";
        let out = |t: &str, lo: usize, hi: usize| Some(soft_tab_outdent(t, lo, hi, 4).into());
        let m = per_line_block(text, &[(8, 8), (9, 9), (17, 17)], out).unwrap();
        assert_eq!(apply(text, &m), "    a\nb\nc");
        // Join each block's line with the next.
        let text = "a\nb\nc\nd";
        let join = |t: &str, lo: usize, hi: usize| join_lines(t, lo, hi, my).map(Into::into);
        let m = per_line_block(text, &[(0, 0), (4, 4)], join).unwrap();
        assert_eq!(apply(text, &m), "a b\nc d");
        // Toggle the case of the word at each caret.
        let text = "select x from t";
        let edits: Option<Vec<RegionEdit>> = [(1, 1), (9, 9)]
            .iter()
            .map(|&(a, b)| toggle_case(text, a, b).map(Into::into))
            .collect();
        let m = compose(edits.unwrap()).unwrap();
        assert_eq!(apply(text, &m), "SELECT x FROM t");
    }

    /// Carets on touching lines, or on one line, share a block — and each
    /// keeps its own place through the block's edit rather than the block
    /// becoming one selection the next key would replace.
    #[test]
    fn every_caret_in_a_line_block_survives_its_edit() {
        let my = SqlDialect::MySql;
        // Shift+Tab with a caret at the end of each of three indented lines.
        let text = "    a\n    b\n    c";
        let m = outdent_all(text, &[(5, 5), (11, 11), (17, 17)], 4).unwrap();
        assert_eq!(apply(text, &m), "a\nb\nc");
        assert_eq!(m.sels, vec![(1, 1), (3, 3), (5, 5)]);
        // Two carets on one line, a third on a line of its own.
        let text = "        a\nb\n    c";
        let m = outdent_all(text, &[(8, 8), (9, 9), (17, 17)], 4).unwrap();
        assert_eq!(apply(text, &m), "    a\nb\nc");
        assert_eq!(m.sels, vec![(4, 4), (5, 5), (9, 9)]);
        // A caret inside the indent stops at the line's new start.
        let text = "    a\n    b";
        let m = outdent_all(text, &[(2, 2), (11, 11)], 4).unwrap();
        assert_eq!(apply(text, &m), "a\nb");
        assert_eq!(m.sels, vec![(0, 0), (3, 3)]);

        let text = "a\nb\nc\nd";
        // Alt+Down: both lines move, each caret with its own.
        let m = move_lines_all(text, &[(1, 1), (3, 3)], false).unwrap();
        assert_eq!(apply(text, &m), "c\na\nb\nd");
        assert_eq!(m.sels, vec![(3, 3), (5, 5)]);
        let m = move_lines_all(text, &[(5, 5), (7, 7)], true).unwrap();
        assert_eq!(apply(text, &m), "a\nc\nd\nb");
        assert_eq!(m.sels, vec![(3, 3), (5, 5)]);
        assert_eq!(move_lines_all(text, &[(0, 0), (2, 2)], true), None);
        // Carets at the start of touching lines move both lines, not one.
        let m = move_lines_all(text, &[(0, 0), (2, 2)], false).unwrap();
        assert_eq!(apply(text, &m), "c\na\nb\nd");
        assert_eq!(m.sels, vec![(2, 2), (4, 4)]);
        // Ctrl+D: the block copied once, each caret onto its line's copy.
        let m = duplicate_lines_all(text, &[(1, 1), (3, 3)]).unwrap();
        assert_eq!(apply(text, &m), "a\nb\na\nb\nc\nd");
        assert_eq!(m.sels, vec![(5, 5), (7, 7)]);
        // Ctrl+Shift+J: each caret's line joins the next, so two carets on
        // touching lines join three lines, each caret at its own join.
        let m = join_lines_all(text, &[(1, 1), (3, 3)], my).unwrap();
        assert_eq!(apply(text, &m), "a b c\nd");
        assert_eq!(m.sels, vec![(1, 1), (3, 3)]);
        // A selection keeps covering the same text through the join.
        let m = join_lines_all("ab\ncd\nef", &[(0, 2), (4, 4)], my).unwrap();
        assert_eq!(apply("ab\ncd\nef", &m), "ab cd ef");
        assert_eq!(m.sels, vec![(0, 2), (5, 5)]);
        // A lone caret in its block behaves exactly as it would alone.
        let text = "a\nb\nc\nd\ne";
        let m = join_lines_all(text, &[(0, 0), (6, 6)], my).unwrap();
        assert_eq!(apply(text, &m), "a b\nc\nd e");
        assert_eq!(m.sels, vec![(1, 1), (7, 7)]);
    }

    /// Ctrl+Shift+U and Ctrl+Shift+/ at several carets: a caret with nothing
    /// to change only keeps its place, and carets asking for the same edit
    /// make it once, rather than either refusing the whole keystroke.
    #[test]
    fn a_caret_with_nothing_to_change_does_not_stop_the_others() {
        let case = |t: &str, a, b| toggle_case(t, a, b).map(Into::into);
        // `1` has no letters; `select` still upper-cases, both carets kept.
        let m = per_caret("select 1", &[(3, 3), (7, 7)], case).unwrap();
        assert_eq!(apply("select 1", &m), "SELECT 1");
        assert_eq!(m.sels, vec![(3, 3), (7, 7)]);
        // Two carets in one word re-case it once.
        let m = per_caret("select 1", &[(1, 1), (4, 4)], case).unwrap();
        assert_eq!(apply("select 1", &m), "SELECT 1");
        assert_eq!(m.sels, vec![(1, 1), (4, 4)]);
        // Nothing to change anywhere is still no edit at all.
        assert_eq!(per_caret("1 2", &[(0, 0), (2, 2)], case), None);

        let my = SqlDialect::MySql;
        let comment = |t: &str, a, b| toggle_block_comment(t, a, b, my).map(Into::into);
        // Two carets on one line wrap it once, each keeping its place.
        let text = "select 1\nselect 2";
        let one = toggle_block_comment(text, 1, 1, my).unwrap();
        let m = per_caret(text, &[(1, 1), (5, 5)], comment).unwrap();
        let mut want = text.to_string();
        want.replace_range(one.start..one.end, &one.text);
        assert_eq!(apply(text, &m), want);
        let grew = want.len() - text.len();
        let other = toggle_block_comment(text, 5, 5, my).unwrap();
        assert_eq!((other.start, other.end), (one.start, one.end));
        assert_eq!(m.sels, vec![one.sel, other.sel]);
        // A caret on a blank line comments nothing and moves with the edit.
        let text = "select 1\n\nselect 2";
        let m = per_caret(text, &[(1, 1), (9, 9)], comment).unwrap();
        assert!(apply(text, &m).starts_with("/*"));
        assert_eq!(m.sels, vec![one.sel, (9 + grew, 9 + grew)]);
    }

    /// Tab at several carets whose edits overlap — two selections on one
    /// line, a bare caret inside another's selected lines — indents those
    /// lines once and keeps every caret, rather than only the first.
    #[test]
    fn tab_with_overlapping_carets_indents_once_and_keeps_them_all() {
        // Two selections on one line.
        let m = indent_all("select 1", &[(0, 2), (4, 6)], 4, true);
        assert_eq!(apply("select 1", &m), "    select 1");
        assert_eq!(m.sels, vec![(0, 6), (8, 10)]);
        // A bare caret at the start of a line another caret has selected.
        let m = indent_all("a\nb", &[(0, 3), (2, 2)], 4, true);
        assert_eq!(apply("a\nb", &m), "    a\n    b");
        assert_eq!(m.sels, vec![(0, 11), (10, 10)]);
        // Hard tabs the same way.
        let m = indent_all("select 1", &[(0, 2), (4, 6)], 4, false);
        assert_eq!(apply("select 1", &m), "\tselect 1");
        assert_eq!(m.sels, vec![(0, 3), (5, 7)]);
        // Carets whose edits do not meet each indent where they are.
        let m = indent_all("a\nb", &[(1, 1), (3, 3)], 4, true);
        assert_eq!(apply("a\nb", &m), "a   \nb   ");
        assert_eq!(m.sels, vec![(4, 4), (9, 9)]);
    }

    #[test]
    fn a_whole_text_rewrite_narrows_to_what_changed() {
        let e = RegionEdit::between("select 1\nselect 2", "-- select 1\nselect 2", (3, 3));
        assert_eq!((e.start, e.end, e.insert.as_str()), (0, 0, "-- "));
        let e = RegionEdit::between("aXb", "ab", (1, 1));
        assert_eq!((e.start, e.end, e.insert.as_str()), (1, 2, ""));
        let e = RegionEdit::between("città", "città!", (0, 0));
        assert_eq!((e.start, e.end, e.insert.as_str()), (6, 6, "!"));
        assert_eq!(RegionEdit::between("same", "same", (0, 0)).insert, "");
    }

    #[test]
    fn overlapping_edits_are_refused_and_selections_move_with_earlier_edits() {
        let at = |s, e, i: &str, sel| RegionEdit {
            start: s,
            end: e,
            insert: i.to_string(),
            sel,
        };
        assert_eq!(
            compose(vec![at(0, 3, "x", (1, 1)), at(2, 4, "", (2, 2))]),
            None
        );
        let m = compose(vec![at(5, 5, "--", (7, 7)), at(0, 2, "", (0, 0))]).unwrap();
        assert_eq!(
            m.edits,
            vec![(0, 2, String::new()), (5, 5, "--".to_string())]
        );
        assert_eq!(m.sels, vec![(0, 0), (5, 5)]);
    }

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

    /// **Match case** takes the needle byte for byte; off, the fold is the
    /// default search's — so the default entry points are unchanged.
    #[test]
    fn match_case_finds_only_the_exact_spelling() {
        let exact = FindOptions {
            match_case: true,
            ..FindOptions::default()
        };
        assert_eq!(find_matches_in("Id id ID", "id", exact), vec![3]);
        assert_eq!(find_matches_in("Café café", "café", exact), vec![6]);
        assert_eq!(
            find_matches_in("Id id ID", "id", FindOptions::default()),
            find_matches("Id id ID", "id")
        );
        assert!(matches_at_in("xId", 1, "Id", exact));
        assert!(!matches_at_in("xid", 1, "Id", exact));
        assert_eq!(
            replace_all_in("Id id", "id", "n", exact),
            ("Id n".to_string(), 1)
        );
    }

    /// **Whole word** wants no word byte either side of the hit — by
    /// `sql::is_word_byte`, the one definition, so `_` and a non-ASCII letter
    /// continue a word as they do in an identifier. An edge of the needle that
    /// is not itself a word byte has no boundary to ask for.
    #[test]
    fn whole_word_finds_the_word_and_not_the_identifier_containing_it() {
        let word = FindOptions {
            whole_word: true,
            ..FindOptions::default()
        };
        let sql = "SELECT id, user_id, idé, ID FROM t WHERE (id)";
        let hits = find_matches_in(sql, "id", word);
        let words: Vec<&str> = hits.iter().map(|&h| &sql[h..h + 2]).collect();
        assert_eq!(words, ["id", "ID", "id"], "{hits:?}");
        assert_eq!(hits, vec![7, 26, 43]);
        // A needle that ends in punctuation still bounds the side that is a word.
        assert_eq!(find_matches_in("f(x) ff(x)", "f(", word), vec![0]);
        assert!(!matches_at_in("user_id", 5, "id", word));
        assert!(matches_at_in("(id)", 1, "id", word));
    }

    /// **Where a word ends is the tab's dialect's**, by `sql::continues_name`:
    /// `$` continues a name on every engine, and `@`/`#` on SQL Server. Whole
    /// word asked `is_word_byte` alone, so `b` was a hit inside the column
    /// `a$b` and Replace All rewrote it, and `id` inside `@id`.
    #[test]
    fn whole_word_ends_a_word_where_the_dialect_ends_a_name() {
        let on = |dialect| FindOptions {
            whole_word: true,
            dialect,
            ..FindOptions::default()
        };
        for d in [
            SqlDialect::MySql,
            SqlDialect::Postgres,
            SqlDialect::Sqlite,
            SqlDialect::MsSql,
        ] {
            assert_eq!(
                find_matches_in("SELECT a$b FROM t", "b", on(d)),
                Vec::<usize>::new(),
                "{d:?}"
            );
            assert_eq!(
                replace_all_in("SELECT a$b, b FROM t", "b", "c", on(d)),
                ("SELECT a$b, c FROM t".to_string(), 1),
                "{d:?}"
            );
        }
        let ms = on(SqlDialect::MsSql);
        assert_eq!(find_matches_in("SELECT @id, #id, id", "id", ms), vec![17]);
        // `@` is no name byte elsewhere: a PostgreSQL operator stays a boundary.
        assert_eq!(
            find_matches_in("SELECT a @ id", "id", on(SqlDialect::Postgres)),
            vec![11]
        );
        // The needle's own prefix is still found whole.
        assert_eq!(find_matches_in("SELECT @id, @idx", "@id", ms), vec![7]);
    }

    /// found whichever case it was typed in, as an ASCII one always was.
    #[test]
    fn find_folds_the_case_of_non_ascii_letters() {
        assert_eq!(find_matches("CAFÉ café Café", "café"), vec![0, 6, 12]);
        assert_eq!(find_matches("Привет ПРИВЕТ", "привет"), vec![0, 13]);
        assert_eq!(find_matches("Größe GRÖSSE größe", "GRÖßE"), vec![0, 16]);
        assert_eq!(find_matches("ΣΟΦΊΑ σοφία", "σοφία"), vec![0, 11]);
        // A word-final sigma: `ς` and `σ` share the capital `Σ`, and a
        // per-character lowercase leaves `ς` alone.
        assert_eq!(find_matches("ΟΔΌΣ οδός", "οδός"), vec![0, 9]);
        assert_eq!(find_matches("οδός ΟΔΌΣ", "ΟΔΌΣ"), vec![0, 9]);
        assert!(matches_at("Élan", 0, "élan"));
        assert_eq!(
            replace_all("Ärger ärger", "ärger", "x"),
            ("x x".to_string(), 2)
        );
    }

    /// Every hit still spans exactly `needle.len()` bytes, which is what the
    /// find bar's selection, highlight and replace all measure it by. A pair
    /// whose two cases differ in UTF-8 length — the Kelvin sign `K` (3 bytes)
    /// and `k`, the long `ſ` and `s` — is therefore left case-sensitive
    /// rather than reported with a span that cuts a character.
    #[test]
    fn a_folded_match_never_changes_the_hits_byte_length() {
        assert_eq!(find_matches("\u{212A}elvin kelvin", "kelvin"), vec![9]);
        assert_eq!(find_matches("ſ s S", "s"), vec![3, 5]);
        for (hay, needle) in [("Größe GRÖSSE", "größe"), ("Привет", "ПРИВЕТ")] {
            for off in find_matches(hay, needle) {
                assert!(hay.get(off..off + needle.len()).is_some());
                assert!(matches_at(hay, off, needle));
            }
        }
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
        let hits = [(3, 2), (10, 5), (20, 1)];
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

    // -- Hit-based find: regex, scope ------------------------------------------

    fn re() -> FindOptions {
        FindOptions {
            regex: true,
            ..FindOptions::default()
        }
    }

    /// The hits as the text they cover.
    fn spans<'a>(hay: &'a str, hits: &[Hit]) -> Vec<&'a str> {
        hits.iter().map(|&(s, n)| &hay[s..s + n]).collect()
    }

    #[test]
    fn plain_hits_are_the_plain_matches_with_the_needles_length() {
        let hay = "SELECT a FROM t; select b";
        let hits = find_hits(hay, "select", FindOptions::default(), None).unwrap();
        assert_eq!(hits, [(0, 6), (17, 6)]);
        // A regex metacharacter is literal text off regex mode.
        assert_eq!(
            find_hits("a.b axb", "a.b", FindOptions::default(), None).unwrap(),
            [(0, 3)]
        );
    }

    #[test]
    fn a_regex_hit_is_as_long_as_its_match() {
        let hay = "id = 1 AND id = 22 or id = 333";
        let hits = find_hits(hay, r"id = \d+", re(), None).unwrap();
        assert_eq!(spans(hay, &hits), ["id = 1", "id = 22", "id = 333"]);
        // Case-insensitive unless match case says otherwise.
        assert_eq!(
            spans(hay, &find_hits(hay, "and|OR", re(), None).unwrap()),
            ["AND", "or"]
        );
        let cased = FindOptions {
            match_case: true,
            ..re()
        };
        assert_eq!(
            spans(hay, &find_hits(hay, "and|OR", cased, None).unwrap()),
            Vec::<&str>::new()
        );
        // `^` and `$` are line ends.
        let lines = "a1\nb2\na3";
        assert_eq!(
            spans(lines, &find_hits(lines, r"^a\d$", re(), None).unwrap()),
            ["a1", "a3"]
        );
    }

    #[test]
    fn a_crlf_line_end_is_a_line_end_to_a_regex() {
        // Pasted from the Windows clipboard, nothing normalises the `\r\n`:
        // `$` still ends the line before it, and `.` stops short of the `\r`.
        let hay = "a;\r\nb;";
        assert_eq!(
            spans(hay, &find_hits(hay, ";$", re(), None).unwrap()),
            [";", ";"]
        );
        let hay = "x, y\r\nz";
        assert_eq!(
            spans(hay, &find_hits(hay, ",.*$", re(), None).unwrap()),
            [", y"]
        );
        assert_eq!(
            spans(hay, &find_hits(hay, "^z", re(), None).unwrap()),
            ["z"]
        );
    }

    #[test]
    fn an_empty_regex_match_is_not_a_hit() {
        assert!(find_hits("bcd", "a*", re(), None).unwrap().is_empty());
        let hay = "baab";
        assert_eq!(
            spans(hay, &find_hits(hay, "a*", re(), None).unwrap()),
            ["aa"]
        );
    }

    #[test]
    fn a_pattern_that_does_not_compile_says_so_in_one_line() {
        let err = find_hits("abc", "a(b", re(), None).unwrap_err();
        assert!(!err.contains('\n'), "{err:?}");
        assert!(err.starts_with("Not a valid pattern"), "{err:?}");
    }

    #[test]
    fn whole_word_bounds_a_regex_hit_by_the_dialects_names() {
        let hay = "user_id id ids";
        let opts = FindOptions {
            whole_word: true,
            ..re()
        };
        assert_eq!(
            spans(hay, &find_hits(hay, r"i\w*", opts, None).unwrap()),
            ["id", "ids"]
        );
    }

    #[test]
    fn a_scope_confines_both_modes_and_keeps_offsets_absolute() {
        let hay = "x a x\nx b x\nx c x";
        let scope = Some((6, 11));
        assert_eq!(
            find_hits(hay, "x", FindOptions::default(), scope).unwrap(),
            [(6, 1), (10, 1)]
        );
        assert_eq!(
            find_hits(hay, "[a-z]", re(), scope).unwrap(),
            [(6, 1), (8, 1), (10, 1)]
        );
        // A hit that runs past the scope's end is not in it.
        assert!(
            find_hits(hay, "x\nx", FindOptions::default(), scope)
                .unwrap()
                .is_empty()
        );
        // Whole word still reads the text outside the scope.
        let hay = "abc";
        let word = FindOptions {
            whole_word: true,
            ..FindOptions::default()
        };
        assert!(find_hits(hay, "b", word, Some((1, 2))).unwrap().is_empty());
    }

    #[test]
    fn the_hit_list_is_capped() {
        let hay = "a".repeat(FIND_HIT_CAP + 10);
        assert_eq!(
            find_hits(&hay, "a", FindOptions::default(), None)
                .unwrap()
                .len(),
            FIND_HIT_CAP
        );
    }

    #[test]
    fn a_stored_hit_holds_only_while_the_text_still_matches_there() {
        let hay = "id = 12; id = 3";
        assert!(hit_holds(hay, (0, 7), r"id = \d+", re(), None));
        // The same start, but the match there is now another length.
        assert!(!hit_holds(hay, (0, 6), r"id = \d+", re(), None));
        assert!(!hit_holds(hay, (1, 6), r"id = \d+", re(), None));
        assert!(hit_holds(hay, (9, 2), "ID", FindOptions::default(), None));
        assert!(!hit_holds(hay, (9, 3), "ID", FindOptions::default(), None));
        assert!(!hit_holds(hay, (99, 2), "id", FindOptions::default(), None));
        // Outside the scope it was found in, it is no hit of that scope's.
        assert!(!hit_holds(
            hay,
            (9, 2),
            "ID",
            FindOptions::default(),
            Some((0, 8))
        ));
    }

    #[test]
    fn a_regex_replacement_fills_its_groups() {
        let hay = "name = 'x'";
        let hit = find_hits(hay, r"(\w+) = ('\w+')", re(), None).unwrap()[0];
        assert_eq!(
            replacement_for(hay, hit, r"(\w+) = ('\w+')", "$2 AS ${1}_x $$", re(), None),
            "'x' AS name_x $"
        );
        // Off regex mode, `$1` is two characters of text.
        assert_eq!(
            replacement_for(hay, (0, 4), "name", "$1", FindOptions::default(), None),
            "$1"
        );
    }

    #[test]
    fn replace_hits_replaces_what_find_hits_found_and_nothing_outside_the_scope() {
        assert_eq!(
            replace_hits("a1 b22 a3", r"([ab])(\d+)", "$2$1", re(), None).unwrap(),
            ("1a 22b 3a".to_string(), 3)
        );
        assert_eq!(
            replace_hits("x x x", "x", "y", FindOptions::default(), Some((1, 4))).unwrap(),
            ("x y x".to_string(), 1)
        );
        // Not re-replaced in the same pass.
        assert_eq!(
            replace_hits("a", "a", "aa", re(), None).unwrap(),
            ("aa".to_string(), 1)
        );
        assert!(replace_hits("a", "(", "b", re(), None).is_err());
    }

    /// **A regex hit cut short by the scope's end is the same hit to every
    /// question**: the search stops at the scope's end, so revalidating,
    /// filling `$1` and Replace All must read the same text, or a greedy match
    /// that ran on past the scope is "gone" to one and longer to another.
    #[test]
    fn a_hit_the_scope_cut_short_is_the_same_hit_everywhere() {
        let hay = "aaaa";
        let scope = Some((0, 2));
        let hits = find_hits(hay, "(a+)", re(), scope).unwrap();
        assert_eq!(hits, [(0, 2)]);
        assert!(hit_holds(hay, hits[0], "(a+)", re(), scope));
        assert!(!hit_holds(hay, hits[0], "(a+)", re(), None));
        assert_eq!(
            replacement_for(hay, hits[0], "(a+)", "<$1>", re(), scope),
            "<aa>"
        );
        assert_eq!(
            replace_hits(hay, "(a+)", "<$1>", re(), scope).unwrap(),
            ("<aa>aa".to_string(), 1)
        );
        // `$` at the scope's end that is no line end.
        let lines = "ab\ncd";
        assert_eq!(
            find_hits(lines, "b$", re(), Some((0, 2))).unwrap(),
            [(1, 1)],
            "the search ends at the scope, as `$` sees it"
        );
    }

    #[test]
    fn a_selection_over_several_lines_is_searched_in_not_for() {
        let text = "SELECT a\nFROM t\nWHERE x";
        assert_eq!(find_scope_seed(text, (16, 2)), Some((2, 16)));
        assert_eq!(find_seed(text, (2, 16)), None);
        // One line is a needle, not a scope; nothing selected is neither.
        assert_eq!(find_scope_seed(text, (0, 6)), None);
        assert_eq!(find_scope_seed(text, (4, 4)), None);
        assert_eq!(find_scope_seed(text, (0, 999)), None);
        // A lone `\r` ends a line too, as `find_seed` reads it.
        assert_eq!(find_scope_seed("a\rb", (0, 3)), Some((0, 3)));
    }

    #[test]
    fn a_revealed_hit_seeds_nothing_on_a_bar_already_open() {
        // `;\s*SELECT` revealed a hit across a line break at 8..17.
        let hit = Some((8, 9));
        assert!(!selection_seeds_find(true, (8, 17), hit));
        assert!(!selection_seeds_find(true, (17, 8), hit), "either order");
        // Any other selection on an open bar is the user's own.
        assert!(selection_seeds_find(true, (0, 17), hit));
        assert!(selection_seeds_find(true, (8, 17), None));
        // A bar being opened reads the selection whatever it is.
        assert!(selection_seeds_find(false, (8, 17), hit));
    }

    #[test]
    fn a_scope_follows_the_edits_around_and_inside_it() {
        // Before it: both bounds move.
        assert_eq!(shift_range((10, 20), 0, 2, 5), Some((13, 23)));
        // After it: nothing moves.
        assert_eq!(shift_range((10, 20), 25, 26, 0), Some((10, 20)));
        // Inside it: the end moves, the scope grows or shrinks.
        assert_eq!(shift_range((10, 20), 12, 14, 0), Some((10, 18)));
        assert_eq!(shift_range((10, 20), 15, 15, 4), Some((10, 24)));
        // Typed at its very start: inside, not before.
        assert_eq!(shift_range((10, 20), 10, 10, 1), Some((10, 21)));
        // Over its whole extent: what replaced it.
        assert_eq!(shift_range((10, 20), 10, 20, 3), Some((10, 13)));
        // Over its start from before it.
        assert_eq!(shift_range((10, 20), 5, 12, 1), Some((5, 14)));
    }

    #[test]
    fn an_edit_enclosing_the_scope_loses_it_rather_than_widening_to_it() {
        // A whole-document Format replaces `0..len` in one edit: where the
        // scope was inside it is gone, and it must not become all of it.
        assert_eq!(shift_range((10, 20), 0, 30, 40), None);
        // Sharing one edge with an edit that runs past the other is the same.
        assert_eq!(shift_range((0, 20), 0, 30, 40), None);
        assert_eq!(shift_range((10, 30), 0, 30, 40), None);
        // A point strictly inside the edit is lost too…
        assert_eq!(shift_range((15, 15), 10, 20, 3), None);
        // …but one at its edge is where the edit began or ended.
        assert_eq!(shift_range((20, 20), 19, 20, 0), Some((19, 19)));
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
        // Carets on the last two lines: the run of cuts ending the text takes
        // the line end before its first line, leaving no blank line.
        let c = cut_lines("w\nx\ny", &[2, 4]);
        assert_eq!(
            (c.clipboard.as_str(), c.text.as_str(), c.caret),
            ("x\ny\n", "w", 0)
        );
        let c = cut_lines(full, &[1, 5, 10, full.len()]);
        assert_eq!(c.clipboard, format!("{full}\n"));
        assert_eq!(c.text, "");
        let c = cut_lines(full, &[1, 10, full.len()]);
        assert_eq!(c.text, "two");
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

    /// The caret follows the characters before it when the case change
    /// alters their byte length — `İ` lower-cases to `i` + U+0307 (2 → 3
    /// bytes), `ı` upper-cases to `I` (2 → 1) — so it never lands inside a
    /// character, where the next keystroke panicked the rope.
    #[test]
    fn toggle_case_keeps_a_bare_caret_on_a_character_boundary() {
        let e = toggle_case("İZMİR", 2, 2).unwrap();
        assert_eq!(e.text, "i\u{307}zmi\u{307}r");
        assert_eq!(e.sel, (3, 3), "just after the i̇");
        assert!(e.text.is_char_boundary(e.sel.0 - e.start));

        let e = toggle_case("ışık", 2, 2).unwrap();
        assert_eq!(e.text, "IŞIK");
        assert_eq!(e.sel, (1, 1), "just after the I");

        // Inside a longer text, measured from the word's start.
        let src = "SELECT 'İZMİR'";
        let e = toggle_case(src, 10, 10).unwrap();
        assert_eq!(e.sel, (11, 11));
        let out = format!("{}{}{}", &src[..e.start], e.text, &src[e.end..]);
        assert!(out.is_char_boundary(e.sel.0));
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
            dedent_typed_word(&clean, at, at, ch, "  ", d).map(|e| {
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

    /// `WHEN` and `ELSE` typed as a line's only word inside a `CASE` go one
    /// unit under the `CASE`'s line — the layout Format gives them — and an
    /// `ELSE` in any other block is left where it is.
    #[test]
    fn typing_a_case_branch_indents_it_under_its_case() {
        use SqlDialect::{MsSql, MySql};
        let typed = |src: &str, ch: char, d| {
            let at = src.find('|').unwrap();
            let clean = src.replacen('|', "", 1);
            dedent_typed_word(&clean, at, at, ch, "  ", d).map(|e| {
                let mut out = format!("{}{}{}", &clean[..e.start], e.text, &clean[e.end..]);
                out.insert(e.sel.0, '|');
                out
            })
        };
        assert_eq!(
            typed("SELECT CASE\nWHE|", 'N', MySql).as_deref(),
            Some("SELECT CASE\n  WHEN|")
        );
        assert_eq!(
            typed("  case\n    when a then 1\n          els|", 'e', MySql).as_deref(),
            Some("  case\n    when a then 1\n    else|")
        );
        assert_eq!(
            typed("SELECT CASE\n  WHE|", 'N', MySql),
            None,
            "already there"
        );
        assert_eq!(
            typed("BEGIN\n  IF x = 1\n    SET a = 1\n      ELS|", 'E', MsSql),
            None,
            "an IF's ELSE"
        );
        assert_eq!(typed("SELECT a\nWHE|", 'N', MySql), None, "no CASE");
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
