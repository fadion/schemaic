//! Prompts for the grid's "AI Summary" actions — one cell, or a whole column.
//!
//! Both ask the assistant the same kind of question ("what is this, and what
//! pattern does it follow?"), so they live together and the grid keeps a thin
//! wrapper. The column variant carries a sample of the loaded values, since a
//! column's *meaning* is usually obvious from a handful of them and far less
//! obvious from its name alone.
//!
//! Sampling reads only what's already on screen — no query, no round-trip, so
//! the menu item is instant. The assistant still has `run_query` if it wants
//! more than the sample shows.

use crate::model::ResultSet;
use crate::prompt;

/// Values sampled from a column for the summary prompt.
pub const COLUMN_SAMPLE: usize = 10;

/// Longest single sampled value kept, in characters. A blob column would
/// otherwise swamp the prompt with one row.
pub const SAMPLE_CHARS: usize = 120;

/// Take up to `max` values from column `ci`, spread evenly across the loaded
/// rows rather than taken from the top.
///
/// Evenly spread beats the first N: results arrive sorted often enough that the
/// first rows share a prefix, a date, or a status, and a sample of those reads
/// as a pattern that isn't there. Deterministic — no RNG — so the same grid
/// always yields the same prompt.
pub fn sample_column(rs: &ResultSet, ci: usize, max: usize) -> Vec<String> {
    let rows = rs.row_count();
    if rows == 0 || max == 0 || ci >= rs.col_count() {
        return Vec::new();
    }
    let take = max.min(rows);
    // Stride across the whole result; `take == rows` degenerates to every row.
    (0..take)
        .map(|n| n * rows / take)
        .filter_map(|r| rs.cell(r, ci))
        .map(|c| truncate(c.display()))
        .collect()
}

/// Clip one sampled value to [`SAMPLE_CHARS`], flattening newlines so each
/// sample stays on its own line in the prompt.
fn truncate(value: &str) -> String {
    let flat = value.replace('\n', " ");
    if flat.chars().count() > SAMPLE_CHARS {
        format!("{}…", flat.chars().take(SAMPLE_CHARS).collect::<String>())
    } else {
        flat
    }
}

/// Fields of the focused cell's own row included as context.
pub const CELL_ROW_FIELDS: usize = 12;

/// The other columns of one row, as `(name, displayed value)`.
///
/// A lone value rarely explains itself; the row it sits in usually does — a
/// status reads differently next to an `order_id` than next to a `job_id`.
/// `skip_ci` is the focused column, quoted separately in the prompt.
pub fn sample_row(
    rs: &ResultSet,
    row: usize,
    skip_ci: usize,
    max_fields: usize,
) -> Vec<(String, String)> {
    if row >= rs.row_count() {
        return Vec::new();
    }
    (0..rs.col_count())
        .filter(|ci| *ci != skip_ci)
        .filter_map(|ci| {
            let name = rs.columns.get(ci)?.name.clone();
            Some((name, truncate(rs.cell(row, ci)?.display())))
        })
        .take(max_fields)
        .collect()
}

/// Prompt for summarizing a single cell.
///
/// Carries everything already on screen — the column's type, the rest of the
/// cell's row, and a sample of the column's other values — so the assistant can
/// answer from context instead of going querying. It has `run_query` when the
/// setting allows, but a menu action shouldn't need a round-trip to say what a
/// value is, and with queries turned off it wouldn't have one.
///
/// `table` is the qualified source table when the result came from one; the row
/// and sample sections are omitted when empty rather than sent as headings with
/// nothing under them.
pub fn cell_prompt(
    table: Option<&str>,
    column: &str,
    type_name: &str,
    value: &str,
    row: &[(String, String)],
    samples: &[String],
) -> String {
    let from = match table {
        Some(t) => format!(" of the `{t}` table"),
        None => String::new(),
    };
    // Every block below is database content — fenced so a value containing its
    // own ``` can't close the block and continue as prose, and labelled so the
    // assistant knows which parts are data rather than instruction.
    let mut out = format!(
        "Summarize this value from the `{column}` column{from} (type `{type_name}`). \
         {}\n{}",
        prompt::UNTRUSTED_NOTE,
        prompt::fenced(value)
    );
    if !row.is_empty() {
        let fields: Vec<String> = row.iter().map(|(k, v)| format!("{k}: {v}")).collect();
        out.push_str(&format!(
            "\n\nThe rest of its row, for context:\n{}",
            prompt::fenced(&fields.join("\n"))
        ));
    }
    if !samples.is_empty() {
        out.push_str(&format!(
            "\n\nOther values in the same column, sampled across the loaded rows:\n{}",
            prompt::fenced(&samples.join("\n"))
        ));
    }
    out.push_str(
        "\n\nSay what it represents and note any pattern, format, or encoding — \
         including whether it looks typical for this column or unusual. Answer \
         from what's above; say so if something you'd need isn't shown.",
    );
    out
}

/// Prompt for summarizing a whole column, given a sample of its values.
///
/// Asks what the column is *for* — the question a name and a type can't answer.
/// The sample is labelled as a sample so the assistant doesn't generalise from
/// it as though it were the full column.
pub fn column_prompt(
    table: Option<&str>,
    column: &str,
    type_name: &str,
    samples: &[String],
) -> String {
    let from = match table {
        Some(t) => format!(" of the `{t}` table"),
        None => String::new(),
    };
    let mut out = format!("What is the `{column}` column{from} for? Its type is `{type_name}`.");
    if samples.is_empty() {
        out.push_str(
            "\n\nNo rows are loaded, so judge from the name and type — and say so if \
             you're guessing.",
        );
        return out;
    }
    out.push_str(&format!(
        "\n\nA sample of {} values currently loaded in the grid. {}\n{}",
        samples.len(),
        prompt::UNTRUSTED_NOTE,
        prompt::fenced(&samples.join("\n"))
    ));
    out.push_str(
        "\n\nDescribe what it holds and any pattern, format, or encoding you can \
         infer. This is a sample, not the whole column — say so if something \
         depends on values you can't see.",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Column, ResultSet, Value};

    fn col(name: &str) -> Column {
        Column {
            name: name.to_string(),
            type_name: "VARCHAR".to_string(),
            origin: None,
        }
    }

    fn rs_of(values: &[&str]) -> ResultSet {
        ResultSet::from_rows(
            vec![col("v")],
            values
                .iter()
                .map(|v| vec![Value::Str((*v).to_string())])
                .collect(),
        )
    }

    #[test]
    fn sample_is_empty_for_an_empty_or_out_of_range_column() {
        assert!(sample_column(&ResultSet::default(), 0, 5).is_empty());
        assert!(sample_column(&rs_of(&["a"]), 9, 5).is_empty());
        assert!(sample_column(&rs_of(&["a"]), 0, 0).is_empty());
    }

    #[test]
    fn sample_takes_every_row_when_there_are_fewer_than_max() {
        assert_eq!(
            sample_column(&rs_of(&["a", "b", "c"]), 0, 10),
            ["a", "b", "c"]
        );
    }

    #[test]
    fn sample_spreads_across_the_result_rather_than_taking_the_top() {
        let values: Vec<String> = (0..100).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = values.iter().map(|s| s.as_str()).collect();
        let got = sample_column(&rs_of(&refs), 0, 5);
        // Evenly spaced, starting at the first row and reaching the far end.
        assert_eq!(got, ["0", "20", "40", "60", "80"]);
    }

    #[test]
    fn sample_truncates_a_long_value_and_flattens_newlines() {
        let long = "x".repeat(SAMPLE_CHARS + 50);
        let got = sample_column(&rs_of(&[&long, "a\nb"]), 0, 2);
        assert_eq!(got[0], format!("{}…", "x".repeat(SAMPLE_CHARS)));
        assert_eq!(got[1], "a b");
    }

    #[test]
    fn sample_renders_null_like_the_grid_does() {
        let rs = ResultSet::from_rows(vec![col("v")], vec![vec![Value::Null]]);
        assert_eq!(sample_column(&rs, 0, 5), ["NULL"]);
    }

    fn wide_row() -> ResultSet {
        ResultSet::from_rows(
            vec![col("id"), col("status"), col("note")],
            vec![
                vec![
                    Value::Int(1),
                    Value::Str("A".into()),
                    Value::Str("first".into()),
                ],
                vec![
                    Value::Int(2),
                    Value::Str("B".into()),
                    Value::Str("second".into()),
                ],
            ],
        )
    }

    #[test]
    fn row_context_lists_the_other_columns_of_that_row() {
        let got = sample_row(&wide_row(), 1, 1, 10);
        // The focused column is skipped — its value is quoted on its own.
        assert_eq!(
            got,
            vec![
                ("id".to_string(), "2".to_string()),
                ("note".to_string(), "second".to_string()),
            ]
        );
    }

    #[test]
    fn row_context_is_capped_and_truncated() {
        let long = "z".repeat(SAMPLE_CHARS + 40);
        let rs = ResultSet::from_rows(
            vec![col("a"), col("b"), col("c")],
            vec![vec![
                Value::Str(long),
                Value::Str("b".into()),
                Value::Str("c".into()),
            ]],
        );
        let got = sample_row(&rs, 0, 9, 2);
        assert_eq!(got.len(), 2); // capped
        assert_eq!(got[0].1, format!("{}…", "z".repeat(SAMPLE_CHARS)));
    }

    #[test]
    fn row_context_is_empty_for_a_missing_row() {
        assert!(sample_row(&wide_row(), 99, 0, 10).is_empty());
    }

    #[test]
    fn cell_prompt_frames_the_value_with_everything_on_screen() {
        let row = vec![("id".to_string(), "2".to_string())];
        let samples = vec!["A".to_string(), "B".to_string()];
        let out = cell_prompt(
            Some("shop.orders"),
            "status",
            "char(1)",
            "B",
            &row,
            &samples,
        );
        assert!(out.contains("`status`"));
        assert!(out.contains("`shop.orders`"));
        assert!(out.contains("char(1)"));
        assert!(out.contains("\nB\n")); // the value itself, fenced
        assert!(out.contains("id: 2")); // the rest of its row
        assert!(out.contains("A\nB")); // sibling values from the column
        // Enough context that it needn't go querying to answer.
        assert!(out.contains("pattern"));
    }

    #[test]
    fn cell_prompt_omits_sections_it_has_nothing_for() {
        let out = cell_prompt(None, "total", "decimal", "12.50", &[], &[]);
        assert!(out.contains("`total`"));
        assert!(out.contains("12.50"));
        // No source table → no dangling "of the `` table".
        assert!(!out.contains("table"));
        assert!(!out.contains("Other values"));
        assert!(!out.contains("rest of"));
    }

    #[test]
    fn column_prompt_carries_the_sample_and_its_size() {
        let samples = vec!["a".to_string(), "b".to_string()];
        let out = column_prompt(Some("shop.orders"), "status", "varchar(20)", &samples);
        assert!(out.contains("`status` column of the `shop.orders` table"));
        assert!(out.contains("type is `varchar(20)`"));
        assert!(out.contains("sample of 2 values"));
        assert!(out.contains("a\nb"));
        // The assistant is told not to treat the sample as the whole column.
        assert!(out.contains("not the whole column"));
    }

    #[test]
    fn column_prompt_without_rows_asks_for_an_honest_guess() {
        let out = column_prompt(None, "status", "varchar(20)", &[]);
        assert!(out.contains("No rows are loaded"));
        assert!(out.contains("say so if you're guessing"));
        assert!(!out.contains("sample of"));
    }
}
