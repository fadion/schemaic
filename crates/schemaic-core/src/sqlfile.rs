//! Reading and writing a tab's SQL as a `.sql` file on disk — the pure half.
//!
//! Everything here is a decision about *bytes and names*, taken away from the
//! two file dialogs and the worker thread that surround it in the app: what a
//! file's bytes become in the editor, what the editor's text becomes on the way
//! back, what the tab is called once it has a file, and what the Save dialog
//! should suggest for one that doesn't.
//!
//! # Line endings are remembered, not normalised away
//!
//! The editor works in `\n`. A `.sql` file checked into a repository on Windows
//! very often does not, and a Save that rewrote every line ending would turn a
//! one-line edit into a whole-file diff — the kind of change that survives review
//! only because nobody can read it. So [`decode`] converts to `\n` *and reports
//! what it found*, and [`encode`] puts it back. The flag rides on the tab (and
//! its saved session entry) between the two.
//!
//! Mixed endings resolve to the majority, and a tie to CRLF: a file that is
//! mostly CRLF with one stray LF is a CRLF file, and normalising the stray one
//! on save is the smaller lie.

use std::path::{Path, PathBuf};

/// The extension a SQL script gets when the user doesn't type one.
pub const SQL_EXT: &str = "sql";

/// Extensions the Open/Save dialogs filter on.
pub const SQL_EXTENSIONS: &[&str] = &["sql"];

/// The name shown beside those extensions in the dialog's type dropdown.
pub const SQL_FILTER_NAME: &str = "SQL script";

/// The suggested file name for a tab that has never been saved.
const FALLBACK_NAME: &str = "query";

/// A file's text as the editor wants it, plus what its line endings were.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlText {
    /// The text with every `\r\n` collapsed to `\n`.
    pub text: String,
    /// The file was CRLF-dominant, so [`encode`] should write it back that way.
    pub crlf: bool,
}

/// Turn a `.sql` file's bytes into editor text.
///
/// Strips a UTF-8 BOM (Windows tools write one and it would otherwise show up as
/// an invisible first character that breaks the first keyword), decodes lossily
/// — a mis-encoded byte should cost the user a replacement character, not the
/// whole file — and normalises line endings, recording what they were.
pub fn decode(bytes: &[u8]) -> SqlText {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let raw = String::from_utf8_lossy(bytes);
    let crlf = is_crlf(&raw);
    SqlText {
        text: raw.replace("\r\n", "\n"),
        crlf,
    }
}

/// Turn editor text back into the bytes to write, restoring CRLF when the file
/// had it. The inverse of [`decode`] for any text the editor can hold (which is
/// `\n`-only, since `decode` is the only way text arrives from a file).
pub fn encode(text: &str, crlf: bool) -> String {
    if crlf {
        // Guard against a stray `\r\n` already in the buffer (pasted from
        // somewhere) doubling into `\r\r\n`.
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        text.to_string()
    }
}

/// Was this text CRLF-dominant? Ties go to CRLF (see the module docs).
fn is_crlf(text: &str) -> bool {
    let crlf = text.matches("\r\n").count();
    if crlf == 0 {
        return false;
    }
    let lf = text.matches('\n').count();
    // `lf` counts the `\n` of every `\r\n` too, so lone LFs are the difference.
    crlf >= lf.saturating_sub(crlf)
}

/// The title a tab takes from its file: the file name, extension included.
///
/// With the extension because that is what every editor shows and what the user
/// typed into the dialog — a strip to `orders` would leave two tabs on
/// `orders.sql` and `orders.txt` indistinguishable. Falls back to the whole path
/// for the pathological case of a path with no final component.
pub fn tab_title(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// What the Save dialog should suggest for a tab, from its display title.
///
/// The title is user-typed (the inline rename) or generated ("Query 3"), so it
/// can hold anything: characters Windows forbids in a file name become `-`, and
/// a title left with no letter or digit at all falls back to `query.sql` rather
/// than handing the dialog a name it will reject — or one made of dashes.
///
/// A title that already ends in `.sql` keeps its single extension — a tab opened
/// from `orders.sql` must not suggest `orders.sql.sql`.
pub fn suggested_name(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            c if (c as u32) < 0x20 => '-',
            c => c,
        })
        .collect();
    // Windows also refuses a name ending in a dot or a space.
    let stem = cleaned.trim().trim_end_matches(['.', ' ']).trim();
    // A stem with nothing alphanumeric left in it isn't a name — a title of `///`
    // scrubs to `---`, which is legal and useless. Fall back instead.
    let stem = if stem.chars().any(|c| c.is_alphanumeric()) {
        stem
    } else {
        FALLBACK_NAME
    };
    if has_sql_ext(Path::new(stem)) {
        stem.to_string()
    } else {
        format!("{stem}.{SQL_EXT}")
    }
}

/// Give a path the `.sql` extension if it has none.
///
/// The native dialogs mostly append the filter's extension themselves, but not
/// on every platform and never when the user typed a trailing dot. A path that
/// already has *some* extension is left alone — `schema.ddl` is the user saying
/// what they want, and quietly saving `schema.ddl.sql` instead is worse than
/// honouring it.
pub fn ensure_extension(path: PathBuf) -> PathBuf {
    match path.extension() {
        Some(e) if !e.is_empty() => path,
        _ => path.with_extension(SQL_EXT),
    }
}

/// Does this path end in `.sql` (case-insensitively)?
fn has_sql_ext(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(SQL_EXT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_strips_a_bom_and_keeps_lf() {
        let got = decode(b"\xEF\xBB\xBFSELECT 1;\nSELECT 2;\n");
        assert_eq!(got.text, "SELECT 1;\nSELECT 2;\n");
        assert!(!got.crlf);
    }

    #[test]
    fn decode_normalises_crlf_and_remembers_it() {
        let got = decode(b"SELECT 1;\r\nSELECT 2;\r\n");
        assert_eq!(got.text, "SELECT 1;\nSELECT 2;\n");
        assert!(got.crlf);
    }

    #[test]
    fn decode_of_empty_bytes_is_empty_and_lf() {
        assert_eq!(
            decode(b""),
            SqlText {
                text: String::new(),
                crlf: false
            }
        );
        // A file that is nothing but a BOM is likewise empty.
        assert_eq!(decode(b"\xEF\xBB\xBF").text, "");
    }

    #[test]
    fn decode_replaces_invalid_utf8_rather_than_failing() {
        let got = decode(b"SELECT '\xFF';");
        assert!(got.text.starts_with("SELECT '"));
        assert!(got.text.contains('\u{FFFD}'));
    }

    #[test]
    fn a_lone_cr_is_not_a_line_ending() {
        // An old-Mac `\r` (or a `\r` inside a string literal) is left exactly as
        // it is: it isn't `\r\n`, so nothing collapses and nothing is claimed.
        let got = decode(b"SELECT '\ra';");
        assert_eq!(got.text, "SELECT '\ra';");
        assert!(!got.crlf);
    }

    #[test]
    fn mixed_endings_resolve_to_the_majority() {
        assert!(decode(b"a\r\nb\r\nc\nd").crlf, "two CRLF against one LF");
        assert!(!decode(b"a\nb\nc\r\nd").crlf, "two LF against one CRLF");
        // A tie is CRLF: normalising the odd lone LF is the smaller change.
        assert!(decode(b"a\r\nb\nc").crlf);
    }

    #[test]
    fn encode_round_trips_both_endings() {
        for raw in [
            &b"SELECT 1;\r\nSELECT 2;\r\n"[..],
            &b"SELECT 1;\nSELECT 2;\n"[..],
        ] {
            let d = decode(raw);
            assert_eq!(
                encode(&d.text, d.crlf).as_bytes(),
                raw,
                "decode → encode must be the identity on a file we opened"
            );
        }
    }

    #[test]
    fn encode_does_not_double_a_crlf_already_in_the_buffer() {
        assert_eq!(encode("a\r\nb\nc", true), "a\r\nb\r\nc");
    }

    #[test]
    fn tab_title_is_the_file_name_with_its_extension() {
        assert_eq!(
            tab_title(Path::new("/tmp/reports/orders.sql")),
            "orders.sql"
        );
        assert_eq!(tab_title(Path::new("orders.sql")), "orders.sql");
        // No final component — the whole path is better than an empty title.
        assert_eq!(tab_title(Path::new("/")), "/");
    }

    #[test]
    fn suggested_name_adds_the_extension_once() {
        assert_eq!(suggested_name("Query 3"), "Query 3.sql");
        assert_eq!(suggested_name("orders.sql"), "orders.sql");
        assert_eq!(suggested_name("orders.SQL"), "orders.SQL");
        // Some *other* extension is a stem, not an extension we should keep bare.
        assert_eq!(suggested_name("orders.txt"), "orders.txt.sql");
    }

    #[test]
    fn suggested_name_scrubs_what_a_file_name_cannot_hold() {
        assert_eq!(suggested_name("db:prod/orders"), "db-prod-orders.sql");
        assert_eq!(suggested_name("a\tb"), "a-b.sql");
        assert_eq!(suggested_name("  spaced  "), "spaced.sql");
        // Trailing dots and spaces are illegal on Windows.
        assert_eq!(suggested_name("report..."), "report.sql");
    }

    #[test]
    fn suggested_name_falls_back_when_nothing_survives() {
        assert_eq!(suggested_name(""), "query.sql");
        assert_eq!(suggested_name("   "), "query.sql");
        assert_eq!(suggested_name("///"), "query.sql");
    }

    #[test]
    fn ensure_extension_only_fills_a_missing_one() {
        assert_eq!(
            ensure_extension(PathBuf::from("/tmp/orders")),
            PathBuf::from("/tmp/orders.sql")
        );
        assert_eq!(
            ensure_extension(PathBuf::from("/tmp/orders.sql")),
            PathBuf::from("/tmp/orders.sql")
        );
        // A deliberate other extension is honoured, not doubled.
        assert_eq!(
            ensure_extension(PathBuf::from("/tmp/schema.ddl")),
            PathBuf::from("/tmp/schema.ddl")
        );
    }
}
