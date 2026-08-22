//! The stored-routine editor — one modal for a function or a procedure, on
//! whichever engine has them.
//!
//! It began life inside [`crate::trigger_editor`], as the half of that module
//! that existed only to serve it: a PostgreSQL trigger holds no body — it is a
//! binding to a function — so offering the trigger form without a way to write
//! one would have been a dead end. That premise is what changed. Routines are
//! browsable now: the schema tree lists them, Find-Anywhere reaches them and the
//! Create menu makes them, so the modal is reached far more often *without* a
//! trigger than with one, and it is its own module. The trigger editor still
//! opens it, still leaves its own target set, and still gets its dropdown
//! refreshed when this one closes.
//!
//! Four things here are load-bearing rather than cosmetic:
//!
//! * **The form is per-engine because the objects are.** PostgreSQL has
//!   volatility, strictness, a language and per-routine `SET` clauses; MySQL has
//!   a definer, a determinism promise, a data-access declaration and a comment,
//!   and no `LANGUAGE` clause at all. Both are modelled on one
//!   [`RoutineInfo`](schemaic_core::schema::RoutineInfo) so introspection never
//!   has to lie about what a server reported, and the form shows what the engine
//!   in front of it can express rather than offering the rest and failing at
//!   apply time.
//! * **MySQL's body is fetched a second time, and it is not an optimisation.**
//!   `information_schema.ROUTINE_DEFINITION` hands back a body whose escapes are
//!   already resolved, and every edit on that engine begins with a `DROP` that
//!   commits on its own — so restating the resolved text can fail *after* the
//!   only copy is gone. `SHOW CREATE` is the source, read when this opens, and
//!   applied to **both** sides of the diff so a routine doesn't open
//!   already-changed.
//! * **The form is built once per open.** Nothing here is keyed on the draft: a
//!   draft-keyed field is torn down mid-keystroke. The footer and the actions
//!   are the two things that do re-render, because they are what the draft
//!   changes.
//! * **A new routine's namespace is inherited, not chosen.** There is no Schema
//!   field, for the reason the view editor has none: the folder or menu the
//!   modal was opened from already said where, and the title discloses it.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::ddl::{self, RoutineDraft};
use schemaic_core::intel::SqlDialect;
use schemaic_core::schema::{RoutineInfo, RoutineKind, SqlDataAccess, Volatility};

use crate::settings::focusable_toggle_row;
use crate::table_designer::{edit_ctx, focusable_owned_dropdown};
use crate::trigger_editor::{unique_name, value_rows};
use crate::widgets::{
    ACTION_GAP, ACTION_TAB, ActionKind, FORM_GAP, FocusRing, MODAL_PAD_H, action_button,
    focus_root_with_ring, form_section, form_setting, modal_footer_split, modal_title_owned,
    panel_style,
};
use crate::{
    FieldCfg, RoutineSrcDoneFn, RoutineSrcRequest, RoutineTarget, Ui, ddl_preview, edit_field,
    object_location, theme,
};

/// Matches the trigger editor's, deliberately: the two are reached from one
/// another, and a panel that changed size on the way through would read as a
/// different modal rather than the next step.
const PANEL_W: f64 = 900.0;
const PANEL_H: f64 = 620.0;
const FIELD_W: f64 = 260.0;
/// The body box's height before it scrolls.
const BODY_ROWS: usize = 12;

// ── opening ──────────────────────────────────────────────────────────────────

fn open(ui: &Ui, target: RoutineTarget, draft: RoutineDraft) {
    let d = ui.ddl;
    // A new editing session: any lazy fetch still in flight for the last one is
    // now for the wrong target and must not land.
    d.session.update(|g| *g += 1);
    d.routine_draft.set(draft);
    // Cleared here rather than on close, so the one path that raises it —
    // `fetch_source`, at the end of this function — is the only one that can
    // leave it raised.
    d.routine_source_pending.set(false);
    d.view_rows.set(BODY_ROWS);
    d.error.set(None);
    d.preview.set(None);
    // Each overlay knows only its own flag, so two open would paint two panels.
    // The **trigger** editor's target is deliberately not cleared: its overlay
    // renders nothing while this one is up, and leaving it set means closing
    // this one puts a half-filled trigger form back exactly as it was.
    d.designer.set(None);
    d.view.set(None);
    d.object.set(None);
    d.routine.set(Some(target));
    fetch_source(ui);
}

/// Open the editor on an existing routine.
pub(crate) fn open_for_routine(ui: &Ui, database: &str, r: &RoutineInfo) {
    let ctx = edit_ctx(ui);
    open(
        ui,
        RoutineTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            dialect: ctx.dialect,
            current: Some(r.clone()),
            read_only: ctx.read_only,
        },
        RoutineDraft::from_info(r),
    );
}

/// Open the editor on a blank draft — Create function / Create procedure.
pub(crate) fn open_for_new(ui: &Ui, database: &str, schema: Option<&str>, kind: RoutineKind) {
    let ctx = edit_ctx(ui);
    open(
        ui,
        RoutineTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            dialect: ctx.dialect,
            current: None,
            read_only: ctx.read_only,
        },
        RoutineDraft::blank(
            kind,
            unique_name(&taken_names(ui, database, schema, kind), stem(kind)),
            schema.map(str::to_string),
            ctx.dialect,
        ),
    );
}

/// Create a trigger function, from the trigger editor's "New function…".
///
/// A different entry point from [`open_for_new`] rather than a flag on it,
/// because it starts from a different draft: a trigger function is `plpgsql`,
/// `RETURNS trigger`, and has to end in a `RETURN` that an ordinary new function
/// has no reason to carry.
///
/// There is no "return to the trigger" flag: the trigger editor's target is
/// never cleared, so closing this one simply reveals it again with its draft
/// intact. A flag would have been a second source of truth for something the
/// signal already answers.
pub(crate) fn open_for_new_trigger_function(ui: &Ui, database: &str, schema: Option<&str>) {
    let ctx = edit_ctx(ui);
    // Against the functions already fetched for this database, so a second
    // "New function…" doesn't propose a name the first one took.
    let taken: Vec<String> = ui.ddl.functions.with_untracked(|l| {
        l.iter()
            .filter(|f| f.schema.as_deref() == schema)
            .map(|f| f.name.clone())
            .collect()
    });
    open(
        ui,
        RoutineTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            dialect: ctx.dialect,
            current: None,
            read_only: ctx.read_only,
        },
        RoutineDraft::blank_trigger(
            unique_name(&taken, "new_trigger_fn"),
            schema.map(str::to_string),
        ),
    );
}

fn stem(kind: RoutineKind) -> &'static str {
    match kind {
        RoutineKind::Function => "new_function",
        RoutineKind::Procedure => "new_procedure",
    }
}

/// The names already taken in this namespace, so a new routine doesn't propose
/// one of them. Read off the schema the tree is showing — the same list the
/// folder was rendered from, so the proposal agrees with what the user can see.
fn taken_names(ui: &Ui, database: &str, schema: Option<&str>, kind: RoutineKind) -> Vec<String> {
    ui.schema.db_nodes.with_untracked(|nodes| {
        let Some(node) = nodes.iter().find(|n| n.database == database) else {
            return Vec::new();
        };
        let crate::SchemaState::Loaded(s) = node.schema.get_untracked() else {
            return Vec::new();
        };
        s.routines
            .iter()
            .filter(|r| r.kind == kind && r.schema.as_deref() == schema)
            .map(|r| r.name.clone())
            .collect()
    })
}

/// Read the routine's body **as written**, on the one engine whose catalogue
/// resolves the escapes out of it.
///
/// Applied to `target.current` **and** the draft, exactly as the trigger
/// editor's source fetch is: patching only the draft would make every MySQL
/// routine open already-changed against a `current` that still held the resolved
/// body, and the footer would say "1 change" over an edit nobody made.
///
/// A draft the user has already edited keeps its **body** — the round trip lands
/// in milliseconds, but "unlikely" is not a reason to overwrite what somebody
/// typed. The body the editor *opened* with is what says whether they have, so
/// it is read before `current` is corrected. The session state is patched either
/// way: it is not editable anywhere in the app, and the `CREATE` it wraps is
/// preceded by a `DROP` that commits on its own, so a keystroke landing first
/// must not be able to strip the wrapper off it.
///
/// Until this lands, Preview is disabled (`routine_source_pending`) — the draft
/// is holding `information_schema`'s escape-resolved copy, and a routine
/// recreated from that is not the one that was there.
///
/// The session guard is what makes a slow reply safe — the user can close this
/// modal and open another routine while the read is in flight.
fn fetch_source(ui: &Ui) {
    let d = ui.ddl;
    let Some(target) = d.routine.get_untracked() else {
        return;
    };
    let Some(current) = target.current.clone() else {
        // A routine that doesn't exist yet has no source to read.
        return;
    };
    if target.dialect != SqlDialect::MySql {
        return;
    }
    let session = d.session.get_untracked();
    let name = current.name.clone();
    d.routine_source_pending.set(true);
    let done: RoutineSrcDoneFn = Rc::new(move |asked: String, src| {
        // A late reply for a routine this modal is no longer editing. The flag
        // belongs to *that* session and was cleared with it.
        if d.session.get_untracked() != session {
            return;
        }
        if asked != name {
            return;
        }
        // Cleared even for a failed read: a role without `SHOW_ROUTINE` gets
        // `information_schema`'s body and always will, so waiting past this
        // point would disable Preview for good.
        d.routine_source_pending.set(false);
        let Some(src) = src else { return };
        let opened_with = d.routine.with_untracked(|t| {
            t.as_ref()
                .and_then(|t| t.current.as_ref())
                .map(|c| c.body.clone())
        });
        // `current` — the left-hand side of every diff.
        d.routine.update(|t| {
            if let Some(t) = t.as_mut()
                && let Some(cur) = t.current.as_mut()
            {
                src.apply_to(cur);
            }
        });
        d.routine_draft.update(|dr| {
            src.apply_session_to(&mut dr.info);
            if opened_with.as_deref() == Some(dr.info.body.as_str()) {
                src.apply_body_to(&mut dr.info);
            }
        });
    });
    (ui.schema_actions.routine_source.clone())(
        RoutineSrcRequest {
            conn_id: target.conn_id,
            database: target.database.clone(),
            name: current.name.clone(),
            kind: current.kind,
        },
        done,
    );
}

// ── bound controls ───────────────────────────────────────────────────────────

/// A field bound to one place in the routine draft.
///
/// Same contract as the view editor's: the local signal is seeded once on build
/// and the effect writes back only on a genuine change, so a rebuild can't read
/// as an edit.
fn bound_field(
    ui: &Ui,
    initial: String,
    cfg: FieldCfg,
    apply: impl Fn(&mut RoutineDraft, &str) + 'static,
) -> AnyView {
    let draft = ui.ddl.routine_draft;
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

/// A dropdown bound to one place in the routine draft, over a fixed vocabulary.
///
/// The options are `(label, value)` pairs so the label a person reads and the
/// value the draft holds can differ — which they must for the enums here, whose
/// SQL spelling (`MODIFIES SQL DATA`) is also the only sensible label, and
/// whose *parse* is what closes the loop.
fn bound_choice<T: Clone + PartialEq + 'static>(
    ui: &Ui,
    initial: T,
    options: Vec<(String, T)>,
    ring: FocusRing,
    tabindex: u32,
    apply: impl Fn(&mut RoutineDraft, T) + 'static,
) -> AnyView {
    let draft = ui.ddl.routine_draft;
    let labels: Vec<String> = options.iter().map(|(l, _)| l.clone()).collect();
    let start = options
        .iter()
        .find(|(_, v)| *v == initial)
        .map(|(l, _)| l.clone())
        .unwrap_or_default();
    let sig = floem::reactive::create_rw_signal(start);
    focusable_owned_dropdown(
        move || sig.get(),
        labels,
        FIELD_W,
        ring,
        tabindex,
        move |label: String| {
            if sig.get_untracked() == label {
                return;
            }
            sig.set(label.clone());
            if let Some((_, v)) = options.iter().find(|(l, _)| *l == label) {
                let v = v.clone();
                draft.update(|d| apply(d, v.clone()));
            }
        },
    )
    .into_any()
}

fn bound_toggle(
    ui: &Ui,
    label: &'static str,
    hint: &'static str,
    initial: bool,
    ring: FocusRing,
    tabindex: u32,
    apply: impl Fn(&mut RoutineDraft, bool) + 'static,
) -> AnyView {
    let draft = ui.ddl.routine_draft;
    let sig = floem::reactive::create_rw_signal(initial);
    create_effect(move |prev: Option<bool>| {
        let v = sig.get();
        if prev.is_some_and(|p| p != v) {
            draft.update(|d| apply(d, v));
        }
        v
    });
    focusable_toggle_row(label, hint, sig, ring, tabindex).into_any()
}

// ── the form ─────────────────────────────────────────────────────────────────

/// Tab indices. One block per control, spaced so the growing `Settings` list at
/// the end (which claims [`crate::widgets::VALUE_TAB`] upwards) never collides
/// with a fixed one.
const TAB_NAME: u32 = 10;
const TAB_ARGS: u32 = 20;
const TAB_RETURNS: u32 = 30;
const TAB_LANGUAGE: u32 = 40;
const TAB_BODY: u32 = 50;
const TAB_OPT: u32 = 60;

fn routine_form(ui: Ui, target: &RoutineTarget, ring: FocusRing) -> AnyView {
    let d = ui.ddl.routine_draft;
    let draft = d.get_untracked();
    let postgres = target.dialect == SqlDialect::Postgres;
    let is_function = draft.info.kind == RoutineKind::Function;
    // A trigger function has no arguments to declare and no return type to
    // choose — both are fixed by what a trigger is — so the two fields are
    // absent rather than shown and refused. `RoutineDraft::validate` is what
    // says so if one arrives some other way.
    let trigger_fn = draft.info.is_trigger_function();

    let name = form_setting(
        "Name",
        bound_field(
            &ui,
            draft.info.name.clone(),
            FieldCfg {
                placeholder: "audit_fn",
                focus: Some((ring.clone(), TAB_NAME)),
                ..Default::default()
            },
            |d, v| d.info.name = v.trim().to_string(),
        )
        .style(move |s| s.width(FIELD_W)),
    );

    // The parameter list as the server renders it, edited as text. It is one
    // string rather than a list of rows because that is what both catalogues
    // publish — `pg_get_function_arguments` and MySQL's rebuilt `PARAMETERS`
    // form — and re-parsing it into fields to re-render it would be a way to
    // change a declaration the user didn't touch.
    let arguments = (!trigger_fn).then(|| {
        form_setting(
            "Parameters",
            bound_field(
                &ui,
                draft.info.arguments.clone(),
                FieldCfg {
                    placeholder: if postgres {
                        "a integer, b text DEFAULT ''"
                    } else {
                        "IN sku VARCHAR(20), OUT n INT"
                    },
                    mono: true,
                    focus: Some((ring.clone(), TAB_ARGS)),
                    ..Default::default()
                },
                |d, v| d.info.arguments = v.trim().to_string(),
            )
            .style(move |s| s.width(FIELD_W * 1.6)),
        )
    });

    // A procedure has no return type on either engine, and stating one is a
    // syntax error rather than a harmless extra.
    let returns = (is_function && !trigger_fn).then(|| {
        form_setting(
            "Returns",
            bound_field(
                &ui,
                draft.info.returns.clone(),
                FieldCfg {
                    placeholder: if postgres { "integer" } else { "INT" },
                    mono: true,
                    focus: Some((ring.clone(), TAB_RETURNS)),
                    ..Default::default()
                },
                |d, v| d.info.returns = v.trim().to_string(),
            )
            .style(move |s| s.width(FIELD_W)),
        )
    });

    // PostgreSQL's alone. MySQL stores only `SQL` routines and accepts no
    // `LANGUAGE` clause, so the control is absent there rather than offering a
    // choice with one entry.
    let language = postgres.then(|| {
        form_setting("Language", {
            let current = draft.info.language.trim().to_string();
            // The two Schemaic proposes, **plus whatever this routine already
            // is**. A `plperl` or `plpython3u` function is edited here quite
            // happily — its body is source text and the emitter dollar-quotes it
            // — but a list that didn't carry its language showed the right label
            // over options that would silently retype the routine the moment the
            // control was touched. (`c` and `internal` don't reach this form at
            // all; see `RoutineInfo::is_editable`.)
            let mut langs: Vec<String> = vec!["plpgsql".into(), "sql".into()];
            if !current.is_empty() && !langs.iter().any(|l| l.eq_ignore_ascii_case(&current)) {
                langs.push(current.clone());
            }
            let sig = floem::reactive::create_rw_signal(draft.info.language.clone());
            focusable_owned_dropdown(
                move || sig.get(),
                langs,
                FIELD_W,
                ring.clone(),
                TAB_LANGUAGE,
                move |v: String| {
                    if sig.get_untracked() != v {
                        sig.set(v.clone());
                        d.update(|dr| dr.info.language = v.clone());
                    }
                },
            )
            .into_any()
        })
    });

    let body = form_setting(
        "Body",
        bound_field(
            &ui,
            draft.info.body.clone(),
            FieldCfg {
                placeholder: if postgres {
                    "BEGIN\n    RETURN NEW;\nEND;"
                } else {
                    "BEGIN\n    SELECT 1;\nEND"
                },
                mono: true,
                multiline: true,
                max_rows: Some(ui.ddl.view_rows),
                // Logical lines, so the box hugs its content on the first frame
                // instead of guessing from a width that hasn't settled.
                no_wrap: true,
                focus: Some((ring.clone(), TAB_BODY)),
                // It's a routine body: Tab indents. Escape leaves.
                tab_indents: true,
                ..Default::default()
            },
            |d, v| d.info.body = v.to_string(),
        )
        .style(|s| s.width_full()),
    );

    // ── options, per engine ──────────────────────────────────────────────
    //
    // Shown because they are *carried*: a redefinition restates the whole
    // routine, so anything the form doesn't offer is something an edit silently
    // resets. On PostgreSQL the sharpest is the `SET search_path` pinned to a
    // SECURITY DEFINER function; on MySQL it is the definer itself.
    let mut options: Vec<AnyView> = Vec::new();
    if postgres {
        // **Volatility and strictness are function-only.** PostgreSQL's
        // `CREATE PROCEDURE` grammar takes a strict subset of a function's
        // attributes and answers anything else with
        // `ERROR: invalid attribute in procedure definition`, so offering these
        // two over a procedure is offering an edit that can only fail at Apply —
        // the "hide what an engine can't express" call this form already makes
        // per engine, made per *kind*.
        if is_function {
            options.push(
                form_setting(
                    "Volatility",
                    bound_choice(
                        &ui,
                        draft.info.volatility,
                        vec![
                            ("VOLATILE".to_string(), Volatility::Volatile),
                            ("STABLE".to_string(), Volatility::Stable),
                            ("IMMUTABLE".to_string(), Volatility::Immutable),
                        ],
                        ring.clone(),
                        TAB_OPT,
                        |d, v| d.info.volatility = v,
                    ),
                )
                .into_any(),
            );
            options.push(bound_toggle(
                &ui,
                "Strict",
                "RETURNS NULL ON NULL INPUT — the body doesn't run when an argument \
                 is NULL.",
                draft.info.strict,
                ring.clone(),
                TAB_OPT + 10,
                |d, v| d.info.strict = v,
            ));
        }
        options.push(bound_toggle(
            &ui,
            "Security definer",
            "Runs with the owner's rights instead of the caller's. Pin a search_path \
             below when you use this.",
            draft.info.security_definer,
            ring.clone(),
            TAB_OPT + 20,
            |d, v| d.info.security_definer = v,
        ));
        options.push(
            form_setting(
                "Settings",
                value_rows(
                    &ui,
                    "search_path=public, pg_temp",
                    "Add setting",
                    true,
                    ring.clone(),
                    crate::widgets::VALUE_TAB,
                    move || d.with(|s| s.info.settings.clone()),
                    move |v| d.update(|s| s.info.settings = v),
                )
                .style(move |s| s.width(FIELD_W * 1.6)),
            )
            .into_any(),
        );
    } else {
        options.push(bound_toggle(
            &ui,
            "Deterministic",
            "Promises the same result for the same arguments. A server with binary \
             logging refuses a non-deterministic routine unless it trusts creators.",
            draft.info.deterministic,
            ring.clone(),
            TAB_OPT,
            |d, v| d.info.deterministic = v,
        ));
        options.push(
            form_setting(
                "Data access",
                bound_choice(
                    &ui,
                    draft.info.data_access,
                    vec![
                        ("CONTAINS SQL".to_string(), SqlDataAccess::ContainsSql),
                        ("NO SQL".to_string(), SqlDataAccess::NoSql),
                        ("READS SQL DATA".to_string(), SqlDataAccess::ReadsSqlData),
                        (
                            "MODIFIES SQL DATA".to_string(),
                            SqlDataAccess::ModifiesSqlData,
                        ),
                    ],
                    ring.clone(),
                    TAB_OPT + 10,
                    |d, v| d.info.data_access = v,
                ),
            )
            .into_any(),
        );
        options.push(bound_toggle(
            &ui,
            "Security definer",
            "Runs with the definer's rights instead of the caller's. This is MySQL's \
             default — turning it off is SQL SECURITY INVOKER.",
            draft.info.security_definer,
            ring.clone(),
            TAB_OPT + 20,
            |d, v| d.info.security_definer = v,
        ));
        // Carried rather than offered as a free choice would be honest either
        // way, and a field is the honest one: a recreate that dropped the clause
        // would hand the routine to whoever applied the edit, and a recreate
        // that restates an account the applier may not impersonate is refused by
        // the server with a message that says so.
        options.push(
            form_setting(
                "Definer",
                bound_field(
                    &ui,
                    draft.info.definer.clone().unwrap_or_default(),
                    FieldCfg {
                        placeholder: "root@localhost",
                        mono: true,
                        focus: Some((ring.clone(), TAB_OPT + 30)),
                        ..Default::default()
                    },
                    |d, v| {
                        let v = v.trim();
                        d.info.definer = (!v.is_empty()).then(|| v.to_string());
                    },
                )
                .style(move |s| s.width(FIELD_W)),
            )
            .into_any(),
        );
        options.push(
            form_setting(
                "Comment",
                bound_field(
                    &ui,
                    draft.info.comment.clone().unwrap_or_default(),
                    FieldCfg {
                        placeholder: "what it does",
                        focus: Some((ring.clone(), TAB_OPT + 40)),
                        ..Default::default()
                    },
                    |d, v| {
                        let v = v.trim();
                        d.info.comment = (!v.is_empty()).then(|| v.to_string());
                    },
                )
                .style(move |s| s.width(FIELD_W * 1.6)),
            )
            .into_any(),
        );
    }

    let mut rows: Vec<AnyView> = vec![
        form_section(if is_function { "Function" } else { "Procedure" }).into_any(),
        name.into_any(),
    ];
    rows.extend(arguments.map(IntoView::into_any));
    rows.extend(returns.map(IntoView::into_any));
    rows.extend(language.map(IntoView::into_any));
    rows.push(form_section("Body").style(|s| s.margin_top(4.0)).into_any());
    rows.push(body.into_any());
    rows.push(
        form_section("Options")
            .style(|s| s.margin_top(4.0))
            .into_any(),
    );
    rows.extend(options);
    v_stack_from_iter(rows)
        .style(|s| s.flex_col().gap(FORM_GAP).width_full())
        .into_any()
}

// ── the modal ────────────────────────────────────────────────────────────────

/// The change set the draft currently describes — the same call the preview
/// emits from, so the footer's count can't disagree with the SQL.
fn change_set(target: &RoutineTarget, draft: &RoutineDraft) -> ddl::ChangeSet {
    match &target.current {
        Some(cur) => ddl::diff_routine(cur, draft, target.dialect),
        None => ddl::create_routine(draft, target.dialect),
    }
}

/// The routine editor. Absolutely positioned over the workspace when
/// `ui.ddl.routine` is `Some`.
pub(crate) fn routine_editor_overlay(ui: Ui) -> impl IntoView {
    let d = ui.ddl;
    // Closing just clears this one. A trigger editor underneath was never
    // cleared, so it reappears with its draft intact — which is why
    // "New function…" isn't a one-way door out of a half-filled trigger form.
    let close = move || d.routine.set(None);

    dyn_container(
        move || (d.routine.get().is_some(), d.preview.get().is_some()),
        move |(open, previewing)| {
            if !open || previewing {
                return empty().into_any();
            }
            let Some(target) = d.routine.get_untracked() else {
                return empty().into_any();
            };
            let ui = ui.clone();
            let dialect = target.dialect;
            let title = match &target.current {
                // The parameter list is part of the title where the engine
                // overloads: a remembered palette hit resolves by name alone, so
                // this is what says *which* `add` is open before Apply rewrites
                // it.
                Some(f) => format!(
                    "Edit {} {}.{}{}",
                    f.kind.label(),
                    object_location(&target.database, f.schema.as_deref()),
                    f.name,
                    f.identity_suffix(dialect)
                ),
                // A new routine's namespace isn't chosen — it is *inherited*
                // from wherever the modal was opened, so the title is where it
                // is disclosed.
                None => format!(
                    "Create {} in {}",
                    d.routine_draft.with_untracked(|r| r.info.kind.label()),
                    object_location(
                        &target.database,
                        d.routine_draft
                            .with_untracked(|f| f.info.schema.clone())
                            .as_deref(),
                    )
                ),
            };

            // The form is built once per open, so one ring covers it.
            let ring = FocusRing::new();
            let root_ring = ring.clone();

            let body = crate::widgets::autohide(scroll(
                routine_form(ui.clone(), &target, ring.clone())
                    .style(|s| s.width_full().padding_horiz(MODAL_PAD_H).padding_vert(18.0)),
            ))
            .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0));

            let status_target = target.clone();
            let status = dyn_container(
                move || (d.routine_draft.get(), d.routine_source_pending.get()),
                move |(draft, pending)| {
                    // Said before the change count, because until the source
                    // lands the count is over `information_schema`'s resolved
                    // body rather than the routine as written.
                    if pending {
                        return text("Reading the routine's source…")
                            .style(|s| s.color(theme::text_faint()).font_size(theme::FONT_LABEL))
                            .into_any();
                    }
                    let errs = draft.validate(dialect);
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
            let ring_actions = ring.clone();
            let actions = dyn_container(
                move || (d.routine_draft.get(), d.routine_source_pending.get()),
                move |(draft, pending)| {
                    let ui = preview_ui.clone();
                    let target = preview_target.clone();
                    let ring = ring_actions.clone();
                    let cs = change_set(&target, &draft);
                    // `!pending`: the draft is still on the escape-resolved
                    // copy until the `SHOW CREATE` lands, and a MySQL recreate
                    // `DROP`s before it `CREATE`s.
                    let ready = !pending && draft.validate(dialect).is_empty() && !cs.is_empty();
                    let subject = draft.info.name.clone();
                    h_stack((
                        action_button(
                            "Cancel",
                            ActionKind::Neutral,
                            true,
                            ring.clone(),
                            ACTION_TAB,
                            close,
                        ),
                        action_button(
                            "Preview SQL",
                            ActionKind::Primary,
                            ready,
                            ring,
                            ACTION_TAB + 10,
                            move || {
                                let cs = change_set(&target, &draft);
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
                    .style(|s| s.flex_row().items_center().gap(ACTION_GAP))
                    .into_any()
                },
            );

            let close_x: Rc<dyn Fn()> = Rc::new(close);
            let panel = v_stack((
                modal_title_owned(title, close_x, root_ring.clone()),
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
        // The same absolute placement every other DDL overlay takes.
        if d.routine.get().is_some() && d.preview.get().is_none() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}
