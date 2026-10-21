# `ui/consts.rs`

`consts.rs` — layout/dimension metrics + `MONO_FAMILY` (glob-imported). Any SQL/code
surface reads that one name — the snippet library's preview, and `FieldCfg::mono` (the DDL
preview's script box, the view editor's definition). Most of the file is `fn() -> f64` rather than
`const`, because the interface scale multiplies anything that boxes text; the module doc lists
what stays a `const` and why — hairlines, editor-relative metrics, seeds for persisted widths,
icon bases, `TERM_FONT_SIZES`, and the two floating-bar insets — and that list is still the
explanation, but **the exemptions are data now**: `unscaled_const_gate` scans this file's own
production code for `const <NAME>: f32|f64` and fails unless each is in `EXEMPT` with the bullet
it belongs to and why. The prose could not fail, and it was found short five times — three by
hand, then `TOOLBAR_ICON_BASE`, a third icon base whose own doc said it was one while the "Icon
bases" bullet did not know it existed, and `COMPLETION_LINE_H`, named by no bullet at all. A
sixth was mis-filed rather than missing: `TREE_ROW_MIN_W` sat under "seeds for persisted,
user-dragged sizes" and is not one — nothing persists or drags it; it is a floor on a signal
carrying unscaled user px, which is a different reason for the same answer. The array carries
the liveness floor `float_inset_gate` already has, so a rename cannot leave a stale licence
behind for the next constant at that spelling; dropping `TOOLBAR_ICON_BASE` from it reports that
name.
`field_input_h()` is the same idea for the
**compact single-line field** every transient bar wears (the editor's find/replace/goto, the
grid's find/goto, the row panel's inputs): `FieldCfg::height` is an `Option`, and leaving it
off is not a neutral default but a *different* control — `None` derives the box from content
at `line_h + chat_pad_v() * 2 + 3`, which is 34px against this 26. The grid's find bar shipped
without it and stood 8px taller than the identical editor bar beside it, so a bar that means
to be compact says so with this metric and never with a literal.
