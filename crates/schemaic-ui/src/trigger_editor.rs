//! The trigger editor, and the function editor that exists to serve it.
//!
//! Two modals in one module because the second is only reachable through the
//! first on any path a user cares about: a PostgreSQL trigger holds no body — it
//! is a binding to a function — so "make a trigger" there means "have a function
//! to point at", and offering the trigger form without a way to write one would
//! be a dead end.
//!
//! Same shape as [`crate::view_editor`], for the same reason it isn't a designer
//! tab: a trigger has no columns, indexes or keys to list, so the
//! list-plus-form layout would be empty panes beside one text box. It wears the
//! shared modal chrome, computes its footer count from the same
//! [`TriggerDraft`] the preview emits from, and ends at [`crate::ddl_preview`]
//! like every other schema edit.
//!
//! Three things here are load-bearing rather than cosmetic:
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

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::ddl::{self, FunctionDraft, TriggerDraft};
use schemaic_core::intel::SqlDialect;
use schemaic_core::schema::{
    RoutineInfo, TriggerAction, TriggerEvent, TriggerInfo, TriggerLevel, TriggerTiming,
};

use crate::settings::settings_toggle_row;
use crate::table_designer::{edit_ctx, owned_dropdown};
use crate::widgets::{
    FORM_GAP, footer_button, form_section, form_setting, form_setting_owned, modal_footer_split,
    modal_title_owned, panel_style,
};
use crate::{
    DdlPreview, FieldCfg, FunctionTarget, TriggerFnDoneFn, TriggerFnRequest, TriggerTarget, Ui,
    ddl_preview, edit_field, theme,
};

const PANEL_W: f64 = 760.0;
const PANEL_H: f64 = 640.0;
const FIELD_W: f64 = 260.0;
/// The body box's height before it scrolls.
const BODY_ROWS: usize = 12;
/// What a new MySQL trigger starts from: valid, obviously a placeholder, and
/// something the preview can emit as-is.
const NEW_BODY: &str = "BEGIN\n    \nEND";

// ── opening ──────────────────────────────────────────────────────────────────

fn open_editor(ui: &Ui, target: TriggerTarget, draft: TriggerDraft) {
    let d = ui.ddl;
    d.trigger_draft.set(draft);
    d.view_rows.set(BODY_ROWS);
    d.error.set(None);
    d.preview.set(None);
    // Each overlay knows only its own flag, so two open would paint two panels.
    d.designer.set(None);
    d.view.set(None);
    d.function.set(None);
    d.trigger.set(Some(target));
    if target_is_pg(ui) {
        fetch_functions(ui);
    }
}

fn target_is_pg(ui: &Ui) -> bool {
    ui.ddl.trigger.with_untracked(|t| {
        t.as_ref()
            .is_some_and(|t| t.dialect == SqlDialect::Postgres)
    })
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

/// Open the editor on an existing trigger.
pub(crate) fn open_for_trigger(ui: &Ui, database: &str, trigger: &TriggerInfo) {
    let ctx = edit_ctx(ui);
    open_editor(
        ui,
        TriggerTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            dialect: ctx.dialect,
            current: Some(trigger.clone()),
            read_only: ctx.read_only,
        },
        TriggerDraft::from_info(trigger),
    );
}

/// Open the editor on a blank draft for `table` — Create trigger.
pub(crate) fn open_for_new(ui: &Ui, database: &str, schema: Option<&str>, table: &str) {
    let ctx = edit_ctx(ui);
    let mut draft = TriggerDraft::blank("new_trigger", table, schema.map(str::to_string));
    draft.info.events = vec![TriggerEvent::Insert];
    if ctx.dialect == SqlDialect::Postgres {
        draft.info.action = TriggerAction::Function {
            name: String::new(),
            args: Vec::new(),
        };
    } else {
        draft.info.action = TriggerAction::Body(NEW_BODY.to_string());
    }
    open_editor(
        ui,
        TriggerTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            dialect: ctx.dialect,
            current: None,
            read_only: ctx.read_only,
        },
        draft,
    );
}

/// Whether this trigger is one Schemaic can edit — the entry point's gate, and
/// the reason a constraint trigger doesn't open a half-populated form: the
/// deferral settings one carries aren't modelled, so re-creating it would drop
/// them.
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
    initial: String,
    cfg: FieldCfg,
    apply: impl Fn(&mut TriggerDraft, &str) + 'static,
) -> AnyView {
    let draft = ui.ddl.trigger_draft;
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

fn bound_choice(
    ui: &Ui,
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
                draft.update(|d| apply(d, &v));
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

fn events_of(draft: &TriggerDraft) -> Vec<TriggerEvent> {
    draft.info.events.clone()
}

fn form(ui: Ui, target: &TriggerTarget) -> AnyView {
    let d = ui.ddl.trigger_draft;
    let draft = d.get_untracked();
    let pg = target.dialect == SqlDialect::Postgres;

    let place = form_setting_owned(
        if target.current.is_some() {
            "On".to_string()
        } else {
            "Creating on".to_string()
        },
        text(match &draft.info.schema {
            Some(s) => format!("{}.{s}.{}", target.database, draft.info.table),
            None => format!("{}.{}", target.database, draft.info.table),
        })
        .style(|s| s.color(theme::text_dim()).font_size(theme::FONT_BODY)),
    );

    let name = form_setting(
        "Name",
        bound_field(
            &ui,
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
        bound_choice(&ui, draft.info.timing.sql().to_string(), timings, |d, v| {
            d.info.timing = TriggerTiming::parse(v).unwrap_or_default();
        }),
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
            let on = floem::reactive::create_rw_signal(events_of(&draft).contains(&ev));
            create_effect(move |prev: Option<bool>| {
                let v = on.get();
                if prev.is_some_and(|p| p != v) {
                    d.update(|dr| {
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
        pg_action(&ui, &draft, target).into_any()
    } else {
        form_setting(
            "Body",
            bound_field(
                &ui,
                match &draft.info.action {
                    TriggerAction::Body(b) => b.clone(),
                    TriggerAction::Function { .. } => String::new(),
                },
                FieldCfg {
                    placeholder: "BEGIN … END",
                    mono: true,
                    multiline: true,
                    max_rows: Some(ui.ddl.view_rows),
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
        place,
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
fn pg_action(ui: &Ui, draft: &TriggerDraft, target: &TriggerTarget) -> AnyView {
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
        let named = d.with(|dr| match &dr.info.action {
            TriggerAction::Function { name, .. } => name.clone(),
            TriggerAction::Body(_) => String::new(),
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
                    d.update(|dr| {
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
    let new_btn = footer_button(
        "New function…",
        theme::text_dim,
        theme::text,
        true,
        move || {
            open_for_new_function(&new_ui, &database, schema.as_deref());
        },
    );

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
            footer_button(
                "Edit…",
                theme::text_dim,
                theme::text,
                found.is_some(),
                move || {
                    if let Some(f) = &found {
                        open_for_function(&ui, &db, f);
                    }
                },
            )
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

fn function_form(ui: Ui, target: &FunctionTarget) -> AnyView {
    let d = ui.ddl.function_draft;
    let draft = d.get_untracked();

    let place = form_setting_owned(
        "In".to_string(),
        text(match &draft.info.schema {
            Some(s) => format!("{}.{s}", target.database),
            None => target.database.clone(),
        })
        .style(|s| s.color(theme::text_dim()).font_size(theme::FONT_BODY)),
    );

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
        place,
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
fn change_set(target: &TriggerTarget, draft: &TriggerDraft) -> ddl::ChangeSet {
    match &target.current {
        Some(cur) => ddl::diff_trigger(cur, draft, target.dialect),
        None => ddl::create_trigger(draft, target.dialect),
    }
}

fn fn_change_set(target: &FunctionTarget, draft: &FunctionDraft) -> ddl::ChangeSet {
    match &target.current {
        Some(cur) => ddl::diff_function(cur, draft, target.dialect),
        None => ddl::create_function(draft, target.dialect),
    }
}

fn preview_from(target: &TriggerTarget, draft: &TriggerDraft, cs: &ddl::ChangeSet) -> DdlPreview {
    let subject = match target.current {
        Some(_) => target.display(),
        None => draft.info.name.clone(),
    };
    ddl_preview::preview_of(
        target.conn_id,
        &target.database,
        subject,
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
            let title = match &target.current {
                Some(_) => format!("Edit trigger {}", target.display()),
                None => format!("Create trigger in {}", target.database),
            };

            let body = crate::widgets::autohide(scroll(
                form(ui.clone(), &target)
                    .style(|s| s.width_full().padding_horiz(20.0).padding_vert(18.0)),
            ))
            .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0));

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
                        footer_button("Cancel", theme::text_dim, theme::text, true, close),
                        footer_button(
                            "Preview SQL",
                            theme::conn_save,
                            theme::conn_save_hover,
                            ready,
                            move || {
                                let cs = change_set(&target, &draft);
                                ddl_preview::open_preview(&ui, preview_from(&target, &draft, &cs));
                            },
                        ),
                    ))
                    .style(|s| s.flex_row().items_center().gap(15.0))
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

            container(panel)
                .keyboard_navigable()
                .request_focus(|| {})
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
                Some(f) => format!("Edit function {}", f.name),
                None => format!("Create function in {}", target.database),
            };

            let body = crate::widgets::autohide(scroll(
                function_form(ui.clone(), &target)
                    .style(|s| s.width_full().padding_horiz(20.0).padding_vert(18.0)),
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
                        footer_button("Cancel", theme::text_dim, theme::text, true, close),
                        footer_button(
                            "Preview SQL",
                            theme::conn_save,
                            theme::conn_save_hover,
                            ready,
                            move || {
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
                            },
                        ),
                    ))
                    .style(|s| s.flex_row().items_center().gap(15.0))
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

            container(panel)
                .keyboard_navigable()
                .request_focus(|| {})
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
