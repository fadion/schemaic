//! The editor for PostgreSQL's standalone objects: an enum type, a domain, or a
//! sequence.
//!
//! One modal for three objects, on the same grounds `trigger_editor` holds two:
//! the chrome, the footer, the change count and the ending at
//! [`ddl_preview`](crate::ddl_preview) are identical, and only the middle section
//! differs — because the objects do. Nothing here is a designer tab, for the
//! reason written into `trigger_editor`: what belongs there is what can be a
//! *clause* of `ALTER TABLE`, and none of these can.
//!
//! Three rules are written down because each was a bug waiting:
//!
//! * **The form is built once per open**, and its list rows are keyed on
//!   `object_rev` — a *structural* counter — not on the draft. A draft-keyed
//!   field is torn down mid-keystroke, and a row keyed on nothing at all would
//!   keep showing the removed item's text after its neighbour shifted up. Same
//!   contract as the designer's detail form.
//! * **An enum's values are edited as rows, not as one newline-separated box.**
//!   A PostgreSQL enum label is arbitrary text and really can contain a newline
//!   — the introspection fixture has one — so a line-per-value box would split
//!   such a label in two and then *rebuild the type around the split*, which
//!   destroys data on apply rather than failing.
//! * **A sequence's numbers are typed, and the draft holds `i64`.** A half-typed
//!   `-` has nowhere to live in the model, so the parse failure goes to
//!   `object_errors` beside the draft and blocks the preview from there. Writing
//!   nothing back on a failed parse would silently ignore what somebody typed.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::ddl::{self, DomainDraft, EnumDraft, ObjectDraft, ObjectKind, SequenceDraft};
use schemaic_core::schema::{CheckInfo, ObjectItem, SchemaState, SequenceInfo, SequenceOwner};

use crate::table_designer::{edit_ctx, focusable_owned_dropdown, suggest_chevron};
use crate::widgets::{
    ACTION_GAP, ActionKind, FORM_GAP, FocusRing, MODAL_PAD_H, action_button, focus_root_with_ring,
    form_section, form_setting, form_setting_owned, modal_footer_split, modal_title_owned,
    panel_style, row_button, row_gap,
};
use crate::{
    DdlPreview, FieldCfg, ObjectTarget, Ui, ddl_preview, edit_field, icons, object_location, theme,
};

const PANEL_W: f64 = 700.0;
const PANEL_H: f64 = 620.0;
/// Text-field width, matching the designer's and the view editor's.
const FIELD_W: f64 = 260.0;
/// Narrower, for the numbers a sequence is made of — a 20-character box for a
/// value that is nearly always one or two digits reads as a text field.
const NUM_W: f64 = 130.0;
/// Where a repeating row's Tab stops start: an enum's values and a domain's
/// checks are lists that grow, so they claim a block of their own above every
/// fixed control in the form.
const VALUE_TAB: u32 = 1000;
/// The gap between two of those, side by side. Wider than [`FORM_GAP`] because
/// it separates two *questions* rather than two rows of one: Increment and Start
/// sitting a form's gap apart read as one control with two boxes.
const NUM_GAP: f64 = 50.0;

// ── opening ──────────────────────────────────────────────────────────────────

fn open_editor(ui: &Ui, target: ObjectTarget, draft: ObjectDraft) {
    let d = ui.ddl;
    // A new editing session — see `DdlUi::session`.
    d.session.update(|g| *g += 1);
    d.object_draft.set(draft);
    d.object_errors.set(Vec::new());
    d.object_rev.update(|r| *r += 1);
    d.error.set(None);
    d.preview.set(None);
    // Every editor shares the preview stacked on top, and each overlay only
    // knows its own flag — two open would paint two panels.
    d.designer.set(None);
    d.view.set(None);
    d.trigger.set(None);
    d.object.set(Some(target));
}

/// The introspected schema of one database, when it has loaded.
fn loaded_schema(
    ui: &Ui,
    database: &str,
) -> Option<std::sync::Arc<schemaic_core::schema::DbSchema>> {
    ui.schema.db_nodes.with_untracked(|nodes| {
        nodes
            .iter()
            .find(|n| n.database == database)
            .and_then(|n| match n.schema.get_untracked() {
                SchemaState::Loaded(s) => Some(s),
                _ => None,
            })
    })
}

/// Open the editor on an existing object.
///
/// The dependents are read **now**, off the schema the tree is showing: they are
/// the columns a rebuild would re-cast, found by the type's *current* name, which
/// the draft may be about to change.
pub(crate) fn open_for_object(ui: &Ui, database: &str, item: &ObjectItem) {
    let ctx = edit_ctx(ui);
    let dependents = match loaded_schema(ui, database) {
        Some(s) => ddl::type_dependents(&s, item.schema(), item.name()),
        None => Vec::new(),
    };
    open_editor(
        ui,
        ObjectTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            schema: item.schema().map(str::to_string),
            dialect: ctx.dialect,
            current: Some(item.clone()),
            dependents,
            read_only: ctx.read_only,
        },
        ObjectDraft::from_item(item),
    );
}

/// Open the editor on a blank draft — Create type / domain / sequence.
pub(crate) fn open_for_new(ui: &Ui, database: &str, schema: Option<&str>, kind: ObjectKind) {
    let ctx = edit_ctx(ui);
    let name = match kind {
        ObjectKind::Enum => "new_type",
        ObjectKind::Domain => "new_domain",
        ObjectKind::Sequence => "new_sequence",
    };
    open_editor(
        ui,
        ObjectTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            schema: schema.map(str::to_string),
            dialect: ctx.dialect,
            current: None,
            dependents: Vec::new(),
            read_only: ctx.read_only,
        },
        ObjectDraft::blank(kind, name, schema.map(str::to_string)),
    );
}

/// Whether this tree node is an object Schemaic can edit — the entry point's
/// enabled test.
///
/// An identity column's counter is *shown* and can even be altered, but it is
/// part of its column rather than an object of its own; opening an editor whose
/// only irreversible action the server refuses would be a dead end, the call a
/// materialized view gets.
pub(crate) fn is_editable_object(item: &ObjectItem) -> bool {
    !item.is_internal()
}

// ── binding helpers ──────────────────────────────────────────────────────────

/// A text field bound to one place in the draft.
///
/// Same contract as the view editor's: the local signal is seeded once on build
/// and the effect writes back only on a genuine change, so a rebuild can't read
/// as an edit.
fn bound_field(
    ui: &Ui,
    initial: String,
    cfg: FieldCfg,
    apply: impl Fn(&mut ObjectDraft, &str) + 'static,
) -> AnyView {
    let draft = ui.ddl.object_draft;
    let sig = floem::reactive::create_rw_signal(initial);
    create_effect(move |prev: Option<String>| {
        let v = sig.get();
        if prev.is_some_and(|p| p != v) {
            draft.update(|d| apply(d, &v));
        }
        v
    });
    edit_field(sig, cfg).into_any()
}

/// A field holding one of a sequence's numbers.
///
/// The draft holds `i64`, so text that isn't a number yet has nowhere to live in
/// it. Rather than drop the keystroke, the message goes to `object_errors` — the
/// signal the footer's status line and its readiness test both read — and is
/// withdrawn as soon as the text parses. One message per field, keyed by its
/// label, so two bad fields report two problems and neither erases the other.
fn num_field(
    ui: &Ui,
    label: &'static str,
    initial: String,
    ring: FocusRing,
    tabindex: u32,
    apply: impl Fn(&mut SequenceDraft, &str) -> Result<(), ()> + 'static,
) -> AnyView {
    let draft = ui.ddl.object_draft;
    let errs = ui.ddl.object_errors;
    let sig = floem::reactive::create_rw_signal(initial);
    create_effect(move |prev: Option<String>| {
        let v = sig.get();
        if prev.is_some_and(|p| p != v) {
            let msg = format!("{label} has to be a whole number.");
            let ok = draft.try_update(|d| match d {
                ObjectDraft::Sequence(s) => apply(s, v.trim()),
                _ => Ok(()),
            });
            if matches!(ok, Some(Ok(()))) {
                errs.update(|e| e.retain(|m| m != &msg));
            } else {
                errs.update(|e| {
                    if !e.contains(&msg) {
                        e.push(msg.clone());
                    }
                });
            }
        }
        v
    });
    edit_field(
        sig,
        FieldCfg {
            placeholder: "",
            focus: Some((ring, tabindex)),
            ..Default::default()
        },
    )
    .style(|s| s.width(NUM_W))
    .into_any()
}

/// A toggle bound to the draft.
fn bound_toggle(
    ui: &Ui,
    title: &'static str,
    hint: &'static str,
    initial: bool,
    ring: FocusRing,
    tabindex: u32,
    apply: impl Fn(&mut ObjectDraft, bool) + 'static,
) -> AnyView {
    let draft = ui.ddl.object_draft;
    let sig = floem::reactive::create_rw_signal(initial);
    create_effect(move |prev: Option<bool>| {
        let v = sig.get();
        if prev.is_some_and(|p| p != v) {
            draft.update(|d| apply(d, v));
        }
        v
    });
    crate::settings::focusable_toggle_row(title, hint, sig, ring, tabindex).into_any()
}

// ── the enum form ────────────────────────────────────────────────────────────

/// The value list: one editable row per value, with move and remove beside it.
///
/// Rows rather than a newline-separated box because a label may *contain* a
/// newline. Order is editable because it is the type's comparison order — and
/// because reordering is one of the two edits that force a rebuild, so being
/// able to do it at all is the difference between an editor and an append box.
/// `ring` places each row's field in the modal's Tab order, one stop per value
/// from `VALUE_TAB` upwards; the move/remove buttons stay pointer-only, like
/// every other button here.
fn enum_values(ui: &Ui, ring: FocusRing) -> AnyView {
    let draft = ui.ddl.object_draft;
    let rev = ui.ddl.object_rev;
    let ui = ui.clone();
    dyn_container(
        // Structural only: adding, removing or moving rebuilds the rows; typing
        // into one must not, or the field being typed into is torn down.
        move || rev.get(),
        move |_| {
            let values: Vec<String> = draft.with_untracked(|d| match d {
                ObjectDraft::Enum(e) => e.info.values.clone(),
                _ => Vec::new(),
            });
            let n = values.len();
            let ui = ui.clone();
            let ring = ring.clone();
            let rows = v_stack_from_iter(values.into_iter().enumerate().map(move |(i, v)| {
                let field = bound_field(
                    &ui,
                    v,
                    FieldCfg {
                        placeholder: "value",
                        focus: Some((ring.clone(), VALUE_TAB + i as u32)),
                        ..Default::default()
                    },
                    move |d, t| {
                        if let ObjectDraft::Enum(e) = d
                            && let Some(slot) = e.info.values.get_mut(i)
                        {
                            *slot = t.to_string();
                        }
                    },
                )
                .style(|s| s.flex_grow(1.0_f32).min_width(0.0));
                let swap = move |a: usize, b: usize| {
                    draft.update(|d| {
                        if let ObjectDraft::Enum(e) = d
                            && a < e.info.values.len()
                            && b < e.info.values.len()
                        {
                            e.info.values.swap(a, b);
                        }
                    });
                    rev.update(|r| *r += 1);
                };
                // The arrow a row can't act on isn't offered: the first value is
                // already first and the last is already last, so an arrow there
                // is a control whose only outcome is nothing happening.
                let up = if i > 0 {
                    row_button(icons::CHEVRON_UP, "Move up", move || swap(i, i - 1))
                } else {
                    row_gap()
                };
                let down = if i + 1 < n {
                    row_button(icons::CHEVRON_DOWN, "Move down", move || swap(i, i + 1))
                } else {
                    row_gap()
                };
                h_stack((
                    field,
                    up,
                    down,
                    row_button(icons::TRASH_2, "Remove", move || {
                        draft.update(|d| {
                            if let ObjectDraft::Enum(e) = d
                                && i < e.info.values.len()
                            {
                                e.info.values.remove(i);
                            }
                        });
                        rev.update(|r| *r += 1);
                    }),
                ))
                .style(|s| s.flex_row().items_center().gap(2.0).width_full())
            }))
            .style(|s| s.flex_col().gap(6.0).width_full());

            let add = crate::widgets::control_button("Add value", move || {
                draft.update(|d| {
                    if let ObjectDraft::Enum(e) = d {
                        e.info.values.push(String::new());
                    }
                });
                rev.update(|r| *r += 1);
            });
            v_stack((
                rows,
                container(add).style(|s| s.width_full().margin_top(2.0)),
            ))
            .style(|s| s.flex_col().gap(6.0).width_full())
            .into_any()
        },
    )
    .style(|s| s.width_full())
    .into_any()
}

fn enum_form(ui: &Ui, d: &EnumDraft, ring: FocusRing) -> AnyView {
    let name = form_setting(
        "Name",
        bound_field(
            ui,
            d.info.name.clone(),
            FieldCfg {
                placeholder: "type_name",
                focus: Some((ring.clone(), 10)),
                ..Default::default()
            },
            |d, v| {
                if let ObjectDraft::Enum(e) = d {
                    e.info.name = v.trim().to_string();
                }
            },
        )
        .style(move |s| s.width(FIELD_W)),
    );
    let comment = form_setting(
        "Comment",
        bound_field(
            ui,
            d.info.comment.clone().unwrap_or_default(),
            FieldCfg {
                placeholder: "—",
                focus: Some((ring.clone(), 20)),
                ..Default::default()
            },
            |d, v| {
                if let ObjectDraft::Enum(e) = d {
                    e.info.comment = Some(v.to_string()).filter(|s| !s.is_empty());
                }
            },
        )
        .style(|s| s.width_full()),
    );
    v_stack((
        form_section("Type"),
        name,
        comment,
        form_section("Values").style(|s| s.margin_top(4.0)),
        form_setting_owned(
            "In comparison order — this is the order ORDER BY uses".to_string(),
            enum_values(ui, ring),
        ),
    ))
    .style(|s| s.flex_col().gap(FORM_GAP).width_full())
    .into_any()
}

// ── the domain form ──────────────────────────────────────────────────────────

/// A domain's `CHECK` constraints: a name and a predicate per row.
///
/// Unordered, so there are no move buttons — unlike an enum's values, where the
/// order is the type's meaning.
/// Each row takes two Tab stops — its name then its predicate — from `VALUE_TAB`
/// upwards.
fn domain_checks(ui: &Ui, ring: FocusRing) -> AnyView {
    let draft = ui.ddl.object_draft;
    let rev = ui.ddl.object_rev;
    let ui = ui.clone();
    dyn_container(
        move || rev.get(),
        move |_| {
            let checks: Vec<CheckInfo> = draft.with_untracked(|d| match d {
                ObjectDraft::Domain(dom) => dom.info.checks.clone(),
                _ => Vec::new(),
            });
            let ui = ui.clone();
            let ring = ring.clone();
            let rows = v_stack_from_iter(checks.into_iter().enumerate().map(move |(i, ck)| {
                let name = bound_field(
                    &ui,
                    ck.name.clone(),
                    FieldCfg {
                        placeholder: "constraint_name",
                        focus: Some((ring.clone(), VALUE_TAB + i as u32 * 2)),
                        ..Default::default()
                    },
                    move |d, t| {
                        if let ObjectDraft::Domain(dom) = d
                            && let Some(c) = dom.info.checks.get_mut(i)
                        {
                            c.name = t.trim().to_string();
                        }
                    },
                )
                .style(|s| s.width(190.0).flex_shrink(0.0_f32));
                let expr = bound_field(
                    &ui,
                    ck.expression.clone(),
                    FieldCfg {
                        placeholder: "VALUE > 0",
                        mono: true,
                        focus: Some((ring.clone(), VALUE_TAB + i as u32 * 2 + 1)),
                        ..Default::default()
                    },
                    move |d, t| {
                        if let ObjectDraft::Domain(dom) = d
                            && let Some(c) = dom.info.checks.get_mut(i)
                        {
                            c.expression = t.to_string();
                        }
                    },
                )
                .style(|s| s.flex_grow(1.0_f32).min_width(0.0));
                h_stack((
                    name,
                    expr,
                    row_button(icons::TRASH_2, "Remove", move || {
                        draft.update(|d| {
                            if let ObjectDraft::Domain(dom) = d
                                && i < dom.info.checks.len()
                            {
                                dom.info.checks.remove(i);
                            }
                        });
                        rev.update(|r| *r += 1);
                    }),
                ))
                .style(|s| s.flex_row().items_center().gap(6.0).width_full())
            }))
            .style(|s| s.flex_col().gap(6.0).width_full());

            let add = crate::widgets::control_button("Add constraint", move || {
                draft.update(|d| {
                    if let ObjectDraft::Domain(dom) = d {
                        // The next free suffix, not `len + 1`: removing a
                        // constraint and adding another re-proposed a name
                        // still in the list, and PostgreSQL always rejects
                        // that plan (`constraint "email_check2" for domain
                        // "email" already exists`).
                        let stem = format!("{}_check", dom.info.name);
                        let n = (1..)
                            .find(|i| {
                                !dom.info
                                    .checks
                                    .iter()
                                    .any(|c| c.name == format!("{stem}{i}"))
                            })
                            .unwrap_or(1);
                        dom.info.checks.push(CheckInfo {
                            name: format!("{stem}{n}"),
                            expression: "VALUE IS NOT NULL".to_string(),
                            ..Default::default()
                        });
                    }
                });
                rev.update(|r| *r += 1);
            });
            v_stack((
                rows,
                container(add).style(|s| s.width_full().margin_top(2.0)),
            ))
            .style(|s| s.flex_col().gap(6.0).width_full())
            .into_any()
        },
    )
    .style(|s| s.width_full())
    .into_any()
}

fn domain_form(
    ui: &Ui,
    d: &DomainDraft,
    dialect: schemaic_core::intel::SqlDialect,
    ring: FocusRing,
) -> AnyView {
    let draft = ui.ddl.object_draft;
    let name = form_setting(
        "Name",
        bound_field(
            ui,
            d.info.name.clone(),
            FieldCfg {
                placeholder: "domain_name",
                focus: Some((ring.clone(), 10)),
                ..Default::default()
            },
            |d, v| {
                if let ObjectDraft::Domain(dom) = d {
                    dom.info.name = v.trim().to_string();
                }
            },
        )
        .style(move |s| s.width(FIELD_W)),
    );
    // A dropdown of the usual types, but writable: a domain can be built on
    // anything the server has, including another domain and an array.
    let base = {
        let sig = floem::reactive::create_rw_signal(d.info.base_type.clone());
        create_effect(move |prev: Option<String>| {
            let v = sig.get();
            if prev.is_some_and(|p| p != v) {
                draft.update(|d| {
                    if let ObjectDraft::Domain(dom) = d {
                        dom.info.base_type = v.trim().to_string();
                    }
                });
            }
            v
        });
        form_setting(
            "Type",
            // The same control the designer's column type wears: a free-text box
            // with a chevron offering the usual types. It was a `Dropdown` parked
            // beside the field, which said the same thing in a second shape — and
            // a domain can be built on a type no fixed list holds (another
            // domain, an array, an extension's), so the box is the answer and the
            // menu is only a shortcut into it.
            h_stack((
                edit_field(
                    sig,
                    FieldCfg {
                        placeholder: "text",
                        focus: Some((ring.clone(), 20)),
                        ..Default::default()
                    },
                )
                .style(move |s| s.width(FIELD_W)),
                suggest_chevron(
                    ui,
                    sig,
                    ddl::common_types(dialect)
                        .iter()
                        .map(|t| t.to_string())
                        .collect(),
                ),
            ))
            .style(|s| s.flex_row().items_center().gap(2.0)),
        )
    };
    let default = form_setting(
        "Default",
        bound_field(
            ui,
            d.info.default_value.clone().unwrap_or_default(),
            FieldCfg {
                placeholder: "—",
                mono: true,
                focus: Some((ring.clone(), 30)),
                ..Default::default()
            },
            |d, v| {
                if let ObjectDraft::Domain(dom) = d {
                    // Blank is *no* default. A domain really can default to the
                    // empty string, but `''` is how that is written in SQL — an
                    // empty box is the absence of a clause.
                    dom.info.default_value = Some(v.trim().to_string()).filter(|s| !s.is_empty());
                }
            },
        )
        .style(move |s| s.width(FIELD_W)),
    );
    let not_null = bound_toggle(
        ui,
        "NOT NULL",
        "Every column of this domain rejects NULL. Turning it on fails if one already holds one.",
        d.info.not_null,
        ring.clone(),
        40,
        |d, v| {
            if let ObjectDraft::Domain(dom) = d {
                dom.info.not_null = v;
            }
        },
    );
    let comment = form_setting(
        "Comment",
        bound_field(
            ui,
            d.info.comment.clone().unwrap_or_default(),
            FieldCfg {
                placeholder: "—",
                focus: Some((ring.clone(), 50)),
                ..Default::default()
            },
            |d, v| {
                if let ObjectDraft::Domain(dom) = d {
                    dom.info.comment = Some(v.to_string()).filter(|s| !s.is_empty());
                }
            },
        )
        .style(|s| s.width_full()),
    );
    v_stack((
        form_section("Domain"),
        name,
        base,
        default,
        not_null,
        comment,
        form_section("Constraints").style(|s| s.margin_top(4.0)),
        domain_checks(ui, ring),
    ))
    .style(|s| s.flex_col().gap(FORM_GAP).width_full())
    .into_any()
}

// ── the sequence form ────────────────────────────────────────────────────────

/// What "Detach from its column" writes to the draft, in **both** directions.
///
/// The `false` direction is the one that was missing: the toggle applied only
/// on `true`, so unticking it left `OWNED BY NONE` in the plan with the control
/// reading *off*. There is nowhere else to recover the owner from — the editor
/// offers no owner picker — so the original comes from `ObjectTarget::current`
/// rather than from the draft it is about to overwrite.
pub(crate) fn detach_apply(orig: Option<&SequenceOwner>, detached: bool) -> Option<SequenceOwner> {
    if detached { None } else { orig.cloned() }
}

/// The sequence a target holds, for reading state the draft may have cleared.
fn target_sequence(target: &ObjectTarget) -> Option<&SequenceInfo> {
    match target.current.as_ref() {
        Some(ObjectItem::Sequence(s)) => Some(s),
        _ => None,
    }
}

fn sequence_form(
    ui: &Ui,
    d: &SequenceDraft,
    orig_owner: Option<SequenceOwner>,
    ring: FocusRing,
) -> AnyView {
    let draft = ui.ddl.object_draft;
    let name = form_setting(
        "Name",
        bound_field(
            ui,
            d.info.name.clone(),
            FieldCfg {
                placeholder: "sequence_name",
                focus: Some((ring.clone(), 10)),
                ..Default::default()
            },
            |d, v| {
                if let ObjectDraft::Sequence(s) = d {
                    s.info.name = v.trim().to_string();
                }
            },
        )
        .style(move |s| s.width(FIELD_W)),
    );
    // The bounds are clamped to the storage type, so changing it re-checks them
    // — `SequenceDraft::validate` is what reports a range that no longer fits.
    let data_type = {
        let sig = floem::reactive::create_rw_signal(d.info.data_type.clone());
        form_setting(
            "Stores",
            focusable_owned_dropdown(
                move || sig.get(),
                ["smallint", "integer", "bigint"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                FIELD_W,
                ring.clone(),
                20,
                move |v: String| {
                    if sig.get_untracked() != v {
                        sig.set(v.clone());
                        draft.update(|d| {
                            if let ObjectDraft::Sequence(s) = d {
                                s.info.data_type = v.clone();
                            }
                        });
                    }
                },
            ),
        )
    };
    // Each number gets exactly its field's width, so the row's `gap` is the gap
    // that lands on screen. `form_setting` is `width_full`, which in a flex row
    // means "an equal share" — so the three columns spread across the panel and
    // the spacing between a 130 px box and the next label came out at whatever
    // the leftovers divided into, twice what it was asked to be and different on
    // the two-field row from the three-field one.
    let num_col = |label: &'static str, field: AnyView| {
        form_setting(label, field).style(|s| s.width(NUM_W).flex_shrink(0.0_f32))
    };
    let numbers = h_stack((
        num_col(
            "Increment",
            num_field(
                ui,
                "Increment",
                d.info.increment.to_string(),
                ring.clone(),
                30,
                |s, t| t.parse().map(|n| s.info.increment = n).map_err(|_| ()),
            ),
        ),
        num_col(
            "Start",
            num_field(
                ui,
                "Start",
                d.info.start.to_string(),
                ring.clone(),
                40,
                |s, t| t.parse().map(|n| s.info.start = n).map_err(|_| ()),
            ),
        ),
        num_col(
            "Cache",
            num_field(
                ui,
                "Cache",
                d.info.cache.to_string(),
                ring.clone(),
                50,
                |s, t| t.parse().map(|n| s.info.cache = n).map_err(|_| ()),
            ),
        ),
    ))
    .style(|s| s.flex_row().gap(NUM_GAP).width_full());
    let bounds = h_stack((
        num_col(
            "Minimum",
            num_field(
                ui,
                "Minimum",
                d.info.min_value.to_string(),
                ring.clone(),
                60,
                |s, t| t.parse().map(|n| s.info.min_value = n).map_err(|_| ()),
            ),
        ),
        num_col(
            "Maximum",
            num_field(
                ui,
                "Maximum",
                d.info.max_value.to_string(),
                ring.clone(),
                70,
                |s, t| t.parse().map(|n| s.info.max_value = n).map_err(|_| ()),
            ),
        ),
    ))
    .style(|s| s.flex_row().gap(NUM_GAP).width_full());
    let cycle = bound_toggle(
        ui,
        "Cycle",
        "Wrap around to the other end of the range instead of failing when the sequence runs out.",
        d.info.cycle,
        ring.clone(),
        80,
        |d, v| {
            if let ObjectDraft::Sequence(s) = d {
                s.info.cycle = v;
            }
        },
    );

    // Where the counter is, and where it could be moved to. `RESTART` is an
    // action rather than a state, so this box is empty on open however many
    // times the editor is re-opened.
    let position = form_setting_owned(
        match d.info.last_value {
            Some(v) => format!("Restart at — the counter last handed out {v}"),
            None => "Restart at — the counter has not been used".to_string(),
        },
        {
            let errs = ui.ddl.object_errors;
            let sig = floem::reactive::create_rw_signal(String::new());
            create_effect(move |prev: Option<String>| {
                let v = sig.get();
                if prev.is_some_and(|p| p != v) {
                    let msg = "Restart at has to be a whole number.".to_string();
                    let t = v.trim();
                    let parsed = if t.is_empty() {
                        Some(None)
                    } else {
                        t.parse::<i64>().ok().map(Some)
                    };
                    match parsed {
                        Some(r) => {
                            errs.update(|e| e.retain(|m| m != &msg));
                            draft.update(|d| {
                                if let ObjectDraft::Sequence(s) = d {
                                    s.restart = r;
                                }
                            });
                        }
                        None => errs.update(|e| {
                            if !e.contains(&msg) {
                                e.push(msg.clone());
                            }
                        }),
                    }
                }
                v
            });
            edit_field(
                sig,
                FieldCfg {
                    placeholder: "leave empty to keep the position",
                    focus: Some((ring.clone(), 100)),
                    ..Default::default()
                },
            )
            .style(|s| s.width(FIELD_W))
        },
    );

    // The owner is context, and detaching is the one change to it worth a
    // control: re-pointing a sequence at a different column needs a table and a
    // column picker for something almost nobody does, and `OWNED BY` is what
    // makes the sequence get dropped with its column.
    //
    // Rendered from the **server's** owner, not the draft's: the draft is what
    // the toggle clears, so reading it here made the row and the toggle vanish
    // the moment the form rebuilt — leaving a pending `OWNED BY NONE` in the
    // change count with nothing on screen that named it or could undo it.
    let owner: AnyView = match orig_owner.clone() {
        Some(o) => v_stack((
            form_setting_owned(
                "Owned by".to_string(),
                text(format!("{}.{}", o.table, o.column))
                    .style(|s| s.color(theme::text_dim()).font_size(theme::FONT_BODY)),
            ),
            bound_toggle(
                ui,
                "Detach from its column",
                "The sequence stops being dropped with the column, and outlives it.",
                d.info.owned_by.is_none(),
                ring.clone(),
                110,
                move |d, v| {
                    if let ObjectDraft::Sequence(s) = d {
                        s.info.owned_by = detach_apply(orig_owner.as_ref(), v);
                    }
                },
            ),
        ))
        .style(|s| s.flex_col().gap(FORM_GAP).width_full())
        .into_any(),
        None => empty().into_any(),
    };

    let comment = form_setting(
        "Comment",
        bound_field(
            ui,
            d.info.comment.clone().unwrap_or_default(),
            FieldCfg {
                placeholder: "—",
                focus: Some((ring, 90)),
                ..Default::default()
            },
            |d, v| {
                if let ObjectDraft::Sequence(s) = d {
                    s.info.comment = Some(v.to_string()).filter(|s| !s.is_empty());
                }
            },
        )
        .style(|s| s.width_full()),
    );

    v_stack((
        form_section("Sequence"),
        name,
        data_type,
        numbers,
        bounds,
        cycle,
        comment,
        form_section("Position").style(|s| s.margin_top(4.0)),
        position,
        owner,
    ))
    .style(|s| s.flex_col().gap(FORM_GAP).width_full())
    .into_any()
}

// ── the modal ────────────────────────────────────────────────────────────────

fn form(ui: &Ui, target: &ObjectTarget, ring: FocusRing) -> AnyView {
    let draft = ui.ddl.object_draft.get_untracked();
    // No "In {database}" row: the modal title names the place now.
    let body = match &draft {
        ObjectDraft::Enum(d) => enum_form(ui, d, ring),
        ObjectDraft::Domain(d) => domain_form(ui, d, target.dialect, ring),
        ObjectDraft::Sequence(d) => sequence_form(
            ui,
            d,
            target_sequence(target).and_then(|s| s.owned_by.clone()),
            ring,
        ),
    };
    v_stack((body,))
        .style(|s| s.flex_col().gap(FORM_GAP).width_full())
        .into_any()
}

/// A new object has no server-side name to be titled after, so the preview is
/// headed by the one the draft is giving it.
fn preview_from(target: &ObjectTarget, draft: &ObjectDraft, cs: &ddl::ChangeSet) -> DdlPreview {
    let subject = match target.current {
        Some(_) => target.display(),
        None => draft.name().to_string(),
    };
    ddl_preview::preview_of(
        target.conn_id,
        &target.database,
        subject,
        cs,
        target.read_only,
    )
}

fn change_set(target: &ObjectTarget, draft: &ObjectDraft) -> ddl::ChangeSet {
    draft.change_set(target.current.as_ref(), &target.dependents, target.dialect)
}

/// The object editor. Absolutely positioned over the workspace when
/// `ui.ddl.object` is `Some`.
pub(crate) fn object_editor_overlay(ui: Ui) -> impl IntoView {
    let d = ui.ddl;
    let close = move || d.object.set(None);

    dyn_container(
        // The preview stacks on top and this stays open behind it (Cancel there
        // returns here with the draft intact), but must render nothing — same
        // pairing as every other editor, for the same reason.
        move || (d.object.get().is_some(), d.preview.get().is_some()),
        move |(open, previewing)| {
            if !open || previewing {
                return empty().into_any();
            }
            let Some(target) = d.object.get_untracked() else {
                return empty().into_any();
            };
            let ui = ui.clone();
            let kind = d.object_draft.with_untracked(|dr| dr.kind());
            let location = object_location(&target.database, target.schema.as_deref());
            let title = match &target.current {
                Some(c) => format!("Edit {} {location}.{}", kind.label(), c.name()),
                None => format!("Create {} in {location}", kind.label()),
            };

            // One ring for the modal: the value/constraint rows rebuild on
            // `object_rev` and re-register themselves, so the root — built once,
            // out here — keeps a stable handle on the current set.
            let ring = FocusRing::new();
            let root_ring = ring.clone();

            let body = crate::widgets::autohide(scroll(
                form(&ui, &target, ring)
                    .style(|s| s.width_full().padding_horiz(MODAL_PAD_H).padding_vert(18.0)),
            ))
            .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0));

            // Validation first (it blocks), then the change count. A field whose
            // text isn't a number yet is a validation failure like any other.
            let status_target = target.clone();
            let status = dyn_container(
                move || (d.object_draft.get(), d.object_errors.get()),
                move |(draft, errs)| {
                    let first = errs.first().cloned().or_else(|| draft.validate().pop());
                    if let Some(first) = first {
                        return text(first)
                            .style(|s| {
                                s.color(theme::error())
                                    .font_size(theme::FONT_LABEL)
                                    .max_width(420.0)
                            })
                            .into_any();
                    }
                    let n = change_set(&status_target, &draft).len();
                    text(match n {
                        0 => "No changes".to_string(),
                        1 => "1 change".to_string(),
                        n => format!("{n} changes"),
                    })
                    .style(move |s| {
                        s.font_size(theme::FONT_LABEL).color(if n == 0 {
                            theme::text_faint()
                        } else {
                            theme::change_count()
                        })
                    })
                    .into_any()
                },
            );

            let preview_ui = ui.clone();
            let preview_target = target.clone();
            let actions = dyn_container(
                move || (d.object_draft.get(), d.object_errors.get()),
                move |(draft, errs)| {
                    let ui = preview_ui.clone();
                    let target = preview_target.clone();
                    let cs = change_set(&target, &draft);
                    let ready = errs.is_empty() && draft.validate().is_empty() && !cs.is_empty();
                    h_stack((
                        action_button("Cancel", ActionKind::Neutral, true, close),
                        action_button("Preview SQL", ActionKind::Primary, ready, move || {
                            let cs = change_set(&target, &draft);
                            ddl_preview::open_preview(&ui, preview_from(&target, &draft, &cs));
                        }),
                    ))
                    .style(|s| s.flex_row().items_center().gap(ACTION_GAP))
                    .into_any()
                },
            );

            let close_x: Rc<dyn Fn()> = Rc::new(close);
            let panel = v_stack((
                modal_title_owned(title, close_x),
                body,
                modal_footer_split(status.style(|s| s.min_width(0.0)), actions),
            ))
            .on_click_stop(|_| {})
            .style(|s| panel_style(s).width(PANEL_W).height(PANEL_H));

            focus_root_with_ring(container(panel), root_ring)
                .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| close())
                .style(|s| {
                    s.size_full()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .background(theme::modal_backdrop())
                })
                .into_any()
        },
    )
    .style(move |s| {
        if d.object.get().is_some() && d.preview.get().is_none() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemaic_core::schema::SequenceInfo;

    /// An identity column's counter is shown and can be altered, but the editor
    /// is a dead end for it — its one irreversible action, the drop, is refused
    /// by the server. Same call a materialized view gets.
    #[test]
    fn an_identity_sequence_is_not_editable() {
        let seq = |internal: bool| {
            ObjectItem::Sequence(SequenceInfo {
                name: "orders_id_seq".into(),
                owned_by: Some(schemaic_core::schema::SequenceOwner {
                    table: "orders".into(),
                    column: "id".into(),
                    internal,
                }),
                ..Default::default()
            })
        };
        assert!(is_editable_object(&seq(false)));
        assert!(!is_editable_object(&seq(true)));
        // Everything else is editable.
        assert!(is_editable_object(&ObjectItem::Enum(Default::default())));
        assert!(is_editable_object(&ObjectItem::Domain(Default::default())));
    }

    /// Detaching has to be undoable, and there is nowhere else to recover the
    /// owner from: the toggle used to apply only on `true`, so changing your
    /// mind left `OWNED BY NONE` in the plan with the control reading *off*.
    #[test]
    fn detaching_a_sequence_round_trips_through_the_toggle() {
        let orig = SequenceOwner {
            table: "orders".into(),
            column: "id".into(),
            internal: false,
        };
        assert_eq!(detach_apply(Some(&orig), true), None);
        assert_eq!(detach_apply(Some(&orig), false), Some(orig.clone()));
        // Some(o) → None → Some(o): the second call has to restore what the
        // first cleared, which is why it reads the *target*, not the draft.
        let after = detach_apply(Some(&orig), true);
        assert_eq!(detach_apply(Some(&orig), after.is_none()), None);
        assert_eq!(detach_apply(Some(&orig), false), Some(orig));
        // A sequence that never had an owner has nothing to restore.
        assert_eq!(detach_apply(None, false), None);
    }
}
