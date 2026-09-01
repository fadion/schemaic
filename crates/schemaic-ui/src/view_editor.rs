//! The view editor: a name, a `SELECT`, and the options that come along for the
//! ride.
//!
//! Deliberately **not** the table designer. A view has no columns to add, no
//! indexes and no keys, so the list-plus-form shape would be three empty panes
//! beside one text box. What it is instead is the same modal chrome every other
//! one wears (`form_section`/`form_setting`/`modal_footer_split`), a real
//! multi-line field for the body, and the same ending: one
//! [`ViewDraft`] that the footer's change count and
//! the preview's SQL are both computed from, so the two can't disagree.
//!
//! Two things here are load-bearing rather than cosmetic:
//!
//! * **The options are shown because they're *carried*.** Redefining a view
//!   replaces it, so a `CREATE OR REPLACE` that doesn't restate the definer or
//!   the security type resets them — and `SQL SECURITY DEFINER` silently
//!   becoming `INVOKER` is a privilege change. They're in the form so the user
//!   can see what's being preserved, not because a view editor needs them.
//! * **PostgreSQL can't always replace.** Its `CREATE OR REPLACE VIEW` may only
//!   *append* columns, so an edit that renames or reorders one has to drop and
//!   re-create — which takes dependent views and grants with it. The plan
//!   detects that where it can ([`ddl::pg_replaceable`]) and says so in the
//!   preview; the toggle is for the cases it can't read off the statement.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::ddl::{self, ViewDraft};
use schemaic_core::intel::SqlDialect;

use crate::settings::focusable_toggle_row;
use crate::table_designer::{edit_ctx, focusable_owned_dropdown, loaded_table};
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, action_gap, focus_root_with_ring, form_gap,
    form_section, form_setting, form_setting_owned, modal_footer_split, modal_h, modal_pad_h,
    modal_title_owned, modal_w, panel_style,
};
use crate::{
    DdlPreview, FieldCfg, Ui, ViewAlgoDoneFn, ViewAlgoRequest, ViewTarget, ddl_preview, edit_field,
    object_location, theme,
};

fn panel_w() -> f64 {
    modal_w(760.0)
}
const PANEL_H: f64 = 620.0;
/// Text-field width, matching the designer's and the connection form's.
fn field_w() -> f64 {
    theme::scaled(260.0)
}
/// The body box's height before it scrolls — deep enough for a join with a
/// couple of conditions to be read whole.
const BODY_ROWS: usize = 14;
/// The `SELECT` a new view starts from: valid, obviously a placeholder, and
/// something the preview can emit as-is.
const NEW_BODY: &str = "SELECT * FROM ";

// ── opening ──────────────────────────────────────────────────────────────────

fn open_editor(ui: &Ui, target: ViewTarget, draft: ViewDraft) {
    let d = ui.ddl;
    // A new editing session — see `DdlUi::session`.
    d.session.update(|g| *g += 1);
    d.view_draft.set(draft);
    d.view_rows.set(BODY_ROWS);
    d.error.set(None);
    d.preview.set(None);
    // Every editor shares the preview stacked on top of them, and each overlay
    // only knows its own flag — two open would paint two panels. This used to
    // clear the designer and nothing else, from when those were the only two;
    // it is one list now (`ddl_preview::close_peers`).
    crate::ddl_preview::close_peers(d, false);
    d.view.set(Some(target));
}

/// Open the editor on an existing view. A base table isn't one, and a view whose
/// schema hasn't loaded has no body to edit.
pub(crate) fn open_for_view(ui: &Ui, database: &str, schema: Option<&str>, view: &str) {
    let Some(info) = loaded_table(ui, database, schema, view).filter(|t| t.is_view) else {
        return;
    };
    let Some(draft) = ViewDraft::from_table(&info) else {
        return;
    };
    let ctx = edit_ctx(ui);
    // MySQL's question and nobody else's — `SHOW CREATE VIEW` is the only place
    // the algorithm lives. Asked as `== MySql`, not `!= Postgres`: the second
    // spelling sent SQLite off to fetch an option it has no concept of.
    let needs_algorithm = ctx.dialect == SqlDialect::MySql
        && info
            .view_options
            .as_ref()
            .is_some_and(|o| o.algorithm.is_none());
    let view_name = info.name.clone();
    open_editor(
        ui,
        ViewTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            schema: info.schema.clone(),
            dialect: ctx.dialect,
            current: Some(info),
            read_only: ctx.read_only,
        },
        draft,
    );
    if needs_algorithm {
        fetch_algorithm(ui, ctx.conn_id, database.to_string(), view_name);
    }
}

/// Fill in the `ALGORITHM` MySQL 8 keeps out of `information_schema`, once the
/// editor is already open.
///
/// Both sides of the diff are patched, and that's the whole subtlety: writing it
/// only into the draft would make every MySQL 8 view open *already changed*,
/// since the introspected `current` it's compared against still said `None`.
/// The draft is left alone if the user has already chosen an algorithm — the
/// round-trip lands in milliseconds, but "unlikely" isn't a reason to overwrite
/// what somebody typed.
fn fetch_algorithm(ui: &Ui, conn_id: u64, database: String, view: String) {
    let d = ui.ddl;
    let session = d.session.get_untracked();
    let done: ViewAlgoDoneFn = Rc::new(move |algo: Option<String>| {
        // The modal can be closed, or reopened on another view, before this
        // lands. Guarded on `session` — a DDL editor session — not on
        // `generation`, which counts preview opens and so both missed a genuine
        // reopen and discarded a fetch whenever the preview went up first.
        if algo.is_none() || d.session.get_untracked() != session {
            return;
        }
        d.view.update(|t| {
            if let Some(o) = t
                .as_mut()
                .and_then(|t| t.current.as_mut())
                .and_then(|c| c.view_options.as_mut())
            {
                o.algorithm = algo.clone();
            }
        });
        d.view_draft.update(|dr| {
            if dr.options.algorithm.is_none() {
                dr.options.algorithm = algo.clone();
            }
        });
    });
    (ui.schema_actions.view_algorithm)(
        ViewAlgoRequest {
            conn_id,
            database,
            view,
        },
        done,
    );
}

/// Open the editor on a blank draft — Create view.
pub(crate) fn open_for_new(ui: &Ui, database: &str, schema: Option<&str>) {
    open_blank(ui, database, schema, NEW_BODY)
}

/// Create a view **out of the query you're looking at** — the editor's
/// right-click entry. Same modal as Create view, seeded with the statement under
/// the caret, so the common case (write a `SELECT`, keep it) doesn't mean
/// retyping it into a form.
///
/// The namespace comes from [`crate::table_designer::default_schema`], the same answer
/// the tree's own Create view gives on a database node.
pub(crate) fn open_from_query(ui: &Ui, database: &str, select: &str) {
    let schema = crate::table_designer::default_schema(ui, database);
    open_blank(ui, database, schema.as_deref(), select);
}

fn open_blank(ui: &Ui, database: &str, schema: Option<&str>, body: &str) {
    let ctx = edit_ctx(ui);
    let mut draft = ViewDraft::blank("new_view", schema.map(str::to_string));
    draft.select = body.to_string();
    open_editor(
        ui,
        ViewTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            schema: schema.map(str::to_string),
            dialect: ctx.dialect,
            current: None,
            read_only: ctx.read_only,
        },
        draft,
    );
}

/// Whether this tree node is a view Schemaic can edit — the entry point's
/// enabled/disabled test, and the reason a materialized view doesn't open a
/// half-populated form: PostgreSQL has no `CREATE OR REPLACE` for one.
///
/// The materialized half is `ddl::is_materialized_view`'s, spelled once: the
/// schema menu asks the same question to *offer* `Refresh view`, and two
/// hand-written copies of "is this a materialized view" are two chances for the
/// editor and the menu to disagree about the same node.
pub(crate) fn is_editable_view(info: Option<&schemaic_core::schema::TableInfo>) -> bool {
    info.is_some_and(|t| t.is_view && !schemaic_core::ddl::is_materialized_view(t))
}

// ── the form ─────────────────────────────────────────────────────────────────

/// A field bound to one place in the draft.
///
/// Same contract as the designer's: the local signal is seeded once on build and
/// the effect writes back only on a genuine change, so a rebuild can't read as
/// an edit. The form is built once per open — nothing here is keyed on the
/// draft — because a draft-keyed field would be torn down mid-keystroke.
fn bound_field(
    ui: &Ui,
    initial: String,
    cfg: FieldCfg,
    apply: impl Fn(&mut ViewDraft, &str) + 'static,
) -> AnyView {
    let draft = ui.ddl.view_draft;
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

/// A dropdown over a fixed set of option values, bound to the draft. The empty
/// string is the "unset" entry and shows as `—`.
fn bound_choice(
    ui: &Ui,
    initial: String,
    options: Vec<String>,
    ring: FocusRing,
    tabindex: u32,
    apply: impl Fn(&mut ViewDraft, &str) + 'static,
) -> AnyView {
    let draft = ui.ddl.view_draft;
    let sig = floem::reactive::create_rw_signal(initial);
    focusable_owned_dropdown(
        move || sig.get(),
        options,
        field_w,
        ring,
        tabindex,
        move |v: String| {
            if sig.get_untracked() != v {
                sig.set(v.clone());
                draft.update(|d| apply(d, &v));
            }
        },
    )
    .into_any()
}

fn form(ui: Ui, target: &ViewTarget, ring: FocusRing) -> AnyView {
    let d = ui.ddl.view_draft;
    let draft = d.get_untracked();
    // Asked per engine rather than as `pg` / `!pg`: SQLite is a third shape, not
    // MySQL's, and every option below belongs to exactly one of them.
    let my = target.dialect == SqlDialect::MySql;
    let pg = target.dialect == SqlDialect::Postgres;
    let sqlite = target.dialect == SqlDialect::Sqlite;

    // No "In {database}" row: the modal title names the place now.
    let name = form_setting(
        "Name",
        bound_field(
            &ui,
            draft.name.clone(),
            FieldCfg {
                placeholder: "view_name",
                focus: Some((ring.clone(), 10)),
                ..Default::default()
            },
            |d, v| d.name = v.trim().to_string(),
        )
        .style(move |s| s.width(field_w())),
    );

    // The body. A plain multi-line field rather than the SQL editor pane: that
    // one is bound to a query tab's document, and this is a fragment, not a
    // statement — but it's the same widget, in the same monospace face, that the
    // DDL preview shows SQL in, so it reads and selects like every other SQL
    // surface here.
    let body = form_setting(
        "SELECT statement",
        bound_field(
            &ui,
            draft.select.clone(),
            FieldCfg {
                multiline: true,
                no_wrap: true,
                mono: true,
                font_size: theme::font_body,
                max_rows: Some(ui.ddl.view_rows),
                placeholder: "SELECT …",
                focus: Some((ring.clone(), 20)),
                // It's SQL: Tab indents. Escape leaves.
                tab_indents: true,
                ..Default::default()
            },
            |d, v| d.select = v.to_string(),
        )
        .style(|s| s.width_full()),
    );

    // ── options ──────────────────────────────────────────────────────────────
    // Shown because they're *preserved*: a replace that doesn't restate them
    // resets them, so the form is where the user sees what's coming along.
    // The one option two of the three engines spell the same way — SQLite has no
    // check option at all.
    let check: AnyView = if sqlite {
        crate::widgets::nothing()
    } else {
        form_setting(
            "Check option",
            bound_choice(
                &ui,
                draft.options.check_option.clone().unwrap_or_default(),
                ["", "CASCADED", "LOCAL"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                ring.clone(),
                30,
                |d, v| d.options.check_option = Some(v.to_string()).filter(|s| !s.is_empty()),
            ),
        )
        .into_any()
    };

    // SQLite's alone, and the only option it has. It matters more here than the
    // others do anywhere: every edit to a SQLite view is a drop and a re-create,
    // so a list that isn't restated is a view whose columns quietly take the
    // body's names.
    let sqlite_only: AnyView = if !sqlite {
        crate::widgets::nothing()
    } else {
        form_setting(
            "Column names",
            bound_field(
                &ui,
                draft.options.column_list.clone().unwrap_or_default(),
                FieldCfg {
                    placeholder: "optional — taken from the SELECT when empty",
                    focus: Some((ring.clone(), 30)),
                    mono: true,
                    ..Default::default()
                },
                |d, v| d.options.column_list = Some(v.trim().to_string()).filter(|s| !s.is_empty()),
            )
            // Wider than the form's other single-line fields: the placeholder is
            // a sentence, and at `field_w()` it now ellipsizes away the half that
            // says what happens when the field is left empty. (It used to *paint
            // over* the border instead — that is fixed in `edit_field`, and this
            // width is no longer a workaround for it.) Same width the trigger
            // editor's `When` and `Of columns` fields take, so the two SQLite
            // forms line up.
            .style(move |s| s.width(field_w() * 1.6)),
        )
        .into_any()
    };

    // MySQL's alone, and built only there rather than built and hidden: a
    // `hide()`n control is still in the modal's Tab order, so Tab would land on
    // something nobody can see.
    let mysql_only: AnyView = if !my {
        crate::widgets::nothing()
    } else {
        let security = form_setting(
            "SQL security",
            bound_choice(
                &ui,
                draft.options.security.clone().unwrap_or_default(),
                ["", "DEFINER", "INVOKER"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                ring.clone(),
                40,
                |d, v| d.options.security = Some(v.to_string()).filter(|s| !s.is_empty()),
            ),
        );
        // The definer is displayed, never edited: setting one you aren't is a
        // privileged operation, and the reason it's here at all is that the
        // emitted statement restates it.
        let definer = form_setting_owned(
            "Definer".to_string(),
            text(draft.options.definer.clone().unwrap_or_else(|| "—".into()))
                .style(|s| s.color(theme::text_dim()).font_size(theme::font_body())),
        );
        v_stack((security, definer))
            .style(|s| s.flex_col().gap(form_gap()).width_full())
            .into_any()
    };

    // PostgreSQL's escape hatch. Off by default — a drop is never the quiet
    // answer — and the preview spells out what taking it costs.
    let recreate: AnyView = if !pg {
        crate::widgets::nothing()
    } else {
        let sig = floem::reactive::create_rw_signal(draft.force_recreate);
        create_effect(move |prev: Option<bool>| {
            let v = sig.get();
            if prev.is_some_and(|p| p != v) {
                d.update(|d| d.force_recreate = v);
            }
            v
        });
        focusable_toggle_row(
            "Re-create instead of replacing",
            "PostgreSQL can only add columns to an existing view. Renaming, retyping or \
             reordering one needs a DROP and a CREATE, which takes dependent views and \
             grants with it.",
            sig,
            ring,
            50,
        )
        .into_any()
    };

    v_stack((
        form_section("View"),
        name,
        form_section("Definition").style(|s| s.margin_top(theme::scaled(4.0))),
        body,
        form_section("Options").style(|s| s.margin_top(theme::scaled(4.0))),
        check,
        sqlite_only,
        mysql_only,
        recreate,
    ))
    .style(|s| s.flex_col().gap(form_gap()).width_full())
    .into_any()
}

// ── the modal ────────────────────────────────────────────────────────────────

/// The change set the draft currently describes — the same call the preview
/// emits from, so the footer's count can never disagree with the SQL.
fn change_set(target: &ViewTarget, draft: &ViewDraft) -> ddl::ChangeSet {
    match &target.current {
        Some(cur) => ddl::diff_view(cur, draft, target.dialect),
        None => ddl::create_view(draft, target.dialect),
    }
}

/// A new view has no server-side name to be titled after, so the preview is
/// headed by the one the draft is giving it.
fn preview_from(target: &ViewTarget, draft: &ViewDraft, cs: &ddl::ChangeSet) -> DdlPreview {
    let subject = match target.current {
        Some(_) => target.display(),
        None => draft.name.clone(),
    };
    ddl_preview::preview_of(
        target.conn_id,
        &target.database,
        subject,
        cs,
        target.read_only,
    )
}

/// The view editor. Absolutely positioned over the workspace when
/// `ui.ddl.view` is `Some`.
pub(crate) fn view_editor_overlay(ui: Ui) -> impl IntoView {
    let d = ui.ddl;
    let close = move || d.view.set(None);

    // The preview stacks on top and this stays open behind it (Cancel there
    // returns here with the draft intact), but must render nothing — see the
    // designer, which has the same pairing for the same reason.
    //
    // **A memo**, as in the routine editor and for the reason `overlay_open_key`
    // gives: `fetch_algorithm` patches `current` after the form is up, and that
    // rebuilt the whole modal, dropping the caret if the user was typing in the
    // SELECT. Nothing is lost by not rebuilding — the algorithm is not a control
    // here, only a term in the diff, so the rebuild delivered nothing to screen.
    let open_key = crate::widgets::overlay_open_key(d.session, d.view, d.preview);

    dyn_container(
        move || open_key.get(),
        move |(_session, open, previewing)| {
            if !open || previewing {
                return empty().into_any();
            }
            let Some(target) = d.view.get_untracked() else {
                return empty().into_any();
            };
            let ui = ui.clone();
            let title = match &target.current {
                Some(t) => format!(
                    "Edit view {}.{}",
                    object_location(&target.database, t.schema.as_deref()),
                    t.name
                ),
                None => format!(
                    "Create view in {}",
                    object_location(&target.database, target.schema.as_deref())
                ),
            };

            // The form is built once per open, so one ring covers it; the root
            // keeps a clone to answer Tab by entering it.
            let ring = FocusRing::new();
            let root_ring = ring.clone();

            let body = crate::widgets::autohide(scroll(
                form(ui.clone(), &target, ring.clone()).style(|s| {
                    s.width_full()
                        .padding_horiz(modal_pad_h())
                        .padding_vert(theme::scaled(18.0))
                }),
            ))
            .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0));

            // Validation first (it blocks), then the change count.
            //
            // **The target comes from the key, not from the capture above.**
            // `target` was cloned once when the modal was built, and
            // `fetch_algorithm` patches `current` *after* that — deliberately,
            // on both sides, so the diff stays empty. Once the modal's own key
            // became a memo it stopped rebuilding on that patch, and this footer
            // kept diffing the draft the fetch had corrected against a `current`
            // it had not. The result was a view opening with "1 change" and a
            // live Preview over an edit nobody made.
            //
            // Reading `d.view` here is safe where reading it in the *modal's*
            // key is not: this container already rebuilds on every keystroke
            // (`d.view_draft`), and it holds the footer, not the form — so no
            // caret lives inside it. That distinction is the whole of `c11dfb8`.
            let status = dyn_container(
                move || (d.view.get(), d.view_draft.get()),
                move |(target, draft)| {
                    let Some(status_target) = target else {
                        return empty().into_any();
                    };
                    let errs = draft.validate();
                    if let Some(first) = errs.first() {
                        return text(first.clone())
                            .style(|s| {
                                s.color(theme::error())
                                    .font_size(theme::font_label())
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
                        s.font_size(theme::font_label()).color(if n == 0 {
                            theme::text_faint()
                        } else {
                            theme::change_count()
                        })
                    })
                    .into_any()
                },
            );

            let preview_ui = ui.clone();
            let ring_actions = ring.clone();
            // Keyed on the target for the same reason `status` is, and it
            // matters more here: this decides whether **Preview SQL** is enabled
            // and builds the plan it opens.
            let actions = dyn_container(
                move || (d.view.get(), d.view_draft.get()),
                move |(current, draft)| {
                    let ui = preview_ui.clone();
                    let Some(target) = current else {
                        return empty().into_any();
                    };
                    let ring = ring_actions.clone();
                    let cs = change_set(&target, &draft);
                    let ready = draft.validate().is_empty() && !cs.is_empty();
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
                                ddl_preview::open_preview(&ui, preview_from(&target, &draft, &cs));
                            },
                        ),
                    ))
                    .style(|s| s.flex_row().items_center().gap(action_gap()))
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
            .style(|s| panel_style(s).width(panel_w()).height(modal_h(PANEL_H)));

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
        if d.view.get().is_some() && d.preview.get().is_none() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}
