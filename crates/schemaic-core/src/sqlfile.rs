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
//! on save is the smaller lie. A UTF-8 BOM is remembered the same way, and for
//! the same reason.
//!
//! # A lossy read is not a licence to write
//!
//! [`decode`] reads bytes it cannot make sense of as U+FFFD, because a
//! mis-encoded byte should cost the user a replacement character rather than the
//! whole file. That is a decision about *reading*. Writing the result back is a
//! different act — it replaces every unreadable byte in the file permanently,
//! including in the thousands of lines the user never touched — so the fact of
//! the loss rides on [`SqlFormat::lossy`] and the caller has to ask first. The
//! `.sql` file is the one artefact in this application that Schemaic cannot
//! regenerate.

use std::path::{Path, PathBuf};

/// The extension a SQL script gets when the user doesn't type one.
pub const SQL_EXT: &str = "sql";

/// Extensions the Open/Save dialogs filter on.
pub const SQL_EXTENSIONS: &[&str] = &["sql"];

/// The name shown beside those extensions in the dialog's type dropdown.
pub const SQL_FILTER_NAME: &str = "SQL script";

/// The suggested file name for a tab that has never been saved.
const FALLBACK_NAME: &str = "query";

/// Everything about a file's bytes that isn't its text — what [`encode`] needs
/// to put back, and what a caller needs to know before it overwrites the file.
///
/// It rides on the tab (and its saved session entry) between the read and the
/// write, which is the only place it can live: the editor holds `\n`-only UTF-8
/// text and has no memory of what the bytes were.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SqlFormat {
    /// The file was CRLF-dominant, so [`encode`] should write it back that way.
    pub crlf: bool,
    /// The file began with a UTF-8 BOM, which [`decode`] strips and [`encode`]
    /// therefore has to put back — a Save that dropped it would rewrite the
    /// file's first three bytes for a one-line edit, and some Windows tools read
    /// the file differently without it.
    pub bom: bool,
    /// **`decode` could not read every byte as UTF-8 and substituted U+FFFD.**
    ///
    /// The lossy read is deliberate — a mis-encoded byte should cost the user a
    /// replacement character rather than the whole file — but it is a decision
    /// about *reading*, and writing the result back is a different act: it
    /// replaces every unreadable byte in the file permanently, including in the
    /// thousands of lines the user never touched. A Latin-1 `mysqldump`, or
    /// anything Notepad wrote in the ANSI codepage, is exactly this shape. So
    /// the fact travels with the text, and the caller must ask before it saves.
    pub lossy: bool,
}

/// A file's text as the editor wants it, plus what its bytes were.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlText {
    /// The text with every `\r\n` collapsed to `\n`.
    pub text: String,
    pub format: SqlFormat,
}

/// Past this, opening a `.sql` file asks first. See [`open_verdict`].
///
/// 1 MB is where the editor's own analysis crosses half a second on an *empty*
/// catalogue — measured at 710 ms for a 1 MB document, 2.78 s at 4 MB, 11.4 s at
/// 16 MB — and a real catalogue is worse than that, not better.
pub const OPEN_WARN_BYTES: u64 = 1 << 20;

/// Past this, opening is refused outright.
///
/// 64 MB puts the same analysis at ~45 s per pause in typing, forever, which is
/// not a slow editor but a hung window. The number is deliberately far above
/// anything a hand-written script reaches: what lands here is a database dump,
/// and the answer for one of those is the import path or a query tab, not a
/// syntax-highlighted editor holding four copies of it.
pub const OPEN_REFUSE_BYTES: u64 = 64 << 20;

/// What opening a file of `bytes` should do.
///
/// **The editor's cost is the whole reason this exists**, not the read: `fs::read`
/// and [`decode`] are cheap even at 256 MB. What is not cheap is
/// `intel::diagnostics`, which runs over the *whole* document on the UI thread
/// 120 ms after every burst of typing — so a file that opens in a moment leaves
/// the window unresponsive for as long as the user keeps it open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenVerdict {
    /// Open it.
    Open,
    /// Ask first, with this many bytes to name in the question.
    Confirm(u64),
    /// Refuse, with this many bytes to name in the message.
    Refuse(u64),
}

/// Should a file this big be opened in an editor tab? See [`OpenVerdict`].
pub fn open_verdict(bytes: u64) -> OpenVerdict {
    if bytes > OPEN_REFUSE_BYTES {
        OpenVerdict::Refuse(bytes)
    } else if bytes > OPEN_WARN_BYTES {
        OpenVerdict::Confirm(bytes)
    } else {
        OpenVerdict::Open
    }
}

/// Turn a `.sql` file's bytes into editor text.
///
/// Strips a UTF-8 BOM (Windows tools write one and it would otherwise show up as
/// an invisible first character that breaks the first keyword), decodes lossily
/// — a mis-encoded byte should cost the user a replacement character, not the
/// whole file — and normalises line endings. **Each of those three is recorded**
/// in the returned [`SqlFormat`], because each is something `encode` has to put
/// back or a caller has to warn about.
pub fn decode(bytes: &[u8]) -> SqlText {
    let bom = bytes.starts_with(b"\xEF\xBB\xBF");
    let bytes = if bom { &bytes[3..] } else { bytes };
    let raw = String::from_utf8_lossy(bytes);
    // `from_utf8_lossy` borrows when every byte was valid UTF-8 and allocates
    // only when it had to substitute — so the fact is already in hand, free.
    let lossy = matches!(raw, std::borrow::Cow::Owned(_));
    let crlf = is_crlf(&raw);
    SqlText {
        text: raw.replace("\r\n", "\n"),
        format: SqlFormat { crlf, bom, lossy },
    }
}

/// Turn editor text back into the bytes to write, restoring the BOM and the CRLF
/// the file had. The inverse of [`decode`] for any text the editor can hold
/// (which is `\n`-only, since `decode` is the only way text arrives from a file)
/// **and any file `decode` read without loss** — a lossy read has no inverse,
/// which is what [`SqlFormat::lossy`] is for.
pub fn encode(text: &str, format: SqlFormat) -> String {
    let body = if format.crlf {
        // Guard against a stray `\r\n` already in the buffer (pasted from
        // somewhere) doubling into `\r\r\n`.
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        text.to_string()
    };
    if format.bom {
        format!("\u{FEFF}{body}")
    } else {
        body
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

/// Does this platform's filesystem read two paths differing only in case as one
/// file?
///
/// Windows (NTFS) and macOS (APFS, as shipped) do; Linux does not, and
/// `Makefile` beside `makefile` really is two files there. It is a `const` rather
/// than a runtime probe because the alternative — a case-folding answer taken
/// from one directory — would be wrong for a path on a mounted volume with the
/// other setting, and being wrong in *that* direction merges two files the user
/// meant to keep apart.
pub const PATHS_IGNORE_CASE: bool = cfg!(any(windows, target_os = "macos"));

/// Do these two paths name the same file, as far as the paths themselves can
/// say?
///
/// **Two tabs bound to one file is a lost edit**: each keeps its own copy of the
/// bytes on disk, so saving the second silently discards the first, and the first
/// tab goes on reporting itself clean because its own copy still matches what it
/// wrote. The comparison that prevented it was `Path`'s own `==`, which is
/// component-wise and case-**sensitive** on every platform — wrong in exactly the
/// direction Windows and macOS need.
///
/// This is the path-shaped half of the question and it is deliberately not the
/// whole of it: a hard link, a junction, an 8.3 short name or a substituted drive
/// still reads as two names for one file. The caller canonicalises first (which
/// resolves all four when the file exists) and asks this afterwards, because a
/// path being saved to for the first time cannot be canonicalised at all.
pub fn same_file(a: &Path, b: &Path) -> bool {
    same_path(a, b, PATHS_IGNORE_CASE)
}

/// [`same_file`]'s rule with the platform's answer passed in, so both sides of it
/// are testable on either platform.
///
/// Case folding is Unicode's simple lowercase, not NTFS's own upcasing table:
/// they differ only for characters outside the Basic Multilingual Plane's common
/// range, and the cost of a disagreement there is the second tab this exists to
/// prevent — not a wrong file.
pub fn same_path(a: &Path, b: &Path, ignore_case: bool) -> bool {
    // `Components` normalises away `.` and repeated separators, which is why the
    // comparison is over components rather than over the strings.
    if !ignore_case {
        return a.components().eq(b.components());
    }
    let folded = |p: &Path| -> Vec<String> {
        p.components()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect()
    };
    folded(a) == folded(b)
}

/// A tab's binding to a file on disk: its path, the text last known to be *on*
/// disk, and the byte shape to write back.
///
/// It exists as one value because the three are only ever correct together, and
/// the places that set them are not the places that read them. Setting the path
/// without the on-disk text leaves a tab that claims to be a saved file and
/// can't tell whether it is modified; clearing the path without clearing the
/// format leaves a fresh document carrying the CRLF and BOM of a file it is no
/// longer bound to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileBinding {
    pub path: Option<PathBuf>,
    /// The file's text as it was last read or written — `None` when that is not
    /// known, which a tab shows as modified. See [`restored_binding`].
    pub disk_sql: Option<String>,
    pub format: SqlFormat,
}

impl FileBinding {
    /// **No file.** What a tab holds when it has never been saved, and what the
    /// "blank slate" left behind by closing a connection's last tab must be
    /// reset to.
    ///
    /// A cleared tab that kept its path is one Ctrl+S from overwriting that file
    /// with an empty document — the tab looks new and the keystroke does not ask.
    /// Expressed as a value rather than three assignments because the bug is
    /// always an assignment that was left out.
    pub fn none() -> Self {
        Self::default()
    }

    /// Is this tab bound to a file at all?
    pub fn is_bound(&self) -> bool {
        self.path.is_some()
    }
}

/// The binding a tab comes back with when a session is restored.
///
/// **The file's text is not persisted**, so whether the on-disk copy is known
/// has exactly one answer per saved tab, and it is this one: a tab saved *clean*
/// had its editor text equal to the file, and that text *is* persisted — so the
/// restored `query` is the on-disk copy. A tab saved *dirty* leaves it unknown,
/// which the tab shows as modified until a save or a reload settles it. A tab
/// with no path has no on-disk copy to know about.
///
/// The trap is the fourth combination: a *dirty* tab whose text is restored and
/// whose `disk_sql` is wrongly set to it comes back looking clean, so the
/// modified marker is gone and Ctrl+S is a no-op over the user's unsaved work.
pub fn restored_binding(
    path: Option<PathBuf>,
    file_dirty: bool,
    query: &str,
    format: SqlFormat,
) -> FileBinding {
    let disk_sql = (path.is_some() && !file_dirty).then(|| query.to_string());
    FileBinding {
        path,
        disk_sql,
        format,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The format of an ordinary LF, no-BOM, valid-UTF-8 file.
    fn plain() -> SqlFormat {
        SqlFormat::default()
    }

    // ── What a restored tab knows about its file ─────────────────────────────

    #[test]
    fn a_clean_file_tab_restores_knowing_its_on_disk_text() {
        let b = restored_binding(Some(PathBuf::from("/w/a.sql")), false, "SELECT 1;", plain());
        assert_eq!(b.disk_sql.as_deref(), Some("SELECT 1;"));
        assert!(b.is_bound());
    }

    /// **The one that loses work if it goes the other way.** A tab saved with
    /// unsaved edits must come back *not* knowing its on-disk text, or it reads
    /// as clean: no modified marker, and Ctrl+S does nothing.
    #[test]
    fn a_dirty_file_tab_restores_knowing_nothing() {
        let b = restored_binding(
            Some(PathBuf::from("/w/a.sql")),
            true,
            "SELECT 2; -- edited",
            plain(),
        );
        assert_eq!(b.disk_sql, None);
        assert!(b.is_bound(), "it is still that file's tab");
    }

    #[test]
    fn a_tab_with_no_file_has_no_on_disk_text_either_way() {
        for dirty in [false, true] {
            let b = restored_binding(None, dirty, "SELECT 3;", plain());
            assert_eq!(b.disk_sql, None, "dirty = {dirty}");
            assert!(!b.is_bound());
        }
    }

    /// The byte shape rides along untouched — it is what the file *was*, not
    /// something derived from the text.
    #[test]
    fn the_restored_binding_carries_the_byte_shape_as_it_was() {
        let crlf_bom = SqlFormat {
            crlf: true,
            bom: true,
            lossy: false,
        };
        let b = restored_binding(Some(PathBuf::from("/w/a.sql")), false, "x", crlf_bom);
        assert_eq!(b.format, crlf_bom);
    }

    /// Shedding the binding must shed **all** of it: a cleared tab that kept its
    /// path is one Ctrl+S from overwriting that file with an empty document, and
    /// one that kept its format writes a fresh script with a BOM and CRLF it
    /// never had.
    #[test]
    fn a_shed_binding_keeps_nothing() {
        let b = FileBinding::none();
        assert_eq!(b.path, None);
        assert_eq!(b.disk_sql, None);
        assert_eq!(b.format, SqlFormat::default());
        assert!(!b.is_bound());
    }

    #[test]
    fn open_verdict_covers_the_three_bands() {
        use OpenVerdict::*;
        assert_eq!(open_verdict(0), Open, "an empty file");
        assert_eq!(open_verdict(47_000), Open, "an ordinary script");
        assert_eq!(
            open_verdict(OPEN_WARN_BYTES),
            Open,
            "the bound is inclusive"
        );
        assert_eq!(
            open_verdict(OPEN_WARN_BYTES + 1),
            Confirm(OPEN_WARN_BYTES + 1)
        );
        assert_eq!(open_verdict(16 << 20), Confirm(16 << 20));
        assert_eq!(
            open_verdict(OPEN_REFUSE_BYTES),
            Confirm(OPEN_REFUSE_BYTES),
            "the refusal bound is inclusive too"
        );
        assert_eq!(
            open_verdict(OPEN_REFUSE_BYTES + 1),
            Refuse(OPEN_REFUSE_BYTES + 1)
        );
        assert_eq!(open_verdict(u64::MAX), Refuse(u64::MAX));
    }

    /// The bands have to be in order, or one of them is unreachable.
    #[test]
    fn the_thresholds_are_ordered() {
        const { assert!(OPEN_WARN_BYTES < OPEN_REFUSE_BYTES) };
    }

    #[test]
    fn decode_strips_a_bom_and_keeps_lf() {
        let got = decode(b"\xEF\xBB\xBFSELECT 1;\nSELECT 2;\n");
        assert_eq!(got.text, "SELECT 1;\nSELECT 2;\n");
        assert!(!got.format.crlf);
        assert!(got.format.bom, "and it remembers there was one");
    }

    #[test]
    fn decode_normalises_crlf_and_remembers_it() {
        let got = decode(b"SELECT 1;\r\nSELECT 2;\r\n");
        assert_eq!(got.text, "SELECT 1;\nSELECT 2;\n");
        assert!(got.format.crlf);
    }

    #[test]
    fn decode_of_empty_bytes_is_empty_and_lf() {
        assert_eq!(
            decode(b""),
            SqlText {
                text: String::new(),
                format: plain(),
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

    /// **The fact a Save has to know.** A lossy read has no inverse: writing the
    /// decoded text back replaces every unreadable byte in the file — including
    /// in the lines the user never touched — permanently. A Latin-1 `mysqldump`
    /// is the ordinary shape of this.
    #[test]
    fn decode_reports_whether_it_had_to_substitute() {
        assert!(decode(b"-- caf\xE9\nSELECT 1;\n").format.lossy, "latin-1");
        assert!(decode(b"-- don\x92t\n").format.lossy, "cp-1252 apostrophe");
        assert!(
            decode(b"\xFF\xFES\0E\0").format.lossy,
            "utf-16le with a BOM"
        );
        assert!(!decode("-- café\nSELECT 1;\n".as_bytes()).format.lossy);
        assert!(!decode(b"").format.lossy);
        assert!(
            !decode(b"\xEF\xBB\xBFSELECT 1;").format.lossy,
            "a BOM is not loss"
        );
    }

    /// And it really is one-way — the assertion the round-trip test below cannot
    /// make, and the reason the flag exists rather than a wider `encode`.
    #[test]
    fn a_lossy_decode_does_not_round_trip() {
        let raw = &b"-- caf\xE9\nSELECT 1;\n"[..];
        let d = decode(raw);
        assert!(d.format.lossy);
        assert_ne!(
            encode(&d.text, d.format).as_bytes(),
            raw,
            "if this ever passes, the flag can go"
        );
    }

    #[test]
    fn a_lone_cr_is_not_a_line_ending() {
        // An old-Mac `\r` (or a `\r` inside a string literal) is left exactly as
        // it is: it isn't `\r\n`, so nothing collapses and nothing is claimed.
        let got = decode(b"SELECT '\ra';");
        assert_eq!(got.text, "SELECT '\ra';");
        assert!(!got.format.crlf);
    }

    #[test]
    fn mixed_endings_resolve_to_the_majority() {
        assert!(
            decode(b"a\r\nb\r\nc\nd").format.crlf,
            "two CRLF against one LF"
        );
        assert!(
            !decode(b"a\nb\nc\r\nd").format.crlf,
            "two LF against one CRLF"
        );
        // A tie is CRLF: normalising the odd lone LF is the smaller change.
        assert!(decode(b"a\r\nb\nc").format.crlf);
    }

    #[test]
    fn encode_round_trips_every_readable_shape() {
        for raw in [
            &b"SELECT 1;\r\nSELECT 2;\r\n"[..],
            &b"SELECT 1;\nSELECT 2;\n"[..],
            // The BOM was stripped on open and never written back, so a Save of
            // an untouched file used to shrink it by three bytes.
            &b"\xEF\xBB\xBFSELECT 1;\n"[..],
            &b"\xEF\xBB\xBFSELECT 1;\r\n"[..],
            &b"\xEF\xBB\xBF"[..],
            "-- café\nSELECT 1;\n".as_bytes(),
        ] {
            let d = decode(raw);
            assert_eq!(
                encode(&d.text, d.format).as_bytes(),
                raw,
                "decode → encode must be the identity on a file we opened"
            );
        }
    }

    #[test]
    fn encode_does_not_double_a_crlf_already_in_the_buffer() {
        assert_eq!(
            encode(
                "a\r\nb\nc",
                SqlFormat {
                    crlf: true,
                    ..plain()
                }
            ),
            "a\r\nb\r\nc"
        );
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

    /// The same path spelled two ways is one file on both readings — a `.`
    /// component and a doubled separator are not differences.
    #[test]
    fn a_path_is_the_same_file_as_itself_however_it_is_spelled() {
        for ignore_case in [true, false] {
            assert!(same_path(
                Path::new("/tmp/orders.sql"),
                Path::new("/tmp/orders.sql"),
                ignore_case
            ));
            assert!(same_path(
                Path::new("/tmp/./orders.sql"),
                Path::new("/tmp/orders.sql"),
                ignore_case
            ));
            assert!(same_path(
                Path::new("/tmp//orders.sql"),
                Path::new("/tmp/orders.sql"),
                ignore_case
            ));
        }
    }

    /// **The case that lost the edit.** On Windows and macOS `ORDERS.SQL` is the
    /// file `orders.sql`; on Linux it is a different file, and merging them would
    /// be the worse mistake. Both answers are asserted here so the platform's is
    /// a choice rather than an accident.
    #[test]
    fn case_is_a_difference_only_where_the_filesystem_says_so() {
        let a = Path::new("/sql/orders.sql");
        let b = Path::new("/sql/ORDERS.SQL");
        assert!(same_path(a, b, true));
        assert!(!same_path(a, b, false));
        assert_eq!(same_file(a, b), PATHS_IGNORE_CASE);
    }

    /// Two genuinely different files stay different on either reading — the
    /// folding must not go so far as to merge names that only look alike.
    #[test]
    fn different_files_are_never_the_same_file() {
        for ignore_case in [true, false] {
            assert!(!same_path(
                Path::new("/sql/orders.sql"),
                Path::new("/sql/orders2.sql"),
                ignore_case
            ));
            assert!(!same_path(
                Path::new("/sql/a/orders.sql"),
                Path::new("/sql/b/orders.sql"),
                ignore_case
            ));
            // A relative path is not the absolute one it may resolve to: this
            // function answers about paths, and the caller canonicalises first.
            assert!(!same_path(
                Path::new("orders.sql"),
                Path::new("/sql/orders.sql"),
                ignore_case
            ));
        }
    }
}
