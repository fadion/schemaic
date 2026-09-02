//! The two forms behind the Users and privileges browser's write half: making
//! an account, and changing what one may do.
//!
//! Both are raised from the browser and both end at
//! [`ddl_preview::preview_account`], so the review between a plan and the server
//! is the same one every other editor here reaches. Neither ever runs a
//! statement itself.
//!
//! **Two forms, because they are two questions.** The account form only ever
//! *creates* — an account is dropped from its own row in the browser, and
//! neither engine offers a rename that is safe to perform, which is the same
//! shape [`crate::database_editor`] has and for the same reasons. The grant form
//! only ever changes privileges, and it never touches the account itself.
//!
//! **The grant form is one form for four statements.** Grant or revoke,
//! privileges or a role: two dropdowns, and the mapping between them and
//! `Change::{Grant,Revoke}{Privileges,Role}` lives in `ddl::grant_change` with
//! its own test — a mapping that is invisible in a rendered form is exactly the
//! kind that ships backwards.
//!
//! **The account form holds a password**, which nothing else in this crate does.
//! It is cleared on open and on Cancel, it is never persisted, and it becomes
//! visible in exactly one place: the preview's SQL. That is deliberate — the
//! preview is the app's one gate between a plan and a server, and a statement it
//! showed with a field blanked would not be the statement it ran.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::ddl;
use schemaic_core::intel::SqlDialect;
use schemaic_core::users::{
    self, AccountDraft, GrantDraft, GrantLevelKind, GrantSubject, Principal, PrincipalKind,
};

use crate::table_designer::{edit_ctx, suggest_chevron};
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, action_gap, autohide, dismiss_layer,
    focus_root_with_ring, form_gap, form_section, form_setting, modal_footer_split, modal_h,
    modal_pad_h, modal_title_owned, modal_w, panel_style,
};
use crate::{AccountTarget, FieldCfg, GrantTarget, Ui, ddl_preview, edit_field, theme};

fn panel_w() -> f64 {
    modal_w(540.0)
}

/// The grant form is the taller of the two — a level, two names and a privilege
/// list — and gets its own width so the checkbox columns are not cramped.
fn grant_panel_w() -> f64 {
    modal_w(620.0)
}

/// A fixed height, for the reason `database_editor::PANEL_H` is one: the body is
/// a `scroll` with `flex_grow(1)`, and inside an auto-height parent that
/// resolves to zero and paints a title bar on a footer.
const ACCOUNT_PANEL_H: f64 = 380.0;

/// Taller than the account form because the privilege list is as long as
/// eighteen rows on MySQL's database level. `modal_h` caps it against short
/// windows, which is what the scroll is for.
const GRANT_PANEL_H: f64 = 560.0;

fn field_w() -> f64 {
    theme::scaled(260.0)
}

// ── opening ──────────────────────────────────────────────────────────────────

/// Open the account form on a blank draft.
///
/// **The read-only refusal is here, not at the call site**, which is the rule
/// every editor in this crate follows: a launch guards itself in the same step
/// that launches it. The browser's button stays dimmed, because that is what
/// *says* the action is unavailable; this is what makes it so.
///
/// `database` is where the plan will run — see [`AccountTarget::database`]. The
/// caller has already asked `users::supports_user_admin`.
pub(crate) fn open_for_new(ui: &Ui, database: &str) {
    let ctx = edit_ctx(ui);
    if ctx.read_only {
        return;
    }
    let d = ui.ddl;
    // A new editing session — see `DdlUi::session`.
    d.session.update(|g| *g += 1);
    // **Blank, every time.** The draft carries a password, and a form that
    // reopened holding the last one would put a credential on screen that
    // nobody typed this time.
    d.account_draft.set(AccountDraft::default());
    d.error.set(None);
    d.preview.set(None);
    ddl_preview::close_peers(d, false);
    d.account.set(Some(AccountTarget {
        conn_id: ctx.conn_id,
        database: database.to_string(),
        dialect: ctx.dialect,
        read_only: ctx.read_only,
    }));
}

/// Open the grant form for one account. Same refusal at the same door.
pub(crate) fn open_for_grant(ui: &Ui, database: &str, account: &Principal) {
    let ctx = edit_ctx(ui);
    if ctx.read_only {
        return;
    }
    let d = ui.ddl;
    d.session.update(|g| *g += 1);
    d.grant_draft.set(initial_grant_draft(ctx.dialect));
    d.error.set(None);
    d.preview.set(None);
    ddl_preview::close_peers(d, false);
    d.grant.set(Some(GrantTarget {
        conn_id: ctx.conn_id,
        database: database.to_string(),
        account: account.clone(),
        dialect: ctx.dialect,
        read_only: ctx.read_only,
    }));
}

/// The draft the grant form opens on.
///
/// **Pre-picked to the widest level the engine has**, so the form opens with its
/// name fields already meaning something rather than with a picker the user has
/// to notice first — and so `level` is `None` *only* on an engine that has no
/// levels at all. The form relies on that: it shows the Level row exactly when
/// the draft holds a level, with no fallback to `levels_for(…).first()` of its
/// own, because a dropdown displaying a level the draft does not hold would
/// leave the rest of the form hidden and picking the entry already shown would
/// not be a change that unstuck it.
///
/// Its own function so that coupling is one call and one test rather than a
/// literal in an opener and an assumption in a view.
pub(crate) fn initial_grant_draft(dialect: SqlDialect) -> GrantDraft {
    GrantDraft {
        level: users::levels_for(dialect).first().copied(),
        ..Default::default()
    }
}

// ── shared field plumbing ────────────────────────────────────────────────────

/// A text field bound to one place in a draft. Same contract as the database
/// editor's: the local signal is seeded once on build and the effect writes back
/// only on a genuine change, so a rebuild can't read as an edit.
fn bound_field<D: Clone + 'static>(
    draft: RwSignal<D>,
    initial: String,
    cfg: FieldCfg,
    apply: impl Fn(&mut D, &str) + 'static,
) -> AnyView {
    let sig = floem::reactive::create_rw_signal(initial);
    create_effect(move |prev: Option<String>| {
        let v = sig.get();
        if prev.is_some_and(|p| p != v) {
            draft.update(|d| apply(d, &v));
        }
        v
    });
    edit_field(sig, cfg)
        .style(|s| s.width(field_w()))
        .into_any()
}

/// A dropdown bound to one place in a draft — the settings modals' `<select>`,
/// which is what every fixed-list choice in the app wears.
///
/// **The local signal exists because the value lives in a struct, not in a
/// signal of its own.** `focusable_dropdown` binds to an `RwSignal<T>`, and a
/// draft field is not one; this is the same seed-once-and-write-back contract
/// [`bound_field`] has, so a rebuild cannot read as an edit.
///
/// `label` is a `fn` rather than a closure because `focusable_dropdown`'s is:
/// see its own note on why a label computed from the value beats one looked up
/// in a table with a defaulting arm.
fn bound_dropdown<D, T, S>(
    draft: RwSignal<D>,
    initial: T,
    options: Vec<T>,
    label: fn(T) -> S,
    ring: FocusRing,
    tabindex: u32,
    apply: impl Fn(&mut D, T) + 'static,
) -> AnyView
where
    D: Clone + 'static,
    T: Copy + PartialEq + 'static,
    S: Into<String> + 'static,
{
    let sig = floem::reactive::create_rw_signal(initial);
    create_effect(move |prev: Option<T>| {
        let v = sig.get();
        if prev.is_some_and(|p| p != v) {
            draft.update(|d| apply(d, v));
        }
        v
    });
    container(crate::settings::focusable_dropdown(
        sig, options, label, ring, tabindex,
    ))
    .style(|s| s.width(field_w()))
    .into_any()
}

/// A switch bound to one place in a draft — the app's own toggle, so a yes/no in
/// a form reads as a yes/no everywhere else it appears.
///
/// It replaced a button labelled "Yes" that ignored its current value entirely:
/// the two rows whose whole job is to show a state showed none of it.
fn bound_toggle<D>(
    draft: RwSignal<D>,
    initial: bool,
    ring: FocusRing,
    tabindex: u32,
    apply: impl Fn(&mut D, bool) + 'static,
) -> AnyView
where
    D: Clone + 'static,
{
    let sig = floem::reactive::create_rw_signal(initial);
    create_effect(move |prev: Option<bool>| {
        let v = sig.get();
        if prev.is_some_and(|p| p != v) {
            draft.update(|d| apply(d, v));
        }
        v
    });
    crate::settings::focusable_toggle(sig, ring, tabindex).into_any()
}

/// A field with a chevron offering values the form knows about — the same pair
/// the database editor's Owner uses.
#[allow(clippy::too_many_arguments)]
fn suggested_field<D: Clone + 'static>(
    ui: &Ui,
    draft: RwSignal<D>,
    initial: String,
    placeholder: &'static str,
    options: impl Fn() -> Vec<String> + Clone + 'static,
    ring: &FocusRing,
    tabindex: u32,
    apply: impl Fn(&mut D, &str) + 'static,
) -> AnyView {
    let sig = floem::reactive::create_rw_signal(initial);
    create_effect(move |prev: Option<String>| {
        let v = sig.get();
        if prev.is_some_and(|p| p != v) {
            draft.update(|d| apply(d, &v));
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

// ── what makes a form rebuild ────────────────────────────────────────────────

/// The account form's **shape**: the one value that decides which fields exist.
///
/// A role takes neither a host nor a password on either engine, so those two
/// rows appear and vanish with the Kind — and nothing else about the draft
/// changes what is on screen.
pub(crate) fn account_form_shape(d: &AccountDraft) -> PrincipalKind {
    d.kind
}

/// The grant form's **shape**: which fields exist depends on these three and on
/// nothing else. Two name fields at a table level, one above it and none at the
/// whole-server level; a role field instead of all of them; and the two option
/// rows only on a grant.
pub(crate) fn grant_form_shape(d: &GrantDraft) -> (GrantSubject, bool, Option<GrantLevelKind>) {
    (d.subject, d.revoke, d.level)
}

// ── the account form ─────────────────────────────────────────────────────────

/// What this form is asking for. Pure, and out of the render for the reason the
/// database editor's `change_of` is: which of the two statements a draft becomes
/// is not visible in a rendered form.
pub(crate) fn account_change(draft: &AccountDraft) -> ddl::Change {
    let mut d = draft.clone();
    d.name = d.name.trim().to_string();
    d.host = d.host.trim().to_string();
    ddl::Change::CreateAccount(Box::new(d))
}

fn account_form(
    target: &AccountTarget,
    seed: &AccountDraft,
    ring: FocusRing,
    d: crate::DdlUi,
) -> AnyView {
    let draft = d.account_draft;
    let mut rows: Vec<AnyView> = Vec::new();

    // **The kind picker comes first**, because it decides what the rest of the
    // form means: a role takes no host and no password on either engine, and the
    // two fields below vanish rather than sitting there inert.
    let kind = seed.kind;
    rows.push(
        form_setting(
            "Kind",
            bound_dropdown(
                draft,
                kind,
                vec![PrincipalKind::User, PrincipalKind::Role],
                PrincipalKind::label,
                ring.clone(),
                8,
                |d, v| d.kind = v,
            ),
        )
        .into_any(),
    );

    rows.push(
        form_setting(
            "Name",
            bound_field(
                draft,
                seed.name.clone(),
                FieldCfg {
                    placeholder: "account_name",
                    focus: Some((ring.clone(), 10)),
                    ..Default::default()
                },
                |d, v| d.name = v.to_string(),
            ),
        )
        .into_any(),
    );

    // **A host is what a MySQL account *is*, and PostgreSQL has none at all** —
    // absent rather than dimmed, the same call every per-engine field in this
    // crate makes. Asked as a property of the account rather than of the engine:
    // a role has no host on either.
    let hosts = target.dialect == SqlDialect::MySql;
    if hosts && kind == PrincipalKind::User {
        rows.push(
            form_setting(
                "Host",
                bound_field(
                    draft,
                    seed.host.clone(),
                    FieldCfg {
                        // The default MySQL itself applies to an unqualified
                        // `CREATE USER`, said rather than left blank so the
                        // account that gets made is not a surprise.
                        placeholder: "% (any host)",
                        focus: Some((ring.clone(), 20)),
                        ..Default::default()
                    },
                    |d, v| d.host = v.to_string(),
                ),
            )
            .into_any(),
        );
    }

    if kind == PrincipalKind::User {
        rows.push(
            form_setting(
                "Password",
                bound_field(
                    draft,
                    seed.password.clone(),
                    FieldCfg {
                        placeholder: "none",
                        focus: Some((ring, 30)),
                        ..Default::default()
                    },
                    |d, v| d.password = v.to_string(),
                ),
            )
            .into_any(),
        );
        // The one field in the app whose value reaches a screenshot, so it says
        // so where it is typed rather than only in the module comment.
        rows.push(
            text("The password appears in the previewed SQL, which is the statement that runs.")
                .style(|s| {
                    s.font_size(theme::font_hint())
                        .color(theme::text_faint())
                        .width_full()
                })
                .into_any(),
        );
    }

    v_stack_from_iter(rows)
        .style(|s| s.flex_col().gap(form_gap()).width_full())
        .into_any()
}

/// The picked/not-picked outline every toggle in these two forms wears.
///
/// **The border is always there; only its colour changes.** Taffy sizes the
/// border box, so adding a 1px rule to a button sized by its own padding grows
/// it by 2px — the button jumped a pixel each way the moment it was chosen, and
/// a row of them re-flowed around it. Painting the resting state
/// `Color::TRANSPARENT` keeps the box identical in both states, which is the
/// same accounting `widgets::row_menu_mark_pad` does one level down for a row
/// that cannot spare the space.
fn picked_outline(s: floem::style::Style, picked: bool) -> floem::style::Style {
    s.border(1.0).border_color(if picked {
        theme::accent()
    } else {
        floem::peniko::Color::TRANSPARENT
    })
}

pub(crate) fn account_editor_overlay(ui: Ui) -> impl IntoView {
    let d = ui.ddl;
    let close = move || d.account.set(None);

    dyn_container(
        // The preview stacks on top and this stays open behind it (Cancel there
        // returns here with the draft intact), but must render nothing — the
        // pairing every other editor uses.
        move || (d.account.get().is_some(), d.preview.get().is_some()),
        move |(open, previewing)| {
            if !open || previewing {
                return empty().into_any();
            }
            let Some(target) = d.account.get_untracked() else {
                return empty().into_any();
            };
            let ring = FocusRing::new();
            let root_ring = ring.clone();
            let ui = ui.clone();

            // **Keyed on a memo over the form's shape, not on the draft.**
            // `dyn_container` has no equality check of its own — floem's
            // `create_updater` calls back on every re-run and the child scope is
            // then disposed and rebuilt unconditionally — so a key closure that
            // reads the draft directly rebuilds the whole form on *any* write to
            // it, a keystroke in Name included: floem clears the focus when a
            // view is removed, so the caret vanishes mid-word and the next
            // characters go nowhere. The same trap, and the same fix, as
            // `widgets::overlay_open_key`.
            let shape =
                floem::reactive::create_memo(move |_| d.account_draft.with(account_form_shape));
            let body_target = target.clone();
            let body = autohide(scroll(
                dyn_container(
                    move || shape.get(),
                    move |_| {
                        account_form(
                            &body_target,
                            &d.account_draft.get_untracked(),
                            ring.clone(),
                            d,
                        )
                    },
                )
                .style(|s| {
                    s.width_full()
                        .padding_horiz(modal_pad_h())
                        .padding_vert(theme::scaled(18.0))
                }),
            ))
            .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0));

            let status = dyn_container(
                move || d.account_draft.get().name.trim().is_empty(),
                move |empty_name| {
                    if empty_name {
                        text("A name is required.")
                            .style(|s| s.color(theme::error()).font_size(theme::font_label()))
                            .into_any()
                    } else {
                        crate::widgets::nothing().into_any()
                    }
                },
            );

            let preview_ui = ui.clone();
            let preview_target = target.clone();
            let ring_actions = root_ring.clone();
            let actions = dyn_container(
                move || d.account_draft.get(),
                move |draft| {
                    let ui = preview_ui.clone();
                    let target = preview_target.clone();
                    let ring = ring_actions.clone();
                    let ready = !draft.name.trim().is_empty();
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
                                let subject = draft.principal(target.dialect).display();
                                ddl_preview::preview_account(
                                    &ui,
                                    &target.database,
                                    &subject,
                                    account_change(&draft),
                                );
                            },
                        ),
                    ))
                    .style(|s| s.flex_row().items_center().gap(action_gap()))
                    .into_any()
                },
            );

            let close_x: Rc<dyn Fn()> = Rc::new(close);
            modal_shell(
                "Create account".to_string(),
                body.into_any(),
                status.into_any(),
                actions.into_any(),
                close_x,
                root_ring,
                panel_w(),
                ACCOUNT_PANEL_H,
            )
        },
    )
    .style(move |s| {
        if d.account.get().is_some() && d.preview.get().is_none() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

// ── the grant form ───────────────────────────────────────────────────────────

/// The two words the Action dropdown offers, over the `bool`
/// [`GrantDraft::revoke`] already is.
///
/// A `fn` rather than a closure because [`bound_dropdown`]'s `label` is one, and
/// spelled out rather than derived so the affirmative reads as the affirmative:
/// `false` is *Grant*.
fn action_label(revoke: bool) -> &'static str {
    if revoke { "Revoke" } else { "Grant" }
}

fn grant_form(
    ui: &Ui,
    target: &GrantTarget,
    seed: &GrantDraft,
    ring: FocusRing,
    d: crate::DdlUi,
) -> AnyView {
    let draft = d.grant_draft;
    let dialect = target.dialect;
    let mut rows: Vec<AnyView> = Vec::new();

    // **The direction is a `bool` on the draft, not an enum**, because that is
    // what `PrivilegeChange` and `ddl::grant_change` read — a two-value dropdown
    // over it is honest, and inventing an enum here would put a second spelling
    // of the same fact one conversion away from the tested one.
    rows.push(
        form_setting(
            "Action",
            bound_dropdown(
                draft,
                seed.revoke,
                vec![false, true],
                action_label,
                ring.clone(),
                6,
                |d, v| d.revoke = v,
            ),
        )
        .into_any(),
    );
    rows.push(
        form_setting(
            "Subject",
            bound_dropdown(
                draft,
                seed.subject,
                vec![GrantSubject::Privileges, GrantSubject::Role],
                GrantSubject::label,
                ring.clone(),
                8,
                |d, v| d.subject = v,
            ),
        )
        .into_any(),
    );

    match seed.subject {
        GrantSubject::Role => {
            // The browser's own account list behind the field, filtered to the
            // roles — a shortcut, not a constraint: a role made since the
            // browser opened can still be typed.
            let roles = ui.overlay.users_state;
            rows.push(
                form_setting(
                    "Role",
                    suggested_field(
                        ui,
                        draft,
                        seed.role.clone(),
                        "role_name",
                        move || match roles.get_untracked() {
                            crate::UsersState::Loaded(list) => list
                                .iter()
                                .filter(|p| p.kind == PrincipalKind::Role)
                                .map(|p| p.name.clone())
                                .collect(),
                            _ => Vec::new(),
                        },
                        &ring,
                        10,
                        |d, v| d.role = v.to_string(),
                    ),
                )
                .into_any(),
            );
            if !seed.revoke {
                rows.push(
                    form_setting(
                        "With admin option",
                        bound_toggle(draft, seed.with_admin_option, ring.clone(), 20, |d, v| {
                            d.with_admin_option = v
                        }),
                    )
                    .into_any(),
                );
            }
        }
        GrantSubject::Privileges => {
            let levels = users::levels_for(dialect);
            // **The draft's level, with no fallback of its own** — see
            // `initial_grant_draft`. `None` here means an engine with no levels,
            // which cannot reach this form, and the rest of the section is gated
            // on the same value below so the dropdown and the fields it governs
            // cannot disagree about what is picked.
            if let Some(current) = seed.level {
                rows.push(
                    form_setting(
                        "Level",
                        bound_dropdown(
                            draft,
                            current,
                            levels.to_vec(),
                            GrantLevelKind::label,
                            ring.clone(),
                            10,
                            |d, v| {
                                // Changing the level changes what may be granted
                                // at it, so the ticks go with it — keeping them
                                // would carry `EVENT` down to a table level that
                                // has no such privilege and emit a statement the
                                // server refuses.
                                d.level = Some(v);
                                d.privileges.clear();
                            },
                        ),
                    )
                    .into_any(),
                );
            }

            if let Some(kind) = seed.level {
                let (q_label, q_placeholder) = match (kind, dialect) {
                    (GrantLevelKind::Database, _) => ("Database", "database_name"),
                    (GrantLevelKind::Schema, _) => ("Schema", "schema_name"),
                    (_, SqlDialect::MySql) => ("Database", "database_name"),
                    _ => ("Schema", "schema_name"),
                };
                if kind != GrantLevelKind::Global {
                    // The database the browser is scoped to is the overwhelmingly
                    // likely answer, offered as a suggestion rather than filled
                    // in: a prefilled name in a form that grants privileges is a
                    // value nobody read.
                    let here = target.database.clone();
                    rows.push(
                        form_setting(
                            q_label,
                            suggested_field(
                                ui,
                                draft,
                                seed.qualifier.clone(),
                                q_placeholder,
                                move || vec![here.clone()],
                                &ring,
                                30,
                                |d, v| d.qualifier = v.to_string(),
                            ),
                        )
                        .into_any(),
                    );
                }
                if matches!(kind, GrantLevelKind::Table | GrantLevelKind::Sequence) {
                    rows.push(
                        form_setting(
                            if kind == GrantLevelKind::Table {
                                "Table"
                            } else {
                                "Sequence"
                            },
                            bound_field(
                                draft,
                                seed.name.clone(),
                                FieldCfg {
                                    placeholder: "name",
                                    focus: Some((ring.clone(), 40)),
                                    ..Default::default()
                                },
                                |d, v| d.name = v.to_string(),
                            ),
                        )
                        .into_any(),
                    );
                }

                rows.push(form_section("Privileges").into_any());
                // **Inline and wrapping, not one per line.** Eighteen is a legal
                // selection at MySQL's database level, and eighteen rows is a
                // column of short words taller than the panel — a set you have to
                // scroll to see the shape of. Wrapped, the whole set is one block
                // the eye takes in at once, which is the question the row is
                // actually asking: *which of these*.
                let all = users::privileges_for(dialect, kind);
                rows.push(
                    h_stack_from_iter(
                        all.iter().enumerate().map(|(i, &p)| {
                            privilege_tag(draft, p, all, ring.clone(), 50 + i as u32)
                        }),
                    )
                    .style(|s| {
                        s.flex_row()
                            .flex_wrap(floem::style::FlexWrap::Wrap)
                            .width_full()
                            .gap(theme::scaled(6.0))
                    })
                    .into_any(),
                );
                if !seed.revoke {
                    rows.push(
                        form_setting(
                            "With grant option",
                            bound_toggle(draft, seed.with_grant_option, ring, 90, |d, v| {
                                d.with_grant_option = v
                            }),
                        )
                        .into_any(),
                    );
                }
            }
        }
    }

    v_stack_from_iter(rows)
        .style(|s| s.flex_col().gap(form_gap()).width_full())
        .into_any()
}

/// One privilege, ticked or not. The style reads the draft, so clicking one tag
/// does not rebuild the cloud of eighteen.
fn privilege_tag(
    draft: RwSignal<GrantDraft>,
    privilege: &'static str,
    order: &'static [&'static str],
    ring: FocusRing,
    tabindex: u32,
) -> AnyView {
    action_button(
        privilege,
        ActionKind::Quiet,
        true,
        ring,
        tabindex,
        move || draft.update(|d| d.toggle(privilege, order)),
    )
    .style(move |s| {
        picked_outline(
            s,
            draft.with(|d| d.privileges.iter().any(|p| p == privilege)),
        )
    })
    .into_any()
}

pub(crate) fn grant_editor_overlay(ui: Ui) -> impl IntoView {
    let d = ui.ddl;
    let close = move || d.grant.set(None);

    dyn_container(
        move || (d.grant.get().is_some(), d.preview.get().is_some()),
        move |(open, previewing)| {
            if !open || previewing {
                return empty().into_any();
            }
            let Some(target) = d.grant.get_untracked() else {
                return empty().into_any();
            };
            let ring = FocusRing::new();
            let root_ring = ring.clone();
            let ui = ui.clone();

            // **Keyed on a memo over the form's shape, not on its contents.**
            // Which fields exist depends on the two dropdowns and the level; the
            // values in them do not — and a `dyn_container` does no equality
            // check of its own, so reading the draft here rebuilt the form on
            // every keystroke in a name field and on every privilege tag. See
            // the account form above, and `widgets::overlay_open_key`.
            let shape = floem::reactive::create_memo(move |_| d.grant_draft.with(grant_form_shape));
            let body_ui = ui.clone();
            let body_target = target.clone();
            let body = autohide(scroll(
                dyn_container(
                    move || shape.get(),
                    move |_| {
                        grant_form(
                            &body_ui,
                            &body_target,
                            &d.grant_draft.get_untracked(),
                            ring.clone(),
                            d,
                        )
                    },
                )
                .style(|s| {
                    s.width_full()
                        .padding_horiz(modal_pad_h())
                        .padding_vert(theme::scaled(18.0))
                }),
            ))
            .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0));

            let who = target.account.clone();
            let status_who = who.clone();
            let status = dyn_container(
                move || d.grant_draft.get(),
                move |draft| {
                    if draft.is_ready(&status_who) {
                        crate::widgets::nothing().into_any()
                    } else {
                        text(match draft.subject {
                            GrantSubject::Role => "Name a role.",
                            GrantSubject::Privileges => {
                                "Pick a level, name what it applies to, and tick a privilege."
                            }
                        })
                        .style(|s| {
                            s.color(theme::error())
                                .font_size(theme::font_label())
                                .max_width(theme::scaled(340.0))
                        })
                        .into_any()
                    }
                },
            );

            let preview_ui = ui.clone();
            let preview_target = target.clone();
            let ring_actions = root_ring.clone();
            let actions = dyn_container(
                move || d.grant_draft.get(),
                move |draft| {
                    let ui = preview_ui.clone();
                    let target = preview_target.clone();
                    let ring = ring_actions.clone();
                    let ready = draft.is_ready(&target.account);
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
                                // `None` is unreachable while `ready` gates the
                                // button — both read `is_ready` — and doing
                                // nothing is the right answer if that ever
                                // drifts, rather than a preview of an empty plan.
                                if let Some(change) = ddl::grant_change(&draft, &target.account) {
                                    ddl_preview::preview_account(
                                        &ui,
                                        &target.database,
                                        &target.account.display(),
                                        change,
                                    );
                                }
                            },
                        ),
                    ))
                    .style(|s| s.flex_row().items_center().gap(action_gap()))
                    .into_any()
                },
            );

            let close_x: Rc<dyn Fn()> = Rc::new(close);
            modal_shell(
                format!("Privileges — {}", who.display()),
                body.into_any(),
                status.into_any(),
                actions.into_any(),
                close_x,
                root_ring,
                grant_panel_w(),
                GRANT_PANEL_H,
            )
        },
    )
    .style(move |s| {
        if d.grant.get().is_some() && d.preview.get().is_none() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

// ── the shell both forms wear ────────────────────────────────────────────────

/// Title bar, body, footer, backdrop and the Escape handler — the parts both
/// forms have identically, written once so they cannot drift into two modals
/// that dismiss differently.
#[allow(clippy::too_many_arguments)]
fn modal_shell(
    title: String,
    body: AnyView,
    status: AnyView,
    actions: AnyView,
    close: Rc<dyn Fn()>,
    ring: FocusRing,
    width: f64,
    height: f64,
) -> AnyView {
    let panel = v_stack((
        modal_title_owned(title, close.clone(), ring.clone()),
        body,
        modal_footer_split(status.style(|s| s.min_width(0.0)), actions),
    ))
    .on_click_stop(|_| {})
    .style(move |s| panel_style(s).width(width).height(modal_h(height)));

    let esc = close.clone();
    let away = close.clone();
    focus_root_with_ring(stack((dismiss_layer(move || (away)()), panel)), ring)
        .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| (esc)())
        .style(|s| {
            s.size_full()
                .items_center()
                .justify_center()
                .background(theme::modal_backdrop())
        })
        .into_any()
}

/// **What decides a rebuild, asserted at the seam rather than on the memo.**
///
/// `overlay_open_key`'s own pin tested its memo in isolation and never a call
/// site, and the regression it introduced walked straight past it — so these
/// assert the *shape functions the keys are built from*, which is where the
/// decision actually lives and the half that drifts: a field added to a draft
/// and folded into the shape by reflex turns every keystroke in it into a
/// rebuild, and a field taken out of the shape stops the form reacting at all.
///
/// What they cannot see is the memo itself — whether the key closure reads
/// `shape.get()` or the draft. That is a line in a view, and it is stated here
/// so the coverage is not read as wider than it is.
#[cfg(test)]
mod form_shape_tests {
    use super::*;

    /// Every text field the account form has. Typing in one must not change what
    /// the form is made of.
    #[test]
    fn typing_never_changes_the_account_forms_shape() {
        let base = AccountDraft {
            kind: PrincipalKind::User,
            ..Default::default()
        };
        let shape = account_form_shape(&base);
        for typed in [
            AccountDraft {
                name: "app".into(),
                ..base.clone()
            },
            AccountDraft {
                host: "localhost".into(),
                ..base.clone()
            },
            AccountDraft {
                password: "hunter2".into(),
                ..base.clone()
            },
        ] {
            assert_eq!(account_form_shape(&typed), shape, "{typed:?}");
        }
    }

    /// And the one value that must: a role has neither of the two fields a user
    /// has.
    #[test]
    fn the_kind_is_what_changes_the_account_forms_shape() {
        let user = AccountDraft::default();
        let role = AccountDraft {
            kind: PrincipalKind::Role,
            ..Default::default()
        };
        assert_ne!(account_form_shape(&user), account_form_shape(&role));
    }

    #[test]
    fn typing_and_tagging_never_change_the_grant_forms_shape() {
        let base = GrantDraft {
            level: Some(GrantLevelKind::Table),
            ..Default::default()
        };
        let shape = grant_form_shape(&base);
        for edited in [
            GrantDraft {
                qualifier: "shop".into(),
                ..base.clone()
            },
            GrantDraft {
                name: "orders".into(),
                ..base.clone()
            },
            GrantDraft {
                role: "readers".into(),
                ..base.clone()
            },
            // A privilege tag: the cloud restyles itself, and rebuilding it
            // would take the focus off the tag that was just clicked.
            GrantDraft {
                privileges: vec!["SELECT".into()],
                ..base.clone()
            },
            // And the two switches, which are rows the shape already decided to
            // show.
            GrantDraft {
                with_grant_option: true,
                ..base.clone()
            },
            GrantDraft {
                with_admin_option: true,
                ..base.clone()
            },
        ] {
            assert_eq!(grant_form_shape(&edited), shape, "{edited:?}");
        }
    }

    /// **The coupling the form leans on**, pinned so it cannot be tidied away:
    /// the grant form shows its Level row exactly when the draft holds a level
    /// and offers no fallback of its own, so an opener that left `level` unset
    /// on an engine that *has* levels would hide the rest of the form with no
    /// way to unstick it — picking the level already displayed would not be a
    /// change.
    #[test]
    fn the_grant_form_opens_holding_a_level_wherever_the_engine_has_one() {
        for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            assert_eq!(
                initial_grant_draft(d).level.is_some(),
                !users::levels_for(d).is_empty(),
                "{d:?}"
            );
        }
    }

    /// And it is the *widest* level, which is the one a form should open on: the
    /// picker's first entry, so the box agrees with the menu behind it.
    #[test]
    fn the_level_it_opens_on_is_the_first_the_picker_offers() {
        for d in [SqlDialect::MySql, SqlDialect::Postgres] {
            assert_eq!(
                initial_grant_draft(d).level,
                users::levels_for(d).first().copied(),
                "{d:?}"
            );
        }
    }

    /// The three dropdowns, and only they. Each rebuilds the form because each
    /// changes which fields it has.
    #[test]
    fn each_dropdown_changes_the_grant_forms_shape() {
        let base = GrantDraft {
            level: Some(GrantLevelKind::Table),
            ..Default::default()
        };
        let shape = grant_form_shape(&base);
        for picked in [
            GrantDraft {
                subject: GrantSubject::Role,
                ..base.clone()
            },
            GrantDraft {
                revoke: true,
                ..base.clone()
            },
            GrantDraft {
                level: Some(GrantLevelKind::Global),
                ..base.clone()
            },
        ] {
            assert_ne!(grant_form_shape(&picked), shape, "{picked:?}");
        }
    }
}
