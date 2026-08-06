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

/// The placeholder a column shows for a value the user hasn't set on a **new** row,
/// derived from the column's wire flags. The single source of truth for both the
/// grid's inline new-row cells and the rich row panel, so the two always agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertSentinel {
    /// Auto-increment — the DB assigns it (`<auto>`).
    Auto,
    /// NOT NULL with no default — a value is required or the `INSERT` fails
    /// ("Field 'x' doesn't have a default value") (`<required>`).
    Required,
    /// Nullable — an unset value inserts SQL `NULL` (`<null>`).
    Null,
    /// NOT NULL with an explicit `DEFAULT` — an unset value takes that default
    /// (`<default>`).
    Default,
}

impl InsertSentinel {
    /// Classify from a column's wire flags (same precedence as the grid's inline
    /// new-row cell: auto-increment → required → nullable → defaulted).
    pub fn from_flags(auto_increment: bool, no_default: bool, not_null: bool) -> Self {
        if auto_increment {
            Self::Auto
        } else if no_default {
            Self::Required
        } else if !not_null {
            Self::Null
        } else {
            Self::Default
        }
    }

    /// The `<…>` label shown in the cell / field.
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "<auto>",
            Self::Required => "<required>",
            Self::Null => "<null>",
            Self::Default => "<default>",
        }
    }

    /// Does leaving this column unset (or blank) fail the `INSERT`? Only a
    /// `<required>` column (NOT NULL, no default, not auto-increment).
    pub fn is_required(self) -> bool {
        matches!(self, Self::Required)
    }
}

/// One column's context for the row editor.
pub struct ColSpec {
    /// Column display name (the result-set header).
    pub name: String,
    /// May this column be written (i.e. go into the `UPDATE ... SET` / `INSERT`)?
    /// Expression / binary columns are `false` — shown for context but not edited.
    pub editable: bool,
    /// May this column hold SQL `NULL`? (`!not_null`.)
    pub nullable: bool,
    /// The row's current (pre-edit) value for this column. For a **new** row this is
    /// [`Value::Null`] and [`ColSpec::sentinel`] carries the placeholder.
    pub value: Value,
    /// For a **new-row** form, the column's insert placeholder (from its wire flags);
    /// `None` when editing an existing row.
    pub sentinel: Option<InsertSentinel>,
}

/// The raw editable text of a value for a per-field editor: the plain value text,
/// empty for SQL `NULL` (NULL is a distinct field *state*, not the text `""`).
pub fn field_value_text(value: &Value) -> String {
    if value.is_null() {
        String::new()
    } else {
        value.display()
    }
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

/// Diff the row panel's per-field state for an **existing** row against the
/// original values, returning the changed **editable** columns as
/// `(col_index, Option<String>)` (`None` = SQL `NULL`) — the input to
/// `build_row_edits`. `state` holds each field's current value (`None` = staged
/// NULL, `Some(text)` = a value); a column absent from `state` is untouched, and a
/// value equal to the original registers no change. Errors on a NOT-NULL→NULL
/// violation, or (as a backstop for the widget's own gate) an edit to a read-only
/// column.
pub fn update_changes(
    cols: &[ColSpec],
    state: &[(usize, Option<String>)],
) -> Result<Vec<(usize, Option<String>)>, String> {
    let mut changes = Vec::new();
    for (ci, new) in state {
        let Some(c) = cols.get(*ci) else { continue };
        let orig = (!c.value.is_null()).then(|| c.value.display());
        if *new == orig {
            continue;
        }
        if !c.editable {
            return Err(format!("Column `{}` is not editable.", c.name));
        }
        if new.is_none() && !c.nullable {
            return Err(format!("Column `{}` cannot be NULL.", c.name));
        }
        changes.push((*ci, new.clone()));
    }
    Ok(changes)
}

/// Assemble the columns to `INSERT` from the row panel's per-field state for a
/// **new** row, as `(col_index, Option<String>)` (`None` = SQL `NULL`). `state`
/// holds only the fields the user *set*; a column absent from `state` is left unset
/// → takes its DB default (auto-increment / `DEFAULT` / implicit `NULL`) and is
/// omitted from the result. Errors (before hitting the DB) if a `<required>` column
/// was left unset or blank, or an edit targets a read-only column.
pub fn insert_values(
    cols: &[ColSpec],
    state: &[(usize, Option<String>)],
) -> Result<Vec<(usize, Option<String>)>, String> {
    let set: HashMap<usize, &Option<String>> = state.iter().map(|(ci, v)| (*ci, v)).collect();
    // Every required column must be provided a non-blank value.
    for (ci, c) in cols.iter().enumerate() {
        if c.sentinel == Some(InsertSentinel::Required) {
            let ok = matches!(set.get(&ci), Some(Some(txt)) if !txt.is_empty());
            if !ok {
                return Err(format!("Column `{}` requires a value.", c.name));
            }
        }
    }
    let mut out = Vec::new();
    for (ci, v) in state {
        match cols.get(*ci) {
            Some(c) if !c.editable => {
                return Err(format!("Column `{}` is not editable.", c.name));
            }
            Some(_) => out.push((*ci, v.clone())),
            None => {}
        }
    }
    Ok(out)
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
            sentinel: None,
        }
    }

    /// A new-row column: value is NULL, carrying an insert sentinel.
    fn new_col(name: &str, editable: bool, nullable: bool, sentinel: InsertSentinel) -> ColSpec {
        ColSpec {
            name: name.to_string(),
            editable,
            nullable,
            value: Value::Null,
            sentinel: Some(sentinel),
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

    // ── sentinel vocabulary ──

    #[test]
    fn insert_sentinel_from_flags_matches_the_grid_precedence() {
        use InsertSentinel::*;
        // auto-increment wins over everything.
        assert_eq!(InsertSentinel::from_flags(true, true, true), Auto);
        // NOT NULL, no default → required.
        assert_eq!(InsertSentinel::from_flags(false, true, true), Required);
        // nullable → an unset value inserts NULL.
        assert_eq!(InsertSentinel::from_flags(false, false, false), Null);
        // NOT NULL with a default → default.
        assert_eq!(InsertSentinel::from_flags(false, false, true), Default);
        assert_eq!(Auto.label(), "<auto>");
        assert_eq!(Required.label(), "<required>");
        assert_eq!(Null.label(), "<null>");
        assert_eq!(Default.label(), "<default>");
        assert!(Required.is_required() && !Default.is_required());
    }

    #[test]
    fn field_value_text_is_empty_for_null_else_plain() {
        assert_eq!(field_value_text(&Value::Null), "");
        assert_eq!(field_value_text(&Value::Int(42)), "42");
        assert_eq!(field_value_text(&Value::Str("hi".into())), "hi");
    }

    // ── update_changes (existing row) ──

    #[test]
    fn update_changes_reports_only_real_edits() {
        let cols = sample();
        // name Kabul→Herat changes; district unchanged; pop unchanged; id untouched.
        let state = vec![
            (1, Some("Herat".to_string())),
            (2, Some("Kabol".to_string())),
            (3, Some("19.99".to_string())),
        ];
        assert_eq!(
            update_changes(&cols, &state),
            Ok(vec![(1, Some("Herat".into()))])
        );
    }

    #[test]
    fn update_changes_null_rules() {
        let cols = sample();
        // Nullable district → NULL is a staged change.
        assert_eq!(update_changes(&cols, &[(2, None)]), Ok(vec![(2, None)]));
        // NOT-NULL name → NULL is rejected.
        let err = update_changes(&cols, &[(1, None)]).unwrap_err();
        assert!(err.contains("`name`") && err.contains("NULL"), "{err}");
    }

    #[test]
    fn update_changes_backstops_readonly_edit() {
        let cols = sample();
        // id is read-only; a changed value is rejected (widget should've blocked it).
        let err = update_changes(&cols, &[(0, Some("2".into()))]).unwrap_err();
        assert!(
            err.contains("`id`") && err.contains("not editable"),
            "{err}"
        );
        // But an unchanged read-only field is a no-op, not an error.
        assert_eq!(update_changes(&cols, &[(0, Some("1".into()))]), Ok(vec![]));
    }

    // ── insert_values (new row) ──

    #[test]
    fn insert_values_omits_unset_and_keeps_set() {
        let cols = vec![
            new_col("id", true, false, InsertSentinel::Auto),
            new_col("name", true, false, InsertSentinel::Required),
            new_col("region", true, true, InsertSentinel::Null),
        ];
        // User set name only; id/region left unset (omitted → server default/NULL).
        let state = vec![(1, Some("Herat".to_string()))];
        assert_eq!(
            insert_values(&cols, &state),
            Ok(vec![(1, Some("Herat".into()))])
        );
    }

    #[test]
    fn insert_values_requires_required_columns() {
        let cols = vec![
            new_col("id", true, false, InsertSentinel::Auto),
            new_col("name", true, false, InsertSentinel::Required),
        ];
        // name (required) left unset → error before hitting the DB.
        let err = insert_values(&cols, &[]).unwrap_err();
        assert!(err.contains("`name`") && err.contains("requires"), "{err}");
        // A blank (empty) value doesn't satisfy a required column either.
        let err2 = insert_values(&cols, &[(1, Some(String::new()))]).unwrap_err();
        assert!(err2.contains("`name`"), "{err2}");
        // A real value satisfies it.
        assert_eq!(
            insert_values(&cols, &[(1, Some("x".into()))]),
            Ok(vec![(1, Some("x".into()))])
        );
    }

    #[test]
    fn insert_values_allows_explicit_null_on_nullable() {
        let cols = vec![new_col("region", true, true, InsertSentinel::Null)];
        // Activating a nullable field and choosing NULL explicitly is fine.
        assert_eq!(insert_values(&cols, &[(0, None)]), Ok(vec![(0, None)]));
    }
}
