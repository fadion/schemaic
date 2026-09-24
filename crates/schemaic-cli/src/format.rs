//! How a result reaches stdout.
//!
//! **Every emitter here is `core::export`'s.** The rule the app already follows
//! for its export menu holds for the CLI too: one renderer per format, so a
//! duplicate column name, an embedded newline or a withheld blob reads the same
//! whether it left through the GUI or through a pipe. Nothing in this module
//! formats a cell; it chooses which of core's emitters to call and what to say
//! about truncation and withheld bytes.
//!
//! **One deliberate difference: CSV here has no formula guard.** The file
//! export prefixes `'` to a value a spreadsheet would evaluate; a pipe's reader
//! is a program, and to it `'+15551234` is a different phone number.

use std::fmt;
use std::str::FromStr;

use schemaic_core::export;
use schemaic_core::model::ResultSet;

/// What `--format` accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// A Markdown pipe table. The default, and with `vertical` the formats
    /// that report truncation in-band, because they are the ones a person reads
    /// directly.
    #[default]
    Table,
    /// A pretty-printed JSON array of row objects — byte-for-byte what
    /// Schemaic's own JSON export writes, so the app can read it back.
    Json,
    /// One compact JSON object per line.
    Jsonl,
    /// RFC 4180 CSV with a header row.
    Csv,
    /// One `name: value` record per row, the `mysql` client's `\G` — for a
    /// row too wide for the table to be readable.
    Vertical,
}

impl Format {
    /// Every accepted spelling, for the usage text and the parse error.
    pub const NAMES: [&'static str; 5] = ["table", "json", "jsonl", "csv", "vertical"];

    /// Is this a format a person reads, rather than a program?
    ///
    /// `table` and `vertical`. They say "there were more rows" inside their
    /// own output, and show a blob as its `<n bytes>` placeholder, which says
    /// what it is. The machine formats are deliberately *pure data* — a JSON
    /// array with a stray metadata object in it, or a CSV with a comment row,
    /// is worse for every consumer than a clean stream plus a line on stderr.
    /// See [`truncation_warning`] and [`withheld_warning`].
    fn for_a_reader(self) -> bool {
        matches!(self, Format::Table | Format::Vertical)
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Format::Table => "table",
            Format::Json => "json",
            Format::Jsonl => "jsonl",
            Format::Csv => "csv",
            Format::Vertical => "vertical",
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
            "vertical" => Ok(Format::Vertical),
            other => Err(format!(
                "unknown format '{other}'; expected one of {}",
                Format::NAMES.join(", ")
            )),
        }
    }
}

/// How stdout is written: the format, and whether its name row leads.
///
/// A format alone converts into one with its header, which is what every
/// caller but `--no-header` means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Output {
    pub format: Format,
    /// The column names — and, for `table`, the `---` row and the row-count
    /// footer, which are the rest of what is not a row.
    pub header: bool,
}

impl From<Format> for Output {
    fn from(format: Format) -> Output {
        Output {
            format,
            header: true,
        }
    }
}

impl Output {
    /// `--format` with `--no-header`, or why the two do not go together.
    ///
    /// **Only `table` and `csv` have a header to leave out.** JSON names every
    /// value in every row and `vertical` every line; taking the flag there
    /// and changing nothing would be a promise the output does not keep.
    pub fn new(format: Format, no_header: bool) -> Result<Output, String> {
        if no_header && !matches!(format, Format::Table | Format::Csv) {
            return Err(format!(
                "--no-header applies to table and csv; {format} names the column \
                 beside every value, so it has no header to leave out"
            ));
        }
        Ok(Output {
            format,
            header: !no_header,
        })
    }

    /// Does the output itself say "there were more rows"? The table's footer
    /// does, and a header-less table has no footer.
    fn reports_truncation_in_band(self) -> bool {
        self.format.for_a_reader() && self.header
    }
}

/// Every row, in the order the result carries them.
fn display_order(rs: &ResultSet) -> Vec<usize> {
    (0..rs.row_count()).collect()
}

/// The rows, as `output` writes them. This is the whole of stdout for a query.
pub fn render_rows(rs: &ResultSet, output: impl Into<Output>) -> String {
    let Output { format, header } = output.into();
    let order = display_order(rs);
    match format {
        Format::Table => {
            // NULL spelled out: `''` is the empty cell, and the two printed
            // identically.
            let mut out = export::export_markdown_null_as(rs, &order, "NULL", header);
            if header && let Some(note) = row_count_note(rs) {
                out.push_str(&note);
            }
            out
        }
        Format::Json => newline_terminated(export::export_json(rs, &order)),
        Format::Jsonl => export::export_jsonl(rs, &order),
        Format::Csv => newline_terminated(export::export_csv_plain(rs, &order, header)),
        Format::Vertical => {
            let mut out = export::export_vertical(rs, &order, "NULL");
            if let Some(note) = row_count_note(rs) {
                // No records means no blank line to set the count off from.
                out.push_str(if out.is_empty() {
                    note.trim_start_matches('\n')
                } else {
                    &note
                });
            }
            out
        }
    }
}

/// What to tell the user on **stderr** about binary columns the format wrote
/// as `null` / an empty field, if any.
///
/// `table` and `vertical` show the `<n bytes>` placeholder, which says what it
/// is; the machine formats cannot, and a column of `null`s reads as "no data" —
/// a script asking which rows have an avatar concluded none did. The GUI's
/// export says the same thing after every file it writes.
pub fn withheld_warning(rs: &ResultSet, format: Format) -> Option<String> {
    if format.for_a_reader() {
        return None;
    }
    let cols = export::withheld_columns(rs, &display_order(rs));
    if cols.is_empty() {
        return None;
    }
    let written = if format == Format::Csv {
        "empty fields"
    } else {
        "null"
    };
    Some(format!(
        "warning: {} {} binary data this format cannot carry, written as {written}; \
         select it hex-encoded to get the bytes.",
        cols.join(", "),
        if cols.len() == 1 { "holds" } else { "hold" },
    ))
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
pub fn truncation_warning(rs: &ResultSet, output: impl Into<Output>) -> Option<String> {
    if !rs.truncated || output.into().reports_truncation_in_band() {
        return None;
    }
    Some(format!(
        "warning: only the {} read; the result was capped. \
         Raise it with --limit, or narrow the query.",
        first_rows(rs.row_count(), "row was", "rows were")
    ))
}

/// "first row was" or "first 5 rows were": `one` and `many` carry the verb,
/// since it agrees with the count too.
fn first_rows(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("first {one}")
    } else {
        format!("first {n} {many}")
    }
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
            "warning: only the {} shown; the statement itself ran in full.",
            first_rows(rs.row_count(), "returned row is", "returned rows are")
        )
    })
}

/// What a write reports: how many rows it changed.
///
/// A purpose-built shape rather than a row renderer, because there are no rows
/// — `export`'s emitters all describe a result set, and a statement that
/// returned none has nothing for them to describe.
///
/// Without a header it is the bare number, for `n=$(schemaic exec …)`.
pub fn render_affected(affected: u64, output: impl Into<Output>) -> String {
    let Output { format, header } = output.into();
    if !header {
        return format!("{affected}\n");
    }
    match format {
        Format::Table | Format::Vertical => format!(
            "({affected} {} affected)\n",
            if affected == 1 { "row" } else { "rows" }
        ),
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

    /// A NULL and an empty string — the same row, two different facts.
    fn null_and_empty() -> ResultSet {
        ResultSet::from_rows(
            vec![col("a"), col("b")],
            vec![vec![Value::Null, Value::Str(String::new())]],
        )
    }

    /// **The table says NULL, and leaves `''` empty** — the `mysql` client's
    /// spelling. They printed identically, so "which rows have no email"
    /// could not be answered by reading the table.
    #[test]
    fn the_table_tells_a_null_from_an_empty_string() {
        let out = render_rows(&null_and_empty(), Format::Table);
        let row = out.lines().nth(2).expect("a data row");
        assert_eq!(row, "| NULL |  |", "{out}");
    }

    /// **CSV quotes the empty string and leaves NULL bare** — PostgreSQL's own
    /// `COPY … CSV` convention, so a loader that follows it reads both back.
    #[test]
    fn csv_tells_a_null_from_an_empty_string() {
        let out = render_rows(&null_and_empty(), Format::Csv);
        assert_eq!(out, "a,b\n,\"\"\n", "{out:?}");
    }

    /// JSON always could: `null` against `""`.
    #[test]
    fn json_tells_a_null_from_an_empty_string() {
        let out = render_rows(&null_and_empty(), Format::Jsonl);
        assert_eq!(out.trim(), r#"{"a":null,"b":""}"#);
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

    /// **"The first 1 rows" is the plural the footer already lost** — both
    /// stderr warnings count a cap of one as a row, as `--limit 1` produces.
    #[test]
    fn a_cap_of_one_row_is_not_plural_on_stderr() {
        let mut one = ResultSet::from_rows(vec![col("id")], vec![vec![Value::Int(1)]]);
        one.truncated = true;
        let w = truncation_warning(&one, Format::Json).unwrap();
        assert!(w.contains("only the first row was read"), "{w}");
        let w = exec_truncation_warning(&one).unwrap();
        assert!(w.contains("only the first returned row is shown"), "{w}");
        let mut two = rs();
        two.truncated = true;
        let w = truncation_warning(&two, Format::Json).unwrap();
        assert!(w.contains("only the first 2 rows were read"), "{w}");
        let w = exec_truncation_warning(&two).unwrap();
        assert!(
            w.contains("only the first 2 returned rows are shown"),
            "{w}"
        );
    }

    /// One row is a row here too — `query`'s footer already said "(1 row)"
    /// while `exec` said "(1 rows affected)".
    #[test]
    fn a_single_affected_row_is_not_plural() {
        assert_eq!(render_affected(1, Format::Table), "(1 row affected)\n");
        assert_eq!(render_affected(0, Format::Table), "(0 rows affected)\n");
        assert_eq!(render_affected(2, Format::Table), "(2 rows affected)\n");
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

    /// A row holding every cell the formats treat differently: a NULL, an
    /// empty string, a withheld blob, a value that starts like a formula, and a
    /// column name used twice.
    fn lossy() -> ResultSet {
        let mut blob = col("avatar");
        blob.type_name = "BLOB".to_string();
        ResultSet::from_rows(
            vec![col("n"), col("e"), blob, col("phone"), col("n")],
            vec![vec![
                Value::Null,
                Value::Str(String::new()),
                Value::Str(schemaic_core::model::binary_display(2)),
                Value::Str("+15551234".to_string()),
                Value::Int(7),
            ]],
        )
    }

    /// **A blob the machine formats cannot carry is said on stderr, naming
    /// it.** It used to reach a pipe as `null` with nothing said, which reads
    /// as "no data".
    #[test]
    fn a_withheld_blob_is_warned_about_in_every_machine_format() {
        for format in [Format::Json, Format::Jsonl, Format::Csv] {
            let w = withheld_warning(&lossy(), format)
                .unwrap_or_else(|| panic!("{format} must warn about the blob"));
            assert!(w.contains("avatar"), "{w}");
        }
        // The table shows the placeholder, which already says what it is.
        assert!(withheld_warning(&lossy(), Format::Table).is_none());
        assert!(render_rows(&lossy(), Format::Table).contains("2 bytes"));
        assert!(withheld_warning(&rs(), Format::Json).is_none());
    }

    /// **CSV down a pipe is RFC 4180 and nothing else.** No apostrophe in
    /// front of a leading `+`, and the same value JSON gives for the cell.
    #[test]
    fn csv_writes_a_formula_like_value_as_it_is() {
        let csv = render_rows(&lossy(), Format::Csv);
        let row = csv.lines().nth(1).unwrap();
        assert!(row.contains(",+15551234,"), "{row}");
        let json: serde_json::Value =
            serde_json::from_str(&render_rows(&lossy(), Format::Json)).unwrap();
        assert_eq!(json[0]["phone"], "+15551234");
    }

    /// What the JSON shapes promise for the lossy cells, identically in both:
    /// NULL is `null`, `''` is `""`, a withheld blob is `null`, and a repeated
    /// name gets `_2` rather than overwriting the first.
    #[test]
    fn json_and_jsonl_render_the_lossy_row_identically() {
        let arr: serde_json::Value =
            serde_json::from_str(&render_rows(&lossy(), Format::Json)).unwrap();
        let line: serde_json::Value =
            serde_json::from_str(render_rows(&lossy(), Format::Jsonl).trim()).unwrap();
        assert_eq!(arr[0], line);
        assert!(line["n"].is_null());
        assert_eq!(line["e"], "");
        assert!(line["avatar"].is_null());
        assert_eq!(line["n_2"], 7);
    }

    fn headerless(format: Format) -> Output {
        Output::new(format, true).expect("a format that has a header")
    }

    /// **`--no-header` is the rows alone** — psql's `-t`: no name row, and for
    /// the table no `---` row and no footer either, so `while read` and `wc -l`
    /// count records.
    #[test]
    fn without_a_header_the_table_and_csv_are_the_rows_alone() {
        assert_eq!(
            render_rows(&rs(), headerless(Format::Table)),
            "| 1 | a |\n| 2 | b |\n"
        );
        assert_eq!(render_rows(&rs(), headerless(Format::Csv)), "1,a\n2,b\n");
        let empty = ResultSet::from_rows(vec![col("id")], vec![]);
        assert_eq!(render_rows(&empty, headerless(Format::Table)), "");
        assert_eq!(render_rows(&empty, headerless(Format::Csv)), "");
    }

    /// **The footer carried the cap; without it, stderr has to.** A header-less
    /// table that was cut short and said nothing is the wrong answer from a
    /// command that succeeded.
    #[test]
    fn a_headerless_table_reports_its_cap_on_stderr() {
        let mut capped = rs();
        capped.truncated = true;
        assert!(!render_rows(&capped, headerless(Format::Table)).contains("capped"));
        assert!(truncation_warning(&capped, headerless(Format::Table)).is_some());
        assert!(truncation_warning(&capped, Format::Table).is_none());
    }

    /// Only the formats with a name row have one to leave out. JSON names
    /// every value in every row and `vertical` every line; accepting the flag
    /// there would be a promise nothing keeps.
    #[test]
    fn no_header_is_refused_where_there_is_no_header() {
        for format in [Format::Json, Format::Jsonl, Format::Vertical] {
            let err = Output::new(format, true).unwrap_err();
            assert!(err.contains("--no-header"), "{err}");
            assert!(Output::new(format, false).is_ok());
        }
        for format in [Format::Table, Format::Csv] {
            assert_eq!(
                Output::new(format, true),
                Ok(Output {
                    format,
                    header: false
                })
            );
        }
    }

    /// A write's count without its label is the number, for `n=$(…)`.
    #[test]
    fn a_headerless_write_reports_the_bare_count() {
        assert_eq!(render_affected(3, headerless(Format::Table)), "3\n");
        assert_eq!(render_affected(3, headerless(Format::Csv)), "3\n");
        assert_eq!(render_affected(3, Format::Csv), "affected\n3\n");
    }

    /// **`vertical` is `\G`**: core's records, NULL spelled out as the table
    /// spells it, and the table's footer — it is the other format a person
    /// reads directly.
    #[test]
    fn vertical_writes_records_with_the_tables_footer() {
        let out = render_rows(&null_and_empty(), Format::Vertical);
        assert_eq!(
            out,
            "*************************** 1. row ***************************\n\
             a: NULL\n\
             b: \n\
             \n(1 row)\n"
        );
    }

    /// A read with no rows says so, without a blank line over nothing.
    #[test]
    fn vertical_of_no_rows_is_just_the_count() {
        let empty = ResultSet::from_rows(vec![col("id")], vec![]);
        assert_eq!(render_rows(&empty, Format::Vertical), "(0 rows)\n");
    }

    /// It reports a cap in-band, like the table, and so not on stderr as
    /// well; and it shows a blob's placeholder, so there is nothing withheld
    /// to warn about.
    #[test]
    fn vertical_reports_a_cap_in_band_and_withholds_nothing() {
        let mut capped = rs();
        capped.truncated = true;
        assert!(render_rows(&capped, Format::Vertical).contains("capped"));
        assert!(truncation_warning(&capped, Format::Vertical).is_none());
        assert!(withheld_warning(&lossy(), Format::Vertical).is_none());
        assert!(render_rows(&lossy(), Format::Vertical).contains("avatar: <2 bytes>"));
        assert_eq!(render_affected(1, Format::Vertical), "(1 row affected)\n");
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
