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
        sql_literal(&Value::Str(path.to_string()), D)
    )
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

    #[test]
    fn the_tab_selects_from_the_view_quoting_only_when_needed() {
        assert_eq!(select_sql("sales"), "SELECT * FROM sales");
        assert_eq!(select_sql("Sales 2026"), "SELECT * FROM \"Sales 2026\"");
    }
}
