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
pub fn backspace_pair(text: &str, caret: usize, dialect: SqlDialect) -> Option<(usize, usize)> {
    let b = text.as_bytes();
    if caret == 0 || caret >= b.len() {
        return None;
    }
    let empty_pair = match (b[caret - 1], b[caret]) {
        (b'(', b')') | (b'\'', b'\'') | (b'"', b'"') => true,
        (b'`', b'`') => dialect.backtick_ident(),
        _ => false,
    };
    empty_pair.then_some((caret - 1, caret + 1))
}

/// If the caret is adjacent to a parenthesis that isn't inside a string/comment,
/// return `(here, partner)` — the byte offset of that paren and of its match.
/// Prefers the paren to the *right* of the caret, else the one to the *left*.
/// `None` when there's no adjacent (code) paren or it has no match.
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
    let is_paren = |p: usize| matches!(b.get(p), Some(b'(') | Some(b')'));
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
            b'(' => stack.push(i),
            b')' => match stack.pop() {
                Some(open) if open == here => return Some((here, i)),
                Some(open) if i == here => return Some((here, open)),
                // An unmatched `)` **at** the caret has no partner and never
                // will — the pairs after it belong to somebody else.
                None if i == here => return None,
                _ => {}
            },
            _ => {}
        }
        i += 1;
    }
    None
}

/// All byte ranges of the identifier under the caret, for "highlight all
/// occurrences of the identifier under the caret." Returns empty unless the caret
/// sits on an identifier in *code* (not a string/comment), the identifier is
/// neither a bare number nor a SQL keyword, and it occurs at least twice.
///
/// Matching is whole-word and ASCII-case-insensitive (SQL identifiers fold for
/// ASCII); occurrences inside strings/comments are skipped via the shared lexer.
/// The occurrence under the caret is included in the result.
pub fn identifier_occurrences(
    text: &str,
    caret: usize,
    dialect: SqlDialect,
) -> Vec<(usize, usize)> {
    let b = text.as_bytes();
    // The caret is "on" a word if the byte at it or just before it is a word byte.
    let on_word = b.get(caret).is_some_and(|&c| is_word_byte(c))
        || (caret > 0 && b.get(caret - 1).is_some_and(|&c| is_word_byte(c)));
    if !on_word {
        return Vec::new();
    }
    // Expand to the whole word straddling the caret.
    let mut ws = caret;
    while ws > 0 && is_word_byte(b[ws - 1]) {
        ws -= 1;
    }
    let mut we = caret;
    while we < b.len() && is_word_byte(b[we]) {
        we += 1;
    }
    if ws == we {
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
        if is_word_byte(b[i]) {
            let start = i;
            while i < b.len() && is_word_byte(b[i]) {
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
    use SqlDialect::{MySql, Postgres, Sqlite};

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

    // --- match_paren -------------------------------------------------------

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
