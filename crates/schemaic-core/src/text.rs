//! Small pure text helpers shared by the UI's display strings and by the
//! parsers that read user files.

/// Drop a leading UTF-8 BOM, which every Windows editor writes and no parser
/// wants.
///
/// **Shared because three modules read files a user picked** and each one broke
/// differently on the same three bytes: `import`'s CSV put U+FEFF inside the
/// first header name, `script`'s splitter sent it to the server on statement 1
/// *and* excluded that statement from the destructive count, and
/// `conn_import`'s INI scanner read `<BOM>[client]` as a bare key in the
/// unnamed section, so a `.my.cnf` imported zero rows and reported nothing
/// skipped. `str::trim` does not remove it — U+FEFF is not whitespace — which
/// is why each of them looked correct.
///
/// One only: a second BOM further into the text is data, and a file that opens
/// with two of them is not one this can repair.
pub fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

/// **The edits made to a text, by revision**, so what keeps an answer about
/// the text can take it forward without keeping a copy of the text to compare
/// with ([`common_affixes`] is that comparison's other form). Each edit is
/// recorded as the bytes it left alone at the front and at the back; the
/// edits since any revision then read as one, the smallest front and the
/// smallest back of them all — bytes outside every edit's window are bytes no
/// edit touched. A bounded number are kept; an answer older than that is
/// rebuilt whole.
#[derive(Clone, Debug, Default)]
pub struct EditLog {
    rev: u64,
    edits: std::collections::VecDeque<(u64, usize, usize)>,
}

/// What [`EditLog::since`] knows of the text since a revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Since {
    /// Nothing has changed.
    Same,
    /// Changed, and the first `pre` and last `suf` bytes are as they were.
    Edit { pre: usize, suf: usize },
    /// Too long ago to say.
    Unknown,
}

impl EditLog {
    /// How many edits are kept.
    const KEEP: usize = 512;

    /// The text's revision now.
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// An edit that left the first `pre` and the last `suf` bytes alone.
    pub fn record(&mut self, pre: usize, suf: usize) {
        self.rev += 1;
        self.edits.push_back((self.rev, pre, suf));
        if self.edits.len() > Self::KEEP {
            self.edits.pop_front();
        }
    }

    /// Every edit since revision `rev`, as one.
    pub fn since(&self, rev: u64) -> Since {
        if rev == self.rev {
            return Since::Same;
        }
        if rev > self.rev || self.edits.front().is_none_or(|&(r, _, _)| r > rev + 1) {
            return Since::Unknown;
        }
        let (pre, suf) = self
            .edits
            .iter()
            .filter(|&&(r, _, _)| r > rev)
            .fold((usize::MAX, usize::MAX), |(p, s), &(_, ep, es)| {
                (p.min(ep), s.min(es))
            });
        Since::Edit { pre, suf }
    }
}

/// How many `\n` bytes `b` holds — eight at a time: a byte is zero after the
/// XOR exactly when it was a newline, and `((t & 0x7f…) + 0x7f…) | t` sets a
/// byte's high bit exactly when the byte is not zero, so the high bits left
/// clear count the newlines with no false positives. The editor's incremental
/// passes ask it of everything before an edit, per keystroke.
pub fn count_newlines(b: &[u8]) -> usize {
    const NL: u64 = 0x0a0a_0a0a_0a0a_0a0a;
    const LOW: u64 = 0x7f7f_7f7f_7f7f_7f7f;
    const HIGH: u64 = 0x8080_8080_8080_8080;
    let (chunks, rest) = b.as_chunks::<8>();
    let mut n = 0usize;
    for c in chunks {
        let t = u64::from_le_bytes(*c) ^ NL;
        let nonzero = ((t & LOW).wrapping_add(LOW)) | t;
        n += (!nonzero & HIGH).count_ones() as usize;
    }
    n + rest.iter().filter(|&&c| c == b'\n').count()
}

/// How many bytes `a` and `b` share at the front, and then — of what is
/// left — at the back: where one edit turned `a` into `b`, everything outside
/// `[pre, len - suf)` of each. What the editor's incremental passes find an
/// edit with, per keystroke, so compared a block at a time (a `memcmp`) and
/// only the block that differs byte by byte.
pub fn common_affixes(a: &[u8], b: &[u8]) -> (usize, usize) {
    const BLOCK: usize = 4096;
    let shortest = a.len().min(b.len());
    let mut pre = 0;
    while pre + BLOCK <= shortest && a[pre..pre + BLOCK] == b[pre..pre + BLOCK] {
        pre += BLOCK;
    }
    pre += a[pre..shortest]
        .iter()
        .zip(&b[pre..shortest])
        .take_while(|(x, y)| x == y)
        .count();
    let room = shortest - pre;
    let mut suf = 0;
    while suf + BLOCK <= room
        && a[a.len() - suf - BLOCK..a.len() - suf] == b[b.len() - suf - BLOCK..b.len() - suf]
    {
        suf += BLOCK;
    }
    suf += a[..a.len() - suf]
        .iter()
        .rev()
        .zip(b[..b.len() - suf].iter().rev())
        .take(room - suf)
        .take_while(|(x, y)| x == y)
        .count();
    (pre, suf)
}

/// Read a text file somebody else's tool wrote, whatever it encoded it in.
///
/// **A config file that is not UTF-8 must not simply vanish.** `read_to_string`
/// answers `Err(InvalidData)` for a cp1252 `~/.my.cnf` (an accented comment is
/// enough) and for the UTF-16LE that Notepad's *Save as → Unicode* writes — and
/// the scan turned that into `None`, so the file disappeared from the list with
/// no note anywhere, under a modal saying the files are "in known places". At
/// its worst the vanished file is `~/.pgpass`, the one that holds passwords.
///
/// Three encodings, decided by BOM and then by fallback:
///
/// - **UTF-16**, LE or BE, by its BOM. Nothing else identifies it, and a
///   UTF-16 file read as UTF-8 is a NUL between every character rather than a
///   decode failure — so guessing here is not optional, it is the difference
///   between reading the file and reading nonsense.
/// - **UTF-8**, the overwhelming case, including a BOM'd one
///   ([`strip_bom`] takes that off downstream).
/// - **Anything else, lossily.** A mis-encoded byte should cost one character,
///   not the file — `sqlfile::decode`'s choice for the same problem. A cp1252
///   `.my.cnf` then parses correctly except for the accented byte, which is
///   almost always in a comment.
pub fn decode_text_file(bytes: &[u8]) -> String {
    match bytes {
        [0xFF, 0xFE, rest @ ..] => decode_utf16(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => decode_utf16(rest, u16::from_be_bytes),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// The UTF-16 half of [`decode_text_file`], byte order supplied by the caller.
/// A trailing odd byte is dropped: the file is truncated, and losing half a
/// character is a better answer than losing all of it.
fn decode_utf16(bytes: &[u8], word: fn([u8; 2]) -> u16) -> String {
    // `as_chunks::<2>` rather than `chunks_exact(2)`: the pairs arrive as
    // `[u8; 2]` already, which is what `word` takes, so the indexing goes too.
    // `.1` is the trailing odd byte, deliberately dropped — see above.
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|&c| word(c)).collect();
    String::from_utf16_lossy(&units)
}

/// The longest a table cell's text runs before [`cell_line`] cuts it.
pub const CELL_LINE_CHARS: usize = 120;

/// A value as one line of a fixed-height table row — Compare results' and the
/// Live monitor's: `NULL` for a null, every mandatory line break (UAX #14
/// BK/CR/LF/NL) as a space, a CRLF as one, cut at [`CELL_LINE_CHARS`] ending in
/// `…`.
///
/// **The one line is load-bearing.** Both tables are virtual stacks over
/// `VirtualItemSize::Fixed`, which places row `i` at `i * height` whatever the
/// row measures, so a value that shaped to two lines would paint across the row
/// below. The cut bounds what a column measures; the record (the comparison, the
/// export) still reads the whole value.
pub fn cell_line(v: Option<&str>) -> String {
    let Some(s) = v else {
        return "NULL".to_string();
    };
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    let mut n = 0;
    while let Some(c) = chars.next() {
        if n == CELL_LINE_CHARS - 1 && chars.peek().is_some() {
            out.push('…');
            break;
        }
        match c {
            '\r' if chars.peek() == Some(&'\n') => continue,
            '\n' | '\r' | '\u{0b}' | '\u{0c}' | '\u{85}' | '\u{2028}' | '\u{2029}' => out.push(' '),
            c => out.push(c),
        }
        n += 1;
    }
    out
}

/// Pick the singular or plural noun for a count: `plural(1, "row", "rows")` →
/// `"row"`; `0` or `2+` → `"rows"`. Returns only the noun form (not the number),
/// so the call site keeps control of how the count itself is rendered — a row
/// count is often humanized (`"1.2k"`), which must stay decoupled from the
/// singular/plural decision (still driven by the true `n`).
pub fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

/// Compact row-count label: `1000 → 1k`, `1250 → 1.25k`, `1_000_000 → 1m`.
/// Up to two decimals, trailing zeros trimmed. Under 1000 stays exact.
///
/// This is the **row-count** printer, and it has a partner it must agree with:
/// every string it emits has to be readable back by
/// [`crate::model::goto_row_index`], because the grid's stats line prints the
/// count here and the go-to-row box parses what the user retyped from it. That
/// round trip is why the function is shared rather than copied — the properties
/// surface and the grid must not disagree about what `200k` means.
///
/// Not to be confused with the token-count printer in [`crate::transcript`],
/// which buckets differently on purpose (`1.2k`, `12k`) and answers to nothing.
pub fn human_count(n: usize) -> String {
    let f = n as f64;
    // **Rounded first, then given a unit.** Picking the unit from the unrounded
    // value and rounding afterwards printed `999,999` as `1000k` — a unit that
    // tops out below 1000 showing 1000 of itself, and doing it in the
    // "Delete all ~1000k rows in orders?" confirmation, whose only job is to
    // convey scale before something irreversible. The threshold is the value at
    // which two decimals round up to 1000.
    const PROMOTE: f64 = 999.995;
    let (val, suffix) = if f >= 1e9 || f / 1e6 >= PROMOTE {
        // `b` is the largest unit there is, so past a trillion it keeps counting.
        (f / 1e9, "b")
    } else if f >= 1e6 || f / 1e3 >= PROMOTE {
        (f / 1e6, "m")
    } else if f >= 1e3 || f >= PROMOTE {
        (f / 1e3, "k")
    } else {
        return n.to_string();
    };
    let s = format!("{val:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s}{suffix}")
}

/// Would showing `msg` in a one-line bar `fits_chars` wide hide any of it?
///
/// The decision behind the error bar's **View** action, which opens the full text
/// in a modal. The bar collapses a message onto one line and ellipsizes it, so
/// View is there to recover what that lost — a server error with a DETAIL and a
/// HINT under it, say. For a short single-line message it lost nothing, and the
/// button opens a modal repeating the same words.
pub fn hides_detail(msg: &str, fits_chars: usize) -> bool {
    msg.trim_end().contains('\n') || msg.chars().count() > fits_chars
}

/// The part of `complete` beyond what has been `typed`, when `typed` is a
/// case-insensitive prefix of it — the dim tail an inline ghost paints after the
/// caret — or `None` when it is not a prefix, or nothing is left to paint.
///
/// **Every slice is taken at a real char boundary of `complete`.** Lowercasing is
/// not length-preserving — `İ` (U+0130) is two bytes and lowercases to `i` plus
/// U+0307, three — so slicing the original-cased `complete` at the length of the
/// lowercased `typed` can land inside a character, and `&str` indexing panics on
/// that. The Find palette's first ghost did exactly this, and a table named
/// `İzmir` plus the typed letter `i` crashed the app. So the walk is over
/// `complete`'s own boundaries, taking the first whose lowercased head covers
/// what was typed.
///
/// An empty `typed` answers `None`, deliberately: a ghost is a completion of
/// something, and a whole value painted into an empty box reads as a value the
/// box already holds. The palette, which does show its first row's whole name in
/// an empty box, asks for that on its side.
pub fn completion_tail<'a>(complete: &'a str, typed: &str) -> Option<&'a str> {
    let q = typed.to_lowercase();
    if q.is_empty() {
        return None;
    }
    complete
        .char_indices()
        .map(|(i, c)| i + c.len_utf8())
        .find(|&i| complete[..i].to_lowercase().starts_with(&q))
        .filter(|&i| i < complete.len())
        .map(|i| &complete[i..])
}

/// The option an inline ghost offers for a free-text field holding `typed`: the
/// first of `options`, in their order, that `typed` is a case-insensitive prefix
/// of and that has something left to add ([`completion_tail`]).
///
/// **`None` once `typed` already *is* an option**, whatever its case. Typing
/// `int` in a type box whose list holds both `int` and `integer` has produced a
/// whole answer; a ghost offering `eger` there would turn the one key that moves
/// on — Tab — into the key that changes the value.
///
/// The order is the caller's: these lists are curated with the common answer
/// first, and the first match is the one the ghost shows.
pub fn field_suggestion<'a>(typed: &str, options: &'a [String]) -> Option<&'a str> {
    let q = typed.to_lowercase();
    if options.iter().any(|o| o.to_lowercase() == q) {
        return None;
    }
    options
        .iter()
        .find(|o| completion_tail(o, typed).is_some())
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One line whatever the value holds: NULL says so, every mandatory break
    /// is a space (a CRLF one), and a long value is cut to the limit.
    #[test]
    fn a_cell_line_is_one_short_line() {
        assert_eq!(cell_line(None), "NULL");
        assert_eq!(cell_line(Some("")), "");
        assert_eq!(cell_line(Some("one\r\ntwo")), "one two");
        assert_eq!(
            cell_line(Some("a\u{2028}b\u{2029}c\u{85}d\u{0b}e\u{0c}f\rg\nh")),
            "a b c d e f g h"
        );
        let exact = "z".repeat(CELL_LINE_CHARS);
        assert_eq!(
            cell_line(Some(&exact)),
            exact,
            "exactly at the limit is whole"
        );
        let cut = cell_line(Some(&"z".repeat(CELL_LINE_CHARS + 1)));
        assert!(cut.ends_with('…'));
        assert_eq!(cut.chars().count(), CELL_LINE_CHARS);
    }

    /// **The log's merged edit leaves alone only what every edit did**: after
    /// any run of edits, the text at an older revision and the text now agree
    /// on the first `pre` and the last `suf` bytes it answers, with no overlap.
    #[test]
    fn the_edit_log_merges_edits_into_bytes_none_touched() {
        let edits: [(usize, usize, &str); 6] = [
            (3, 5, "XYZ"),
            (0, 0, "head "),
            (20, 22, ""),
            (10, 10, "\n\n"),
            (2, 30, "mid"),
            (8, 8, "tail"),
        ];
        let mut texts = vec!["select a, b from orders where a = 1 and b = 2".to_string()];
        let mut log = EditLog::default();
        assert_eq!(log.since(0), Since::Same);
        for &(lo, hi, ins) in &edits {
            let old = texts.last().unwrap().clone();
            let (lo, hi) = (lo.min(old.len()), hi.min(old.len()));
            let new = format!("{}{ins}{}", &old[..lo], &old[hi..]);
            log.record(lo, old.len() - hi);
            texts.push(new);
        }
        let now = texts.last().unwrap();
        for (rev, old) in texts.iter().enumerate() {
            match log.since(rev as u64) {
                Since::Same => assert_eq!(old, now),
                Since::Unknown => panic!("revision {rev} is kept"),
                Since::Edit { pre, suf } => {
                    assert!(pre + suf <= old.len().min(now.len()), "{rev}");
                    assert_eq!(old[..pre], now[..pre], "front since {rev}");
                    assert_eq!(
                        old[old.len() - suf..],
                        now[now.len() - suf..],
                        "back since {rev}"
                    );
                }
            }
        }
        // Past what is kept, it does not guess.
        let mut long = EditLog::default();
        for _ in 0..EditLog::KEEP + 5 {
            long.record(0, 0);
        }
        assert_eq!(long.since(1), Since::Unknown);
        assert_eq!(long.since(long.rev() + 1), Since::Unknown);
    }

    /// The block compare answers what a byte-by-byte one does — across block
    /// edges, with the shared front and back overlapping in a repetitive text,
    /// and for equal, empty and wholly different texts.
    #[test]
    fn common_affixes_find_the_edit() {
        let naive = |a: &[u8], b: &[u8]| {
            let pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
            let room = a.len().min(b.len()) - pre;
            let suf = a
                .iter()
                .rev()
                .zip(b.iter().rev())
                .take(room)
                .take_while(|(x, y)| x == y)
                .count();
            (pre, suf)
        };
        let base: Vec<u8> = (0..20_000u32)
            .map(|i| b"abcab\n"[(i % 6) as usize])
            .collect();
        for at in [0, 1, 4095, 4096, 4097, 8191, 10_000, 19_999, 20_000] {
            for ins in [&b""[..], b"x", b"ab", &[b'q'; 5000][..]] {
                for del in [0usize, 1, 3, 4096] {
                    let end = (at + del).min(base.len());
                    let mut new = base[..at].to_vec();
                    new.extend_from_slice(ins);
                    new.extend_from_slice(&base[end..]);
                    assert_eq!(
                        common_affixes(&base, &new),
                        naive(&base, &new),
                        "at {at} del {del}"
                    );
                    assert_eq!(
                        common_affixes(&new, &base),
                        naive(&new, &base),
                        "at {at} del {del}"
                    );
                }
            }
        }
        // Newlines counted eight at a time match a plain count, whatever
        // surrounds them — every byte value beside one, and every tail length.
        let all: Vec<u8> = (0..=255u8).flat_map(|c| [c, b'\n', c, c]).collect();
        for len in 0..all.len() {
            let s = &all[..len];
            assert_eq!(
                count_newlines(s),
                s.iter().filter(|&&c| c == b'\n').count(),
                "{len}"
            );
        }
        assert_eq!(
            count_newlines(&base),
            base.iter().filter(|&&c| c == b'\n').count()
        );
        assert_eq!(common_affixes(b"", b""), (0, 0));
        assert_eq!(common_affixes(b"abc", b"abc"), (3, 0));
        assert_eq!(common_affixes(b"abc", b"xyz"), (0, 0));
    }

    fn opts(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_completion_tail_is_what_is_left_after_a_case_insensitive_prefix() {
        assert_eq!(completion_tail("varchar(255)", "var"), Some("char(255)"));
        // The tail keeps the completion's own casing, whatever was typed.
        assert_eq!(completion_tail("varchar(255)", "VAR"), Some("char(255)"));
        assert_eq!(
            completion_tail("utf8mb4_0900_ai_ci", "utf8MB4_"),
            Some("0900_ai_ci")
        );
    }

    #[test]
    fn a_completion_tail_is_none_when_nothing_is_left_or_it_is_no_prefix() {
        assert_eq!(completion_tail("text", "text"), None);
        assert_eq!(completion_tail("text", "TEXT"), None);
        assert_eq!(completion_tail("text", "tex t"), None);
        assert_eq!(completion_tail("text", "texts"), None);
        assert_eq!(completion_tail("text", "x"), None);
        assert_eq!(completion_tail("", "a"), None);
    }

    /// A ghost completes something; an empty box has nothing to complete.
    #[test]
    fn an_empty_typed_value_has_no_completion_tail() {
        assert_eq!(completion_tail("integer", ""), None);
        assert_eq!(completion_tail("", ""), None);
    }

    /// The crash the palette's ghost shipped with: `İ` lowercases to three
    /// bytes from two, so a slice at the typed length is not a boundary.
    #[test]
    fn a_completion_tail_is_cut_at_a_char_boundary_when_case_changes_length() {
        assert_eq!(completion_tail("İzmir", "i"), Some("zmir"));
        assert_eq!(completion_tail("İzmir", "İz"), Some("mir"));
        assert_eq!(completion_tail("çağrı", "ÇA"), Some("ğrı"));
    }

    #[test]
    fn a_field_suggestion_is_the_first_option_the_typed_text_begins() {
        let types = opts(&["integer", "bigint", "int", "varchar(255)", "varbinary(255)"]);
        assert_eq!(field_suggestion("var", &types), Some("varchar(255)"));
        assert_eq!(field_suggestion("VARB", &types), Some("varbinary(255)"));
        assert_eq!(field_suggestion("i", &types), Some("integer"));
        assert_eq!(field_suggestion("big", &types), Some("bigint"));
    }

    /// Typing a whole option is an answer, not the start of a longer one.
    #[test]
    fn a_field_suggestion_is_none_once_the_typed_text_is_an_option() {
        let types = opts(&["integer", "int"]);
        assert_eq!(field_suggestion("int", &types), None);
        assert_eq!(field_suggestion("INT", &types), None);
        assert_eq!(field_suggestion("integer", &types), None);
        // Short of a whole option, the longer one is still offered.
        assert_eq!(field_suggestion("inte", &types), Some("integer"));
    }

    #[test]
    fn a_field_suggestion_is_none_for_an_empty_value_list_or_no_match() {
        let types = opts(&["integer", "text"]);
        assert_eq!(field_suggestion("", &types), None);
        assert_eq!(field_suggestion("x", &types), None);
        assert_eq!(field_suggestion("te", &[]), None);
        assert_eq!(field_suggestion("text(", &types), None);
    }

    #[test]
    fn a_leading_bom_comes_off_and_nothing_else_does() {
        assert_eq!(strip_bom("\u{feff}[client]"), "[client]");
        assert_eq!(strip_bom("[client]"), "[client]");
        assert_eq!(strip_bom(""), "");
        // Only the first — a second is data.
        assert_eq!(strip_bom("\u{feff}\u{feff}x"), "\u{feff}x");
        // Not whitespace, and not something `trim` would have caught.
        assert_eq!("\u{feff}x".trim(), "\u{feff}x");
        // Mid-text is left alone.
        assert_eq!(strip_bom("x\u{feff}y"), "x\u{feff}y");
    }

    /// **A config file that is not UTF-8 vanished from the scan**, silently, and
    /// worst of all when it was `~/.pgpass`. Both of the encodings measured are
    /// covered: cp1252 (an accented comment is enough) and the UTF-16LE
    /// Notepad's *Save as → Unicode* writes.
    #[test]
    fn a_client_config_file_is_read_whatever_it_was_encoded_in() {
        // Plain UTF-8, and UTF-8 with a BOM — `strip_bom` takes that off later.
        assert_eq!(
            decode_text_file(b"[client]\nhost=h\n"),
            "[client]\nhost=h\n"
        );
        assert_eq!(
            decode_text_file("\u{feff}[client]".as_bytes()),
            "\u{feff}[client]"
        );

        // UTF-16LE, which is not a decode *failure* when read as UTF-8 — it is
        // a NUL between every character, so guessing here is the difference
        // between reading the file and reading nonsense.
        let mut le = vec![0xFF, 0xFE];
        for u in "[client]\nhost=h\n".encode_utf16() {
            le.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode_text_file(&le), "[client]\nhost=h\n");

        let mut be = vec![0xFE, 0xFF];
        for u in "[client]".encode_utf16() {
            be.extend_from_slice(&u.to_be_bytes());
        }
        assert_eq!(decode_text_file(&be), "[client]");

        // cp1252: `password=café` with the accent as a single 0xE9 byte. One
        // replacement character, and every other line intact — which is what
        // makes the file usable rather than absent.
        let cp1252 = b"[client]\nuser=caf\xE9\npassword=pw\n";
        let out = decode_text_file(cp1252);
        assert!(out.contains("password=pw"), "{out}");
        assert!(out.starts_with("[client]"), "{out}");
        assert!(
            out.contains('\u{fffd}'),
            "the bad byte costs one char: {out}"
        );
    }

    /// A truncated UTF-16 file loses half a character, not all of them.
    #[test]
    fn an_odd_trailing_byte_does_not_swallow_the_file() {
        let mut le = vec![0xFF, 0xFE];
        for u in "ab".encode_utf16() {
            le.extend_from_slice(&u.to_le_bytes());
        }
        le.push(0x63);
        assert_eq!(decode_text_file(&le), "ab");
    }

    #[test]
    fn a_short_one_line_message_hides_nothing() {
        // What the bar shows *is* the message, so a View would repeat it.
        assert!(!hides_detail(
            "Invalid JSON: expected value at line 1 column 1",
            90
        ));
        assert!(!hides_detail("", 90));
    }

    #[test]
    fn a_multi_line_message_hides_its_tail() {
        // A server error with its DETAIL under it — collapsed to one line, the
        // rest is only reachable through View.
        assert!(hides_detail(
            "ERROR: null value violates not-null constraint\nDETAIL: Failing row contains (1, null).",
            90
        ));
        // A trailing newline is not detail.
        assert!(!hides_detail("ERROR: syntax error\n", 90));
    }

    #[test]
    fn a_long_message_is_ellipsized_so_it_hides_its_tail() {
        assert!(hides_detail(&"x".repeat(91), 90));
        // Exactly the width still fits.
        assert!(!hides_detail(&"x".repeat(90), 90));
    }

    #[test]
    fn width_is_counted_in_characters_not_bytes() {
        // A multi-byte name in a server error must not be mistaken for overflow.
        assert!(!hides_detail(&"é".repeat(90), 90));
    }

    #[test]
    fn one_is_singular_everything_else_plural() {
        assert_eq!(plural(1, "row", "rows"), "row");
        assert_eq!(plural(1, "col", "cols"), "col");
        assert_eq!(plural(1, "key", "keys"), "key");
    }

    #[test]
    fn zero_is_plural() {
        assert_eq!(plural(0, "row", "rows"), "rows");
        assert_eq!(plural(0, "key", "keys"), "keys");
    }

    #[test]
    fn many_is_plural() {
        assert_eq!(plural(2, "col", "cols"), "cols");
        assert_eq!(plural(6, "col", "cols"), "cols");
        assert_eq!(plural(1_000, "row", "rows"), "rows");
    }

    /// The tested namesake in `crate::transcript` is a **different** function
    /// with different buckets — so grepping the name finds coverage that does
    /// not apply. These are the exact strings the grid's stats line prints, and
    /// therefore the exact strings `goto_row_index` has to read back.
    #[test]
    fn a_count_reads_the_way_the_stats_line_prints_it() {
        assert_eq!(human_count(0), "0");
        assert_eq!(human_count(999), "999", "under 1000 stays exact");
        assert_eq!(human_count(1_000), "1k");
        assert_eq!(human_count(1_250), "1.25k");
        assert_eq!(human_count(200_000), "200k");
        assert_eq!(human_count(1_000_000), "1m");
        assert_eq!(human_count(1_500_000), "1.5m");
        assert_eq!(human_count(1_000_000_000), "1b");
    }

    /// **A unit that tops out below 1000 must never print 1000 of itself.**
    /// Picking the unit from the unrounded value and rounding afterwards made
    /// `999,999` read `1000k` — and this string reaches
    /// *"Delete all ~1000k rows in orders? This can't be undone."*, a dialog
    /// whose only job is to convey scale before something irreversible.
    #[test]
    fn a_count_is_promoted_rather_than_rounded_past_its_unit() {
        assert_eq!(human_count(999_999), "1m");
        assert_eq!(human_count(999_995), "1m", "the first value that rounds up");
        assert_eq!(human_count(999_994), "999.99k", "and the last that doesn't");
        assert_eq!(human_count(999_999_999), "1b");
        assert_eq!(human_count(999_995_000), "1b");
        assert_eq!(human_count(999_994_999), "999.99m");
        // The bottom boundary is the same rule seen from below.
        assert_eq!(human_count(999), "999");
        assert_eq!(human_count(1_000), "1k");
    }

    /// Every shape the printer can emit round-trips through the go-to-row box,
    /// which is the property the two functions have to hold together: the count
    /// on screen is the one a user types back.
    #[test]
    fn every_printed_count_is_readable_by_go_to_row() {
        for n in [1usize, 999, 1_000, 1_250, 200_000, 1_000_000, 1_500_000] {
            let printed = human_count(n);
            assert_eq!(
                crate::model::goto_row_index(&printed, n),
                Some(n - 1),
                "{printed:?} came from {n}"
            );
        }
    }

    /// The round trip **without the clamp**. `goto_row_index` clamps to the row
    /// count, so passing `n` as the count let a printed string that parses to
    /// something else pass this property by accident — which is exactly what
    /// `1000k` did.
    #[test]
    fn a_printed_count_parses_back_to_within_rounding_of_itself() {
        for n in [
            1usize,
            999,
            1_000,
            1_250,
            200_000,
            999_994,
            999_995,
            999_999,
            1_000_000,
            1_500_000,
            999_999_999,
        ] {
            let printed = human_count(n);
            let parsed = crate::model::goto_row_index(&printed, usize::MAX)
                .unwrap_or_else(|| panic!("{printed:?} is unreadable"))
                + 1;
            // Two decimals of the printed unit is the resolution the string has,
            // i.e. 0.001 of the value — never a whole unit out, which is what
            // `1000k` was.
            let slack = (n as f64 * 0.001).max(1.0);
            assert!(
                (parsed as f64 - n as f64).abs() <= slack,
                "{printed:?} came from {n} and reads back as {parsed}"
            );
        }
    }
}
