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

#[cfg(test)]
mod tests {
    use super::*;

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
