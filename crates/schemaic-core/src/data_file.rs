//! **Open data file**: a CSV, Parquet or JSON file read through a view in a
//! throwaway in-memory DuckDB session.
//!
//! The file stays where it is and stays the data: the session holds a view
//! over DuckDB's own reader for it (`read_csv`, `read_parquet`, `read_json`),
//! not a copy, so opening a large file costs nothing until it is queried and
//! nothing written to the session reaches the file. This module decides the
//! words — which reader a path takes, the view's name and its `CREATE` — and
//! `db::duckdb::DataSession` holds the database they run in.

use crate::export::{ident_if_needed, ident_sql, sql_literal};
use crate::intel::SqlDialect;
use crate::model::Value;

const D: SqlDialect = SqlDialect::DuckDb;

/// The kind of data file, which picks DuckDB's reader for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFileKind {
    /// Delimited text — DuckDB's sniffer finds the delimiter, so a `.tsv` is
    /// one too.
    Csv,
    Parquet,
    /// A JSON array or newline-delimited JSON; `read_json` detects which.
    Json,
}

impl DataFileKind {
    /// DuckDB's table function for the kind.
    pub fn reader(self) -> &'static str {
        match self {
            DataFileKind::Csv => "read_csv",
            DataFileKind::Parquet => "read_parquet",
            DataFileKind::Json => "read_json",
        }
    }
}

/// The extensions the file dialog offers. A dialog filter matches the *last*
/// extension alone, so the compressed text kinds are offered by their
/// compression suffix and [`kind_of`] has the last word on what opens.
pub const EXTENSIONS: &[&str] = &[
    "csv", "tsv", "parquet", "json", "jsonl", "ndjson", "gz", "zst",
];

/// Compression suffixes DuckDB's text readers undo themselves (`x.csv.gz`).
const COMPRESSED: &[&str] = &["gz", "zst"];

/// The file name of `path`, either separator.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// `path`'s stem and kind: the file name split from its data extension, with
/// a compression suffix taken off first.
fn split(path: &str) -> Option<(&str, DataFileKind)> {
    let name = file_name(path);
    let (rest, compressed) = match name.rsplit_once('.') {
        Some((rest, ext)) if COMPRESSED.iter().any(|c| ext.eq_ignore_ascii_case(c)) => (rest, true),
        _ => (name, false),
    };
    let (stem, ext) = rest.rsplit_once('.')?;
    let ext = ext.to_ascii_lowercase();
    let kind = match ext.as_str() {
        "csv" | "tsv" => DataFileKind::Csv,
        "json" | "jsonl" | "ndjson" => DataFileKind::Json,
        "parquet" if !compressed => DataFileKind::Parquet,
        _ => return None,
    };
    Some((stem, kind))
}

/// The reader `path` takes, by its extension — or `None` for a file this
/// does not open. A compression suffix is looked through for the text kinds
/// alone: Parquet compresses inside the file.
pub fn kind_of(path: &str) -> Option<DataFileKind> {
    split(path).map(|(_, kind)| kind)
}

/// The view's name for `path`: its file name without the data and
/// compression extensions, made distinct from `taken` (compared without case,
/// as DuckDB resolves a name) by a `_2`, `_3`… suffix.
pub fn view_name(path: &str, taken: &[String]) -> String {
    let stem = match split(path) {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => "data",
    };
    let free = |n: &str| !taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    if free(stem) {
        return stem.to_string();
    }
    (2..)
        .map(|i| format!("{stem}_{i}"))
        .find(|n| free(n))
        .expect("an unbounded range finds a free name")
}

/// The `CREATE VIEW` over `path` read as `kind`.
pub fn create_view_sql(name: &str, path: &str, kind: DataFileKind) -> String {
    format!(
        "CREATE VIEW {} AS SELECT * FROM {}({})",
        ident_sql(name, D),
        kind.reader(),
        path_literal(path)
    )
}

/// `path` as the literal a DuckDB reader reads **as that one file**.
///
/// The readers glob their argument before they try the literal file, so a
/// `[`, `*` or `?` in a file's name matched other files: `a[1].csv` read
/// `a1.csv`'s rows when both were there (probed on 2.0.0). Each is written as
/// a one-character class — `[[]`, `[*]`, `[?]` — which matches only itself.
fn path_literal(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            '[' | '*' | '?' => {
                escaped.push('[');
                escaped.push(c);
                escaped.push(']');
            }
            _ => escaped.push(c),
        }
    }
    sql_literal(&Value::Str(escaped), D)
}

/// [`path_literal`]'s escape undone: the file a reader's argument names, or
/// `None` when it is a glob — any `[`, `*` or `?` not in one of the three
/// one-character classes — which the options form would rewrite into a
/// literal read of a file of that name.
fn unescape_path(arg: &str) -> Option<String> {
    let mut out = String::with_capacity(arg.len());
    let mut rest = arg;
    while let Some(c) = rest.chars().next() {
        match c {
            '[' => {
                let mut it = rest.chars();
                it.next();
                let (Some(m), Some(']')) = (it.next(), it.next()) else {
                    return None;
                };
                if !matches!(m, '[' | '*' | '?') {
                    return None;
                }
                out.push(m);
                rest = &rest[1 + m.len_utf8() + 1..];
            }
            '*' | '?' => return None,
            _ => {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    Some(out)
}

/// The delimiter a CSV view reads with: DuckDB's sniffer's guess, or one of
/// the four a file is nearly always delimited by. A closed list rather than
/// free text, so no escaping question (a tab typed as `\t`) reaches the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CsvDelimiter {
    #[default]
    Auto,
    Comma,
    Semicolon,
    Tab,
    Pipe,
}

impl CsvDelimiter {
    pub const ALL: [CsvDelimiter; 5] = [
        CsvDelimiter::Auto,
        CsvDelimiter::Comma,
        CsvDelimiter::Semicolon,
        CsvDelimiter::Tab,
        CsvDelimiter::Pipe,
    ];

    /// The character, or `None` for the sniffer's choice.
    fn char(self) -> Option<char> {
        match self {
            CsvDelimiter::Auto => None,
            CsvDelimiter::Comma => Some(','),
            CsvDelimiter::Semicolon => Some(';'),
            CsvDelimiter::Tab => Some('\t'),
            CsvDelimiter::Pipe => Some('|'),
        }
    }

    fn of(s: &str) -> Option<CsvDelimiter> {
        let mut cs = s.chars();
        let c = cs.next()?;
        if cs.next().is_some() {
            return None;
        }
        CsvDelimiter::ALL.into_iter().find(|d| d.char() == Some(c))
    }
}

/// Whether a CSV view's first line is its column names: the sniffer's guess,
/// or said outright.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CsvHeader {
    #[default]
    Auto,
    Yes,
    No,
}

/// The read-time options of a CSV view that DuckDB's sniffer otherwise
/// guesses — what a file it guesses wrong needs said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CsvReadOptions {
    pub delimiter: CsvDelimiter,
    pub header: CsvHeader,
}

/// A CSV view as [`csv_view_of`] reads its body back: the file, and the
/// options it is read with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvView {
    pub path: String,
    pub options: CsvReadOptions,
}

/// The body of a view reading `path` as CSV with `options` — what an
/// **Open data file** view is replaced with when its options change. Only
/// the options set are written, so [`CsvReadOptions::default`] gives the
/// body [`create_view_sql`] opens with.
pub fn csv_view_body(path: &str, options: CsvReadOptions) -> String {
    let lit = |s: &str| sql_literal(&Value::Str(s.to_string()), D);
    let mut args = vec![path_literal(path)];
    if let Some(c) = options.delimiter.char() {
        args.push(format!("delim = {}", lit(&c.to_string())));
    }
    match options.header {
        CsvHeader::Auto => {}
        CsvHeader::Yes => args.push("header = true".into()),
        CsvHeader::No => args.push("header = false".into()),
    }
    format!("SELECT * FROM read_csv({})", args.join(", "))
}

/// The file and options of a view whose body is `SELECT * FROM
/// read_csv('path', …)` — as [`csv_view_body`] writes it, or as DuckDB
/// re-prints it (`(delim = ';'), ("header" = false)`, measured on 2.0.0).
///
/// `read_csv_auto` is read too, and comes back from [`csv_view_body`] as
/// `read_csv`: DuckDB has made the one an alias of the other.
///
/// `None` for any other body, **and for one carrying an option this does not
/// model** (`columns = …`, a two-character delimiter, a second file, a glob
/// rather than [`path_literal`]'s escaped file name): the
/// options form writes the whole call again, so offering it there would drop
/// what the user wrote by hand.
pub fn csv_view_of(body: &str) -> Option<CsvView> {
    use crate::intel::TableFunctionArg as A;
    let (f, args) = crate::intel::star_from_table_function(body, D)?;
    if !f.eq_ignore_ascii_case("read_csv") && !f.eq_ignore_ascii_case("read_csv_auto") {
        return None;
    }
    let mut args = args.into_iter();
    let Some(A::Text(path)) = args.next() else {
        return None;
    };
    let mut options = CsvReadOptions::default();
    for a in args {
        match a {
            A::NamedText(k, v)
                if k.eq_ignore_ascii_case("delim") || k.eq_ignore_ascii_case("sep") =>
            {
                options.delimiter = CsvDelimiter::of(&v)?;
            }
            A::NamedBool(k, b) if k.eq_ignore_ascii_case("header") => {
                options.header = if b { CsvHeader::Yes } else { CsvHeader::No };
            }
            _ => return None,
        }
    }
    Some(CsvView {
        path: unescape_path(&path)?,
        options,
    })
}

/// The query the session's tab opens with.
pub fn select_sql(name: &str) -> String {
    format!("SELECT * FROM {}", ident_if_needed(name, D))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reader_follows_the_extension_without_case() {
        assert_eq!(kind_of("C:\\data\\sales.CSV"), Some(DataFileKind::Csv));
        assert_eq!(kind_of("/d/x.tsv"), Some(DataFileKind::Csv));
        assert_eq!(kind_of("/d/x.parquet"), Some(DataFileKind::Parquet));
        assert_eq!(kind_of("/d/x.json"), Some(DataFileKind::Json));
        assert_eq!(kind_of("/d/x.jsonl"), Some(DataFileKind::Json));
        assert_eq!(kind_of("/d/x.ndjson"), Some(DataFileKind::Json));
    }

    #[test]
    fn compression_is_looked_through_for_text_alone() {
        assert_eq!(kind_of("/d/x.csv.gz"), Some(DataFileKind::Csv));
        assert_eq!(kind_of("/d/x.json.zst"), Some(DataFileKind::Json));
        assert_eq!(kind_of("/d/x.parquet.gz"), None);
        assert_eq!(kind_of("/d/x.gz"), None);
    }

    #[test]
    fn other_files_are_not_opened() {
        for p in ["/d/x.duckdb", "/d/x", "", "/d/.csv/", "/d/csv"] {
            assert_eq!(kind_of(p), None, "{p}");
        }
    }

    #[test]
    fn a_directory_named_like_a_file_does_not_count() {
        assert_eq!(kind_of("/d/x.csv/y.txt"), None);
        assert_eq!(kind_of("C:\\a.parquet\\y"), None);
    }

    #[test]
    fn the_view_is_named_for_the_file() {
        assert_eq!(view_name("C:\\data\\Sales 2026.csv.gz", &[]), "Sales 2026");
        assert_eq!(view_name("/d/events.ndjson", &[]), "events");
        assert_eq!(view_name("/d/a.b.parquet", &[]), "a.b");
    }

    #[test]
    fn a_taken_name_gets_a_suffix_compared_without_case() {
        let taken = vec!["sales".to_string(), "SALES_2".to_string()];
        assert_eq!(view_name("/d/Sales.csv", &taken), "Sales_3");
        assert_eq!(view_name("/d/other.csv", &taken), "other");
    }

    #[test]
    fn a_file_with_no_stem_still_gets_a_name() {
        assert_eq!(view_name("/d/.csv", &[]), "data");
    }

    #[test]
    fn the_view_reads_the_file_with_its_path_as_a_literal() {
        assert_eq!(
            create_view_sql("sales", "C:\\data\\o'brien.csv", DataFileKind::Csv),
            "CREATE VIEW \"sales\" AS SELECT * FROM read_csv('C:\\data\\o''brien.csv')"
        );
        assert_eq!(
            create_view_sql("a\"b", "/d/x.parquet", DataFileKind::Parquet),
            "CREATE VIEW \"a\"\"b\" AS SELECT * FROM read_parquet('/d/x.parquet')"
        );
    }

    /// Only the options set are written; the defaults give the body a file
    /// opens with.
    #[test]
    fn a_csv_body_writes_the_options_set() {
        let o = |delimiter, header| CsvReadOptions { delimiter, header };
        assert_eq!(
            csv_view_body("C:\\d\\o'b.csv", CsvReadOptions::default()),
            "SELECT * FROM read_csv('C:\\d\\o''b.csv')"
        );
        assert!(
            create_view_sql("x", "/d/x.csv", DataFileKind::Csv)
                .ends_with(&csv_view_body("/d/x.csv", CsvReadOptions::default()))
        );
        assert_eq!(
            csv_view_body("/d/x.csv", o(CsvDelimiter::Semicolon, CsvHeader::No)),
            "SELECT * FROM read_csv('/d/x.csv', delim = ';', header = false)"
        );
        assert_eq!(
            csv_view_body("/d/x.csv", o(CsvDelimiter::Tab, CsvHeader::Yes)),
            "SELECT * FROM read_csv('/d/x.csv', delim = '\t', header = true)"
        );
    }

    /// What is written reads back, and so does DuckDB's re-printing of it.
    #[test]
    fn a_csv_body_reads_back_written_or_reprinted() {
        for delimiter in CsvDelimiter::ALL {
            for header in [CsvHeader::Auto, CsvHeader::Yes, CsvHeader::No] {
                let options = CsvReadOptions { delimiter, header };
                assert_eq!(
                    csv_view_of(&csv_view_body("C:/d/x.csv", options)),
                    Some(CsvView {
                        path: "C:/d/x.csv".into(),
                        options
                    }),
                    "{options:?}"
                );
            }
        }
        assert_eq!(
            csv_view_of("SELECT * FROM read_csv('/d/x.csv', (delim = '|'), (\"header\" = false))"),
            Some(CsvView {
                path: "/d/x.csv".into(),
                options: CsvReadOptions {
                    delimiter: CsvDelimiter::Pipe,
                    header: CsvHeader::No
                }
            })
        );
    }

    /// Any other body is not a CSV view the form can rewrite — another
    /// reader, a filter, an alias, an option it does not model.
    #[test]
    fn a_body_the_form_would_rewrite_lossily_is_not_read() {
        for body in [
            "SELECT * FROM read_parquet('/d/x.parquet')",
            "SELECT * FROM read_csv('/d/x.csv') WHERE a > 1",
            "SELECT a FROM read_csv('/d/x.csv')",
            "SELECT * FROM read_csv('/d/x.csv') AS t",
            "SELECT * FROM read_csv('/d/x.csv', columns = {'a': 'INT'})",
            "SELECT * FROM read_csv('/d/x.csv', delim = '::')",
            "SELECT * FROM read_csv(['/d/a.csv', '/d/b.csv'])",
            // A sample or an ordinality prints inside the source, so the
            // whole statement's print-back still matched.
            "SELECT * FROM read_csv('/d/x.csv') TABLESAMPLE System(50.0 PERCENT)",
            "SELECT * FROM read_csv('/d/x.csv') WITH ORDINALITY",
            "SELECT * FROM t",
            "not sql",
        ] {
            assert_eq!(csv_view_of(body), None, "{body}");
        }
    }

    /// **A file name is not a glob.** DuckDB's readers glob their argument
    /// before trying the literal file, so `a[1].csv` read `a1.csv`'s rows
    /// (probed on 2.0.0) and, where `*` and `?` are legal, `sales*.csv` read
    /// every sales file. Each metacharacter is written as a one-character
    /// class, which DuckDB reads as the character itself.
    #[test]
    fn glob_characters_in_the_path_are_read_literally() {
        assert_eq!(
            create_view_sql("a", "C:\\d\\a[1].csv", DataFileKind::Csv),
            "CREATE VIEW \"a\" AS SELECT * FROM read_csv('C:\\d\\a[[]1].csv')"
        );
        assert_eq!(
            create_view_sql("s", "/d/sales*?.parquet", DataFileKind::Parquet),
            "CREATE VIEW \"s\" AS SELECT * FROM read_parquet('/d/sales[*][?].parquet')"
        );
        assert_eq!(
            csv_view_body("/d/Report [Q1].csv", CsvReadOptions::default()),
            "SELECT * FROM read_csv('/d/Report [[]Q1].csv')"
        );
    }

    /// The escape round-trips through the options form, and a body that
    /// globs on purpose is a read the form would rewrite into a literal one,
    /// so it is not offered.
    #[test]
    fn an_escaped_path_reads_back_and_a_real_glob_is_not_offered() {
        let path = "C:\\d\\a[1]*?.csv";
        assert_eq!(
            csv_view_of(&csv_view_body(path, CsvReadOptions::default())),
            Some(CsvView {
                path: path.into(),
                options: CsvReadOptions::default()
            })
        );
        for body in [
            "SELECT * FROM read_csv('/d/*.csv')",
            "SELECT * FROM read_csv('/d/a?.csv')",
            "SELECT * FROM read_csv('/d/a[12].csv')",
            "SELECT * FROM read_csv('/d/a[.csv')",
        ] {
            assert_eq!(csv_view_of(body), None, "{body}");
        }
    }

    #[test]
    fn the_tab_selects_from_the_view_quoting_only_when_needed() {
        assert_eq!(select_sql("sales"), "SELECT * FROM sales");
        assert_eq!(select_sql("Sales 2026"), "SELECT * FROM \"Sales 2026\"");
    }

    /// **A file's stem can be a word DuckDB will not take as a bare name**
    /// though it reserves nothing: `semi.csv` opened a tab whose
    /// `SELECT * FROM semi` was a parser error (`SEMI JOIN`). `map` is a
    /// keyword DuckDB still takes as a table name.
    #[test]
    fn a_stem_duckdb_reads_as_a_join_keyword_is_quoted() {
        assert_eq!(select_sql("semi"), "SELECT * FROM \"semi\"");
        assert_eq!(select_sql("left"), "SELECT * FROM \"left\"");
        assert_eq!(select_sql("map"), "SELECT * FROM map");
    }
}
