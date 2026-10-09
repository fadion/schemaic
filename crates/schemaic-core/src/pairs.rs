//! Auto-close bracket/quote pairs, type-over, wrap-selection, bracket matching,
//! and identifier-occurrence highlighting for the SQL editor. Pure (`&str` →
//! data, no UI) and **boundary-aware**
//! — every decision that must respect string / comment / identifier boundaries is
//! built on the one shared lexer [`crate::sql::skip_noncode`], so it agrees with
//! statement splitting, highlighting, and the intelligence layer by construction
//! (never a second hand-rolled scanner).
//!
//! Dialect-pluggable via [`SqlDialect`]: MySQL treats `` ` `` as an identifier
//! quote (auto-closed) where PostgreSQL doesn't, and each dialect's comment/string
//! boundaries flow through `skip_noncode`.

use crate::intel::SqlDialect;
use crate::sql::skip_noncode;

/// What kind of span the byte at a caret offset sits *inside*. A boundary
/// position (right at an opening delimiter, or just past a closing one) is
/// [`Region::Code`] — you're at a code position about to type there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Code,
    Str,
    Comment,
}

/// The edit an auto-pair keystroke resolves to. The UI applies it as a single
/// `edit_single` (one undo step) + cursor move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairAction {
    /// Replace `[start, end]` with `insert`, then set the selection to
    /// `[sel.0, sel.1]` (equal → a bare caret).
    Insert {
        start: usize,
        end: usize,
        insert: String,
        sel: (usize, usize),
    },
    /// Don't edit — just move the caret to `caret` (type over an existing closer).
    Skip { caret: usize },
}

/// Word byte for the adjacency guards — invariant 11's one definition.
use crate::sql::is_word_byte;

/// The role a typed character plays in the configured pair set, or `None` if it
/// isn't a pair character in this dialect.
enum Cat {
    /// An opening bracket `(open, close)`.
    Open(char, char),
    /// A closing bracket, carrying its own char.
    Close(char),
    /// A quote-style delimiter (open == close), e.g. `'`, `"`, `` ` ``.
    Quote(char),
}

fn classify(ch: char, dialect: SqlDialect) -> Option<Cat> {
    match ch {
        '(' => Some(Cat::Open('(', ')')),
        ')' => Some(Cat::Close(')')),
        '\'' => Some(Cat::Quote('\'')),
        '"' => Some(Cat::Quote('"')),
        // Backtick is a MySQL/MariaDB identifier quote, which SQLite takes too;
        // PostgreSQL has no such syntax, so don't auto-close it there. The
        // capability is `sql`'s, and asking it rather than re-spelling the
        // comparison is the same rule this module's own doc states about never
        // hand-rolling a second scanner.
        '`' if dialect.backtick_ident() => Some(Cat::Quote('`')),
        _ => None,
    }
}

/// True if the non-code span opening at `b[i]` is a comment (vs. a string /
/// quoted identifier). Only classifies a span the lexer already found — it does
/// **not** re-scan boundaries.
///
/// Delegates to [`crate::sql::comment_open`], as the boundary invariant requires:
/// this was a private byte test that had to be kept in step with the lexer by
/// hand, and it said `#` opened a comment on everything but Postgres — a claim
/// that stopped being true the moment SQLite existed.
fn is_comment_start(b: &[u8], i: usize, dialect: SqlDialect) -> bool {
    crate::sql::comment_open(b, i, dialect)
}

/// Classify what span the byte at `offset` sits inside, walking the shared
/// `skip_noncode` lexer from the start. A position exactly at a delimiter's start
/// or one just past its end counts as [`Region::Code`].
///
/// **It stops at the caret.** The answer is settled once the walk reaches
/// `offset`: a span opening at or after it cannot contain it, since the only
/// way to return is `offset > i && offset < j`. Everything past that was a scan
/// of the rest of the document for nothing — and this is on the editor's
/// *undebounced* per-caret-move path (`match_paren` and
/// `identifier_occurrences` are two `create_effect`s with no debounce and no
/// size cap, and `sqlfile::open_verdict` will open a 64 MiB script). At 16 MiB
/// the full walk measured 37.9 ms, so one arrow key was ~123 ms of UI-thread
/// work across the two effects.
pub fn region_at(text: &str, offset: usize, dialect: SqlDialect) -> Region {
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if i >= offset {
            break;
        }
        if let Some(j) = skip_noncode(b, i, dialect) {
            // Interior of the non-code span `[i, j)` is `i < offset < j`; the
            // boundaries themselves are code positions.
            if offset > i && offset < j {
                return if is_comment_start(b, i, dialect) {
                    Region::Comment
                } else {
                    Region::Str
                };
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    Region::Code
}

/// [`region_at`] for **many** offsets, in one walk of the text.
///
/// `offsets` must be non-decreasing; the answers come back in the same order,
/// one per offset, and each is exactly what `region_at` would have said.
///
/// Asking `region_at` in a loop is quadratic, and the loop is real: Ctrl+/
/// inside a PostgreSQL `$$ … $$` body called it **once per selected line**,
/// which measured 223 ms over a 3,200-line selection and quadrupled with every
/// doubling — around 11 s extrapolated at 1 MiB, on the UI thread, for one
/// keystroke. Every one of those walks re-scanned the same prefix.
///
/// The walk is the shared `skip_noncode` lexer, as everything here must be: an
/// offset the walk passes without finding a span that strictly contains it is
/// `Code`, which is `region_at`'s own rule read forwards.
pub fn regions_at(text: &str, offsets: &[usize], dialect: SqlDialect) -> Vec<Region> {
    debug_assert!(
        offsets.windows(2).all(|w| w[0] <= w[1]),
        "regions_at needs its offsets in order"
    );
    let b = text.as_bytes();
    let mut out = vec![Region::Code; offsets.len()];
    let mut k = 0usize;
    let mut i = 0usize;
    while i < b.len() && k < offsets.len() {
        // A span opening at or after an offset cannot contain it, so every
        // offset the walk has reached is settled — as `Code`, which `out` is
        // already filled with.
        while k < offsets.len() && offsets[k] <= i {
            k += 1;
        }
        if k >= offsets.len() {
            break;
        }
        if let Some(j) = skip_noncode(b, i, dialect) {
            let kind = if is_comment_start(b, i, dialect) {
                Region::Comment
            } else {
                Region::Str
            };
            // The interior of `[i, j)` — the boundaries are code positions,
            // and `offsets[k] > i` is what the skip above established.
            while k < offsets.len() && offsets[k] < j {
                out[k] = kind;
                k += 1;
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

/// Resolve an auto-pair keystroke. `sel_start`/`sel_end` are the current selection
/// (equal → a bare caret); `ch` is the character being typed. Returns `None` to
/// let the editor insert `ch` normally.
///
/// Behaviours: auto-close an opener/quote at a code position; wrap a non-empty
/// selection; type over a closing bracket or quote already at the caret. All
/// respect string/comment boundaries via [`region_at`].
pub fn auto_pair(
    text: &str,
    sel_start: usize,
    sel_end: usize,
    ch: char,
    dialect: SqlDialect,
) -> Option<PairAction> {
    let lo = sel_start.min(sel_end);
    let hi = sel_start.max(sel_end);
    let b = text.as_bytes();
    let cat = classify(ch, dialect)?;

    // Non-empty selection → wrap it with the pair (openers/quotes only). A lone
    // closer over a selection falls through so the editor replaces as usual.
    if lo != hi {
        let inner = text.get(lo..hi)?;
        return match cat {
            Cat::Open(o, c) => Some(PairAction::Insert {
                start: lo,
                end: hi,
                insert: format!("{o}{inner}{c}"),
                sel: (lo + 1, hi + 1),
            }),
            Cat::Quote(q) => Some(PairAction::Insert {
                start: lo,
                end: hi,
                insert: format!("{q}{inner}{q}"),
                sel: (lo + 1, hi + 1),
            }),
            Cat::Close(..) => None,
        };
    }

    let caret = lo;
    let next = b.get(caret).copied();
    match cat {
        Cat::Open(o, c) => {
            // Only auto-close in code, and not when it would run straight into a
            // word (`(name` must stay `(name`, not become `()name`).
            if region_at(text, caret, dialect) != Region::Code {
                return None;
            }
            if next.is_some_and(is_word_byte) {
                return None;
            }
            Some(PairAction::Insert {
                start: caret,
                end: caret,
                insert: format!("{o}{c}"),
                sel: (caret + 1, caret + 1),
            })
        }
        Cat::Close(c) => {
            // Type over an existing closer sitting at the caret (only in code — a
            // `)` inside a string/comment is literal text).
            if region_at(text, caret, dialect) == Region::Code && next == Some(c as u8) {
                Some(PairAction::Skip { caret: caret + 1 })
            } else {
                None
            }
        }
        Cat::Quote(q) => {
            let qb = q as u8;
            match region_at(text, caret, dialect) {
                // Never auto-anything inside a comment.
                Region::Comment => None,
                // Inside a string: typing the same quote at its close types over
                // (closes it); anywhere else it's literal → normal insert.
                Region::Str => {
                    if next == Some(qb) {
                        Some(PairAction::Skip { caret: caret + 1 })
                    } else {
                        None
                    }
                }
                Region::Code => {
                    // Right before an existing quote → let it insert plainly rather
                    // than auto-closing into `''<existing>`.
                    if next == Some(qb) {
                        return None;
                    }
                    // Don't attach a quote pair to an adjacent word (avoids
                    // `x''` / `''x`).
                    let prev = caret.checked_sub(1).and_then(|i| b.get(i).copied());
                    if prev.is_some_and(is_word_byte) || next.is_some_and(is_word_byte) {
                        return None;
                    }
                    Some(PairAction::Insert {
                        start: caret,
                        end: caret,
                        insert: format!("{q}{q}"),
                        sel: (caret + 1, caret + 1),
                    })
                }
            }
        }
    }
}

/// If the caret sits between an empty auto-inserted pair (`(|)`, `'|'`, `` `|` ``,
/// `"|"`), return the byte range `[caret-1, caret+1]` so a Backspace deletes both
/// halves. `None` otherwise (normal Backspace).
///
/// **Asked of the lexer, not just the two bytes.** A quote pair is an empty
/// pair only when its first half opens a string (or quoted name) that is
/// exactly those two bytes; a paren pair only when it is code. Between the
/// halves of an escaped quote — `'it'|'s'` — the two bytes look the same, and
/// Backspace took both, leaving `'its'`; inside a string `'f(|)'` it took a
/// paren pair nobody had typed as one.
pub fn backspace_pair(text: &str, caret: usize, dialect: SqlDialect) -> Option<(usize, usize)> {
    let b = text.as_bytes();
    if caret == 0 || caret >= b.len() {
        return None;
    }
    let (quote, paren) = match (b[caret - 1], b[caret]) {
        (b'(', b')') => (false, true),
        (b'\'', b'\'') | (b'"', b'"') => (true, false),
        (b'`', b'`') => (dialect.backtick_ident(), false),
        _ => (false, false),
    };
    let span = || noncode_span_at(b, caret - 1, dialect);
    let empty_pair =
        (quote && span() == Some((caret - 1, caret + 1))) || (paren && span().is_none());
    empty_pair.then_some((caret - 1, caret + 1))
}

/// The string, comment or quoted name holding byte `pos`, as `[start, end)` —
/// read from the start through [`skip_noncode`], since only the text before a
/// quote says whether it opens a string or closes one.
fn noncode_span_at(b: &[u8], pos: usize, dialect: SqlDialect) -> Option<(usize, usize)> {
    let mut i = 0;
    while i <= pos {
        match skip_noncode(b, i, dialect) {
            Some(j) => {
                let j = j.max(i + 1);
                if pos < j {
                    return Some((i, j));
                }
                i = j;
            }
            None => i += 1,
        }
    }
    None
}

/// How many brackets — `(` and `[`, in code — are open at the start of each
/// line of `text`, one entry per line: the depth a per-line highlighter needs
/// to colour a line's brackets by nesting (rainbow brackets). A stray close
/// opens nothing below zero. Brackets in a string or comment are not counted,
/// and nor is a SQL Server or SQLite `[name]`, which is a quoted name.
pub fn bracket_depth_at_lines(text: &str, dialect: SqlDialect) -> Vec<usize> {
    line_depths(text, dialect).depth
}

/// Each line's bracket depth ([`bracket_depth_at_lines`]), and whether the
/// line starts in code rather than inside a string or comment — which is what
/// lets [`line_depths_after`] take an edit without rescanning the document.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineDepths {
    pub depth: Vec<usize>,
    pub code: Vec<bool>,
}

/// [`LineDepths`] of `text`, in one pass.
pub fn line_depths(text: &str, dialect: SqlDialect) -> LineDepths {
    let mut out = LineDepths {
        depth: vec![0],
        code: vec![true],
    };
    scan_depths(text.as_bytes(), 0, 0, dialect, &mut out, |_, _, _| false);
    out
}

/// [`line_depths`] of `new`, from `prev` — `line_depths(old)` — where an edit
/// turned `old` into `new`: the scan restarts at the last line before the edit
/// that starts in code, at its depth, and stops at the first line start past
/// the edit that is in code at the same depth in both texts; the old answer
/// stands for everything after it. An edit that leaves a bracket open past
/// itself changes every depth below, and rescans to the end.
pub fn line_depths_after(
    old: &str,
    prev: &LineDepths,
    new: &str,
    dialect: SqlDialect,
) -> LineDepths {
    let (o, n) = (old.as_bytes(), new.as_bytes());
    let (pre, suf) = crate::text::common_affixes(o, n);
    if pre == o.len() && pre == n.len() {
        return prev.clone();
    }
    line_depths_after_edit(prev, new, pre, suf, dialect)
}

/// [`line_depths_after`] with the edit already known — the first `pre` and
/// last `suf` bytes of the old text and `new` the same — as the editor knows it
/// from its own edits ([`crate::text::EditLog`]), with no copy of the old text
/// kept to compare. The lines it moved come from the two line counts.
pub fn line_depths_after_edit(
    prev: &LineDepths,
    new: &str,
    pre: usize,
    suf: usize,
    dialect: SqlDialect,
) -> LineDepths {
    let n = new.as_bytes();
    let newlines = crate::text::count_newlines;
    let shift = (newlines(n) + 1) as isize - prev.depth.len() as isize;
    let mut line = newlines(&n[..pre]).min(prev.code.len().saturating_sub(1));
    let mut start = n[..pre]
        .iter()
        .rposition(|&c| c == b'\n')
        .map_or(0, |p| p + 1);
    while line > 0 && !prev.code[line] {
        line -= 1;
        start = n[..start - 1]
            .iter()
            .rposition(|&c| c == b'\n')
            .map_or(0, |p| p + 1);
    }
    let mut out = LineDepths {
        depth: prev.depth[..=line].to_vec(),
        code: prev.code[..=line].to_vec(),
    };
    let common_from = n.len() - suf;
    let depth = prev.depth[line];
    scan_depths(n, start, depth, dialect, &mut out, |out, s, depth| {
        if s - 1 < common_from {
            return false;
        }
        let Ok(old_line) = usize::try_from(out.depth.len() as isize - 1 - shift) else {
            return false;
        };
        if old_line >= prev.code.len() || !prev.code[old_line] || prev.depth[old_line] != depth {
            return false;
        }
        out.depth.extend_from_slice(&prev.depth[old_line + 1..]);
        out.code.extend_from_slice(&prev.code[old_line + 1..]);
        true
    });
    out
}

/// The one depth scan: from `start`, in code at `depth`, pushing each line
/// start after it onto `out`, until the end — or until `stop`, asked at each
/// line start reached in code with its offset and depth, says the rest of
/// `out` is filled.
fn scan_depths(
    b: &[u8],
    start: usize,
    mut depth: usize,
    dialect: SqlDialect,
    out: &mut LineDepths,
    mut stop: impl FnMut(&mut LineDepths, usize, usize) -> bool,
) {
    let mut i = start;
    while i < b.len() {
        if let Some(j) = skip_noncode(b, i, dialect) {
            // A string or comment over several lines: each line it starts is
            // at the depth outside it, and inside it. **Never in code**, even
            // where the span ends right at the line start: no closed span ends
            // in a `\n` (a line comment stops before its own), so that is an
            // unterminated one running to the end of the text — `/* a\n` —
            // and the empty line after it is still inside. Marked code, the
            // incremental scan restarted there outside the comment when the
            // next character was typed, and every depth below drifted.
            let j = j.max(i + 1).min(b.len());
            for _ in (i..j).filter(|&p| b[p] == b'\n') {
                out.depth.push(depth);
                out.code.push(false);
            }
            i = j;
            continue;
        }
        match b[i] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b'\n' => {
                out.depth.push(depth);
                out.code.push(true);
                if stop(out, i + 1, depth) {
                    return;
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// Where Ctrl+Shift+\ puts the caret: on the same side of the matching paren
/// as it was of the one beside it — before `(` goes before its `)`, after `)`
/// goes after its `(` — so a second press comes back. `None` with no code paren
/// beside the caret, or none matching ([`match_paren`]).
pub fn bracket_jump(text: &str, caret: usize, dialect: SqlDialect) -> Option<usize> {
    let (here, partner) = match_paren(text, caret, dialect)?;
    Some(if here == caret { partner } else { partner + 1 })
}

/// If the caret is adjacent to a parenthesis or square bracket that isn't inside
/// a string/comment, return `(here, partner)` — the byte offset of that paren
/// and of its match, which is always of the same kind. Prefers the paren to the
/// *right* of the caret, else the one to the *left*. `None` when there's no
/// adjacent (code) paren or it has no match. On SQL Server and SQLite a
/// `[name]` is a quoted name the lexer skips, so it pairs nothing here.
///
/// **Returns at the pair, not after every pair.** This used to build a `Vec` of
/// every balanced pair in the document and then `find_map` the one touching the
/// caret; on the editor's undebounced per-caret-move effect that was 44.6 ms at
/// 16 MiB for a single arrow key. The walk is the same walk — one stack, the
/// shared lexer skipping strings and comments — and it stops the moment
/// `here`'s partner is known, which for a closing paren is at the caret itself.
///
/// The pair it picks is unchanged: closes are still visited in increasing
/// order, and a position is the open or the close of exactly one pair.
pub fn match_paren(text: &str, caret: usize, dialect: SqlDialect) -> Option<(usize, usize)> {
    let b = text.as_bytes();
    let is_paren = |p: usize| matches!(b.get(p), Some(b'(' | b')' | b'[' | b']'));
    let here = if is_paren(caret) {
        caret
    } else if caret > 0 && is_paren(caret - 1) {
        caret - 1
    } else {
        return None;
    };
    // A paren inside a string/comment is skipped by the lexer, so an
    // adjacent-but-non-code paren simply finds no partner here.
    let mut stack: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if let Some(j) = skip_noncode(b, i, dialect) {
            i = j.max(i + 1);
            continue;
        }
        match b[i] {
            b'(' | b'[' => stack.push(i),
            close @ (b')' | b']') => {
                let opener = if close == b')' { b'(' } else { b'[' };
                match stack.last() {
                    Some(&open) if b[open] == opener => {
                        stack.pop();
                        if open == here {
                            return Some((here, i));
                        }
                        if i == here {
                            return Some((here, open));
                        }
                    }
                    // An unmatched close **at** the caret — none open, or one
                    // of the other kind — has no partner and never will: the
                    // pairs after it belong to somebody else. Elsewhere a
                    // stray close is passed over, so it cannot take the pair
                    // from the close of the right kind.
                    _ if i == here => return None,
                    _ => {}
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Where Ctrl+Shift+\ puts the caret over a block word ([`match_block`]): on
/// the same side of the partner as it was of its own word — its end to the
/// partner's end, anywhere else to the partner's start — so a second press
/// comes back, as [`bracket_jump`] does for a paren.
pub fn block_jump(text: &str, caret: usize, dialect: SqlDialect) -> Option<usize> {
    let ((_, here_hi), (lo, hi)) = match_block(text, caret, dialect)?;
    Some(if caret == here_hi { hi } else { lo })
}

/// The block the caret's word opens or closes: `BEGIN` or `CASE` and its
/// `END`, as `(here, partner)` word ranges, when the caret is on or just after
/// one of those words in code — and, where an `END` names what it closes
/// ([`names_its_block_ends`]), a compound statement's `IF`, `LOOP`, `WHILE`,
/// `REPEAT` or `FOR` at the start of a statement, with the `END` of its
/// `END IF` and the rest. The bracket pair's question asked of words — see
/// [`match_paren`].
///
/// What is not a block is passed over: a transaction's `BEGIN` (`BEGIN TRAN`,
/// `TRANSACTION`, `WORK`, or a `BEGIN;`); the `IF()` function, told from an
/// `IF (…) THEN` by the `THEN`; and elsewhere an `END IF`/`LOOP`/`WHILE`/
/// `REPEAT`/`FOR`, which closes a block this does not track. `END CASE` and
/// T-SQL's `END TRY`/`END CATCH` are `END`s. A word after `.` is a name.
pub fn match_block(
    text: &str,
    caret: usize,
    dialect: SqlDialect,
) -> Option<((usize, usize), (usize, usize))> {
    let b = text.as_bytes();
    // **Asked of the caret's word before anything is scanned**: this runs on
    // every caret move, undebounced, as `match_paren` does — and the caret is
    // almost never on one of these three words.
    let caret = caret.min(b.len());
    let mut lo = caret;
    while lo > 0 && crate::sql::is_word_byte(b[lo - 1]) {
        lo -= 1;
    }
    let mut hi = caret;
    while hi < b.len() && crate::sql::is_word_byte(b[hi]) {
        hi += 1;
    }
    let block_word = [
        "BEGIN", "CASE", "END", "IF", "LOOP", "WHILE", "REPEAT", "FOR",
    ]
    .iter()
    .any(|k| text[lo..hi].eq_ignore_ascii_case(k));
    if !block_word {
        return None;
    }
    let words = block_words(text, dialect);
    let here = words.iter().position(|w| w.lo <= caret && caret <= w.hi)?;
    if words[here].role == BlockRole::Other {
        return None;
    }
    let range = |w: usize| (words[w].lo, words[w].hi);
    let mut stack: Vec<usize> = Vec::new();
    for (w, word) in words.iter().enumerate() {
        match word.role {
            BlockRole::Open => stack.push(w),
            BlockRole::Close => match stack.pop() {
                Some(open) if open == here => return Some((range(here), range(w))),
                Some(open) if w == here => return Some((range(here), range(open))),
                None if w == here => return None,
                _ => {}
            },
            BlockRole::Other => {}
        }
    }
    None
}

/// The innermost block still open at `at` — a `BEGIN`, a `CASE`, or where
/// [`names_its_block_ends`] a compound statement's opener — its word range,
/// and whether it is a `CASE`, by [`match_block`]'s rules for what opens and
/// closes a block. `None` outside any.
pub fn enclosing_block(text: &str, at: usize, dialect: SqlDialect) -> Option<(usize, usize, bool)> {
    let words = block_words(text, dialect);
    let mut stack: Vec<usize> = Vec::new();
    for (w, word) in words.iter().enumerate().take_while(|(_, w)| w.hi <= at) {
        match word.role {
            BlockRole::Open => stack.push(w),
            BlockRole::Close => {
                stack.pop();
            }
            BlockRole::Other => {}
        }
    }
    let w = &words[*stack.last()?];
    Some((w.lo, w.hi, w.up == "CASE"))
}

/// What a word does to the block structure [`match_block`] reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockRole {
    Open,
    Close,
    Other,
}

/// One word of code, as [`block_words`] reads it.
struct BlockWord {
    lo: usize,
    hi: usize,
    up: String,
    role: BlockRole,
}

/// Whether `dialect`'s compound statements — `IF`, `LOOP`, `WHILE`, `REPEAT`,
/// `FOR` — close with an `END` that names them, in code the editor scans.
/// MySQL's routine bodies are plain code. PostgreSQL's `END IF` lives in a
/// dollar-quoted body, which is a string here; T-SQL's `IF` and `WHILE` take
/// one statement or a `BEGIN … END` and have no `END` of their own; SQLite
/// has no compound statements.
pub(crate) fn names_its_block_ends(dialect: SqlDialect) -> bool {
    match dialect {
        SqlDialect::MySql => true,
        SqlDialect::Postgres | SqlDialect::Sqlite | SqlDialect::MsSql => false,
    }
}

/// Every word of `text` in code, in order, with what it does to the block
/// structure — the rules [`match_block`]'s doc sets out.
fn block_words(text: &str, dialect: SqlDialect) -> Vec<BlockWord> {
    let b = text.as_bytes();
    // Every word in code, in order: its range, upper-cased, whether a `.`
    // makes it a member (`t.end`), and whether it starts a statement.
    let mut words: Vec<(usize, usize, String, bool, bool)> = Vec::new();
    // What the last token of code was: nothing yet, a word, or this byte.
    let mut last: Option<Option<u8>> = None;
    // The terminator a MySQL `DELIMITER` set, while it is not `;`: it ends a
    // statement as `;` does, and a name — `END$$` is an `END`.
    let mut delim: Option<String> = None;
    let mut i = 0;
    while i < b.len() {
        if let Some(j) = skip_noncode(b, i, dialect) {
            if !matches!(
                crate::sql::noncode_kind(b, i, dialect),
                Some(crate::sql::NonCode::Comment)
            ) {
                last = Some(Some(b'\''));
            }
            i = j.max(i + 1);
            continue;
        }
        if matches!(b[i], b'd' | b'D')
            && matches!(last, None | Some(Some(b';')))
            && let Some((end, token)) = crate::sql::delimiter_directive(text, i, dialect)
        {
            delim = (token != ";").then_some(token);
            last = Some(Some(b';'));
            i = end;
            continue;
        }
        if let Some(d) = &delim
            && b[i..].starts_with(d.as_bytes())
        {
            last = Some(Some(b';'));
            i += d.len();
            continue;
        }
        // **A name ends where its engine ends it** (`sql::continues_name`),
        // and T-SQL's `@end`, `@@x` and `#begin` are names with their sigil
        // (`sql::t_sql_name_prefix`): read from the byte after it, `SET @end
        // = 1` closed the `BEGIN` above it, and `a$end` did on MySQL.
        let prefix = crate::sql::t_sql_name_prefix(b, i, dialect);
        if prefix > 0 || crate::sql::is_word_start(b[i]) {
            let mut j = i + prefix.max(1);
            while j < b.len()
                && crate::sql::continues_name(b[j], dialect)
                && !delim
                    .as_ref()
                    .is_some_and(|d| b[j..].starts_with(d.as_bytes()))
            {
                j += 1;
            }
            let member = i > 0 && b[i - 1] == b'.';
            // At the start of a statement: the text's first, after a `;` or
            // a label's `:`, or after a word a statement list follows.
            let starts = match last {
                None | Some(Some(b';' | b':')) => true,
                Some(None) => words.last().is_some_and(|w| {
                    matches!(
                        w.2.as_str(),
                        "BEGIN" | "THEN" | "ELSE" | "DO" | "LOOP" | "REPEAT"
                    )
                }),
                Some(Some(_)) => false,
            };
            words.push((i, j, text[i..j].to_ascii_uppercase(), member, starts));
            last = Some(None);
            i = j;
            continue;
        }
        if !b[i].is_ascii_whitespace() {
            last = Some(Some(b[i]));
        }
        i += 1;
    }
    // The first code byte after `at` that is not blank.
    let next_code = |at: usize| {
        let mut k = at;
        while k < b.len() {
            if b[k].is_ascii_whitespace() {
                k += 1;
            } else if let Some(j) = skip_noncode(b, k, dialect).filter(|_| {
                matches!(
                    crate::sql::noncode_kind(b, k, dialect),
                    Some(crate::sql::NonCode::Comment)
                )
            }) {
                k = j;
            } else {
                return Some(k);
            }
        }
        None
    };
    let word_after = |w: usize| words.get(w + 1).map(|n| n.2.as_str());
    // Past the `)` that closes the `(` at `at`, if one does.
    let past_parens = |at: usize| {
        let mut depth = 0usize;
        let mut k = at;
        while k < b.len() {
            if let Some(j) = skip_noncode(b, k, dialect) {
                k = j.max(k + 1);
                continue;
            }
            match b[k] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(k + 1);
                    }
                }
                _ => {}
            }
            k += 1;
        }
        None
    };
    let named_ends = names_its_block_ends(dialect);
    let opens = |w: usize| {
        let (_, hi, up, member, starts) = &words[w];
        if *member {
            return false;
        }
        match up.as_str() {
            // A compound statement, where its END names it. `IF (` is the
            // statement when THEN follows the parens, else `IF()`.
            "IF" if named_ends && *starts => match next_code(*hi) {
                Some(k) if b[k] == b'(' => past_parens(k).is_some_and(|after| {
                    words
                        .iter()
                        .find(|n| n.0 >= after)
                        .is_some_and(|n| n.2 == "THEN")
                }),
                _ => true,
            },
            "LOOP" | "WHILE" | "REPEAT" | "FOR" => {
                named_ends && *starts && next_code(*hi).is_none_or(|k| b[k] != b'(')
            }
            // `END CASE` closes; it does not open another.
            "CASE" => w == 0 || words[w - 1].2 != "END" || words[w - 1].3,
            "BEGIN" => {
                let transaction = matches!(
                    word_after(w),
                    Some("TRAN" | "TRANSACTION" | "WORK" | "DISTRIBUTED")
                );
                let bare = next_code(*hi).is_none_or(|k| b[k] == b';');
                !transaction && !bare
            }
            _ => false,
        }
    };
    // An `END IF` and its kin close the statement it names where that is
    // tracked, and elsewhere a block this does not track.
    let closes = |w: usize| {
        words[w].2 == "END"
            && !words[w].3
            && (named_ends
                || !matches!(
                    word_after(w),
                    Some("IF" | "LOOP" | "WHILE" | "REPEAT" | "FOR")
                ))
    };
    (0..words.len())
        .map(|w| BlockWord {
            lo: words[w].0,
            hi: words[w].1,
            up: words[w].2.clone(),
            role: if opens(w) {
                BlockRole::Open
            } else if closes(w) {
                BlockRole::Close
            } else {
                BlockRole::Other
            },
        })
        .collect()
}

/// All byte ranges of the identifier under the caret, for "highlight all
/// occurrences of the identifier under the caret." Returns empty unless the caret
/// sits on an identifier in *code* (not a string/comment), the identifier is
/// neither a bare number nor a SQL keyword, and it occurs at least twice.
///
/// A word is a name as its engine spells it — through `$`, and on SQL Server
/// with its `@`/`#` — so `@id` and `id` are two names. Matching is whole-word
/// and ASCII-case-insensitive (SQL identifiers fold for
/// ASCII); occurrences inside strings/comments are skipped via the shared lexer.
/// The occurrence under the caret is included in the result.
pub fn identifier_occurrences(
    text: &str,
    caret: usize,
    dialect: SqlDialect,
) -> Vec<(usize, usize)> {
    let b = text.as_bytes();
    // **A name is its whole spelling on its engine**: it runs on where
    // `sql::continues_name` says (`a$b` is one name), and on SQL Server a
    // leading `@`/`@@`/`#`/`##` is part of it (`sql::t_sql_name_prefix`) —
    // `@id` is a variable, not the column `id`. The scan below reads names
    // the same way, so the two cannot disagree about where one starts.
    let in_name = |c: u8| crate::sql::continues_name(c, dialect);
    let name_at = |i: usize| crate::sql::t_sql_name_prefix(b, i, dialect) > 0;
    // The caret is "on" a word if the byte at it or just before it is a name
    // byte, or it stands on a name's sigil.
    let on_word = b.get(caret).is_some_and(|&c| is_word_byte(c))
        || name_at(caret)
        || (caret > 0 && b.get(caret - 1).is_some_and(|&c| in_name(c)));
    if !on_word {
        return Vec::new();
    }
    // Expand to the whole word straddling the caret.
    let mut ws = caret;
    while ws > 0 && in_name(b[ws - 1]) {
        ws -= 1;
    }
    let mut we = caret + crate::sql::t_sql_name_prefix(b, caret, dialect);
    while we < b.len() && in_name(b[we]) {
        we += 1;
    }
    // The run takes in a T-SQL sigil, `continues_name` holding `@` and `#`
    // there; a byte that can only continue a name (`$`) starts none.
    while ws < we && !is_word_byte(b[ws]) && !name_at(ws) {
        ws += 1;
    }
    if ws >= we {
        return Vec::new();
    }
    let target = &text[ws..we];
    // Skip bare numbers and keywords — highlighting every `select`/`123` is noise.
    if target.bytes().all(|c| c.is_ascii_digit()) || crate::intel::is_sql_keyword(target) {
        return Vec::new();
    }
    // Whole-word, case-insensitive matches in code regions only.
    //
    // **The caret's own region is answered by this pass**, not by a
    // `region_at` before it. The scan skips strings and comments wholesale, so
    // the word straddling the caret is in code **iff** this pass finds a word
    // starting exactly at `ws` — and it can only be `target`, which is that
    // word. One walk of the document instead of two: the `region_at` call this
    // replaces was the other 37.9 ms of the 78.5 ms an arrow key cost at
    // 16 MiB, on an effect with no debounce.
    //
    // The equivalence is exact rather than close. A span's opening byte is
    // never a word byte (`'`, `"`, `` ` ``, `-`, `/`, `#`), so `ws` can never
    // be a span start — the one position where `region_at` says `Code` inside
    // a span it found. And `ws == j`, just past a span, is where the scan
    // resumes.
    let mut hits = Vec::new();
    let mut on_caret_word = false;
    let mut i = 0;
    while i < b.len() {
        if let Some(j) = skip_noncode(b, i, dialect) {
            i = j.max(i + 1);
            continue;
        }
        if is_word_byte(b[i]) || name_at(i) {
            let start = i;
            i += 1;
            while i < b.len() && in_name(b[i]) {
                i += 1;
            }
            if start == ws {
                on_caret_word = true;
            }
            if text[start..i].eq_ignore_ascii_case(target) {
                hits.push((start, i));
            }
        } else {
            i += 1;
        }
    }
    if !on_caret_word {
        return Vec::new();
    }
    // A lone occurrence (only the one under the caret) isn't worth a box.
    if hits.len() >= 2 { hits } else { Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SqlDialect::{MsSql, MySql, Postgres, Sqlite};

    /// Every `(old, new)` a corpus of edits makes: each base document edited at
    /// every byte by inserting each snippet and by deleting one or three bytes
    /// — edits that open and close comments, strings, dollar quotes and
    /// brackets, and add and remove lines.
    pub(crate) fn edit_corpus() -> Vec<(String, String)> {
        let bases = [
            "select 1;\n/* a\nb */ select (2);\nselect 'x\ny' from t;\n-- c\nselect [3];\n",
            "select $$a\nb$$, $q$c\n$q$;\n\nselect (1,\n 2)\n;",
            "a\n\n(b\n)\n'c\n'\n/*\n*/\n",
            "f((\n(\n))\n)\nx",
        ];
        let snippets = [
            "/*", "*/", "'", "\n", "x", "(", ")", "$$", "--", "\n\n", "[", "]",
        ];
        let mut out = Vec::new();
        for base in bases {
            for at in 0..=base.len() {
                for s in snippets {
                    out.push((
                        base.to_string(),
                        format!("{}{s}{}", &base[..at], &base[at..]),
                    ));
                }
                for n in [1, 3] {
                    if at + n <= base.len() {
                        out.push((
                            base.to_string(),
                            format!("{}{}", &base[..at], &base[at + n..]),
                        ));
                    }
                }
            }
        }
        out
    }

    /// **The incremental depths answer what the whole pass does**, for every
    /// edit of the corpus, on every engine.
    #[test]
    fn line_depths_after_an_edit_match_a_full_pass() {
        for d in [MySql, Postgres, MsSql, Sqlite] {
            for (old, new) in edit_corpus() {
                let prev = line_depths(&old, d);
                assert_eq!(
                    line_depths_after(&old, &prev, &new, d),
                    line_depths(&new, d),
                    "{d:?}: {old:?} -> {new:?}"
                );
                // As the editor's edit log hands it over: a front and back
                // smaller than the most the texts share, as several merged
                // edits leave them.
                let (pre, suf) = crate::text::common_affixes(old.as_bytes(), new.as_bytes());
                assert_eq!(
                    line_depths_after_edit(&prev, &new, pre / 2, suf / 2, d),
                    line_depths(&new, d),
                    "{d:?} merged: {old:?} -> {new:?}"
                );
            }
        }
    }

    /// **Typing on past an open comment at the end of the text** — the way a
    /// block comment gets written, `/*` not being auto-paired — keeps the
    /// incremental depths equal to a full pass: the empty line after the
    /// comment's last `\n` is still inside it, not code.
    #[test]
    fn line_depths_after_typing_into_an_open_comment_match_a_full_pass() {
        let full = "/* a\n(\n*/ (b)\nx";
        for d in [MySql, Postgres, MsSql, Sqlite] {
            let mut old = String::new();
            let mut depths = line_depths(&old, d);
            for ch in full.chars() {
                let mut new = old.clone();
                new.push(ch);
                depths = line_depths_after(&old, &depths, &new, d);
                assert_eq!(depths, line_depths(&new, d), "{d:?}: {new:?}");
                old = new;
            }
        }
    }

    /// The depth at each line's start counts the code brackets above it,
    /// through strings and comments that span lines, never below zero; and a
    /// `[name]` is a name where it is one.
    #[test]
    fn bracket_depth_is_counted_at_each_line_start() {
        assert_eq!(bracket_depth_at_lines("", MySql), [0]);
        assert_eq!(
            bracket_depth_at_lines("SELECT f(a,\n  g(b,\n    c))\nFROM t", MySql),
            [0, 1, 2, 0]
        );
        assert_eq!(
            bracket_depth_at_lines("SELECT ('(\n', /* ( \n */ 1\n)", MySql),
            [0, 1, 1, 1]
        );
        assert_eq!(bracket_depth_at_lines(")\n(\nx", MySql), [0, 0, 1]);
        assert_eq!(bracket_depth_at_lines("SELECT a[1\n]", Postgres), [0, 1]);
        assert_eq!(bracket_depth_at_lines("SELECT [a\nb]", MsSql), [0, 0]);
        assert_eq!(bracket_depth_at_lines("SELECT [a\nb]", Sqlite), [0, 0]);
    }

    // --- region_at ---------------------------------------------------------

    #[test]
    fn region_classifies_code_string_comment() {
        let sql = "select 'ab' -- c";
        // '0' in "select" → code
        assert_eq!(region_at(sql, 0, MySql), Region::Code);
        // inside the 'ab' literal (offset 8 is the 'a')
        assert_eq!(region_at(sql, 9, MySql), Region::Str);
        // the opening quote position itself is a code boundary
        assert_eq!(region_at(sql, 7, MySql), Region::Code);
        // inside the -- comment
        assert_eq!(region_at(sql, 15, MySql), Region::Comment);
    }

    #[test]
    fn region_hash_comment_is_dialect_aware() {
        let sql = "a # b";
        assert_eq!(region_at(sql, 4, MySql), Region::Comment);
        // Postgres: `#` is an operator, not a comment → code
        assert_eq!(region_at(sql, 4, Postgres), Region::Code);
        // **SQLite, the engine that motivated the fix this test guards.**
        // `is_comment_start` was a private byte test saying `#` opened a comment
        // on everything but Postgres — a claim that stopped being true the
        // moment SQLite existed, which is why it now delegates to
        // `sql::comment_open`. That went in with no assertion on the engine it
        // was about, and this file had none anywhere.
        assert_eq!(region_at(sql, 4, Sqlite), Region::Code);
    }

    /// SQLite's `[…]` identifiers, the module's other documented divergence.
    ///
    /// `region_at` has to call the span a `Str` there — it is a quoted name,
    /// and a caret inside one is not in code, so nothing may offer to complete
    /// or auto-close into it. On the other two engines `[` is an ordinary
    /// character, so the same text is code throughout.
    #[test]
    fn a_bracket_identifier_is_a_quoted_span_on_sqlite_only() {
        let sql = "[a b]";
        assert_eq!(region_at(sql, 2, Sqlite), Region::Str);
        for d in [MySql, Postgres] {
            assert_eq!(region_at(sql, 2, d), Region::Code, "{d:?}");
        }
        // The caret one past the closing `]` is out of the span again.
        assert_eq!(region_at(sql, 5, Sqlite), Region::Code);
    }

    /// Backtick auto-close on SQLite, which takes MySQL's identifier quote for
    /// compatibility — the capability `SqlDialect::backtick_ident` states and
    /// both `classify` and `backspace_pair` ask.
    #[test]
    fn sqlite_takes_the_backtick_pair_and_postgres_does_not() {
        assert_eq!(backspace_pair("``", 1, Sqlite), Some((0, 2)));
        assert_eq!(backspace_pair("``", 1, MySql), Some((0, 2)));
        assert_eq!(backspace_pair("``", 1, Postgres), None);
        // The pairs that are every engine's.
        for d in [MySql, Postgres, Sqlite] {
            assert_eq!(backspace_pair("()", 1, d), Some((0, 2)), "{d:?}");
            assert_eq!(backspace_pair("''", 1, d), Some((0, 2)), "{d:?}");
            assert_eq!(backspace_pair("\"\"", 1, d), Some((0, 2)), "{d:?}");
        }
    }

    #[test]
    fn region_inside_closing_quote_is_string() {
        // caret just before the closing quote of 'abc'
        let sql = "'abc'";
        assert_eq!(region_at(sql, 4, MySql), Region::Str);
        // one past the closing quote → code
        assert_eq!(region_at(sql, 5, MySql), Region::Code);
    }

    // --- auto_pair: auto-close --------------------------------------------

    #[test]
    fn autoclose_paren_at_eof() {
        let a = auto_pair("count", 5, 5, '(', MySql);
        assert_eq!(
            a,
            Some(PairAction::Insert {
                start: 5,
                end: 5,
                insert: "()".into(),
                sel: (6, 6),
            })
        );
    }

    #[test]
    fn autoclose_paren_before_whitespace_and_closer() {
        assert!(matches!(
            auto_pair("a  b", 1, 1, '(', MySql),
            Some(PairAction::Insert { .. })
        ));
        // before an existing `)` (not a word byte) → still auto-closes
        assert!(matches!(
            auto_pair("f)", 1, 1, '(', MySql),
            Some(PairAction::Insert { .. })
        ));
    }

    #[test]
    fn no_autoclose_paren_before_word() {
        // `(name` must not become `()name`
        assert_eq!(auto_pair("name", 0, 0, '(', MySql), None);
    }

    #[test]
    fn no_autoclose_inside_string_or_comment() {
        // caret inside 'a|b'
        assert_eq!(auto_pair("'ab'", 2, 2, '(', MySql), None);
        // caret inside a -- comment
        assert_eq!(auto_pair("-- xy", 4, 4, '(', MySql), None);
    }

    #[test]
    fn autoclose_quote_at_code_boundary() {
        let a = auto_pair("where x = ", 10, 10, '\'', MySql);
        assert_eq!(
            a,
            Some(PairAction::Insert {
                start: 10,
                end: 10,
                insert: "''".into(),
                sel: (11, 11),
            })
        );
    }

    #[test]
    fn no_autoclose_quote_next_to_word() {
        // prev char is a word byte
        assert_eq!(auto_pair("abc", 3, 3, '\'', MySql), None);
        // next char is a word byte
        assert_eq!(auto_pair(" abc", 1, 1, '\'', MySql), None);
    }

    #[test]
    fn backtick_autoclose_is_dialect_aware() {
        assert!(matches!(
            auto_pair("select ", 7, 7, '`', MySql),
            Some(PairAction::Insert { .. })
        ));
        // Postgres has no backtick syntax → not a pair char
        assert_eq!(auto_pair("select ", 7, 7, '`', Postgres), None);
    }

    // --- auto_pair: type-over ---------------------------------------------

    #[test]
    fn typeover_closing_paren() {
        // "()" with caret between → typing ')' skips over
        assert_eq!(
            auto_pair("()", 1, 1, ')', MySql),
            Some(PairAction::Skip { caret: 2 })
        );
    }

    #[test]
    fn no_typeover_paren_when_next_differs() {
        assert_eq!(auto_pair("(x", 1, 1, ')', MySql), None);
    }

    #[test]
    fn typeover_closing_quote_closes_string() {
        // "''" caret between, region is Str, next is the closing quote
        assert_eq!(
            auto_pair("''", 1, 1, '\'', MySql),
            Some(PairAction::Skip { caret: 2 })
        );
        // 'abc|' → typing ' types over the close
        assert_eq!(
            auto_pair("'abc'", 4, 4, '\'', MySql),
            Some(PairAction::Skip { caret: 5 })
        );
    }

    #[test]
    fn no_typeover_closer_inside_string() {
        // a ')' literal inside a string: typing ')' should insert normally
        assert_eq!(auto_pair("'a)'", 2, 2, ')', MySql), None);
    }

    // --- auto_pair: wrap selection ----------------------------------------

    #[test]
    fn wrap_selection_with_paren() {
        let a = auto_pair("select x", 7, 8, '(', MySql);
        assert_eq!(
            a,
            Some(PairAction::Insert {
                start: 7,
                end: 8,
                insert: "(x)".into(),
                sel: (8, 9),
            })
        );
    }

    #[test]
    fn wrap_selection_with_quote() {
        let a = auto_pair("ab", 0, 2, '\'', MySql);
        assert_eq!(
            a,
            Some(PairAction::Insert {
                start: 0,
                end: 2,
                insert: "'ab'".into(),
                sel: (1, 3),
            })
        );
    }

    #[test]
    fn closer_over_selection_is_no_op() {
        assert_eq!(auto_pair("ab", 0, 2, ')', MySql), None);
    }

    // --- backspace_pair ----------------------------------------------------

    #[test]
    fn backspace_deletes_empty_pairs() {
        assert_eq!(backspace_pair("()", 1, MySql), Some((0, 2)));
        assert_eq!(backspace_pair("''", 1, MySql), Some((0, 2)));
        assert_eq!(backspace_pair("``", 1, MySql), Some((0, 2)));
        assert_eq!(backspace_pair("\"\"", 1, MySql), Some((0, 2)));
    }

    #[test]
    fn backspace_ignores_non_pairs_and_edges() {
        assert_eq!(backspace_pair("ab", 1, MySql), None);
        assert_eq!(backspace_pair("()", 0, MySql), None); // caret at start
        assert_eq!(backspace_pair("()", 2, MySql), None); // caret at end
        assert_eq!(backspace_pair("(x)", 1, MySql), None); // not empty
        // backtick pair not recognised under Postgres
        assert_eq!(backspace_pair("``", 1, Postgres), None);
    }

    /// Between an escaped quote's halves, or at a paren pair inside a string,
    /// Backspace deletes one character; an empty pair after other code is
    /// still taken whole.
    #[test]
    fn backspace_takes_only_an_empty_pair_the_lexer_agrees_is_one() {
        for d in [MySql, Postgres, Sqlite] {
            assert_eq!(backspace_pair("SELECT 'it''s'", 11, d), None, "{d:?}");
            assert_eq!(backspace_pair("SELECT 'f()'", 10, d), None, "{d:?}");
            assert_eq!(
                backspace_pair("WHERE a = '' AND b = ''", 22, d),
                Some((21, 23)),
                "{d:?}"
            );
            assert_eq!(backspace_pair("SELECT f()", 9, d), Some((8, 10)), "{d:?}");
        }
        assert_eq!(backspace_pair("SELECT \"a\"\"b\"", 10, Postgres), None);
    }

    // --- bracket_jump ------------------------------------------------------

    /// The caret lands on the same side of the partner as it was of the paren
    /// beside it, so a second press returns; no code paren, no jump.
    #[test]
    fn a_bracket_jump_lands_beside_the_partner_and_comes_back() {
        let sql = "SELECT f(a, (b)) FROM t";
        // Before the outer `(` (8) → before its `)` (15) → back.
        assert_eq!(bracket_jump(sql, 8, MySql), Some(15));
        assert_eq!(bracket_jump(sql, 15, MySql), Some(8));
        // After the outer `)` (16) → after its `(` (9) → back after the `)`.
        assert_eq!(bracket_jump(sql, 16, MySql), Some(9));
        assert_eq!(bracket_jump(sql, 9, MySql), Some(16));
        assert_eq!(bracket_jump(sql, 3, MySql), None);
        assert_eq!(bracket_jump("SELECT '(' x", 7, MySql), None);
    }

    // --- match_paren -------------------------------------------------------

    /// The text of a range pair, for readable assertions.
    fn blocks(sql: &str, caret: usize, d: SqlDialect) -> Option<(&str, &str, usize, usize)> {
        match_block(sql, caret, d).map(|((a, b), (c, e))| (&sql[a..b], &sql[c..e], a, c))
    }

    /// `BEGIN` and `CASE` pair with their `END`, nested blocks with their own,
    /// from the caret on or beside either word.
    #[test]
    fn a_block_word_matches_its_end() {
        use SqlDialect::{MsSql, MySql};
        let sql = "BEGIN SELECT CASE WHEN a THEN 1 END; END";
        let outer_end = sql.rfind("END").unwrap();
        assert_eq!(blocks(sql, 0, MsSql), Some(("BEGIN", "END", 0, outer_end)));
        assert_eq!(blocks(sql, 3, MsSql), Some(("BEGIN", "END", 0, outer_end)));
        assert_eq!(blocks(sql, 5, MsSql), Some(("BEGIN", "END", 0, outer_end)));
        assert_eq!(
            blocks(sql, sql.len(), MsSql),
            Some(("END", "BEGIN", outer_end, 0))
        );
        let case = sql.find("CASE").unwrap();
        let inner = sql.find("END;").unwrap();
        assert_eq!(blocks(sql, case, MsSql), Some(("CASE", "END", case, inner)));
        assert_eq!(
            blocks(sql, inner + 1, MsSql),
            Some(("END", "CASE", inner, case))
        );
        // Lower case, and `END CASE` / `BEGIN TRY … END TRY` too.
        let try_ = "begin try select 1; end try";
        assert_eq!(blocks(try_, 0, MsSql).map(|b| b.1), Some("end"));
        let routine = "CASE x WHEN 1 THEN SET y = 1; END CASE";
        assert_eq!(blocks(routine, 0, MySql).map(|b| b.3), routine.find("END"));
    }

    /// Ctrl+Shift+\ over a block word lands on its partner on the same side —
    /// a word's start to its partner's start, its end to its partner's end —
    /// so a second press comes back.
    #[test]
    fn a_block_jump_lands_on_the_same_side_and_comes_back() {
        let d = SqlDialect::MsSql;
        let sql = "BEGIN SELECT 1; END";
        let end = sql.rfind("END").unwrap();
        assert_eq!(block_jump(sql, 0, d), Some(end));
        assert_eq!(block_jump(sql, end, d), Some(0));
        assert_eq!(block_jump(sql, 5, d), Some(sql.len()));
        assert_eq!(block_jump(sql, sql.len(), d), Some(5));
        assert_eq!(block_jump("SELECT 1", 0, d), None);
    }

    /// No block: a transaction's `BEGIN`, an `END IF`'s `END`, a column named
    /// `end`, the word in a string or comment, and a caret on nothing.
    #[test]
    fn a_word_that_opens_or_closes_no_block_matches_nothing() {
        use SqlDialect::{MsSql, MySql};
        assert_eq!(blocks("BEGIN TRAN; COMMIT", 0, MsSql), None);
        assert_eq!(blocks("BEGIN; SELECT 1; COMMIT;", 0, MySql), None);
        // `END IF` closes its own `IF`, so `BEGIN`'s END is the last.
        let sql = "BEGIN IF x THEN SET y = 1; END IF; END";
        assert_eq!(blocks(sql, 0, MySql).map(|b| b.3), sql.rfind("END"));
        assert_eq!(blocks("SELECT t.end FROM t", 10, MySql), None);
        assert_eq!(blocks("SELECT 'begin' x", 9, MySql), None);
        assert_eq!(blocks("-- begin\nSELECT 1", 4, MySql), None);
        assert_eq!(blocks("SELECT 1", 3, MySql), None);
        assert_eq!(blocks("BEGIN SELECT 1", 0, MsSql), None, "unclosed");
    }

    /// A name is read where it ends on its engine: T-SQL's `@end`, `@@x`
    /// and `#begin` and MySQL's `a$end` are names, not the keyword after the
    /// sigil or the `$` — while a `DELIMITER`'s `$$` still ends an `END`.
    #[test]
    fn a_prefixed_or_dollar_name_is_no_block_word() {
        use SqlDialect::{MsSql, MySql};
        let sql = "BEGIN\n  SET @end = 1;\nEND";
        let end = sql.rfind("END").unwrap();
        assert_eq!(blocks(sql, 0, MsSql), Some(("BEGIN", "END", 0, end)));
        assert_eq!(blocks(sql, sql.find("end").unwrap() + 1, MsSql), None);
        let sql = "BEGIN\n  DECLARE @begin date;\n  SELECT #end.x FROM #end;\nEND";
        let end = sql.rfind("END").unwrap();
        assert_eq!(blocks(sql, 0, MsSql), Some(("BEGIN", "END", 0, end)));
        // Inside a `CASE`, `@end` closes nothing: the `ELSE` line is the CASE's.
        let sql = "SELECT CASE WHEN @end > 0 THEN 1\nELSE";
        let case = sql.find("CASE").unwrap();
        assert_eq!(
            enclosing_block(sql, sql.len(), MsSql),
            Some((case, case + 4, true))
        );
        // MySQL: `a$end` is one name.
        let sql = "BEGIN SELECT a$end FROM t; END";
        let end = sql.rfind("END").unwrap();
        assert_eq!(blocks(sql, 0, MySql), Some(("BEGIN", "END", 0, end)));
        // … and `END$$` after `DELIMITER $$` is still an `END`.
        let sql = "DELIMITER $$\nCREATE PROCEDURE p() BEGIN SELECT 1; END$$\nDELIMITER ;";
        let begin = sql.find("BEGIN").unwrap();
        let end = sql.find("END").unwrap();
        assert_eq!(
            blocks(sql, begin, MySql),
            Some(("BEGIN", "END", begin, end))
        );
    }

    /// MySQL's compound statements pair with their named `END`: `IF … END
    /// IF`, `[label:] LOOP … END LOOP`, `WHILE … END WHILE`, `REPEAT … END
    /// REPEAT` and MariaDB's `FOR … END FOR`, nested in each other and in a
    /// `BEGIN`. The partner is the `END`.
    #[test]
    fn a_compound_statement_matches_its_named_end() {
        use SqlDialect::MySql;
        let sql = "BEGIN IF x THEN SET y = 1; END IF; END";
        let end_if = sql.find("END IF").unwrap();
        assert_eq!(blocks(sql, 6, MySql), Some(("IF", "END", 6, end_if)));
        assert_eq!(blocks(sql, end_if, MySql), Some(("END", "IF", end_if, 6)));
        for (sql, word) in [
            ("l: LOOP SET i = i + 1; END LOOP l", "LOOP"),
            ("WHILE i < 3 DO SET i = i + 1; END WHILE", "WHILE"),
            ("REPEAT SET i = i + 1; UNTIL i > 3 END REPEAT", "REPEAT"),
            ("FOR i IN 1..3 DO SELECT i; END FOR", "FOR"),
            ("IF (a > b) THEN SELECT 1; END IF", "IF"),
            ("WHILE a DO IF b THEN SELECT 1; END IF; END WHILE", "WHILE"),
        ] {
            let at = sql.find(word).unwrap();
            assert_eq!(
                blocks(sql, at, MySql).map(|b| b.3),
                sql.rfind("END"),
                "{sql}"
            );
        }
    }

    /// No compound statement: the `IF()` and `REPEAT()` functions, `IF [NOT]
    /// EXISTS`, `FOR UPDATE`, a trigger's `FOR EACH ROW` — and on SQL Server an
    /// `IF`, which has no `END` of its own.
    #[test]
    fn a_word_that_is_no_compound_statement_opens_nothing() {
        use SqlDialect::{MsSql, MySql};
        let sql = "BEGIN SELECT IF(a, 1, 2), REPEAT('x', 3); DROP TABLE IF EXISTS t; \
                   CREATE TABLE IF NOT EXISTS u (a INT); SELECT * FROM t FOR UPDATE; END";
        assert_eq!(blocks(sql, 0, MySql).map(|b| b.3), sql.rfind("END"));
        let case = "SELECT CASE WHEN a THEN IF(b, 1, 2) END";
        assert_eq!(blocks(case, 7, MySql).map(|b| b.3), case.rfind("END"));
        let trig = "CREATE TRIGGER tr BEFORE INSERT ON t FOR EACH ROW BEGIN SET NEW.a = 1; END";
        let begin = trig.find("BEGIN").unwrap();
        assert_eq!(blocks(trig, begin, MySql).map(|b| b.3), trig.rfind("END"));
        let tsql = "IF @a = 1 BEGIN SELECT 1; END";
        assert_eq!(blocks(tsql, 0, MsSql), None);
        let begin = tsql.find("BEGIN").unwrap();
        assert_eq!(blocks(tsql, begin, MsSql).map(|b| b.3), tsql.rfind("END"));
    }

    /// The block an `END` typed inside an `IF` closes is the `IF`.
    #[test]
    fn an_open_compound_statement_encloses_what_follows() {
        let sql = "BEGIN\n  IF x THEN\n    SET y = 1;\n    ";
        let at_if = sql.find("IF").unwrap();
        assert_eq!(
            enclosing_block(sql, sql.len(), SqlDialect::MySql),
            Some((at_if, at_if + 2, false))
        );
    }

    #[test]
    fn match_paren_prefers_right_then_left() {
        // "a(b)c" — caret before '(' at offset 1
        assert_eq!(match_paren("a(b)c", 1, MySql), Some((1, 3)));
        // caret after ')' at offset 4 → matches the paren on the left (3)
        assert_eq!(match_paren("a(b)c", 4, MySql), Some((3, 1)));
        // caret between ')(' prefers the right paren '('
        assert_eq!(match_paren(")(", 1, MySql), None); // unmatched → no partner
    }

    #[test]
    fn match_paren_nested() {
        // "((x))"
        assert_eq!(match_paren("((x))", 0, MySql), Some((0, 4)));
        assert_eq!(match_paren("((x))", 1, MySql), Some((1, 3)));
    }

    #[test]
    fn match_paren_ignores_parens_in_strings_and_comments() {
        // the '(' inside the string has no code match
        assert_eq!(match_paren("'('", 1, MySql), None);
        // real parens around a string-literal paren still match each other
        // "( ')' )" → outer parens at 0 and 6
        let sql = "( ')' )";
        assert_eq!(match_paren(sql, 0, MySql), Some((0, 6)));
    }

    /// Square brackets pair as parens do — PostgreSQL's arrays and slices —
    /// each with its own kind: a `]` never closes a `(`, and a stray close of
    /// the wrong kind is passed over rather than taking the pair from the
    /// right one. On SQL Server and SQLite `[x]` is a quoted name, not a bracket.
    #[test]
    fn match_paren_pairs_square_brackets_by_kind() {
        let sql = "SELECT a[f(1)][2:3]";
        assert_eq!(match_paren(sql, 8, Postgres), Some((8, 13)));
        assert_eq!(match_paren(sql, 13, Postgres), Some((13, 8)));
        // Between `][` the right-hand one is taken, as with `)(`.
        assert_eq!(match_paren(sql, 14, Postgres), Some((14, 18)));
        assert_eq!(match_paren(sql, 10, Postgres), Some((10, 12)));
        assert_eq!(bracket_jump(sql, 19, Postgres), Some(15));
        // A stray `]` inside parens neither matches `(` nor steals its `)`.
        assert_eq!(match_paren("(a])", 0, MySql), Some((0, 3)));
        assert_eq!(match_paren("(a])", 2, MySql), None);
        assert_eq!(match_paren("(a]", 0, MySql), None);
        assert_eq!(match_paren("[a)", 0, MySql), None);
        assert_eq!(match_paren("SELECT [a b]", 7, MsSql), None);
        assert_eq!(match_paren("SELECT [a b]", 7, Sqlite), None);
    }

    #[test]
    fn match_paren_none_when_not_adjacent() {
        assert_eq!(match_paren("abc", 1, MySql), None);
        assert_eq!(match_paren("", 0, MySql), None);
    }

    /// **The early return picks the same pair the full sweep did.**
    ///
    /// `match_paren` used to materialise every balanced pair in the document
    /// and then look for the caret's; it now returns at the pair. Three shapes
    /// separate "the first pair that closes" from "the caret's pair", which a
    /// return placed one line too early would confuse:
    /// a pair that closes *before* the caret's, a pair that closes *inside*
    /// it, and an unmatched paren at the caret with well-formed pairs after it.
    #[test]
    fn match_paren_returns_the_carets_pair_not_the_first_one_closed() {
        // An earlier, unrelated pair closes first.
        assert_eq!(match_paren("(a)(b)", 3, MySql), Some((3, 5)));
        assert_eq!(match_paren("(a)(b)", 6, MySql), Some((5, 3)));
        // A nested pair closes inside the caret's.
        assert_eq!(match_paren("(()x)", 0, MySql), Some((0, 4)));
        assert_eq!(match_paren("(()x)", 1, MySql), Some((1, 2)));
        // An unmatched `)` at the caret takes no partner from the pair after
        // it. (The walk returns here rather than finishing the document; the
        // answer was the same before, so this pins the behaviour the early
        // return had to preserve, not a bug it fixed.)
        assert_eq!(match_paren(")(a)", 0, MySql), None);
        assert_eq!(match_paren(")(a)", 1, MySql), Some((1, 3)));
        // ...nor from one before it.
        assert_eq!(match_paren("(a))", 3, MySql), None);
    }

    // --- identifier_occurrences -------------------------------------------

    #[test]
    fn occurrences_finds_all_matches() {
        // "select id from t where id = 1" — caret on the first `id` (offset 8)
        let sql = "select id from t where id = 1";
        let hits = identifier_occurrences(sql, 8, MySql);
        assert_eq!(hits, vec![(7, 9), (23, 25)]);
    }

    #[test]
    fn occurrences_case_insensitive_whole_word() {
        // `ID` matches `id`; `identity` must NOT match (whole word only)
        let sql = "ID id identity";
        let hits = identifier_occurrences(sql, 0, MySql);
        assert_eq!(hits, vec![(0, 2), (3, 5)]);
    }

    #[test]
    fn occurrences_caret_after_word_counts() {
        // caret at offset 2 (just past `id`) still targets `id`
        let sql = "id x id";
        assert_eq!(identifier_occurrences(sql, 2, MySql), vec![(0, 2), (5, 7)]);
    }

    #[test]
    fn occurrences_skips_keywords_numbers_and_strings() {
        // keyword `from` → nothing even though it appears once
        assert!(identifier_occurrences("from from", 0, MySql).is_empty());
        // bare number
        assert!(identifier_occurrences("1 + 1", 0, MySql).is_empty());
        // caret inside a string literal
        assert!(identifier_occurrences("'ab' ab", 2, MySql).is_empty());
        // a matching word *inside* a string is not counted → only one real hit
        assert!(identifier_occurrences("x 'x'", 0, MySql).is_empty());
    }

    #[test]
    fn occurrences_empty_when_single_or_off_word() {
        // single occurrence → no highlight
        assert!(identifier_occurrences("alpha beta", 0, MySql).is_empty());
        // caret on whitespace, not adjacent to any word
        assert!(identifier_occurrences("a   a", 2, MySql).is_empty());
    }

    /// A name is its whole spelling on its engine: on SQL Server `@id` and
    /// `#t` are not the column `id` or the table `t`, and `a$b` is one name
    /// everywhere rather than an `a` and a `b`.
    #[test]
    fn occurrences_keep_a_sigil_or_dollar_as_part_of_the_name() {
        let sql = "DECLARE @id int; SELECT id FROM t WHERE id = @id";
        let vars = vec![(8, 11), (45, 48)];
        assert_eq!(identifier_occurrences(sql, 9, MsSql), vars);
        assert_eq!(identifier_occurrences(sql, 8, MsSql), vars, "on the @");
        assert_eq!(
            identifier_occurrences(sql, 25, MsSql),
            vec![(24, 26), (40, 42)]
        );
        let sql = "SELECT * FROM #t JOIN t ON #t.a = t.a";
        assert_eq!(
            identifier_occurrences(sql, 15, MsSql),
            vec![(14, 16), (27, 29)]
        );
        let sql = "SELECT a$b, b FROM t WHERE b = a$b";
        assert_eq!(
            identifier_occurrences(sql, 9, MySql),
            vec![(7, 10), (31, 34)]
        );
        assert_eq!(
            identifier_occurrences(sql, 12, MySql),
            vec![(12, 13), (27, 28)]
        );
    }

    /// **`regions_at` must answer exactly what `region_at` would**, offset for
    /// offset — it exists only to stop the caller asking in a loop, and
    /// `toggle_line_comment` decides from it whether a line is code to comment
    /// out or the inside of a string literal to leave alone.
    ///
    /// Every offset of each corpus is checked, so span starts, span interiors,
    /// the byte just past a span end and the position past the text are all in
    /// it, on all three dialects — which is where the two could differ:
    /// `region_at` settles at the caret and `regions_at` settles when the walk
    /// *passes* it, and the boundary cases are where those two readings of the
    /// same rule can come apart.
    #[test]
    fn regions_at_answers_offset_for_offset_what_region_at_does() {
        let corpora = [
            "",
            "select 1",
            "select 'a b' from t -- tail",
            "/* head */ select 1 /* tail",
            "select \"q\" , `b` , [c] from t",
            "select $$ a 'b' -- c $$ , $tag$ d $tag$ from t",
            "-- only a comment",
            "'unterminated",
            "a\n  'str\n  more\n  ' end\nb",
        ];
        for sql in corpora {
            for d in [MySql, Postgres, Sqlite] {
                let offsets: Vec<usize> = (0..=sql.len()).collect();
                let batch = regions_at(sql, &offsets, d);
                for &o in &offsets {
                    assert_eq!(batch[o], region_at(sql, o, d), "{d:?} {sql:?} at {o}");
                }
                // A sparse, duplicated, non-decreasing list is the shape the
                // caller actually hands over.
                let sparse: Vec<usize> = offsets.iter().copied().filter(|o| o % 3 == 0).collect();
                let mut dup = sparse.clone();
                dup.extend(sparse.iter().copied());
                dup.sort_unstable();
                let got = regions_at(sql, &dup, d);
                for (k, &o) in dup.iter().enumerate() {
                    assert_eq!(got[k], region_at(sql, o, d), "{d:?} {sql:?} sparse at {o}");
                }
            }
        }
    }

    /// **A caret word that is not in code still answers nothing** — now that
    /// the scan decides that itself instead of a `region_at` call before it.
    ///
    /// The two halves are separate mistakes. Dropping the check entirely makes
    /// the *first* case highlight from inside a comment; deciding it from
    /// "did the target appear anywhere in code" instead of "did a word start
    /// at `ws`" makes the *second* one do it, because the same name is a real
    /// identifier twice elsewhere in the statement.
    #[test]
    fn occurrences_refuse_a_caret_word_in_a_comment_or_a_string() {
        // Caret on `ab` inside the line comment; `ab` is a real identifier
        // twice below it.
        let sql = "-- ab\nab = ab";
        assert!(identifier_occurrences(sql, 3, MySql).is_empty());
        // The same, with the caret on a real one, to show the corpus is not
        // simply inert.
        assert_eq!(
            identifier_occurrences(sql, 6, MySql),
            vec![(6, 8), (11, 13)]
        );

        // Block comment, and a string — same rule.
        let sql = "/* ab */ ab = ab";
        assert!(identifier_occurrences(sql, 3, MySql).is_empty());
        let sql = "'ab' ab = ab";
        assert!(identifier_occurrences(sql, 2, MySql).is_empty());

        // A dollar-quoted body on PostgreSQL is a string too, and the caret
        // word inside it is not an identifier occurrence.
        let sql = "$$ ab $$ ab = ab";
        assert!(identifier_occurrences(sql, 3, Postgres).is_empty());
    }
}
