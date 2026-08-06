//! Per-field model for the grid's structured "Edit Row" / "New Row" panel.
//!
//! A [`ColSpec`] per result column carries what the panel needs — name, editability,
//! nullability, the current value, and (for a new row) an [`InsertSentinel`].
//! [`update_changes`] diffs an existing row's per-field state into the changed
//! editable columns for an `UPDATE`; [`insert_values`] assembles a new row's set
//! columns for an `INSERT` (omitting unset columns, pre-checking required ones).
//! JSON-column editing lives in [`crate::jsontree`].
//!
//! Pure + unit-tested — the UI is a thin wrapper. The DB stays the type authority:
//! validation here covers NULL / required-column / read-only rules; whether a value
//! actually fits a column's type is left to the commit (its error surfaces in the
//! panel).

use std::collections::HashMap;

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
