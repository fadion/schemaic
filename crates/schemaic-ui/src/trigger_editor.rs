//! The trigger editor, and the function editor that exists to serve it.
//!
//! Two modals in one module because the second is only reachable through the
//! first on any path a user cares about: a PostgreSQL trigger holds no body — it
//! is a binding to a function — so "make a trigger" there means "have a function
//! to point at", and offering the trigger form without a way to write one would
//! be a dead end.
//!
//! The trigger modal is the designer's **list-plus-form** shape — a table's
//! triggers on the left, the selected one's details on the right, with `+`/`−`
//! under the list — because that is what they are: several sibling objects on
//! one table, edited together. It wears the shared modal chrome, computes its
//! footer count from the same [`TriggerSetDraft`] the preview emits from, and
//! ends at [`crate::ddl_preview`] like every other schema edit.
//!
//! **It is not a designer tab, and that is the point.** What belongs in the
//! designer is what can be a *clause* of `ALTER TABLE` — columns, indexes,
//! foreign keys, checks — because MySQL's emitter coalesces those into one
//! statement, which is the only atomicity that engine offers. A trigger needs a
//! statement of its own, so folding it in would turn one `ALTER TABLE` into an
//! `ALTER` plus N trigger statements that commit one at a time, and
//! `DdlError::applied` would stop meaning much. Keeping this a separate plan
//! costs a menu entry and buys that back.
//!
//! Four things here are load-bearing rather than cosmetic:
//!
//! * **The form is per-engine because the objects are.** MySQL's trigger owns a
//!   body and fires on exactly one event; PostgreSQL's calls a function, may
//!   fire on several, and can carry a `WHEN` guard. The model holds both shapes
//!   so introspection never has to lie about what a server reported, and
//!   [`TriggerDraft::validate`] is what refuses the impossible one — so this
//!   form hides what an engine can't express rather than offering it and failing
//!   at apply time.
//! * **Every edit is a re-creation.** Neither engine can alter a trigger, so
//!   there is no "cheap" field here: changing the name costs exactly what
//!   changing the body costs. The footer says "1 change" either way, and the
//!   preview states what a drop-first costs on a non-transactional engine.
//! * **The function dropdown must not reset the draft.** The list arrives
//!   asynchronously ([`crate::TriggerFnFn`]), so it is empty for the first
//!   frames; a dropdown that selected its first entry on arrival would silently
//!   re-point a trigger the user had already aimed.
//! * **The list re-renders on every draft change; the form must not.** Same
//!   split, and the same reason, as the designer's: a draft-keyed form is torn
//!   down mid-keystroke. The form is keyed on `(selected, rev)`, with `rev`
//!   bumped on add/remove because deleting the selected row leaves `selected`
//!   unchanged while the item at that index is now a different trigger. The
//!   two modals share `selected`/`rev` — only one of them is ever open.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::ddl::{self, FunctionDraft, TriggerDraft, TriggerSetDraft};
use schemaic_core::intel::SqlDialect;
use schemaic_core::schema::{
    RoutineInfo, TriggerAction, TriggerEvent, TriggerInfo, TriggerLevel, TriggerTiming,
};

use crate::settings::settings_toggle_row;
use crate::table_designer::{
    edit_ctx, empty_hint, list_actions, list_pane, list_row_plain, loaded_table, owned_dropdown,
};
use crate::widgets::{
    ACTION_GAP, ActionKind, FORM_GAP, MODAL_PAD_H, action_button, control_button,
    control_button_enabled, focus_root, form_section, form_setting, form_setting_owned,
    modal_footer_split, modal_title_owned, panel_style,
};
use crate::{
    DdlPreview, FieldCfg, FunctionTarget, TriggerFnDoneFn, TriggerFnRequest, TriggerTarget, Ui,
    ddl_preview, edit_field, object_location, theme,
};

/// Matches the table designer's, deliberately: this is the same list-plus-form
/// layout over the same fixed-width list, so a different panel width squeezes
/// the form pane and reads as the list being over-padded.
const PANEL_W: f64 = 900.0;
const PANEL_H: f64 = 620.0;
const FIELD_W: f64 = 260.0;
/// The body box's height before it scrolls.
const BODY_ROWS: usize = 12;
/// What a new MySQL trigger starts from: valid, obviously a placeholder, and
/// something the preview can emit as-is.
const NEW_BODY: &str = "BEGIN\n    \nEND";

// ── opening ──────────────────────────────────────────────────────────────────

fn open_editor(ui: &Ui, target: TriggerTarget, draft: TriggerSetDraft) {
    let d = ui.ddl;
    let pg = target.dialect == SqlDialect::Postgres;
    d.trigger_draft.set(draft);
    d.view_rows.set(BODY_ROWS);
    d.selected.set(0);
    d.rev.update(|r| *r += 1);
    d.error.set(None);
    d.preview.set(None);
    // Each overlay knows only its own flag, so two open would paint two panels.
    d.designer.set(None);
    d.view.set(None);
    d.function.set(None);
    d.functions.set(Vec::new());
    d.trigger.set(Some(target));
    if pg {
        fetch_functions(ui);
    }
}

/// Load the database's trigger functions once the editor is already open.
///
/// Guarded on `generation` like every other off-thread callback here: the modal
/// can be closed, or reopened on another trigger, before this lands.
fn fetch_functions(ui: &Ui) {
    let d = ui.ddl;
    let Some((conn_id, database)) = d
        .trigger
        .with_untracked(|t| t.as_ref().map(|t| (t.conn_id, t.database.clone())))
    else {
        return;
    };
    let generation = d.generation.get_untracked();
    let done: TriggerFnDoneFn = Rc::new(move |fns: Vec<RoutineInfo>| {
        if d.generation.get_untracked() != generation {
            return;
        }
        d.functions.set(fns);
    });
    (ui.schema_actions.trigger_functions)(TriggerFnRequest { conn_id, database }, done);
}

/// Open the editor on a table's triggers.
pub(crate) fn open_for_table(ui: &Ui, database: &str, schema: Option<&str>, table: &str) {
    let Some(info) = loaded_table(ui, database, schema, table) else {
        return;
    };
    let ctx = edit_ctx(ui);
    open_editor(
        ui,
        TriggerTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            schema: info.schema.clone(),
            table: info.name.clone(),
            dialect: ctx.dialect,
            current: info.triggers.clone(),
            read_only: ctx.read_only,
        },
        TriggerSetDraft::from_table(&info),
    );
}

/// A blank trigger for the list's `+` — pre-shaped per engine, so the form opens
/// on something the preview can emit rather than on three validation errors.
fn blank_trigger(
    existing: &[TriggerDraft],
    table: &str,
    schema: Option<String>,
    pg: bool,
) -> TriggerDraft {
    let taken: Vec<String> = existing.iter().map(|t| t.info.name.clone()).collect();
    let mut name = "new_trigger".to_string();
    let mut n = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&name)) {
        name = format!("new_trigger_{n}");
        n += 1;
    }
    let mut draft = TriggerDraft::blank(name, table, schema);
    draft.info.events = vec![TriggerEvent::Insert];
    draft.info.action = if pg {
        TriggerAction::Function {
            name: String::new(),
            args: Vec::new(),
        }
    } else {
        TriggerAction::Body(NEW_BODY.to_string())
    };
    draft
}

/// Whether this trigger is one Schemaic can edit — the form's gate, and the
/// reason a constraint trigger shows its details read-only: the deferral
/// settings one carries aren't modelled, so re-creating it would drop them.
pub(crate) fn is_editable_trigger(t: &TriggerInfo) -> bool {
    !t.constraint
}

// ── the function editor ──────────────────────────────────────────────────────

fn open_function(ui: &Ui, target: FunctionTarget, draft: FunctionDraft) {
    let d = ui.ddl;
    d.function_draft.set(draft);
    d.view_rows.set(BODY_ROWS);
    d.error.set(None);
    d.preview.set(None);
    d.designer.set(None);
    d.view.set(None);
    // The trigger editor's target is deliberately *left set*: its overlay
    // renders nothing while this one is up (so only one panel paints), and
    // leaving it means closing this one puts the half-filled trigger form back
    // exactly as it was, with no target to rebuild and nothing to guess.
    d.function.set(Some(target));
}

pub(crate) fn open_for_function(ui: &Ui, database: &str, f: &RoutineInfo) {
    let ctx = edit_ctx(ui);
    open_function(
        ui,
        FunctionTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            dialect: ctx.dialect,
            current: Some(f.clone()),
            read_only: ctx.read_only,
        },
        FunctionDraft::from_info(f),
    );
}

/// Create a function, from the trigger editor's "New function…".
///
/// There is no "return to the trigger" flag: the trigger editor's target is
/// never cleared, so closing this one simply reveals it again with its draft
/// intact. A flag would have been a second source of truth for something the
/// signal already answers.
pub(crate) fn open_for_new_function(ui: &Ui, database: &str, schema: Option<&str>) {
    let ctx = edit_ctx(ui);
    open_function(
        ui,
        FunctionTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            dialect: ctx.dialect,
            current: None,
            read_only: ctx.read_only,
        },
        FunctionDraft::blank_trigger("new_trigger_fn", schema.map(str::to_string)),
    );
}

// ── bound controls ───────────────────────────────────────────────────────────

/// A field bound to one place in the trigger draft.
///
/// Same contract as the view editor's: the local signal is seeded once on build
/// and the effect writes back only on a genuine change, so a rebuild can't read
/// as an edit. The form is built once per open — nothing is keyed on the draft —
/// because a draft-keyed field would be torn down mid-keystroke.
fn bound_field(
    ui: &Ui,
    i: usize,
    initial: String,
    cfg: FieldCfg,
    apply: impl Fn(&mut TriggerDraft, &str) + 'static,
) -> AnyView {
    let draft = ui.ddl.trigger_draft;
    let sig = floem::reactive::create_rw_signal(initial);
    create_effect(move |prev: Option<String>| {
        let v = sig.get();
        if prev.is_some_and(|p| p != v) {
            // `get_mut`, not indexing: the form is keyed on `(selected, rev)`,
            // but an effect can still fire once against a list the removal has
            // already shortened.
            draft.update(|d| {
                if let Some(t) = d.triggers.get_mut(i) {
                    apply(t, &v)
                }
            });
        }
        v
    });
    edit_field(sig, cfg).into_any()
}

fn bound_choice(
    ui: &Ui,
    i: usize,
    initial: String,
    options: Vec<String>,
    apply: impl Fn(&mut TriggerDraft, &str) + 'static,
) -> AnyView {
    let draft = ui.ddl.trigger_draft;
    let sig = floem::reactive::create_rw_signal(initial);
    owned_dropdown(
        move || sig.get(),
        options,
        FIELD_W,
        move |v: String| {
            if sig.get_untracked() != v {
                sig.set(v.clone());
                draft.update(|d| {
                    if let Some(t) = d.triggers.get_mut(i) {
                        apply(t, &v)
                    }
                });
            }
        },
    )
    .into_any()
}

/// A field bound to one place in the function draft.
fn bound_fn_field(
    ui: &Ui,
    initial: String,
    cfg: FieldCfg,
    apply: impl Fn(&mut FunctionDraft, &str) + 'static,
) -> AnyView {
    let draft = ui.ddl.function_draft;
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

// ── the trigger form ─────────────────────────────────────────────────────────

/// The detail form for the trigger at `i`.
///
/// Built once per `(selected, rev)` — never keyed on the draft — for the reason
/// the designer's form isn't: a draft-keyed form is torn down mid-keystroke.
fn form(ui: Ui, target: &TriggerTarget, i: usize) -> AnyView {
    let d = ui.ddl.trigger_draft;
    let Some(draft) = d.with_untracked(|s| s.triggers.get(i).cloned()) else {
        // Same empty state the designer's tabs show, in the same place: this pane
        // is otherwise a blank half of the modal with no indication that the list
        // beside it is what fills it.
        return empty_hint("No trigger selected.").into_any();
    };
    let pg = target.dialect == SqlDialect::Postgres;

    // A constraint trigger is shown so it can be seen and removed, but its
    // deferral settings aren't modelled, so editing it would silently drop them.
    if !is_editable_trigger(&draft.info) {
        return v_stack((
            form_section("Trigger"),
            form_setting_owned(
                "Name".to_string(),
                text(draft.info.name.clone())
                    .style(|s| s.color(theme::text()).font_size(theme::FONT_BODY)),
            ),
            text(
                "This is a constraint trigger. Schemaic doesn't model the deferral \
                 settings one carries, so it can be removed here but not edited — \
                 change it in SQL instead.",
            )
            .style(|s| {
                s.color(theme::text_dim())
                    .font_size(theme::FONT_LABEL)
                    .max_width(420.0)
            }),
        ))
        .style(|s| s.flex_col().gap(FORM_GAP).width_full())
        .into_any();
    }

    let name = form_setting(
        "Name",
        bound_field(
            &ui,
            i,
            draft.info.name.clone(),
            FieldCfg {
                placeholder: "trigger_name",
                ..Default::default()
            },
            |d, v| d.info.name = v.trim().to_string(),
        )
        .style(move |s| s.width(FIELD_W)),
    );

    // INSTEAD OF is PostgreSQL's alone, so the option only exists there.
    let timings: Vec<String> = if pg {
        vec!["BEFORE".into(), "AFTER".into(), "INSTEAD OF".into()]
    } else {
        vec!["BEFORE".into(), "AFTER".into()]
    };
    let timing = form_setting(
        "Timing",
        bound_choice(
            &ui,
            i,
            draft.info.timing.sql().to_string(),
            timings,
            |d, v| {
                d.info.timing = TriggerTiming::parse(v).unwrap_or_default();
            },
        ),
    );

    // MySQL fires on exactly one event, PostgreSQL on any combination — so this
    // is a dropdown on one engine and a row of toggles on the other, rather than
    // one control that lies about what the server accepts.
    let events: AnyView = if pg {
        let mut rows: Vec<AnyView> = Vec::new();
        for ev in [
            TriggerEvent::Insert,
            TriggerEvent::Update,
            TriggerEvent::Delete,
            TriggerEvent::Truncate,
        ] {
            let on = floem::reactive::create_rw_signal(draft.info.events.contains(&ev));
            create_effect(move |prev: Option<bool>| {
                let v = on.get();
                if prev.is_some_and(|p| p != v) {
                    d.update(|s| {
                        let Some(dr) = s.triggers.get_mut(i) else {
                            return;
                        };
                        dr.info.events.retain(|e| *e != ev);
                        if v {
                            dr.info.events.push(ev);
                        }
                        // Keep PostgreSQL's print order, which is what the
                        // emitter round-trips against.
                        dr.info.events.sort();
                    });
                }
                v
            });
            rows.push(
                settings_toggle_row(
                    ev.sql(),
                    match ev {
                        TriggerEvent::Truncate => "Statement-level only.",
                        _ => "",
                    },
                    on,
                )
                .into_any(),
            );
        }
        v_stack_from_iter(rows)
            .style(|s| s.flex_col().gap(FORM_GAP).width_full())
            .into_any()
    } else {
        form_setting(
            "Event",
            bound_choice(
                &ui,
                i,
                draft
                    .info
                    .events
                    .first()
                    .map(|e| e.sql().to_string())
                    .unwrap_or_else(|| "INSERT".into()),
                vec!["INSERT".into(), "UPDATE".into(), "DELETE".into()],
                |d, v| {
                    d.info.events = TriggerEvent::parse(v).into_iter().collect();
                },
            ),
        )
        .into_any()
    };

    // FOR EACH ROW is the only level MySQL has, so the control is PostgreSQL's.
    let level = form_setting(
        "Fires",
        bound_choice(
            &ui,
            i,
            draft.info.level.sql().to_string(),
            vec!["FOR EACH ROW".into(), "FOR EACH STATEMENT".into()],
            |d, v| {
                d.info.level = if v.contains("STATEMENT") {
                    TriggerLevel::Statement
                } else {
                    TriggerLevel::Row
                };
            },
        ),
    )
    .style(move |s| if pg { s } else { s.hide() });

    let when = form_setting(
        "When",
        bound_field(
            &ui,
            i,
            draft.info.condition.clone().unwrap_or_default(),
            FieldCfg {
                placeholder: "new.total > 0",
                mono: true,
                ..Default::default()
            },
            |d, v| {
                d.info.condition = Some(v.trim().to_string()).filter(|c| !c.is_empty());
            },
        )
        .style(move |s| s.width(FIELD_W * 1.6)),
    )
    .style(move |s| if pg { s } else { s.hide() });

    // The action: a body on MySQL, a function to call on PostgreSQL.
    let action: AnyView = if pg {
        pg_action(&ui, i, &draft, target).into_any()
    } else {
        form_setting(
            "Body",
            bound_field(
                &ui,
                i,
                match &draft.info.action {
                    TriggerAction::Body(b) => b.clone(),
                    TriggerAction::Function { .. } => String::new(),
                },
                FieldCfg {
                    placeholder: "BEGIN … END",
                    mono: true,
                    multiline: true,
                    max_rows: Some(ui.ddl.view_rows),
                    // Height from the *logical* line count, not the wrapped one.
                    // Wrapped rows depend on the box's width, which isn't settled
                    // on the first layout pass inside this flex row — so the box
                    // opened at some tall guess and snapped down on first click.
                    // Long SQL lines scrolling sideways is also what the editor
                    // itself does by default.
                    no_wrap: true,
                    ..Default::default()
                },
                |d, v| d.info.action = TriggerAction::Body(v.to_string()),
            )
            .style(|s| s.width_full()),
        )
        .into_any()
    };

    v_stack((
        form_section("Trigger"),
        name,
        timing,
        events,
        level,
        form_section("Condition").style(move |s| {
            let s = s.margin_top(4.0);
            if pg { s } else { s.hide() }
        }),
        when,
        form_section("Action").style(|s| s.margin_top(4.0)),
        action,
    ))
    .style(|s| s.flex_col().gap(FORM_GAP).width_full())
    .into_any()
}

/// PostgreSQL's action: pick a function, or write a new one.
///
/// The dropdown offers what the lazy fetch found and **keeps whatever the draft
/// already names** if that isn't in the list yet — the list arrives a round trip
/// after the modal, and re-pointing a trigger the user had already aimed would
/// be a silent edit.
fn pg_action(ui: &Ui, i: usize, draft: &TriggerDraft, target: &TriggerTarget) -> AnyView {
    let d = ui.ddl.trigger_draft;
    let fns = ui.ddl.functions;
    let current = match &draft.info.action {
        TriggerAction::Function { name, .. } => name.clone(),
        TriggerAction::Body(_) => String::new(),
    };

    // The selection lives in a signal, not in the rebuilt closure: `owned_dropdown`
    // needs a `Copy` getter, and a captured `String` isn't one. It also keeps the
    // control's own state independent of the list arriving.
    let sel = floem::reactive::create_rw_signal(current);
    create_effect(move |_| {
        let named = d.with(|s| match s.triggers.get(i).map(|t| &t.info.action) {
            Some(TriggerAction::Function { name, .. }) => name.clone(),
            _ => String::new(),
        });
        if sel.get_untracked() != named {
            sel.set(named);
        }
    });
    let picker = dyn_container(
        move || (fns.get(), sel.get()),
        move |(list, named)| {
            let mut options: Vec<String> = list.iter().map(|f| f.name.clone()).collect();
            // Whatever the draft names stays selectable even before the fetch
            // lands, or on a server where it isn't a trigger function any more.
            if !named.is_empty() && !options.contains(&named) {
                options.insert(0, named);
            }
            owned_dropdown(
                move || sel.get(),
                options,
                FIELD_W,
                move |v: String| {
                    d.update(|s| {
                        let Some(dr) = s.triggers.get_mut(i) else {
                            return;
                        };
                        let args = match &dr.info.action {
                            TriggerAction::Function { args, .. } => args.clone(),
                            TriggerAction::Body(_) => Vec::new(),
                        };
                        dr.info.action = TriggerAction::Function { name: v, args };
                    });
                },
            )
            .into_any()
        },
    );

    let new_ui = ui.clone();
    let database = target.database.clone();
    let schema = draft.info.schema.clone();
    // `control_button`, not the footer's: these sit *in* the form beside the
    // picker, where the app's bordered in-form button is what "Choose file…",
    // "Add value" and "Add constraint" already wear. They were the last two
    // callers of the text-only footer button, in the one place it wasn't a
    // footer.
    let new_btn = control_button("New function", move || {
        open_for_new_function(&new_ui, &database, schema.as_deref());
    });

    // Editing an existing function lives here rather than in the schema tree
    // because this is where the list exists: it is fetched lazily when this
    // editor opens, so the tree — built synchronously from `db_nodes` — has
    // nothing to offer.
    let edit_ui = ui.clone();
    let edit_db = target.database.clone();
    let edit_btn = dyn_container(
        move || (sel.get(), fns.get()),
        move |(named, list)| {
            let found = list.iter().find(|f| f.name == named).cloned();
            let ui = edit_ui.clone();
            let db = edit_db.clone();
            control_button_enabled("Edit", found.is_some(), move || {
                if let Some(f) = &found {
                    open_for_function(&ui, &db, f);
                }
            })
            .into_any()
        },
    );

    v_stack((
        form_setting_owned(
            "Function".to_string(),
            h_stack((picker, edit_btn, new_btn)).style(|s| s.flex_row().items_center().gap(10.0)),
        ),
        form_setting(
            "Arguments",
            bound_field(
                ui,
                i,
                match &draft.info.action {
                    TriggerAction::Function { args, .. } => args.join(", "),
                    TriggerAction::Body(_) => String::new(),
                },
                FieldCfg {
                    placeholder: "audit, orders",
                    ..Default::default()
                },
                |d, v| {
                    let args: Vec<String> = v
                        .split(',')
                        .map(|a| a.trim().to_string())
                        .filter(|a| !a.is_empty())
                        .collect();
                    let name = match &d.info.action {
                        TriggerAction::Function { name, .. } => name.clone(),
                        TriggerAction::Body(_) => String::new(),
                    };
                    d.info.action = TriggerAction::Function { name, args };
                },
            )
            .style(move |s| s.width(FIELD_W * 1.6)),
        ),
    ))
    .style(|s| s.flex_col().gap(FORM_GAP).width_full())
    .into_any()
}

// ── the function form ────────────────────────────────────────────────────────

fn function_form(ui: Ui, _target: &FunctionTarget) -> AnyView {
    let d = ui.ddl.function_draft;
    let draft = d.get_untracked();

    // No "In {database}" row: the modal title names the place now.
    let name = form_setting(
        "Name",
        bound_fn_field(
            &ui,
            draft.info.name.clone(),
            FieldCfg {
                placeholder: "audit_fn",
                ..Default::default()
            },
            |d, v| d.info.name = v.trim().to_string(),
        )
        .style(move |s| s.width(FIELD_W)),
    );

    let language = form_setting("Language", {
        let sig = floem::reactive::create_rw_signal(draft.info.language.clone());
        owned_dropdown(
            move || sig.get(),
            vec!["plpgsql".into(), "sql".into()],
            FIELD_W,
            move |v: String| {
                if sig.get_untracked() != v {
                    sig.set(v.clone());
                    d.update(|dr| dr.info.language = v.clone());
                }
            },
        )
        .into_any()
    });

    let body = form_setting(
        "Body",
        bound_fn_field(
            &ui,
            draft.info.body.clone(),
            FieldCfg {
                placeholder: "BEGIN\n    RETURN NEW;\nEND;",
                mono: true,
                multiline: true,
                max_rows: Some(ui.ddl.view_rows),
                // As the trigger body above: logical lines, so the box hugs its
                // content on the first frame instead of guessing from a width
                // that hasn't settled.
                no_wrap: true,
                ..Default::default()
            },
            |d, v| d.info.body = v.to_string(),
        )
        .style(|s| s.width_full()),
    );

    // Shown because they're *carried*: `CREATE OR REPLACE FUNCTION` replaces the
    // whole routine, so anything not restated reverts — and dropping the
    // `SET search_path` from a SECURITY DEFINER function is a privilege hole.
    let secdef = {
        let sig = floem::reactive::create_rw_signal(draft.info.security_definer);
        create_effect(move |prev: Option<bool>| {
            let v = sig.get();
            if prev.is_some_and(|p| p != v) {
                d.update(|dr| dr.info.security_definer = v);
            }
            v
        });
        settings_toggle_row(
            "Security definer",
            "Runs with the owner's rights instead of the caller's. Pin a search_path \
             below when you use this.",
            sig,
        )
    };

    let settings = form_setting(
        "Settings",
        bound_fn_field(
            &ui,
            draft.info.settings.join(", "),
            FieldCfg {
                placeholder: "search_path=public",
                mono: true,
                ..Default::default()
            },
            |d, v| {
                d.info.settings = v
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            },
        )
        .style(move |s| s.width(FIELD_W * 1.6)),
    );

    v_stack((
        form_section("Function"),
        name,
        language,
        form_section("Body").style(|s| s.margin_top(4.0)),
        body,
        form_section("Options").style(|s| s.margin_top(4.0)),
        secdef,
        settings,
    ))
    .style(|s| s.flex_col().gap(FORM_GAP).width_full())
    .into_any()
}

// ── the modals ───────────────────────────────────────────────────────────────

/// The change set the trigger draft currently describes — the same call the
/// preview emits from, so the footer's count can't disagree with the SQL.
fn change_set(target: &TriggerTarget, draft: &TriggerSetDraft) -> ddl::ChangeSet {
    ddl::diff_triggers(&target.current, draft, target.dialect)
}

/// The list of the table's triggers, with the `+`/`−` bar under it.
///
/// Re-renders on every draft change — unlike the form, which must not — so a
/// rename shows in the list as you type it. Same split as the designer's.
fn trigger_list(ui: Ui, pg: bool, table: String, schema: Option<String>) -> impl IntoView {
    let d = ui.ddl.trigger_draft;
    let selected = ui.ddl.selected;
    let rev = ui.ddl.rev;

    let rows_ui = ui.clone();
    let rows = dyn_container(
        move || d.get(),
        move |draft| {
            let rows: Vec<AnyView> = draft
                .triggers
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    let detail = format!(
                        "{} {}",
                        t.info.timing.sql(),
                        t.info
                            .events
                            .iter()
                            .map(|e| e.sql())
                            .collect::<Vec<_>>()
                            .join("/")
                    );
                    list_row_plain(rows_ui.clone(), i, t.info.name.clone(), detail).into_any()
                })
                .collect();
            v_stack_from_iter(rows)
                .style(|s| s.flex_col().width_full())
                .into_any()
        },
    )
    // The `dyn_container` needs it too, not just the stack inside it: without a
    // width of its own it shrinks to content, and the inner `width_full` then
    // resolves against *that* — so the rows sat inset from the list's edges.
    // The designer has no such wrapper (its whole section rebuilds instead), so
    // this is the one place it can go wrong.
    .style(|s| s.width_full());

    let add = move || {
        d.update(|s| {
            let fresh = blank_trigger(&s.triggers, &table, schema.clone(), pg);
            s.triggers.push(fresh);
        });
        // Select what was just added, and bump `rev`: `selected` may be
        // unchanged in value while pointing at a different item.
        let n = d.with_untracked(|s| s.triggers.len());
        selected.set(n.saturating_sub(1));
        rev.update(|r| *r += 1);
    };
    let remove = move || {
        let i = selected.get_untracked();
        d.update(|s| {
            if i < s.triggers.len() {
                s.triggers.remove(i);
            }
        });
        let n = d.with_untracked(|s| s.triggers.len());
        selected.set(i.min(n.saturating_sub(1)));
        rev.update(|r| *r += 1);
    };
    // No arrows: a trigger's position in this list is display order, not
    // execution order — MySQL's ordering is the `FOLLOWS` clause and
    // PostgreSQL fires alphabetically. Offering ↑/↓ here would imply a control
    // over firing order that moving a row does not have.
    list_pane(rows, list_actions(add, remove, None, None))
}

fn fn_change_set(target: &FunctionTarget, draft: &FunctionDraft) -> ddl::ChangeSet {
    match &target.current {
        Some(cur) => ddl::diff_function(cur, draft, target.dialect),
        None => ddl::create_function(draft, target.dialect),
    }
}

fn preview_from(target: &TriggerTarget, cs: &ddl::ChangeSet) -> DdlPreview {
    ddl_preview::preview_of(
        target.conn_id,
        &target.database,
        // The subject is the table: a plan here can touch several triggers, so
        // heading it with one of their names would misdescribe it.
        format!("triggers on {}", target.display()),
        cs,
        target.read_only,
    )
}

/// The trigger editor. Absolutely positioned over the workspace when
/// `ui.ddl.trigger` is `Some`.
pub(crate) fn trigger_editor_overlay(ui: Ui) -> impl IntoView {
    let d = ui.ddl;
    let close = move || d.trigger.set(None);

    // Re-fetch the function list when the function editor closes back to here:
    // a function just created has to be in the dropdown, and nothing else would
    // put it there. Created once, outside the `dyn_container`, so it survives
    // the panel being rebuilt.
    {
        let ui = ui.clone();
        create_effect(move |prev: Option<bool>| {
            let fn_open = d.function.get().is_some();
            let closed = prev == Some(true) && !fn_open;
            if closed && d.trigger.get_untracked().is_some() {
                fetch_functions(&ui);
            }
            fn_open
        });
    }

    dyn_container(
        // The preview stacks on top and this stays open behind it (Cancel there
        // returns here with the draft intact), but must render nothing. Same for
        // the function editor, which is reached *from* here and returns here.
        move || {
            (
                d.trigger.get().is_some(),
                d.preview.get().is_some() || d.function.get().is_some(),
            )
        },
        move |(open, previewing)| {
            if !open || previewing {
                return empty().into_any();
            }
            let Some(target) = d.trigger.get_untracked() else {
                return empty().into_any();
            };
            let ui = ui.clone();
            let title = format!(
                "Triggers on {}.{}",
                object_location(&target.database, target.schema.as_deref()),
                target.table
            );

            // The form is keyed on `(selected, rev)` and NOT on the draft: a
            // draft-keyed form is torn down mid-keystroke. `rev` is in the key
            // because removing the selected row leaves `selected` unchanged
            // while the item at that index is now a different trigger.
            let form_ui = ui.clone();
            let form_target = target.clone();
            let (selected, rev) = (ui.ddl.selected, ui.ddl.rev);
            let detail = dyn_container(
                move || (selected.get(), rev.get()),
                move |(i, _)| {
                    form(form_ui.clone(), &form_target, i)
                        .style(|s| s.width_full())
                        .into_any()
                },
            );

            let body = h_stack((
                trigger_list(
                    ui.clone(),
                    target.dialect == SqlDialect::Postgres,
                    target.table.clone(),
                    target.schema.clone(),
                ),
                crate::widgets::autohide(scroll(
                    container(detail)
                        .style(|s| s.width_full().padding_left(18.0).padding_right(4.0)),
                ))
                .style(|s| s.flex_grow(1.0_f32).min_width(0.0).height_full()),
            ))
            .style(|s| {
                s.flex_row()
                    .width_full()
                    .flex_grow(1.0_f32)
                    .min_height(0.0)
                    .padding_horiz(MODAL_PAD_H)
                    .padding_vert(18.0)
                    .gap(0.0)
            });

            let status_target = target.clone();
            let status = dyn_container(
                move || d.trigger_draft.get(),
                move |draft| {
                    let errs = draft.validate(status_target.dialect);
                    if let Some(first) = errs.first() {
                        return text(first.clone())
                            .style(|s| {
                                s.color(theme::error())
                                    .font_size(theme::FONT_LABEL)
                                    .max_width(460.0)
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
                move || d.trigger_draft.get(),
                move |draft| {
                    let ui = preview_ui.clone();
                    let target = preview_target.clone();
                    let cs = change_set(&target, &draft);
                    let ready = draft.validate(target.dialect).is_empty() && !cs.is_empty();
                    h_stack((
                        action_button("Cancel", ActionKind::Neutral, true, close),
                        action_button("Preview SQL", ActionKind::Primary, ready, move || {
                            let cs = change_set(&target, &draft);
                            ddl_preview::open_preview(&ui, preview_from(&target, &cs));
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

            focus_root(container(panel))
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
        if d.trigger.get().is_some() && d.preview.get().is_none() && d.function.get().is_none() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// The function editor.
pub(crate) fn function_editor_overlay(ui: Ui) -> impl IntoView {
    let d = ui.ddl;
    // Closing just clears this one. The trigger editor's target was never
    // cleared, so it reappears underneath with its draft intact — which is why
    // "New function…" isn't a one-way door out of a half-filled trigger form.
    let close = move || d.function.set(None);

    dyn_container(
        move || (d.function.get().is_some(), d.preview.get().is_some()),
        move |(open, previewing)| {
            if !open || previewing {
                return empty().into_any();
            }
            let Some(target) = d.function.get_untracked() else {
                return empty().into_any();
            };
            let ui = ui.clone();
            let title = match &target.current {
                Some(f) => format!(
                    "Edit function {}.{}",
                    object_location(&target.database, f.schema.as_deref()),
                    f.name
                ),
                // A new function has no namespace chosen yet — the form's own
                // Schema field is where that lands — so the title names the
                // database and stops there.
                None => format!("Create function in {}", target.database),
            };

            let body = crate::widgets::autohide(scroll(
                function_form(ui.clone(), &target)
                    .style(|s| s.width_full().padding_horiz(MODAL_PAD_H).padding_vert(18.0)),
            ))
            .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0));

            let status_target = target.clone();
            let status = dyn_container(
                move || d.function_draft.get(),
                move |draft| {
                    let errs = draft.validate();
                    if let Some(first) = errs.first() {
                        return text(first.clone())
                            .style(|s| {
                                s.color(theme::error())
                                    .font_size(theme::FONT_LABEL)
                                    .max_width(460.0)
                            })
                            .into_any();
                    }
                    let n = fn_change_set(&status_target, &draft).len();
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
                move || d.function_draft.get(),
                move |draft| {
                    let ui = preview_ui.clone();
                    let target = preview_target.clone();
                    let cs = fn_change_set(&target, &draft);
                    let ready = draft.validate().is_empty() && !cs.is_empty();
                    let subject = draft.info.name.clone();
                    h_stack((
                        action_button("Cancel", ActionKind::Neutral, true, close),
                        action_button("Preview SQL", ActionKind::Primary, ready, move || {
                            let cs = fn_change_set(&target, &draft);
                            ddl_preview::open_preview(
                                &ui,
                                ddl_preview::preview_of(
                                    target.conn_id,
                                    &target.database,
                                    subject.clone(),
                                    &cs,
                                    target.read_only,
                                ),
                            );
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

            focus_root(container(panel))
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
        if d.function.get().is_some() && d.preview.get().is_none() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}
