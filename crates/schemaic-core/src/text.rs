//! Small pure text helpers shared by the UI's display strings.

/// Pick the singular or plural noun for a count: `plural(1, "row", "rows")` →
/// `"row"`; `0` or `2+` → `"rows"`. Returns only the noun form (not the number),
/// so the call site keeps control of how the count itself is rendered — a row
/// count is often humanized (`"1.2k"`), which must stay decoupled from the
/// singular/plural decision (still driven by the true `n`).
pub fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

#[cfg(test)]
mod tests {
    use super::*;

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
