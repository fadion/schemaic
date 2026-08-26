//! **Taking a declared type apart** — the one place that knows where a type
//! string's parentheses are and what the words around them mean.
//!
//! `varchar(45)`, `int(11) unsigned`, `numeric(10,2)`,
//! `timestamp(3) without time zone`, `enum('a','b')`: a column's declared type
//! arrives as text from three different engines and three different catalogues,
//! and three parts of the app need to read it —
//!
//! * [`crate::ddl`] asks **are these two types the same** ([`crate::ddl::normalize_type`],
//!   which sits on the DDL round-trip gate);
//! * [`crate::celledit`] asks **which editor does this column get** — a `tinyint(1)`
//!   is a checkbox, an `enum(…)` is a picker over the values in the parentheses;
//! * [`crate::import`] asks **what kind of value goes in it** ([`crate::import::classify`]).
//!
//! They asked in three different ways. Two of them were a line-for-line copy of
//! each other ([`split`] and [`base`] below were `celledit::base_type` and
//! `ddl::split_type`'s opening), and the third split on `['(', ' ']` and took the
//! first word — which is a *fourth* answer again, not a simplification of the
//! others, because it drops the trailing words that `timestamp without time zone`
//! and `int unsigned` are made of.
//!
//! Now there is one splitter and the three questions are asked of it. The
//! questions stay different — [`leading_word`] exists precisely because
//! `import::classify` wants only `timestamp` where `ddl` wants
//! `timestamp without time zone` — but *where the parentheses are* is answered
//! once.
//!
//! **Nothing here understands what is inside the parentheses.** `enum('a','b')`
//! yields the text `'a','b'`; turning that into values is
//! `celledit::value_list`'s job, and it goes through `sql::skip_noncode` because
//! a member may contain a quote, a comma or a `)`. This module only finds the
//! boundaries.

/// A declared type's three lexical parts: the words before the parentheses,
/// what is between them, and the words after.
///
/// Every part is borrowed from the input and **trimmed**, so a caller that only
/// wants the arguments allocates nothing.
///
/// The closing paren is the **last** one, not the first: an `ENUM` member may
/// contain one (`enum('a)','b')`). The opening is the first, for the same reason
/// in reverse.
///
/// A type with an opening paren and no closing one is **not** treated as having
/// arguments — `head` is everything before the paren and `args` is empty. That is
/// a choice, and it is `ddl::split_type`'s rather than `celledit::base_type`'s:
/// the latter used to hand back `foo(bar` whole as the base name. Neither answer
/// is reachable from a live catalogue, which is exactly why the two were free to
/// disagree for as long as they did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeText<'a> {
    pub head: &'a str,
    pub args: &'a str,
    pub tail: &'a str,
}

/// Split a declared type at its parentheses. See [`TypeText`].
pub fn split(t: &str) -> TypeText<'_> {
    let t = t.trim();
    let (head, rest) = match t.find('(') {
        Some(i) => (&t[..i], &t[i + 1..]),
        None => (t, ""),
    };
    let (args, tail) = match rest.rfind(')') {
        Some(j) => (&rest[..j], &rest[j + 1..]),
        None => ("", ""),
    };
    TypeText {
        head: head.trim(),
        args: args.trim(),
        tail: tail.trim(),
    }
}

/// The type's base keyword(s), lower-cased and single-spaced: `varchar(45)` →
/// `varchar`, `int(11) unsigned` → `int unsigned`,
/// `timestamp(3) without time zone` → `timestamp without time zone`.
///
/// **The parenthesised part is dropped and the words after it kept**, which is
/// what makes PostgreSQL's `timestamp(3) with time zone` reachable from the same
/// match arm as its unparameterised spelling.
pub fn base(t: &str) -> String {
    let p = split(t);
    format!("{} {}", p.head, p.tail)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// What is between the parentheses (`enum('a','b')` → `'a','b'`, `tinyint(1)` →
/// `1`), or the empty string. Borrowed, and trimmed.
pub fn args(t: &str) -> &str {
    split(t).args
}

/// The **first** word of [`base`] — `timestamp without time zone` → `timestamp`,
/// `int unsigned` → `int`, `double precision` → `double`.
///
/// The reading `import::classify` wants, and its own doc says why it is not the
/// whole base: it matches a fixed list of scalar keywords, and `interval` and
/// `point` both *contain* "int", so a substring test would read every value they
/// hold as a rejected integer. Taking the leading word of a properly-split base
/// is the narrow version of that test.
pub fn leading_word(t: &str) -> String {
    base(t)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Does this type carry the `unsigned` modifier?
///
/// Asked of the **base**, not of the raw text, and that is the difference worth
/// having: `enum('unsigned','signed')` is not an unsigned column, and a
/// `contains("unsigned")` over the whole string said it was. Harmless where it
/// was asked — the flag is only consulted on an integer base — but it is one
/// fewer thing that has to stay harmless.
pub fn is_unsigned(t: &str) -> bool {
    base(t).split_whitespace().any(|w| w == "unsigned")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The corpus: every spelling the three consumers actually see, from all
    /// three engines' catalogues. Kept in one place because the point of this
    /// module is that they get **one** answer.
    const REAL_TYPES: &[&str] = &[
        "int",
        "int(11)",
        "int(10) unsigned",
        "INT UNSIGNED",
        "tinyint(1)",
        "bigint",
        "varchar(45)",
        "character varying(45)",
        "character varying",
        "text",
        "numeric(10,2)",
        "decimal(10, 2)",
        "double precision",
        "float8",
        "timestamp",
        "timestamp(3)",
        "timestamp with time zone",
        "timestamp(3) without time zone",
        "timestamptz",
        "date",
        "enum('a','b')",
        "enum('a)','b')",
        "set('r','w')",
        "boolean",
        "json",
        "  varchar(45)  ",
    ];

    #[test]
    fn the_base_keeps_the_words_after_the_parentheses() {
        assert_eq!(base("varchar(45)"), "varchar");
        assert_eq!(base("int(11) unsigned"), "int unsigned");
        assert_eq!(
            base("timestamp(3) without time zone"),
            "timestamp without time zone"
        );
        assert_eq!(
            base("timestamp(3) with time zone"),
            "timestamp with time zone"
        );
        assert_eq!(base("INT UNSIGNED"), "int unsigned");
        // Runs of whitespace collapse, so two spellings of one type agree.
        assert_eq!(base("decimal(10, 2)"), base("decimal(10,2)"));
    }

    /// The `)` that closes the arguments is the **last** one — an `ENUM` member
    /// may contain one, and taking the first cut the value list in half.
    #[test]
    fn the_closing_paren_is_the_last_one() {
        assert_eq!(args("enum('a)','b')"), "'a)','b'");
        assert_eq!(base("enum('a)','b')"), "enum");
    }

    #[test]
    fn the_arguments_are_what_is_between_the_parentheses() {
        assert_eq!(args("tinyint(1)"), "1");
        assert_eq!(args("numeric(10,2)"), "10,2");
        assert_eq!(args("decimal( 10 , 2 )"), "10 , 2");
        assert_eq!(args("int"), "");
        assert_eq!(args("timestamp with time zone"), "");
    }

    /// A type that opens a paren and never closes it has **no** arguments, and
    /// its base is the word before the paren. Not reachable from a live
    /// catalogue; pinned because the two implementations this module replaced
    /// disagreed here, and a reader should be able to find out which answer won.
    #[test]
    fn an_unclosed_paren_yields_no_arguments() {
        assert_eq!(args("foo(bar"), "");
        assert_eq!(base("foo(bar"), "foo");
        // And a stray `)` with no `(` is part of the name, not a delimiter.
        assert_eq!(base("foo)bar"), "foo)bar");
    }

    #[test]
    fn the_leading_word_drops_the_trailing_modifiers() {
        assert_eq!(leading_word("timestamp(3) without time zone"), "timestamp");
        assert_eq!(leading_word("int(10) unsigned"), "int");
        assert_eq!(leading_word("double precision"), "double");
        assert_eq!(leading_word("character varying(45)"), "character");
        assert_eq!(leading_word(""), "");
    }

    /// The modifier is a **word** of the base, not a substring of the type text.
    #[test]
    fn unsigned_is_a_word_and_not_a_substring() {
        assert!(is_unsigned("int(10) unsigned"));
        assert!(is_unsigned("BIGINT UNSIGNED"));
        assert!(!is_unsigned("int"));
        // The case the substring test got wrong.
        assert!(!is_unsigned("enum('unsigned','signed')"));
    }

    /// **Every part is borrowed and trimmed, and the three fit back together.**
    /// A `head`/`args`/`tail` that didn't account for the whole string would let
    /// a modifier fall down the gap between two consumers.
    #[test]
    fn every_real_type_splits_into_parts_that_account_for_it() {
        for t in REAL_TYPES {
            let p = split(t);
            let trimmed = t.trim();
            assert_eq!(p.head, p.head.trim(), "{t:?}: head not trimmed");
            assert_eq!(p.args, p.args.trim(), "{t:?}: args not trimmed");
            assert_eq!(p.tail, p.tail.trim(), "{t:?}: tail not trimmed");
            // Reassembled, ignoring the whitespace the split trimmed away, the
            // parts are the original — nothing was dropped or duplicated.
            // The tail's leading separator was trimmed off it, so it is put back
            // — otherwise `int(11) unsigned` reassembles as `int(11)unsigned`
            // and the test fails on its own arithmetic rather than on the split.
            let rebuilt = match p.args.is_empty() && !trimmed.contains('(') {
                true => format!("{} {}", p.head, p.tail),
                false => format!("{}({}) {}", p.head, p.args, p.tail),
            };
            let squash = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
            assert_eq!(
                squash(&rebuilt),
                squash(trimmed),
                "{t:?}: the parts do not account for the whole type"
            );
        }
    }

    /// The base is a prefix-preserving reduction: it never invents a word, and
    /// its leading word is always the first word of the type itself.
    #[test]
    fn the_base_never_invents_a_word() {
        for t in REAL_TYPES {
            let b = base(t);
            let first = t.trim().split(['(', ' ']).next().unwrap_or_default();
            assert_eq!(
                leading_word(t),
                first.to_ascii_lowercase(),
                "{t:?}: the leading word moved"
            );
            assert!(
                !b.contains('('),
                "{t:?}: the base kept a parenthesis: {b:?}"
            );
        }
    }
}
