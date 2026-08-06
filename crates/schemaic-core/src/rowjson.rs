//! Per-field model for the grid's structured "Edit Row" panel.
//!
//! A [`ColSpec`] per result column carries what the panel needs — name, editability,
//! nullability, and the current value. [`update_changes`] diffs the panel's per-field
//! state into the changed editable columns for an `UPDATE`. JSON-column editing lives
//! in [`crate::jsontree`].
//!
//! Pure + unit-tested — the UI is a thin wrapper. The DB stays the type authority:
//! validation here covers NULL / read-only rules; whether a value actually fits a
//! column's type is left to the commit (its error surfaces in the panel).

use crate::model::Value;

/// One column's context for the row editor.
pub struct ColSpec {
    /// Column display name (the result-set header).
    pub name: String,
    /// May this column be written (i.e. go into the `UPDATE ... SET`)? Expression /
    /// binary columns are `false` — shown for context but not edited.
    pub editable: bool,
    /// May this column hold SQL `NULL`? (`!not_null`.)
    pub nullable: bool,
    /// The row's current (pre-edit) value for this column.
    pub value: Value,
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
}
