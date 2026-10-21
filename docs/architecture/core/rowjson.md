# `core/rowjson.rs`

`rowjson.rs` — the per-field model behind the grid's whole-row **view/edit panel**. A `ColSpec`
per result column carries what the panel needs (name, editability, nullability, current value);
`field_value_text` renders a cell into its editable text and `update_changes` diffs the panel's
per-field state back into the changed *editable* columns for an `UPDATE` (rejecting a read-only
edit or a NOT-NULL→null, and treating an untouched field as a no-op via normalized-text
compare). The rejection is a `FieldRefusal` naming the column — `NotEditable` or
`NotNullable`, its `Display` the English — rather than the `String` it was, which the panel
could only show in English; the grid words it (`field_refusal_text`). Validation here covers NULL and read-only rules only — **type-correctness stays the
DB's job**, and its error surfaces in the panel. Pure + unit-tested.
