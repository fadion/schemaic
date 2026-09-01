//! The form for creating a **container**: a database, or one of PostgreSQL's
//! namespaces inside one.
//!
//! The smallest of the schema editors, and deliberately so — a name, and the one
//! or two options the engine has that are safe to offer. Everything a server
//! would let you set about a database is not here: see `ddl::DatabaseDraft`'s
//! field docs for why PostgreSQL's `ENCODING` in particular is left to the
//! template rather than given a field that mostly produces a refusal.
//!
//! Three things separate it from its peers, each of which is the reason it is a
//! module rather than an arm of [`crate::object_editor`]:
//!
//! * **It only ever creates.** There is no `current`, no diff and no change
//!   count: a container is dropped from its own row's menu, and neither engine
//!   offers a rename that is safe to perform. The footer counts nothing, so it
//!   says what will be made instead.
//! * **A database's plan is server-level** ([`crate::DdlScope::Server`]) and a
//!   namespace's is not. Both leave here through [`ddl_preview::preview_container`],
//!   which reads that off the change rather than taking it from this form.
//! * **It has two homes.** The schema tree's `Create ▸` submenu raises it, and so
//!   does the SCHEMA gear — the second because a connection whose tree already
//!   fills the panel has no blank space to right-click, which is exactly the
//!   connection on which a new database is worth making.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::ddl::{self, DatabaseDraft};

use crate::table_designer::{edit_ctx, suggest_chevron};
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, action_gap, focus_root_with_ring, form_gap,
    form_section, form_setting, modal_footer_split, modal_h, modal_pad_h, modal_title_owned,
    modal_w, panel_style,
};
use crate::{ContainerKind, DatabaseTarget, FieldCfg, Ui, ddl_preview, edit_field, theme};

fn panel_w() -> f64 {
    modal_w(520.0)
}

/// **A fixed height, like every other modal here, and not because the content
/// wants one.** This form is two to four rows tall, so sizing the panel to its
/// content is the obvious call — and it is the one that ships an empty modal:
/// the body is a `scroll` with `flex_grow(1)` and `min_height(0)`, and inside an
/// auto-height parent there is no free space for it to grow into, so it resolves
/// to zero and the panel renders a title bar sitting directly on a footer.
///
/// **Set so the tallest form fits without scrolling**, which is MySQL's three
/// fields — a scrollbar over four rows reads as a modal that got clipped rather
/// than one with more to say. Measured at 100%; `modal_h` caps it against short
/// windows, where the scroll is what it is for.
const PANEL_H: f64 = 390.0;

/// Text-field width, matching the object editor's so two modals opened from the
/// same menu don't disagree about how wide a name is.
fn field_w() -> f64 {
    theme::scaled(260.0)
}

// ── opening ──────────────────────────────────────────────────────────────────

/// Open the form on a blank draft.
///
/// `database` is the database a namespace goes in, and is ignored for a
/// database — which has none, being the thing about to exist. The caller has
/// already asked the capability (`ddl::supports_database_editing` /
/// `supports_namespace_editing`); this opens what it is given.
///
/// **The read-only refusal is here, not at the four call sites.** This action
/// has more homes than any other create in the app — the tree's `Create ▸`
/// submenu, the SCHEMA gear, the blank-space menu, and again for a namespace —
/// and the invariant is that a launch guards itself in the same step that
/// launches it rather than through the disabled entry. Written per site that
/// would be four copies of the same `if`, and it had already been written only
/// three times: `create_submenu`'s arm relied on `MenuEntry::disabled` alone.
/// One refusal at the door is the only version that stays true when a fifth home
/// appears. The entries stay dimmed, because that is what *says* the action is
/// unavailable; this is what makes it so.
pub(crate) fn open_for_new(ui: &Ui, kind: ContainerKind, database: Option<&str>) {
    let ctx = edit_ctx(ui);
    if ctx.read_only {
        return;
    }
    let d = ui.ddl;
    // A new editing session — see `DdlUi::session`.
    d.session.update(|g| *g += 1);
    d.database_draft.set(DatabaseDraft::blank(match kind {
        ContainerKind::Database => "new_database",
        ContainerKind::Schema => "new_schema",
    }));
    d.error.set(None);
    d.preview.set(None);
    // Every editor shares the preview stacked on top, and each overlay knows
    // only its own flag — two open would paint two panels.
    ddl_preview::close_peers(d, false);
    d.database.set(Some(DatabaseTarget {
        conn_id: ctx.conn_id,
        kind,
        database: database.map(str::to_string),
        dialect: ctx.dialect,
        read_only: ctx.read_only,
    }));
    fetch_roles(ui, &ctx);
}

/// Start the Owner shortcut's role fetch, if this engine has owners.
///
/// **Cleared first, then filled.** The list is one signal shared by every
/// opening of this modal, so leaving the previous connection's roles in it would
/// offer another server's names until the reply landed — and if the fetch fails,
/// for as long as the modal is open.
///
/// The reply is **stamped with the session it was asked in**, the guard
/// `DdlUi::session` exists for: this is a fetch across connections, and one
/// started for a PostgreSQL connection can land after the user has closed the
/// modal and reopened it on another. Failure writes nothing and says nothing —
/// see [`crate::RolesFn`].
fn fetch_roles(ui: &Ui, ctx: &crate::table_designer::EditCtx) {
    let d = ui.ddl;
    d.roles.set(Vec::new());
    if !schemaic_core::ddl::supports_owners(ctx.dialect) {
        return;
    }
    let asked = d.session.get_untracked();
    (ui.schema_actions.roles)(
        ctx.conn_id,
        Rc::new(move |roles| {
            if d.session.get_untracked() == asked {
                d.roles.set(roles);
            }
        }),
    );
}

// ── the plan ─────────────────────────────────────────────────────────────────

/// The change this form is asking for.
///
/// Pure, and separate from the view for the reason every decision in this crate
/// is: the two kinds produce different statements at different levels, and
/// getting that mapping backwards is not visible in a rendered form. `owner` is
/// carried on both — PostgreSQL takes one for either — and is dropped for a
/// MySQL database by `DatabaseDraft::create_sql`, which writes only the clauses
/// its dialect has.
pub(crate) fn change_of(kind: ContainerKind, draft: &DatabaseDraft) -> ddl::Change {
    let name = draft.name.trim().to_string();
    match kind {
        ContainerKind::Database => {
            let mut d = draft.clone();
            d.name = name;
            ddl::Change::CreateDatabase(Box::new(d))
        }
        ContainerKind::Schema => ddl::Change::CreateSchema {
            name,
            owner: draft
                .owner
                .clone()
                .map(|o| o.trim().to_string())
                .filter(|o| !o.is_empty()),
        },
    }
}

/// Where the resulting plan runs: a namespace is created **in** its database, a
/// database is created on a server-level connection that names none.
///
/// The empty string for a database is not a placeholder standing in for a real
/// value — under [`crate::DdlScope::Server`] the field is what the run must
/// *avoid*, and there is nothing to avoid when nothing exists yet.
fn plan_database(target: &DatabaseTarget) -> String {
    target.database.clone().unwrap_or_default()
}

// ── the form ─────────────────────────────────────────────────────────────────

/// A text field bound to one place in the draft. Same contract as the object
/// editor's: the local signal is seeded once on build and the effect writes back
/// only on a genuine change, so a rebuild can't read as an edit.
fn bound_field(
    ui: &Ui,
    initial: String,
    cfg: FieldCfg,
    apply: impl Fn(&mut DatabaseDraft, &str) + 'static,
) -> AnyView {
    let draft = ui.ddl.database_draft;
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

/// An optional field, written back as `None` when it is emptied — which is what
/// makes "let the server choose" reachable again after something was typed —
/// with a [`suggest_chevron`] beside it offering `options`.
///
/// **Free text plus a shortcut, not a picker**, which is the standing the
/// designer's column-type field has and the reason that control is shared rather
/// than reimplemented here. Every value these three fields take is per-server
/// and per-version: a character set this build has never heard of, a collation
/// only MariaDB has, a role created five minutes ago. A closed list would make
/// each of those a dead end; an open one with a menu beside it makes the common
/// answer one click and the uncommon one still reachable.
///
/// The chevron takes `tabindex + 1`, so Tab walks the box you type in and then
/// the list of things you could have typed — which is why the callers space
/// their indices by ten.
#[allow(clippy::too_many_arguments)] // a UI builder; grouping into a struct adds no clarity
fn optional_field(
    ui: &Ui,
    initial: Option<String>,
    placeholder: &'static str,
    options: impl Fn() -> Vec<String> + 'static,
    ring: &FocusRing,
    tabindex: u32,
    apply: impl Fn(&mut DatabaseDraft, Option<String>) + 'static,
) -> AnyView {
    let draft = ui.ddl.database_draft;
    let sig = floem::reactive::create_rw_signal(initial.unwrap_or_default());
    create_effect(move |prev: Option<String>| {
        let v = sig.get();
        if prev.is_some_and(|p| p != v) {
            draft.update(|d| apply(d, Some(v.trim().to_string()).filter(|s| !s.is_empty())));
        }
        v
    });
    h_stack((
        edit_field(
            sig,
            FieldCfg {
                placeholder,
                focus: Some((ring.clone(), tabindex)),
                ..Default::default()
            },
        )
        .style(move |s| s.width(field_w())),
        suggest_chevron(ui, sig, options, ring.clone(), tabindex + 1),
    ))
    .style(|s| s.flex_row().items_center().gap(theme::scaled(2.0)))
    .into_any()
}

fn form(ui: &Ui, target: &DatabaseTarget, draft: &DatabaseDraft, ring: FocusRing) -> AnyView {
    let name = form_setting(
        "Name",
        bound_field(
            ui,
            draft.name.clone(),
            FieldCfg {
                placeholder: match target.kind {
                    ContainerKind::Database => "database_name",
                    ContainerKind::Schema => "schema_name",
                },
                focus: Some((ring.clone(), 10)),
                ..Default::default()
            },
            |d, v| d.name = v.trim().to_string(),
        )
        .style(move |s| s.width(field_w())),
    );

    let mut rows: Vec<AnyView> = vec![
        form_section(match target.kind {
            ContainerKind::Database => "Database",
            ContainerKind::Schema => "Schema",
        })
        .into_any(),
        name.into_any(),
    ];

    // **The option fields are per-engine, and absent rather than dimmed where
    // the engine has none** — the same call `create_children` makes about the
    // entries that open this form. A greyed "Character set" on PostgreSQL would
    // read as "not right now"; it is "not on this engine, ever".
    //
    // Capabilities rather than `dialect ==` tests: both of these were spelled as
    // engine comparisons first, which is the shape that compiles cleanly while
    // sorting a fourth engine onto whichever side it happens to fall.
    let owns = ddl::supports_owners(target.dialect);
    // A namespace takes no character set on any engine — it is a name in a
    // catalogue, not a store — so this asks the *kind* as well as the engine.
    let charsets =
        target.kind == ContainerKind::Database && ddl::supports_database_charset(target.dialect);
    if charsets {
        rows.push(
            form_setting(
                "Character set",
                optional_field(
                    ui,
                    draft.charset.clone(),
                    "server default",
                    || ddl::MYSQL_CHARSETS.iter().map(|c| c.to_string()).collect(),
                    &ring,
                    20,
                    |d, v| d.charset = v,
                ),
            )
            .into_any(),
        );
        rows.push(
            form_setting(
                "Collation",
                optional_field(
                    ui,
                    draft.collation.clone(),
                    "server default",
                    || {
                        ddl::MYSQL_COLLATIONS
                            .iter()
                            .map(|c| c.to_string())
                            .collect()
                    },
                    &ring,
                    30,
                    |d, v| d.collation = v,
                ),
            )
            .into_any(),
        );
    }
    if owns {
        // **Read when the chevron is pressed, not now.** The roles come from a
        // fetch started in `open_for_new`, which lands after this form is built;
        // `suggest_chevron` calls this closure at press time, so a late reply
        // reaches the menu without rebuilding the field.
        let roles = ui.ddl.roles;
        rows.push(
            form_setting(
                "Owner",
                optional_field(
                    ui,
                    draft.owner.clone(),
                    "you",
                    move || roles.get_untracked(),
                    &ring,
                    40,
                    |d, v| d.owner = v,
                ),
            )
            .into_any(),
        );
    }

    v_stack_from_iter(rows)
        .style(|s| s.flex_col().gap(form_gap()).width_full())
        .into_any()
}

// ── the modal ────────────────────────────────────────────────────────────────

pub(crate) fn database_editor_overlay(ui: Ui) -> impl IntoView {
    let d = ui.ddl;
    let close = move || d.database.set(None);

    dyn_container(
        // The preview stacks on top and this stays open behind it (Cancel there
        // returns here with the draft intact), but must render nothing — the
        // same pairing every other editor uses, for the same reason.
        move || (d.database.get().is_some(), d.preview.get().is_some()),
        move |(open, previewing)| {
            if !open || previewing {
                return empty().into_any();
            }
            let Some(target) = d.database.get_untracked() else {
                return empty().into_any();
            };
            let ui = ui.clone();
            let title = match (target.kind, target.database.as_deref()) {
                (ContainerKind::Schema, Some(db)) => format!("Create schema in {db}"),
                (kind, _) => format!("Create {}", kind.label()),
            };

            let ring = FocusRing::new();
            let root_ring = ring.clone();
            // **Built once, from the draft as it stands at open.** The fields
            // seed local signals and write back; keying the form on the draft
            // would tear a field down mid-keystroke, which is the rule the
            // object editor's module comment states.
            let seed = d.database_draft.get_untracked();
            let body = crate::widgets::autohide(scroll(
                form(&ui, &target, &seed, ring.clone()).style(|s| {
                    s.width_full()
                        .padding_horiz(modal_pad_h())
                        .padding_vert(theme::scaled(18.0))
                }),
            ))
            .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0));

            // **The footer says nothing while the form is valid**, unlike its
            // peers, and that is the whole difference between this modal and
            // theirs: they show a change *count*, which is information — a diff
            // the reader can't otherwise see. Here there is no diff, so the only
            // sentence available was "Creates the database <name>", which
            // restates the modal's title and the name field the reader is
            // looking at. A status line that only ever agrees with what is
            // already on screen teaches the eye to skip the place the *errors*
            // appear.
            let status = dyn_container(
                move || d.database_draft.get(),
                move |draft| match draft.validate().pop() {
                    Some(first) => text(first)
                        .style(|s| {
                            s.color(theme::error())
                                .font_size(theme::font_label())
                                .max_width(theme::scaled(320.0))
                        })
                        .into_any(),
                    None => crate::widgets::nothing().into_any(),
                },
            );

            let preview_ui = ui.clone();
            let preview_target = target.clone();
            let ring_actions = ring.clone();
            let actions = dyn_container(
                move || d.database_draft.get(),
                move |draft| {
                    let ui = preview_ui.clone();
                    let target = preview_target.clone();
                    let ring = ring_actions.clone();
                    let ready = draft.validate().is_empty();
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
                                let name = draft.name.trim().to_string();
                                ddl_preview::preview_container(
                                    &ui,
                                    &plan_database(&target),
                                    &name,
                                    change_of(target.kind, &draft),
                                );
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
        if d.database.get().is_some() && d.preview.get().is_none() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemaic_core::intel::SqlDialect;

    fn draft(name: &str) -> DatabaseDraft {
        DatabaseDraft::blank(name)
    }

    /// **The mapping this module exists to get right**, and the one a rendered
    /// form cannot show you: the two kinds are different statements at different
    /// levels, so a swapped arm would create a database where the title said
    /// schema — and send it down the server-level run path, or fail to.
    #[test]
    fn each_kind_asks_for_its_own_change() {
        let c = change_of(ContainerKind::Database, &draft("shop"));
        assert!(matches!(c, ddl::Change::CreateDatabase(_)));
        assert!(ddl::is_server_level(&c));

        let c = change_of(ContainerKind::Schema, &draft("sales"));
        assert!(matches!(c, ddl::Change::CreateSchema { .. }));
        assert!(
            !ddl::is_server_level(&c),
            "a namespace is created inside its database, on the ordinary path"
        );
    }

    /// A name arrives from a text field, so it arrives with whatever whitespace
    /// was typed around it. Trimming at the change rather than in the emitter
    /// keeps `CREATE DATABASE " shop "` — a real, differently-named database on
    /// both engines — from being one stray space away.
    #[test]
    fn a_typed_name_is_trimmed_before_it_becomes_a_plan() {
        let c = change_of(ContainerKind::Database, &draft("  shop \t"));
        let ddl::Change::CreateDatabase(d) = c else {
            panic!("expected a database")
        };
        assert_eq!(d.name, "shop");
        assert_eq!(
            ddl::server_level("shop", SqlDialect::MySql, ddl::Change::CreateDatabase(d)).emit(),
            vec!["CREATE DATABASE `shop`;"]
        );

        let c = change_of(ContainerKind::Schema, &draft(" sales "));
        let ddl::Change::CreateSchema { name, .. } = c else {
            panic!("expected a schema")
        };
        assert_eq!(name, "sales");
    }

    /// An owner field that was typed into and then cleared means "let the server
    /// choose" again, not `AUTHORIZATION ""`. The form writes `None` for an empty
    /// box; this pins that the change agrees, since the two are written in
    /// different places and only the pair is the behaviour.
    #[test]
    fn a_blank_owner_is_no_owner() {
        let mut d = draft("sales");
        d.owner = Some("   ".into());
        let ddl::Change::CreateSchema { owner, .. } = change_of(ContainerKind::Schema, &d) else {
            panic!("expected a schema")
        };
        assert_eq!(owner, None);

        d.owner = Some("app".into());
        let ddl::Change::CreateSchema { owner, .. } = change_of(ContainerKind::Schema, &d) else {
            panic!("expected a schema")
        };
        assert_eq!(owner.as_deref(), Some("app"));
    }

    /// A new database has no database to run in, and that is the field
    /// `DdlScope::Server` reads as what the run must *avoid* — so it has to be
    /// empty rather than carrying some plausible-looking name.
    #[test]
    fn a_new_database_names_no_database_to_run_in() {
        let target = |kind, database: Option<&str>| DatabaseTarget {
            conn_id: 1,
            kind,
            database: database.map(str::to_string),
            dialect: SqlDialect::Postgres,
            read_only: false,
        };
        assert_eq!(plan_database(&target(ContainerKind::Database, None)), "");
        assert_eq!(
            plan_database(&target(ContainerKind::Schema, Some("shop"))),
            "shop"
        );
    }
}
