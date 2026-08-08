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

/// Prompt for summarizing a single cell. `table` is the qualified source table
/// when the result came from one.
pub fn cell_prompt(table: Option<&str>, column: &str, value: &str) -> String {
    let from = match table {
        Some(t) => format!(" from the `{t}` table"),
        None => String::new(),
    };
    format!(
        "Summarize this value{from}, column `{column}`:\n```\n{value}\n```\n\
         If you can infer a pattern, format, or meaning from it, note that too."
    )
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
        "\n\nA sample of {} values currently loaded in the grid:\n```\n{}\n```",
        samples.len(),
        samples.join("\n")
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

    #[test]
    fn cell_prompt_names_the_table_when_known() {
        let with = cell_prompt(Some("shop.orders"), "total", "12.50");
        assert!(with.contains("from the `shop.orders` table"));
        assert!(with.contains("column `total`"));
        assert!(with.contains("12.50"));
        // No source table → no dangling "from the `` table".
        let without = cell_prompt(None, "total", "12.50");
        assert!(!without.contains("table"));
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
