//! How a result reaches stdout.
//!
//! **Every emitter here is `core::export`'s.** The rule the app already follows
//! for its export menu holds for the CLI too: one renderer per format, so a
//! duplicate column name, an embedded newline or a withheld blob reads the same
//! whether it left through the GUI or through a pipe. Nothing in this module
//! formats a cell; it chooses which of core's emitters to call and what to say
//! about truncation.

use std::fmt;
use std::str::FromStr;

use schemaic_core::export;
use schemaic_core::model::ResultSet;

/// What `--format` accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// A Markdown pipe table. The default, and the only format that reports
    /// truncation in-band, because it is the only one a person reads directly.
    #[default]
    Table,
    /// A pretty-printed JSON array of row objects — byte-for-byte what
    /// Schemaic's own JSON export writes, so the app can read it back.
    Json,
    /// One compact JSON object per line.
    Jsonl,
    /// RFC 4180 CSV with a header row.
    Csv,
}

impl Format {
    /// Every accepted spelling, for the usage text and the parse error.
    pub const NAMES: [&'static str; 4] = ["table", "json", "jsonl", "csv"];

    /// Can this format say "there were more rows" inside its own output?
    ///
    /// Only `table` can. The machine formats are deliberately *pure data* —
    /// a JSON array with a stray metadata object in it, or a CSV with a comment
    /// row, is worse for every consumer than a clean stream plus a line on
    /// stderr. See [`truncation_warning`].
    fn reports_truncation_in_band(self) -> bool {
        matches!(self, Format::Table)
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Format::Table => "table",
            Format::Json => "json",
            Format::Jsonl => "jsonl",
            Format::Csv => "csv",
        };
        f.write_str(s)
    }
}

impl FromStr for Format {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "table" => Ok(Format::Table),
            "json" => Ok(Format::Json),
            "jsonl" | "ndjson" => Ok(Format::Jsonl),
            "csv" => Ok(Format::Csv),
            other => Err(format!(
                "unknown format '{other}'; expected one of {}",
                Format::NAMES.join(", ")
            )),
        }
    }
}

/// Every row, in the order the result carries them.
fn display_order(rs: &ResultSet) -> Vec<usize> {
    (0..rs.row_count()).collect()
}

/// The rows, as `format` writes them. This is the whole of stdout for a query.
pub fn render_rows(rs: &ResultSet, format: Format) -> String {
    let order = display_order(rs);
    match format {
        Format::Table => {
            let mut out = export::export_markdown(rs, &order);
            if let Some(note) = row_count_note(rs) {
                out.push_str(&note);
            }
            out
        }
        Format::Json => newline_terminated(export::export_json(rs, &order)),
        Format::Jsonl => export::export_jsonl(rs, &order),
        Format::Csv => newline_terminated(export::export_csv(rs, &order)),
    }
}

/// End a non-empty rendering with a newline.
///
/// `core::export` writes for *files*, where a trailing newline is noise; a
/// terminal wants one, or the shell prompt lands on the last line of the data.
/// An empty rendering stays empty — a lone newline on stdout is not nothing,
/// and a reader counting lines would see a record that is not there.
fn newline_terminated(mut s: String) -> String {
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// The `table` footer: how many rows, and whether that is all of them.
fn row_count_note(rs: &ResultSet) -> Option<String> {
    let n = rs.row_count();
    let rows = if n == 1 { "row" } else { "rows" };
    Some(if rs.truncated {
        format!("\n({n} {rows}, capped — more were available)\n")
    } else {
        format!("\n({n} {rows})\n")
    })
}

/// What to tell the user on **stderr** about a capped result, if the format
/// could not say it in-band.
///
/// `None` when the result is complete, or when the format already said so.
///
/// **This is not cosmetic.** A caller — very often an agent — that reads 200
/// rows off stdout and concludes the table has 200 rows in it has been given a
/// wrong answer by a command that succeeded. The cap is named in the message so
/// the fix is in the reader's hands.
pub fn truncation_warning(rs: &ResultSet, format: Format) -> Option<String> {
    if !rs.truncated || format.reports_truncation_in_band() {
        return None;
    }
    Some(format!(
        "warning: only the first {} rows were read; the result was capped. \
         Raise it with --limit, or narrow the query.",
        rs.row_count()
    ))
}

/// What `exec` says on **stderr** when a statement returned more rows than it
/// prints — `UPDATE … RETURNING`, a `CALL` that selects.
///
/// Every format, the table's included: its in-band footer says "more were
/// available", which is true and still misleading, because what a reader needs
/// to know about a write is that the cap applied to the *display* and not to the
/// statement. And no `--limit` advice — `exec` has no such flag.
pub fn exec_truncation_warning(rs: &ResultSet) -> Option<String> {
    rs.truncated.then(|| {
        format!(
            "warning: only the first {} returned rows are shown; the statement itself ran in full.",
            rs.row_count()
        )
    })
}

/// What a write reports: how many rows it changed.
///
/// A purpose-built shape rather than a row renderer, because there are no rows
/// — `export`'s emitters all describe a result set, and a statement that
/// returned none has nothing for them to describe.
pub fn render_affected(affected: u64, format: Format) -> String {
    match format {
        Format::Table => format!("({affected} rows affected)\n"),
        Format::Json => format!("{{\n  \"affected\": {affected}\n}}\n"),
        Format::Jsonl => format!("{{\"affected\":{affected}}}\n"),
        Format::Csv => format!("affected\n{affected}\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemaic_core::model::{Column, ResultSet, Value};

    fn col(name: &str) -> Column {
        Column {
            name: name.to_string(),
            type_name: "VARCHAR".to_string(),
            origin: None,
        }
    }

    fn rs() -> ResultSet {
        ResultSet::from_rows(
            vec![col("id"), col("name")],
            vec![
                vec![Value::Int(1), Value::Str("a".to_string())],
                vec![Value::Int(2), Value::Str("b".to_string())],
            ],
        )
    }

    #[test]
    fn every_name_in_the_usage_text_actually_parses() {
        for name in Format::NAMES {
            let f: Format = name.parse().expect("a listed name parses");
            assert_eq!(f.to_string(), name, "Display must round-trip FromStr");
        }
    }

    #[test]
    fn the_format_name_is_case_insensitive_and_trimmed() {
        assert_eq!("  JSON ".parse::<Format>().unwrap(), Format::Json);
    }

    /// The error has to name the alternatives; a bare "unknown format" sends
    /// the reader to the help text for something the message could have said.
    #[test]
    fn an_unknown_format_is_refused_and_lists_the_real_ones() {
        let err = "yaml".parse::<Format>().unwrap_err();
        assert!(err.contains("yaml"));
        for name in Format::NAMES {
            assert!(err.contains(name), "the error must list {name}");
        }
    }

    #[test]
    fn the_default_is_the_human_one() {
        assert_eq!(Format::default(), Format::Table);
    }

    #[test]
    fn json_rows_parse_and_carry_every_row() {
        let out = render_rows(&rs(), Format::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 2);
        assert_eq!(v[1]["name"], "b");
    }

    #[test]
    fn jsonl_rows_are_one_line_each() {
        let out = render_rows(&rs(), Format::Jsonl);
        assert_eq!(out.lines().count(), 2);
    }

    #[test]
    fn csv_leads_with_a_header_row() {
        let out = render_rows(&rs(), Format::Csv);
        assert!(out.lines().next().unwrap().contains("id"));
        assert_eq!(out.lines().count(), 3, "header plus two rows");
    }

    #[test]
    fn the_table_form_footers_its_row_count() {
        let out = render_rows(&rs(), Format::Table);
        assert!(out.contains("(2 rows)"));
    }

    /// One row is a row. A footer reading "(1 rows)" is the kind of thing a
    /// person notices every single time they run the command.
    #[test]
    fn a_single_row_is_not_plural() {
        let one = ResultSet::from_rows(vec![col("id")], vec![vec![Value::Int(1)]]);
        assert!(render_rows(&one, Format::Table).contains("(1 row)"));
    }

    /// **Every format ends with a newline, or the next shell prompt lands on
    /// the last line of the data.** `export_json` has no trailing newline of
    /// its own — it is written for files, where one is noise.
    #[test]
    fn every_non_empty_rendering_ends_with_a_newline() {
        for name in Format::NAMES {
            let format: Format = name.parse().unwrap();
            let out = render_rows(&rs(), format);
            assert!(out.ends_with('\n'), "{format} must end with a newline");
        }
        assert!(render_affected(1, Format::Json).ends_with('\n'));
    }

    /// **A capped result must say so, in exactly one place.** The table says it
    /// in the footer and must not also warn on stderr; the machine formats say
    /// nothing in-band and must warn.
    #[test]
    fn a_capped_result_is_reported_in_band_for_table_and_on_stderr_otherwise() {
        let mut capped = rs();
        capped.truncated = true;

        let table = render_rows(&capped, Format::Table);
        assert!(table.contains("capped"), "the footer carries it");
        assert!(
            truncation_warning(&capped, Format::Table).is_none(),
            "and so must not be repeated on stderr"
        );

        for format in [Format::Json, Format::Jsonl, Format::Csv] {
            let warning = truncation_warning(&capped, format)
                .unwrap_or_else(|| panic!("{format} must warn about a cap"));
            assert!(warning.contains("--limit"), "and must say how to raise it");
        }
    }

    /// The warning is about a cap, not about every result.
    #[test]
    fn a_complete_result_never_warns_in_any_format() {
        for name in Format::NAMES {
            let format: Format = name.parse().unwrap();
            assert!(truncation_warning(&rs(), format).is_none());
        }
    }

    /// Machine output stays pure data even when capped — a JSON array with a
    /// metadata object appended is worse for a consumer than a clean array.
    #[test]
    fn a_capped_machine_format_still_emits_only_rows() {
        let mut capped = rs();
        capped.truncated = true;
        let out = render_rows(&capped, Format::Json);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 2, "rows only, no envelope");
        assert!(!render_rows(&capped, Format::Csv).contains("capped"));
    }

    /// **An `exec` that returned more rows than it shows must say so — and must
    /// say the statement ran in full.** `UPDATE … RETURNING` over a thousand
    /// rows printed one of them and nothing else, which reads as a one-row
    /// update. `exec` has no `--limit`, so the query's advice to raise it would
    /// point at a flag that isn't there.
    #[test]
    fn a_capped_exec_result_warns_that_the_statement_still_ran_in_full() {
        let mut capped = rs();
        capped.truncated = true;
        let warning = exec_truncation_warning(&capped).expect("a capped exec warns");
        assert!(warning.contains("first 2"), "{warning}");
        assert!(warning.contains("ran in full"), "{warning}");
        assert!(!warning.contains("--limit"), "{warning}");
        assert!(exec_truncation_warning(&rs()).is_none());
    }

    #[test]
    fn a_write_reports_its_row_count_in_every_format() {
        assert!(render_affected(3, Format::Table).contains("3 rows affected"));
        let json: serde_json::Value =
            serde_json::from_str(&render_affected(3, Format::Json)).unwrap();
        assert_eq!(json["affected"], 3);
        let line: serde_json::Value =
            serde_json::from_str(render_affected(3, Format::Jsonl).trim()).unwrap();
        assert_eq!(line["affected"], 3);
        assert_eq!(render_affected(3, Format::Csv), "affected\n3\n");
    }

    #[test]
    fn an_empty_result_renders_without_panicking_in_every_format() {
        let empty = ResultSet::from_rows(vec![col("id")], vec![]);
        for name in Format::NAMES {
            let format: Format = name.parse().unwrap();
            let _ = render_rows(&empty, format);
        }
        assert_eq!(render_rows(&empty, Format::Jsonl), "");
    }
}
