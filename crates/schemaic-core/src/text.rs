//! Small pure text helpers shared by the UI's display strings.

/// Pick the singular or plural noun for a count: `plural(1, "row", "rows")` →
/// `"row"`; `0` or `2+` → `"rows"`. Returns only the noun form (not the number),
/// so the call site keeps control of how the count itself is rendered — a row
/// count is often humanized (`"1.2k"`), which must stay decoupled from the
/// singular/plural decision (still driven by the true `n`).
pub fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
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
}
