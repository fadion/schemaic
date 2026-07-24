//! Result-set export — pure over [`ResultSet`] + a display order, no UI.
//!
//! `order` is the display→data-row permutation (post-sort); callers pass the
//! grid's live order so exports match what's on screen. The UI keeps only thin
//! clipboard wrappers around these.

use crate::model::{ResultSet, Value};

/// A cell as a JSON value (non-finite floats → null).
pub fn value_to_json(v: &Value) -> serde_json::Value {
    use serde_json::Value as J;
    match v {
        Value::Null => J::Null,
        Value::Int(i) => J::from(*i),
        Value::UInt(u) => J::from(*u),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(J::Number)
            .unwrap_or(J::Null),
        Value::Str(s) => J::String(s.clone()),
    }
}

/// Quote a CSV field if it contains a delimiter/quote/newline, and neutralize
/// spreadsheet formula/DDE injection (§7.5): a value a spreadsheet would evaluate
/// as a formula — leading `=`, `+`, `@`, or a `\t`/`\r` control char — is prefixed
/// with a single quote so Excel/Sheets import it as text (a cell `=HYPERLINK(...)`
/// otherwise executes on open). Leading `-` is deliberately NOT guarded: it's a
/// valid numeric sign and prefixing it would corrupt every negative number.
pub fn csv_field(s: &str) -> String {
    let guarded;
    let s = if matches!(
        s.as_bytes().first(),
        Some(b'=' | b'+' | b'@' | b'\t' | b'\r')
    ) {
        guarded = format!("'{s}");
        guarded.as_str()
    } else {
        s
    };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// A cell as a SQL literal (non-finite float → NULL; strings escaped).
pub fn sql_literal(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) if !f.is_finite() => "NULL".to_string(),
        Value::Float(f) => f.to_string(),
        Value::Str(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "''")),
    }
}

/// Backtick-quote a SQL identifier, doubling any embedded backtick.
pub fn ident_sql(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// The whole result as a pretty JSON array of row objects (keyed by column name).
/// Duplicate column names (e.g. `a.id, b.id` from a join) are suffixed `_2`,
/// `_3`, … so a JSON object doesn't silently drop all but the last (§7.4).
pub fn export_json(rs: &ResultSet, order: &[usize]) -> String {
    let keys = unique_column_keys(rs);
    let arr: Vec<serde_json::Value> = order
        .iter()
        .filter_map(|&di| rs.rows.get(di))
        .map(|row| {
            let mut obj = serde_json::Map::new();
            for (ci, key) in keys.iter().enumerate() {
                obj.insert(
                    key.clone(),
                    row.get(ci)
                        .map(value_to_json)
                        .unwrap_or(serde_json::Value::Null),
                );
            }
            serde_json::Value::Object(obj)
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::Value::Array(arr)).unwrap_or_default()
}

/// Column names made unique for use as JSON object keys: a repeated name gets a
/// `_2`/`_3`/… suffix (first occurrence keeps the bare name).
fn unique_column_keys(rs: &ResultSet) -> Vec<String> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    rs.columns
        .iter()
        .map(|c| {
            let n = seen.entry(c.name.clone()).or_insert(0);
            *n += 1;
            if *n == 1 {
                c.name.clone()
            } else {
                format!("{}_{}", c.name, n)
            }
        })
        .collect()
}

/// One column's values as a JSON array (for building arrays out of a column).
pub fn export_column_json(rs: &ResultSet, order: &[usize], ci: usize) -> String {
    let arr: Vec<serde_json::Value> = order
        .iter()
        .map(|&di| {
            rs.rows
                .get(di)
                .and_then(|r| r.get(ci))
                .map(value_to_json)
                .unwrap_or(serde_json::Value::Null)
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::Value::Array(arr)).unwrap_or_default()
}

/// One column's values as a newline-separated list (a single-column CSV).
pub fn export_column_csv(rs: &ResultSet, order: &[usize], ci: usize) -> String {
    let mut out = String::new();
    for &di in order {
        let v = match rs.rows.get(di).and_then(|r| r.get(ci)) {
            Some(Value::Null) | None => String::new(),
            Some(v) => csv_field(&v.display()),
        };
        out.push_str(&v);
        out.push('\n');
    }
    out
}

/// The whole result as CSV (header row + data rows; NULL → empty field).
pub fn export_csv(rs: &ResultSet, order: &[usize]) -> String {
    let mut out = rs
        .columns
        .iter()
        .map(|c| csv_field(&c.name))
        .collect::<Vec<_>>()
        .join(",");
    out.push('\n');
    for &di in order {
        if let Some(row) = rs.rows.get(di) {
            let line = (0..rs.columns.len())
                .map(|ci| match row.get(ci) {
                    Some(Value::Null) | None => String::new(),
                    Some(v) => csv_field(&v.display()),
                })
                .collect::<Vec<_>>()
                .join(",");
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

/// Escape a Markdown table cell. A `|` starts a new column, so it must be
/// backslash-escaped; backslash is Markdown's escape char, so a literal `\`
/// doubles (else it would swallow a following `|`). Newlines would break the
/// row — GitHub renders `<br>` inside table cells, so map them there (a lone CR
/// is dropped so CRLF doesn't emit a double break).
pub fn md_cell(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\r', "")
        .replace('\n', "<br>")
}

/// Escape text for HTML element content. `&` is replaced first so the `&` in
/// the `&lt;`/`&gt;` entities isn't re-escaped.
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The whole result as a GitHub-Flavored-Markdown table (header row + `---`
/// separator + data rows). Cells are escaped via [`md_cell`]; NULL renders as an
/// empty cell (matching [`export_csv`]).
pub fn export_markdown(rs: &ResultSet, order: &[usize]) -> String {
    let n = rs.columns.len();
    let row_line = |cells: Vec<String>| format!("| {} |\n", cells.join(" | "));
    let mut out = row_line(rs.columns.iter().map(|c| md_cell(&c.name)).collect());
    out.push_str(&row_line((0..n).map(|_| "---".to_string()).collect()));
    for &di in order {
        if let Some(row) = rs.rows.get(di) {
            let cells = (0..n)
                .map(|ci| match row.get(ci) {
                    Some(Value::Null) | None => String::new(),
                    Some(v) => md_cell(&v.display()),
                })
                .collect();
            out.push_str(&row_line(cells));
        }
    }
    out
}

/// The whole result as an HTML `<table>` (thead + tbody). Cells/headers are
/// escaped via [`html_escape`]; NULL renders as an empty `<td>` (matching
/// [`export_csv`]).
pub fn export_html(rs: &ResultSet, order: &[usize]) -> String {
    let mut out = String::from("<table>\n<thead>\n<tr>");
    for c in &rs.columns {
        out.push_str("<th>");
        out.push_str(&html_escape(&c.name));
        out.push_str("</th>");
    }
    out.push_str("</tr>\n</thead>\n<tbody>\n");
    for &di in order {
        if let Some(row) = rs.rows.get(di) {
            out.push_str("<tr>");
            for ci in 0..rs.columns.len() {
                out.push_str("<td>");
                match row.get(ci) {
                    Some(Value::Null) | None => {}
                    Some(v) => out.push_str(&html_escape(&v.display())),
                }
                out.push_str("</td>");
            }
            out.push_str("</tr>\n");
        }
    }
    out.push_str("</tbody>\n</table>\n");
    out
}

/// The result as `INSERT` statements. `source` is the real `(database, table)`
/// when known; otherwise a `` `table` `` placeholder is emitted for the user to
/// fill in. Identifiers are backtick-escaped.
pub fn export_inserts(rs: &ResultSet, order: &[usize], source: Option<(&str, &str)>) -> String {
    let table_sql = match source {
        Some((db, table)) => format!("{}.{}", ident_sql(db), ident_sql(table)),
        None => "`table`".to_string(),
    };
    let cols = rs
        .columns
        .iter()
        .map(|c| ident_sql(&c.name))
        .collect::<Vec<_>>()
        .join(", ");
    let mut out = String::new();
    for &di in order {
        if let Some(row) = rs.rows.get(di) {
            let vals = (0..rs.columns.len())
                .map(|ci| {
                    row.get(ci)
                        .map(sql_literal)
                        .unwrap_or_else(|| "NULL".to_string())
                })
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(
                "INSERT INTO {table_sql} ({cols}) VALUES ({vals});\n"
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Column;

    fn col(name: &str) -> Column {
        Column {
            name: name.to_string(),
            type_name: "VARCHAR".to_string(),
            origin: None,
        }
    }

    fn rs() -> ResultSet {
        ResultSet {
            columns: vec![col("id"), col("a`b")],
            rows: vec![
                vec![Value::Int(1), Value::Str("x".to_string())],
                vec![Value::Null, Value::Str("y".to_string())],
            ],
            elapsed_ms: 0,
            truncated: false,
            affected: None,
        }
    }

    #[test]
    fn ident_doubles_backticks() {
        assert_eq!(ident_sql("a`b"), "`a``b`");
    }

    #[test]
    fn sql_literal_handles_nonfinite_and_escapes() {
        assert_eq!(sql_literal(&Value::Float(f64::NAN)), "NULL");
        assert_eq!(sql_literal(&Value::Float(f64::INFINITY)), "NULL");
        assert_eq!(sql_literal(&Value::Str("O'Hara".to_string())), "'O''Hara'");
    }

    #[test]
    fn c5_inserts_use_real_table_and_escape_identifiers() {
        let out = export_inserts(&rs(), &[0, 1], Some(("shop", "cust")));
        // Real qualified table, not a `table` placeholder; column `a`b` escaped.
        assert!(out.contains("INSERT INTO `shop`.`cust` (`id`, `a``b`) VALUES"));
        assert!(out.contains("(1, 'x')"));
        assert!(out.contains("(NULL, 'y')"));
        // Placeholder only when the source is unknown.
        assert!(export_inserts(&rs(), &[0], None).contains("INSERT INTO `table` ("));
    }

    #[test]
    fn csv_quotes_only_when_needed() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("he\"llo"), "\"he\"\"llo\"");
    }

    #[test]
    fn csv_neutralizes_formula_injection() {
        // Leading formula/DDE triggers get a `'` prefix (then quoted if needed).
        assert_eq!(csv_field("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(csv_field("+1+2"), "'+1+2");
        assert_eq!(csv_field("@SUM(A1)"), "'@SUM(A1)");
        // Tab isn't a CSV delimiter, so the guarded value isn't additionally quoted.
        assert_eq!(csv_field("\tcmd"), "'\tcmd");
        // Negative numbers are NOT guarded (would corrupt every negative value).
        assert_eq!(csv_field("-5"), "-5");
        // A `=` mid-value is harmless — only leading chars trigger a formula.
        assert_eq!(csv_field("a=b"), "a=b");
    }

    #[test]
    fn json_suffixes_duplicate_columns() {
        let rs = ResultSet {
            columns: vec![col("id"), col("id"), col("id")],
            rows: vec![vec![Value::Int(1), Value::Int(2), Value::Int(3)]],
            elapsed_ms: 0,
            truncated: false,
            affected: None,
        };
        let v: serde_json::Value = serde_json::from_str(&export_json(&rs, &[0])).unwrap();
        assert_eq!(v[0]["id"], 1);
        assert_eq!(v[0]["id_2"], 2);
        assert_eq!(v[0]["id_3"], 3);
    }

    #[test]
    fn json_respects_display_order() {
        // order [1, 0] → the NULL-id row first.
        let out = export_json(&rs(), &[1, 0]);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v[0]["id"].is_null());
        assert_eq!(v[1]["id"], 1);
    }

    #[test]
    fn value_to_json_maps_each_variant_and_nulls_nonfinite() {
        use serde_json::Value as J;
        assert_eq!(value_to_json(&Value::Null), J::Null);
        assert_eq!(value_to_json(&Value::Int(-3)), J::from(-3i64));
        assert_eq!(value_to_json(&Value::UInt(3)), J::from(3u64));
        assert_eq!(value_to_json(&Value::Float(1.5)), J::from(1.5));
        assert_eq!(
            value_to_json(&Value::Str("s".into())),
            J::String("s".into())
        );
        // Non-finite floats have no JSON representation → null.
        assert_eq!(value_to_json(&Value::Float(f64::NAN)), J::Null);
        assert_eq!(value_to_json(&Value::Float(f64::INFINITY)), J::Null);
    }

    #[test]
    fn export_csv_has_header_and_nulls_are_empty() {
        let out = export_csv(&rs(), &[0, 1]);
        let lines: Vec<&str> = out.lines().collect();
        // Header quotes the backtick column only because... it has no comma; stays bare.
        assert_eq!(lines[0], "id,a`b");
        assert_eq!(lines[1], "1,x");
        // NULL id renders as an empty leading field.
        assert_eq!(lines[2], ",y");
    }

    #[test]
    fn export_column_csv_is_newline_separated_with_blank_nulls() {
        // Column 0 (id): 1, then NULL → blank line.
        let out = export_column_csv(&rs(), &[0, 1], 0);
        assert_eq!(out, "1\n\n");
    }

    #[test]
    fn md_cell_escapes_pipe_backslash_and_newline() {
        // A pipe would start a new column — escape it. Backslash is Markdown's
        // escape char, so a literal `\` must double (else it'd escape the `|`).
        assert_eq!(md_cell("a|b"), "a\\|b");
        assert_eq!(md_cell("C:\\x"), "C:\\\\x");
        assert_eq!(md_cell("a\\|b"), "a\\\\\\|b");
        // Newlines would break the row → GFM `<br>`; a lone CR is dropped.
        assert_eq!(md_cell("a\nb"), "a<br>b");
        assert_eq!(md_cell("a\r\nb"), "a<br>b");
        assert_eq!(md_cell("plain"), "plain");
    }

    #[test]
    fn export_markdown_has_header_separator_and_orders_rows() {
        // order [1, 0] → NULL-id row first; NULL renders as an empty cell.
        let out = export_markdown(&rs(), &[1, 0]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "| id | a`b |");
        assert_eq!(lines[1], "| --- | --- |");
        assert_eq!(lines[2], "|  | y |");
        assert_eq!(lines[3], "| 1 | x |");
    }

    #[test]
    fn html_escape_orders_ampersand_first() {
        // `&` must be escaped before `<`/`>` or `&lt;` would become `&amp;lt;`.
        assert_eq!(html_escape("a<b>&c"), "a&lt;b&gt;&amp;c");
        assert_eq!(html_escape("plain"), "plain");
    }

    #[test]
    fn export_html_escapes_entities_and_nulls_are_empty() {
        let rs = ResultSet {
            columns: vec![col("a<b>")],
            rows: vec![
                vec![Value::Str("x&y".to_string())],
                vec![Value::Null],
            ],
            elapsed_ms: 0,
            truncated: false,
            affected: None,
        };
        let out = export_html(&rs, &[0, 1]);
        assert!(out.contains("<th>a&lt;b&gt;</th>"));
        assert!(out.contains("<td>x&amp;y</td>"));
        // NULL → empty cell, not the literal "NULL".
        assert!(out.contains("<td></td>"));
        // Well-formed table scaffolding.
        assert!(out.trim_start().starts_with("<table>"));
        assert!(out.contains("<thead>") && out.contains("<tbody>"));
        assert!(out.trim_end().ends_with("</table>"));
    }

    #[test]
    fn export_column_json_is_array_in_display_order() {
        // Column 1 (a`b) in reversed order.
        let out = export_column_json(&rs(), &[1, 0], 1);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v[0], "y");
        assert_eq!(v[1], "x");
        // Column 0 with a NULL becomes JSON null.
        let out = export_column_json(&rs(), &[0, 1], 0);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v[0], 1);
        assert!(v[1].is_null());
    }
}
