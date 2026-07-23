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
