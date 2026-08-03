//! Full-row JSON view/edit for the grid's "Edit Row" panel.
//!
//! [`row_to_json`] serializes one result row to a pretty JSON object (keys in
//! column order); [`parse_row_edit`] parses an edited JSON object back into the
//! set of **changed, editable** columns to feed an `UPDATE` (`RowEdit.set`).
//!
//! Pure + unit-tested — the UI is a thin wrapper. The DB stays the type authority:
//! validation here covers JSON shape, unknown columns, edits to read-only columns,
//! and NOT-NULL violations; whether a value actually fits a column's type is left
//! to the commit (its error surfaces in the panel). Forward serialization reuses
//! [`crate::export::value_to_json`], so it agrees with the JSON export.

use std::collections::HashMap;

use crate::export::value_to_json;
use crate::model::Value;

/// One column's context for the row editor.
pub struct ColSpec {
    /// Column display name (the result-set header).
    pub name: String,
    /// May this column be written (i.e. go into the `UPDATE ... SET`)? Primary-key /
    /// expression / binary columns are `false` — shown for context but edits rejected.
    pub editable: bool,
    /// May this column hold SQL `NULL`? (`!not_null`.)
    pub nullable: bool,
    /// The row's current (pre-edit) value for this column.
    pub value: Value,
}

/// JSON object keys for the columns. Normally the display name; when a name repeats
/// (a join projecting same-named columns), *every* occurrence is suffixed with its
/// column index (`name#ci`) so serialize/parse agree and keys stay unambiguous.
fn keys_for(cols: &[ColSpec]) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for c in cols {
        *counts.entry(c.name.as_str()).or_default() += 1;
    }
    cols.iter()
        .enumerate()
        .map(|(ci, c)| {
            if counts[c.name.as_str()] > 1 {
                format!("{}#{ci}", c.name)
            } else {
                c.name.clone()
            }
        })
        .collect()
}

/// The text a JSON scalar contributes to `RowEdit.set` (`None` = SQL `NULL`).
/// Numbers keep their canonical text (so a DECIMAL never round-trips through a lossy
/// float); an object/array is re-serialized compactly (for JSON-typed columns).
fn json_to_set_text(v: &serde_json::Value) -> Option<String> {
    use serde_json::Value as J;
    match v {
        J::Null => None,
        J::Bool(b) => Some(if *b { "true" } else { "false" }.to_string()),
        J::Number(n) => Some(n.to_string()),
        J::String(s) => Some(s.clone()),
        J::Array(_) | J::Object(_) => Some(v.to_string()),
    }
}

/// Serialize a row to a pretty JSON object, keys in column order. Built by hand (not
/// via `serde_json::Map`, which sorts keys without the `preserve_order` feature) so
/// the on-screen order matches the result columns; values + keys are escaped by
/// `serde_json`.
pub fn row_to_json(cols: &[ColSpec]) -> String {
    if cols.is_empty() {
        return "{}".to_string();
    }
    let keys = keys_for(cols);
    let mut out = String::from("{\n");
    for (i, (c, k)) in cols.iter().zip(&keys).enumerate() {
        let key = serde_json::to_string(k).unwrap_or_else(|_| "\"\"".to_string());
        let val = serde_json::to_string(&value_to_json(&c.value)).unwrap_or_else(|_| "null".into());
        out.push_str("  ");
        out.push_str(&key);
        out.push_str(": ");
        out.push_str(&val);
        if i + 1 < cols.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push('}');
    out
}

/// Parse edited JSON against the original row; return the changed **editable**
/// columns as `(col_index, Option<String>)` (`None` = SQL `NULL`), or a human-readable
/// error message. A column absent from the object is left unchanged; a value equal to
/// the original (compared on its normalized text form) is not a change.
pub fn parse_row_edit(
    cols: &[ColSpec],
    edited: &str,
) -> Result<Vec<(usize, Option<String>)>, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(edited).map_err(|e| format!("Invalid JSON: {e}"))?;
    let obj = parsed
        .as_object()
        .ok_or_else(|| "The row must be a JSON object.".to_string())?;

    let keys = keys_for(cols);
    let known: HashMap<&str, usize> = keys
        .iter()
        .enumerate()
        .map(|(ci, k)| (k.as_str(), ci))
        .collect();
    // Reject keys that don't map to a column (typo / stray field).
    for k in obj.keys() {
        if !known.contains_key(k.as_str()) {
            return Err(format!("Unknown column `{k}`."));
        }
    }

    let mut changes = Vec::new();
    for (ci, c) in cols.iter().enumerate() {
        let Some(jv) = obj.get(&keys[ci]) else {
            continue; // absent → unchanged
        };
        // Normalize both sides through the same mapping so an untouched value (which
        // round-trips via `value_to_json`) never registers as a change.
        let new_text = json_to_set_text(jv);
        let orig_text = json_to_set_text(&value_to_json(&c.value));
        if new_text == orig_text {
            continue;
        }
        if !c.editable {
            return Err(format!("Column `{}` is not editable.", c.name));
        }
        if new_text.is_none() && !c.nullable {
            return Err(format!("Column `{}` cannot be NULL.", c.name));
        }
        changes.push((ci, new_text));
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, editable: bool, nullable: bool, value: Value) -> ColSpec {
        ColSpec {
            name: name.to_string(),
            editable,
            nullable,
            value,
        }
    }

    /// A typical single-table row: read-only PK `id`, editable `name` (NOT NULL),
    /// editable nullable `district`, editable DECIMAL `pop` (stored as Str).
    fn sample() -> Vec<ColSpec> {
        vec![
            col("id", false, false, Value::Int(1)),
            col("name", true, false, Value::Str("Kabul".into())),
            col("district", true, true, Value::Str("Kabol".into())),
            col("pop", true, false, Value::Str("19.99".into())),
        ]
    }

    #[test]
    fn row_to_json_orders_and_types() {
        let s = row_to_json(&sample());
        // Keys appear in column order (not sorted), id before name.
        assert!(s.find("\"id\"").unwrap() < s.find("\"name\"").unwrap());
        // Int → JSON number, Str → JSON string (DECIMAL stays quoted, never a float).
        assert!(s.contains("\"id\": 1"));
        assert!(s.contains("\"name\": \"Kabul\""));
        assert!(s.contains("\"pop\": \"19.99\""));
        // It round-trips: no edits → no changes.
        assert_eq!(parse_row_edit(&sample(), &s), Ok(vec![]));
    }

    #[test]
    fn null_renders_and_round_trips() {
        let cols = vec![col("d", true, true, Value::Null)];
        let s = row_to_json(&cols);
        assert!(s.contains("\"d\": null"));
        assert_eq!(parse_row_edit(&cols, &s), Ok(vec![]));
    }

    #[test]
    fn edits_editable_fields() {
        let edited = r#"{"id":1,"name":"Herat","district":"Herat","pop":"19.99"}"#;
        assert_eq!(
            parse_row_edit(&sample(), edited),
            Ok(vec![(1, Some("Herat".into())), (2, Some("Herat".into()))])
        );
    }

    #[test]
    fn decimal_number_or_string_is_not_a_spurious_change() {
        // `pop` shown as "19.99"; re-typed as a bare number 19.99 → still no change.
        let edited = r#"{"id":1,"name":"Kabul","district":"Kabol","pop":19.99}"#;
        assert_eq!(parse_row_edit(&sample(), edited), Ok(vec![]));
    }

    #[test]
    fn set_to_null_and_from_null() {
        // Nullable district → null is allowed and staged as None.
        let edited = r#"{"id":1,"name":"Kabul","district":null,"pop":"19.99"}"#;
        assert_eq!(parse_row_edit(&sample(), edited), Ok(vec![(2, None)]));
    }

    #[test]
    fn not_null_to_null_is_rejected() {
        let edited = r#"{"id":1,"name":null,"district":"Kabol","pop":"19.99"}"#;
        let err = parse_row_edit(&sample(), edited).unwrap_err();
        assert!(err.contains("`name`") && err.contains("NULL"), "{err}");
    }

    #[test]
    fn read_only_change_is_rejected_but_unchanged_is_fine() {
        // Changing the PK id → rejected, naming the column.
        let edited = r#"{"id":2,"name":"Kabul","district":"Kabol","pop":"19.99"}"#;
        let err = parse_row_edit(&sample(), edited).unwrap_err();
        assert!(
            err.contains("`id`") && err.contains("not editable"),
            "{err}"
        );
        // Leaving id as-is while editing name → fine (read-only unchanged).
        let edited = r#"{"id":1,"name":"Herat","district":"Kabol","pop":"19.99"}"#;
        assert_eq!(
            parse_row_edit(&sample(), edited),
            Ok(vec![(1, Some("Herat".into()))])
        );
    }

    #[test]
    fn unknown_key_is_rejected() {
        let edited = r#"{"id":1,"name":"Kabul","district":"Kabol","pop":"19.99","bogus":1}"#;
        let err = parse_row_edit(&sample(), edited).unwrap_err();
        assert!(
            err.contains("Unknown column") && err.contains("bogus"),
            "{err}"
        );
    }

    #[test]
    fn missing_key_is_left_unchanged() {
        // `district` omitted entirely → not a change, and other edits still apply.
        let edited = r#"{"id":1,"name":"Herat","pop":"19.99"}"#;
        assert_eq!(
            parse_row_edit(&sample(), edited),
            Ok(vec![(1, Some("Herat".into()))])
        );
    }

    #[test]
    fn bad_json_and_non_object_are_reported() {
        assert!(
            parse_row_edit(&sample(), "{not json")
                .unwrap_err()
                .contains("Invalid JSON")
        );
        assert!(
            parse_row_edit(&sample(), "[1,2,3]")
                .unwrap_err()
                .contains("JSON object")
        );
    }

    #[test]
    fn duplicate_names_are_disambiguated() {
        // Two columns both named "id" (a join) → keys become id#0 / id#1.
        let cols = vec![
            col("id", false, false, Value::Int(1)),
            col("id", true, false, Value::Int(2)),
        ];
        let s = row_to_json(&cols);
        assert!(
            s.contains("\"id#0\": 1") && s.contains("\"id#1\": 2"),
            "{s}"
        );
        // Editing the second (editable) id via its disambiguated key works.
        let edited = r#"{"id#0":1,"id#1":9}"#;
        assert_eq!(
            parse_row_edit(&cols, edited),
            Ok(vec![(1, Some("9".into()))])
        );
    }
}
