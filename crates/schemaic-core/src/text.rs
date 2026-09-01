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
