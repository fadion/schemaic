//! The two forms behind the Users and privileges browser's write half: making
//! an account, and changing what one may do.
//!
//! Both are raised from the browser and both end at
//! [`ddl_preview::preview_account`], so the review between a plan and the server
//! is the same one every other editor here reaches. Neither ever runs a
//! statement itself.
//!
//! **Two forms, because they are two questions.** The account form *creates* an
//! account, or **resets an existing one's password** — and those two are one
//! form rather than two because the password field is the whole of the second,
//! and `masked_edit_field` is `pub(crate)` precisely so there is one of it. The
//! grant form only ever changes privileges, and it never touches the account
//! itself.
//!
//! **A password reset is the only `ALTER` on an account this app emits.** An
//! account is still dropped from its own row in the browser, and neither engine
//! offers a rename that is safe to perform. The reset exists because it was the
//! one gap that could not be worked around: `CREATE USER … IDENTIFIED BY` with
//! the wrong password produced an account this app could then only drop, and the
//! repair meant reaching for another client.
//!
//! Which statement the form means is carried on [`AccountTarget::resetting`],
//! not inferred from whether some field is filled — `account_change` reads the
//! subject from there, so a reset cannot rename or re-host the account it is
//! resetting. That mapping is invisible in a rendered form, which is the kind
//! that ships backwards, so it has a test.
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

use crate::table_designer::suggest_chevron;
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, action_gap, autohide, dismiss_layer,
    focus_root_with_ring, form_gap, form_section, form_setting, modal_footer_split, modal_h,
    modal_pad_h, modal_title_owned, modal_w, panel_style,
};
use crate::{
    AccountTarget, ConnUi, DdlUi, FieldCfg, GrantTarget, OverlayUi, UsersTarget, ddl_preview,
    edit_field, theme,
};

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

/// Where the privilege tag cloud's Tab stops begin.
///
/// **A growing block inside the fixed band, which is the shape `VALUE_TAB`'s
/// doc names as unsafe** — so it is given a *reserved span* rather than left to
/// run into its neighbour. `With grant option` used to sit at a fixed `90`
/// fifteen lines below a block claiming `50 + i`: today's widest list is MySQL's
/// eighteen, so the ceiling is 67 and nothing collides, but
/// `users::privileges_for`'s own doc calls that list "curated, not exhaustive"
/// and names the dozens left out. Adding 23 of them would put a tag at exactly
/// 90, and `FocusRing::register` inserts *after* an equal index — so Tab would
/// visit the last tag and the toggle in build order, and `focus_at(90)` would
/// resolve to whichever happened to be first.
///
/// The block cannot be moved above the toggle (`VALUE_TAB`'s home) because the
/// toggle comes after it on screen, so the span is what makes the collision
/// impossible instead: `every_privilege_list_fits_its_tab_span` asserts every
/// dialect × level fits, and the compile-time chain below keeps the whole thing
/// inside the fixed band.
const PRIVILEGE_TAB: u32 = 50;

/// How many Tab stops [`PRIVILEGE_TAB`] reserves before the next fixed control.
///
/// Enough for every privilege either engine has ever documented, several times
/// over — the point is that the number is stated and checked rather than being
/// the distance to whatever was written next.
const PRIVILEGE_TAB_SPAN: u32 = 200;

const _: () = {
    assert!(
        PRIVILEGE_TAB + PRIVILEGE_TAB_SPAN < crate::widgets::FIXED_TAB_END,
        "the privilege block and the control after it must stay in the fixed band"
    );
};

fn field_w() -> f64 {
    theme::scaled(260.0)
}

// ── opening ──────────────────────────────────────────────────────────────────

/// Clear what a fresh form must not inherit, **then** seed this one's draft.
///
/// The order is the whole of it, and getting it backwards is not a subtle
/// failure: `close_peers` clears every editor's draft — including the one being
/// opened — so seeding first and clearing second left the grant form with
/// `GrantDraft::default()`, whose `level` is `None`. The form shows its Level row
/// only when the draft holds a level, so the entire body vanished: no level, no
/// name fields, no privilege cloud, and a footer reading "Pick a level, name what
/// it applies to, and tick a privilege" about controls that were not on screen.
///
/// One function rather than the sequence written out twice, so a third form
/// cannot get the order wrong — `a_freshly_opened_form_keeps_the_draft_it_was_
/// seeded_with` is what holds it.
fn reset_then_seed<T: 'static>(d: crate::DdlUi, draft: floem::reactive::RwSignal<T>, seed: T) {
    d.error.set(None);
    d.preview.set(None);
    ddl_preview::close_peers(d, false);
    draft.set(seed);
}

/// Is the connection this door is **for** read-only?
///
/// **Not `edit_ctx`'s, which is the switcher's.** The three doors below asked
/// `ctx.read_only` while the buttons that launch them were lit from the
/// *target's* flag (`users_view::write_gate` → `target_read_only`), and nothing
/// closes the Users browser when the schema tree switches connection. So: open
/// the browser on a writable connection A, switch the tree to a read-only B,
/// and **+ New account**, **Privileges** and **Reset password** stayed lit —
/// lit from A — while all three returned having set nothing, because `edit_ctx`
/// answered about B. A lit control that does nothing, with no modal, no message
/// and no dimming, on the three writes the browser offers.
///
/// The same row gets it right one button along: **Drop** asks
/// `launch_read_only(conn.connections, plan_conn_id)`, whose doc says it is
/// "about the connection the plan is for, which is the one the account lives
/// on". Three doors and one button, same click, two answers — this is the
/// button's answer, given to the doors.
///
/// `open_for_grant`'s note used to justify the divergence: *"`read_only` stays
/// live, and deliberately: it is the refusal, not the address, and `WriteGate`
/// reads it the same way."* The second clause was false — `WriteGate` reads
/// `target.conn_id` — so the premise the decision rested on was contradicted by
/// the function it cited.
fn door_read_only(conn: ConnUi, from: &UsersTarget) -> bool {
    crate::users_view::launch_read_only(conn.connections, from.conn_id)
}

/// Open the account form on a blank draft.
///
/// **The read-only refusal is here, not at the call site**, which is the rule
/// every editor in this crate follows: a launch guards itself in the same step
/// that launches it. The browser's button stays dimmed, because that is what
/// *says* the action is unavailable; this is what makes it so.
///
/// `database` is where the plan will run — see [`AccountTarget::database`]. The
/// caller has already asked `users::supports_user_admin`.
///
/// **`from` is the browser's own [`UsersTarget`], and it is where `conn_id` and
/// `dialect` come from** — not `edit_ctx`, which reads the switcher *now*. The
/// reasoning is written out once, over `open_for_grant`'s copy of the same four
/// lines, and `anchor_gate` is what holds both to it.
pub(crate) fn open_for_new(conn: ConnUi, d: DdlUi, from: &UsersTarget, database: &str) {
    // **No `edit_ctx` here at all.** These three read nothing else from it, and
    // what they did read was the switcher's flag — see `door_read_only`. A live
    // read of the active connection in a door launched from a captured target
    // is the fault, not just the answer it gave.
    let door_read_only = door_read_only(conn, from);
    if door_read_only {
        return;
    }
    // A new editing session — see `DdlUi::session`.
    d.session.update(|g| *g += 1);
    // **Blank, every time.** The draft carries a password, and a form that
    // reopened holding the last one would put a credential on screen that
    // nobody typed this time.
    reset_then_seed(d, d.account_draft, AccountDraft::default());
    d.account.set(Some(AccountTarget {
        // **`from`, not `ctx`** — see the note above `open_for_grant`'s twin
        // line. `read_only` is the one field that stays live.
        conn_id: from.conn_id,
        database: database.to_string(),
        dialect: from.dialect,
        read_only: door_read_only,
        resetting: None,
    }));
}

/// Open the account form to **reset an existing account's password**.
///
/// The same door, the same refusal, the same anchor as [`open_for_new`] — and
/// the same form, which is the point: the masked field is `pub(crate)` so there
/// is exactly one of it, and a second modal to type a password into would be a
/// second place for the replay rule to be got subtly wrong.
///
/// **Blank, every time**, for the reason `open_for_new` is: the draft carries a
/// password, and a form that reopened holding the last one would put a
/// credential on screen nobody typed this time. The draft's other fields are
/// seeded from `account` so the form can say whose password it is about, and
/// `account_change` reads the statement's subject from `resetting` rather than
/// from them — a reset must not be able to rename or re-host the account it is
/// resetting.
///
/// The caller has already asked `users::supports_password_reset`; that is what
/// dims the button, and this is what makes the refusal real.
pub(crate) fn open_for_reset(
    conn: ConnUi,
    d: DdlUi,
    from: &UsersTarget,
    database: &str,
    account: &Principal,
) {
    // **No `edit_ctx` here at all.** These three read nothing else from it, and
    // what they did read was the switcher's flag — see `door_read_only`. A live
    // read of the active connection in a door launched from a captured target
    // is the fault, not just the answer it gave.
    let door_read_only = door_read_only(conn, from);
    if door_read_only {
        return;
    }
    // A role has no password on either engine — nor has a MySQL 8 row the
    // catalogue cannot tell from one. The browser withholds the button, and this
    // is the same refusal one step in: `set_password_sql` would return `None`
    // and the plan would be empty, which reads as a broken button rather than a
    // refused one.
    if !schemaic_core::users::supports_password_reset(from.dialect, account) {
        return;
    }
    d.session.update(|g| *g += 1);
    reset_then_seed(
        d,
        d.account_draft,
        AccountDraft {
            name: account.name.clone(),
            host: account.host.clone().unwrap_or_default(),
            kind: account.kind,
            password: String::new(),
        },
    );
    d.account.set(Some(AccountTarget {
        conn_id: from.conn_id,
        database: database.to_string(),
        dialect: from.dialect,
        read_only: door_read_only,
        resetting: Some(account.clone()),
    }));
}

/// Open the grant form for one account. Same refusal at the same door, and the
/// same anchor — the account was fetched from `from`'s server, so the grant has
/// to run there.
pub(crate) fn open_for_grant(
    conn: ConnUi,
    d: DdlUi,
    from: &UsersTarget,
    database: &str,
    account: &Principal,
) {
    // **No `edit_ctx` here at all.** These three read nothing else from it, and
    // what they did read was the switcher's flag — see `door_read_only`. A live
    // read of the active connection in a door launched from a captured target
    // is the fault, not just the answer it gave.
    let door_read_only = door_read_only(conn, from);
    if door_read_only {
        return;
    }
    d.session.update(|g| *g += 1);
    reset_then_seed(d, d.grant_draft, initial_grant_draft(from.dialect));
    d.grant.set(Some(GrantTarget {
        // **The browser's captured target, not the live connection.**
        // `UsersTarget`'s own doc states the rule in the imperative — *"the
        // browser describes the server it was opened on, even if the switcher
        // has since moved"*, and *"Its two sibling targets (`AccountTarget`,
        // `GrantTarget`) carry one for the same reason"* — and both launchers
        // read `edit_ctx` instead, which resolves the switcher at the moment the
        // button is pressed. This target then paired a `Principal` fetched from
        // connection A's `mysql.user` with B's `conn_id` and dialect: the plan
        // was emitted at B's grammar and `GRANT … TO 'app'@'%'` ran on B, for an
        // account that lives on A.
        //
        // The module's third write action already spells it this way, eight
        // lines away — the Drop path captures `let plan_conn_id =
        // target.conn_id;` *"so the preview is built for the server this account
        // lives on, not for whichever the switcher points at by the time the
        // confirm is answered"*.
        //
        // **`read_only` stays live**, and deliberately: it is the refusal, not
        // the address, and `WriteGate` reads it the same way. A connection
        // marked read-only while the browser is open must stop the write it is
        // about to authorise — which is also why it is still the stamp
        // `read_only_door_gate` finds this file's doors by.
        conn_id: from.conn_id,
        database: database.to_string(),
        account: account.clone(),
        dialect: from.dialect,
        read_only: door_read_only,
    }));
}

/// The draft the grant form opens on.
///
/// **Pre-picked to the widest level below the whole server**, so the form opens
/// with its name fields already meaning something rather than with a picker the
/// user has to notice first — and so `level` is `None` *only* on an engine that
/// has no levels at all. The form relies on that: it shows the Level row exactly
/// when the draft holds a level, with no fallback to `levels_for(…).first()` of
/// its own, because a dropdown displaying a level the draft does not hold would
/// leave the rest of the form hidden and picking the entry already shown would
/// not be a change that unstuck it.
///
/// It reads [`users::default_grant_level`] rather than `levels_for(…).first()`,
/// which is what it used to. That rationale above is true on PostgreSQL and was
/// false on MySQL: `Global` heads MySQL's list and takes *no* name fields, so
/// the form opened already satisfied at the widest scope the engine has, and
/// two clicks — tick a privilege, press Preview SQL — emitted
/// `GRANT … ON *.*`.
///
/// Its own function so that coupling is one call and one test rather than a
/// literal in an opener and an assumption in a view.
pub(crate) fn initial_grant_draft(dialect: SqlDialect) -> GrantDraft {
    GrantDraft {
        level: users::default_grant_level(dialect),
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
    // `OverlayUi`, not `&Ui`: the chevron reads two signals off it and this
    // function reads none, so a whole-bundle parameter here was budget spent on
    // nothing. See `whole_ui_gate`.
    overlay: crate::OverlayUi,
    draft: RwSignal<D>,
    initial: String,
    placeholder: &'static str,
    options: impl Fn() -> Vec<String> + Clone + 'static,
    // What the chevron's menu says when `options` answers nothing — see
    // `suggest_chevron`.
    empty_note: &'static str,
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
        suggest_chevron(
            overlay,
            sig,
            options,
            empty_note,
            ring.clone(),
            tabindex + 1,
        ),
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
pub(crate) fn account_change(draft: &AccountDraft, resetting: Option<&Principal>) -> ddl::Change {
    // **The subject comes from `resetting`, never from the draft.** The form
    // seeds the draft's name and host so it can say whose password this is, and
    // reading them back here would let a reset rename or re-host the very
    // account it is resetting — silently, since `ALTER USER 'b'@'%'` on an
    // account that does not exist is an error the preview would blame on the
    // server.
    if let Some(account) = resetting {
        return ddl::Change::SetAccountPassword(Box::new(schemaic_core::users::PasswordReset {
            account: account.clone(),
            password: draft.password.clone(),
        }));
    }
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

    // **A reset is the password row and nothing else.** Kind, name and host all
    // describe an account that does not exist yet; on an account that does they
    // are not merely redundant but wrong to offer, because editing one would
    // read as changing it and `account_change` deliberately ignores them. So the
    // form states whose password this is, in a line that cannot be typed into,
    // and shows the one field that can.
    if let Some(account) = &target.resetting {
        rows.push(
            form_setting(
                "Account",
                text(account.display())
                    .style(|s| s.font_size(theme::font_body()).color(theme::text()))
                    .into_any(),
            )
            .into_any(),
        );
        rows.push(password_row(d, ring));
        return v_stack_from_iter(rows)
            .style(|s| s.width_full().flex_col().gap(theme::scaled(10.0)))
            .into_any();
    }

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
        rows.push(password_row(d, ring));
    }

    v_stack_from_iter(rows)
        .style(|s| s.flex_col().gap(form_gap()).width_full())
        .into_any()
}

/// The masked password field and the sentence under it, shared by the form's two
/// modes.
///
/// **Masked, like the app's three other secret fields.** This was the one that
/// was not: the real characters were in the editor's own document, so they were
/// on screen and a select-all away from the clipboard. `masked_edit_field` keeps
/// only `*`s in the document and replays each edit onto the value from the
/// editor's own delta, which is why there is one of it rather than a second copy
/// here — and why this row is one function rather than one per mode.
///
/// **This form is why the replay has to be exact rather than close**: the
/// connection form's mangled password fails to connect and can be retyped, while
/// this one reaches `CREATE USER … IDENTIFIED BY` or `ALTER USER … IDENTIFIED
/// BY`, and a mangled reset locks the account out of whatever was using it.
///
/// It seeds from the draft *untracked*: the seed is the value this row starts
/// from, and reading it tracked would rebuild the field on every keystroke it
/// itself caused.
fn password_row(d: crate::DdlUi, ring: FocusRing) -> AnyView {
    let draft = d.account_draft;
    let pw = floem::reactive::create_rw_signal(draft.with_untracked(|a| a.password.clone()));
    create_effect(move |prev: Option<String>| {
        let v = pw.get();
        if prev.is_some_and(|p| p != v) {
            draft.update(|d| d.password = v.clone());
        }
        v
    });
    v_stack((
        form_setting(
            "Password",
            crate::connection_form::masked_edit_field(pw, ring, 30)
                .style(|s| s.width(field_w()))
                .into_any(),
        ),
        // The one field in the app whose value reaches a screenshot, so it says
        // so where it is typed rather than only in the module comment.
        text("The password appears in the previewed SQL, which is the statement that runs.").style(
            |s| {
                s.font_size(theme::font_hint())
                    .color(theme::text_faint())
                    .width_full()
            },
        ),
    ))
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

/// **Takes `DdlUi` alone.** Nothing under this modal is an opening path — the
/// three doors above are — so the draft, the target and the preview hand-off are
/// the whole of what it touches.
pub(crate) fn account_editor_overlay(d: DdlUi) -> impl IntoView {
    // **The draft goes with the form.** `account_draft` is app-lifetime, so
    // clearing only the target left the plaintext password in a signal for the
    // rest of the process — after Cancel as much as after Apply. The form
    // re-seeds itself from its target on open, so nothing is lost. Same rule and
    // same reason as `ddl_preview::close_peers`, which is the other door.
    let close = move || {
        d.account.set(None);
        d.account_draft.set(Default::default());
    };

    dyn_container(
        // The preview stacks on top and this stays open behind it (Cancel there
        // returns here with the draft intact), but must render nothing — the
        // pairing every other editor uses.
        move || {
            (
                d.account.with(Option::is_some),
                d.preview.with(Option::is_some),
            )
        },
        move |(open, previewing)| {
            if !open || previewing {
                return empty().into_any();
            }
            let Some(target) = d.account.get_untracked() else {
                return empty().into_any();
            };
            let ring = FocusRing::new();
            let root_ring = ring.clone();

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

            // **The two modes require different fields, so they say different
            // things.** A create needs a name; a reset has one already and needs
            // the password, because `set_password_sql` refuses a blank one —
            // `ALTER USER … IDENTIFIED BY ''` sets a *blank* password rather
            // than leaving one unset. Without this arm the button would be live
            // over an empty field and the plan would come back with nothing in
            // it, which reads as the app being broken.
            let resetting = target.resetting.is_some();
            let status = dyn_container(
                // `with`, not `get`: this re-runs on every edit of the draft and
                // asks one question about one field, so cloning the whole
                // `AccountDraft` to reach it is the defect `consts`'
                // `get_clone_gate` is named for.
                move || {
                    d.account_draft.with(|a| {
                        if resetting {
                            a.password.is_empty()
                        } else {
                            a.name.trim().is_empty()
                        }
                    })
                },
                move |missing| {
                    if missing {
                        text(if resetting {
                            "A password is required."
                        } else {
                            "A name is required."
                        })
                        .style(|s| s.color(theme::error()).font_size(theme::font_label()))
                        .into_any()
                    } else {
                        crate::widgets::nothing().into_any()
                    }
                },
            );

            let preview_target = target.clone();
            let ring_actions = root_ring.clone();
            let actions = dyn_container(
                move || d.account_draft.get(),
                move |draft| {
                    let target = preview_target.clone();
                    let ring = ring_actions.clone();
                    let ready = match &target.resetting {
                        Some(_) => !draft.password.is_empty(),
                        None => !draft.name.trim().is_empty(),
                    };
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
                                    d,
                                    (&target).into(),
                                    &subject,
                                    account_change(&draft, target.resetting.as_ref()),
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
                // No ellipsis, per the house rule — and the two modes are named
                // apart because "Create account" over a form that will emit an
                // `ALTER` is the one thing the title may not do.
                match target.resetting.is_some() {
                    true => "Reset password".to_string(),
                    false => "Create account".to_string(),
                },
                ShellParts {
                    body: body.into_any(),
                    status: status.into_any(),
                    actions: actions.into_any(),
                },
                close_x,
                root_ring,
                ShellSize {
                    width: panel_w(),
                    height: ACCOUNT_PANEL_H,
                },
            )
        },
    )
    .style(move |s| {
        if d.account.with(Option::is_some) && d.preview.with(Option::is_none) {
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
    overlay: OverlayUi,
    target: &GrantTarget,
    seed: &GrantDraft,
    ring: FocusRing,
    d: DdlUi,
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
            let roles = overlay.users_state;
            rows.push(
                form_setting(
                    "Role",
                    suggested_field(
                        overlay,
                        draft,
                        seed.role.clone(),
                        "role_name",
                        move || match roles.get_untracked() {
                            crate::UsersState::Loaded(list) => list
                                .list
                                .iter()
                                .filter(|p| p.kind == PrincipalKind::Role)
                                .map(|p| p.name.clone())
                                .collect(),
                            _ => Vec::new(),
                        },
                        // Reachable, and the reason this note exists: MySQL,
                        // MariaDB and PostgreSQL all have roles and a great many
                        // servers have none, so the shortcut has nothing to
                        // shortcut — and a chevron that answered with silence
                        // read as broken.
                        "This server has no roles",
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
                                overlay,
                                draft,
                                seed.qualifier.clone(),
                                q_placeholder,
                                move || vec![here.clone()],
                                "No suggestions",
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
                    h_stack_from_iter(all.iter().enumerate().map(|(i, &p)| {
                        privilege_tag(draft, p, all, ring.clone(), PRIVILEGE_TAB + i as u32)
                    }))
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
                            bound_toggle(
                                draft,
                                seed.with_grant_option,
                                ring,
                                PRIVILEGE_TAB + PRIVILEGE_TAB_SPAN,
                                |d, v| d.with_grant_option = v,
                            ),
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

/// **Takes `DdlUi` and `OverlayUi`**, the pair [`grant_form`] needs: the draft
/// and target on one, the Role and qualifier fields' suggestion dropdown on the
/// other. Nothing under it is an opening path.
pub(crate) fn grant_editor_overlay(d: DdlUi, overlay: OverlayUi) -> impl IntoView {
    // The grant draft carries no secret, but it takes the same rule for the same
    // reason its sibling above does: one closing behaviour, so the pair cannot
    // drift into two.
    let close = move || {
        d.grant.set(None);
        d.grant_draft.set(Default::default());
    };

    dyn_container(
        move || {
            (
                d.grant.with(Option::is_some),
                d.preview.with(Option::is_some),
            )
        },
        move |(open, previewing)| {
            if !open || previewing {
                return empty().into_any();
            }
            let Some(target) = d.grant.get_untracked() else {
                return empty().into_any();
            };
            let ring = FocusRing::new();
            let root_ring = ring.clone();

            // **Keyed on a memo over the form's shape, not on its contents.**
            // Which fields exist depends on the two dropdowns and the level; the
            // values in them do not — and a `dyn_container` does no equality
            // check of its own, so reading the draft here rebuilt the form on
            // every keystroke in a name field and on every privilege tag. See
            // the account form above, and `widgets::overlay_open_key`.
            let shape = floem::reactive::create_memo(move |_| d.grant_draft.with(grant_form_shape));
            let body_target = target.clone();
            let body = autohide(scroll(
                dyn_container(
                    move || shape.get(),
                    move |_| {
                        grant_form(
                            overlay,
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

            let preview_target = target.clone();
            let ring_actions = root_ring.clone();
            let actions = dyn_container(
                move || d.grant_draft.get(),
                move |draft| {
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
                                        d,
                                        (&target).into(),
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
                ShellParts {
                    body: body.into_any(),
                    status: status.into_any(),
                    actions: actions.into_any(),
                },
                close_x,
                root_ring,
                ShellSize {
                    width: grant_panel_w(),
                    height: GRANT_PANEL_H,
                },
            )
        },
    )
    .style(move |s| {
        if d.grant.with(Option::is_some) && d.preview.with(Option::is_none) {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

// ── the shell both forms wear ────────────────────────────────────────────────

/// The three views [`modal_shell`] lays out.
///
/// **Named fields, because they are all `AnyView`.** Three same-typed
/// positionals in a row transpose silently — a status line where the actions go
/// compiles and renders a footer with the buttons on the wrong side — and this
/// is the shell both forms wear, so the mistake would be invisible in one of
/// them until someone opened it.
struct ShellParts {
    body: AnyView,
    status: AnyView,
    actions: AnyView,
}

/// The panel's size. Same reason: two `f64`s in a row.
struct ShellSize {
    width: f64,
    height: f64,
}

/// Title bar, body, footer, backdrop and the Escape handler — the parts both
/// forms have identically, written once so they cannot drift into two modals
/// that dismiss differently.
fn modal_shell(
    title: String,
    parts: ShellParts,
    close: Rc<dyn Fn()>,
    ring: FocusRing,
    size: ShellSize,
) -> AnyView {
    let ShellParts {
        body,
        status,
        actions,
    } = parts;
    let ShellSize { width, height } = size;
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

    /// **And it is never the whole server.** The composition, not the capability
    /// on its own: `users::default_grant_level` answering `Database` is worth
    /// nothing if the opener still reads `levels_for(…).first()`, which is what
    /// it did — and on MySQL that first entry is `Global`, a level taking no
    /// name fields, so the form's shortest path was `GRANT … ON *.*` in two
    /// clicks.
    #[test]
    fn the_level_it_opens_on_is_never_the_whole_server() {
        for d in [SqlDialect::MySql, SqlDialect::Postgres] {
            assert_eq!(
                initial_grant_draft(d).level,
                users::default_grant_level(d),
                "{d:?}"
            );
            assert_ne!(
                initial_grant_draft(d).level,
                Some(users::GrantLevelKind::Global),
                "{d:?} opens the grant form at the widest scope it has"
            );
        }
        // The one that regressed: MySQL lists Global first, and the opener used
        // to take it.
        assert_eq!(
            initial_grant_draft(SqlDialect::MySql).level,
            Some(users::GrantLevelKind::Database)
        );
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

    /// **The reserved span is only worth having if something checks it.** Every
    /// list either engine offers, at every level it offers one, has to fit
    /// between `PRIVILEGE_TAB` and the fixed control after it — otherwise a tag
    /// and that control share an index, `FocusRing::register` orders them by
    /// build order, and `focus_at` resolves to whichever is first.
    #[test]
    fn every_privilege_list_fits_its_tab_span() {
        use schemaic_core::users::{levels_for, privileges_for};
        for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            for kind in levels_for(d) {
                let n = privileges_for(d, *kind).len() as u32;
                assert!(
                    n < PRIVILEGE_TAB_SPAN,
                    "{d:?}/{kind:?} has {n} privileges, past the {PRIVILEGE_TAB_SPAN} reserved"
                );
            }
        }
    }

    /// **The regression this ordering exists for.** `close_peers` clears every
    /// editor's draft, and both openers seeded theirs *before* calling it — so
    /// the grant form opened on `GrantDraft::default()`, whose `level` is
    /// `None`, and the form renders its Level row (and everything below it: the
    /// name fields and the whole privilege cloud) only when the draft holds one.
    /// The modal came up with an Action, a Subject and nothing else, under a
    /// footer asking for a level that was not on screen.
    #[test]
    fn a_freshly_opened_form_keeps_the_draft_it_was_seeded_with() {
        use floem::reactive::Scope;
        let scope = Scope::new();
        let d = crate::ddl_preview::test_ddl_ui(scope);

        // A previous session's leftovers, which the reset is there to remove.
        d.grant_draft.set(GrantDraft {
            role: "stale".into(),
            ..Default::default()
        });
        d.account_draft.set(AccountDraft {
            name: "stale".into(),
            password: "hunter2".into(),
            ..Default::default()
        });

        for dialect in [SqlDialect::MySql, SqlDialect::Postgres] {
            let seed = initial_grant_draft(dialect);
            assert!(seed.level.is_some(), "{dialect:?}: the seed has a level");
            reset_then_seed(d, d.grant_draft, seed.clone());
            assert_eq!(
                d.grant_draft.get_untracked(),
                seed,
                "{dialect:?}: the reset wiped the seed"
            );
        }

        // And the account form's blank seed is the blank draft, so the *only*
        // way to tell the two orderings apart there is that the password from
        // the previous session must be gone either way.
        reset_then_seed(d, d.account_draft, AccountDraft::default());
        assert_eq!(d.account_draft.get_untracked(), AccountDraft::default());

        scope.dispose();
    }
}

/// **A form's *address* comes from the browser that raised it, not from the
/// connection switcher.**
///
/// The two launchers above stamped `conn_id` and `dialect` off `edit_ctx`, which
/// resolves `ui.conn.active_conn` at the moment the button is pressed, while the
/// `UsersTarget` beside them carries both captured at open — and its own doc
/// states the rule in the imperative: *"the browser describes the server it was
/// opened on, even if the switcher has since moved"*, *"Its two sibling targets
/// ([`AccountTarget`], [`GrantTarget`]) carry one for the same reason"*. A
/// `GrantTarget` therefore paired a `Principal` read out of connection A's
/// `mysql.user` with B's connection and B's grammar, and Apply ran the grant on
/// B for an account that lives on A. The module's third write action, Drop, was
/// already correct and says so eight lines away.
///
/// **A source gate, because there is nothing else to point a test at.** The
/// decision is two struct literals inside `fn`s that take the whole `Ui`, and
/// building one in a test means 36 fields and 91 more transitively (A2-L7-01).
/// What *can* be asserted mechanically is the spelling — that `edit_ctx`'s
/// answer never becomes a target's address in this file — and that is exactly
/// the regression, since `edit_ctx` is the only way to reach the switcher from
/// here. Scoped to this file on purpose: every other editor is launched from the
/// schema tree, where the active connection *is* the target, so
/// `read_only: ctx.read_only` beside `conn_id: ctx.conn_id` is right there.
#[cfg(test)]
mod account_change_tests {
    use super::*;

    fn an_account() -> Principal {
        Principal {
            name: "app".into(),
            host: Some("10.0.0.%".into()),
            kind: PrincipalKind::User,
            system: false,
            attributes: Vec::new(),
            role_ambiguous: false,
        }
    }

    /// No `resetting` means the form is creating, and the draft is the subject.
    #[test]
    fn without_a_reset_target_the_form_creates() {
        let d = AccountDraft {
            name: "  app  ".into(),
            host: "  %  ".into(),
            kind: PrincipalKind::User,
            password: "hunter2".into(),
        };
        match account_change(&d, None) {
            ddl::Change::CreateAccount(a) => {
                // Trimmed on the way out, which is the other thing this function
                // does and the reason it is not a bare constructor call.
                assert_eq!(a.name, "app");
                assert_eq!(a.host, "%");
                assert_eq!(a.password, "hunter2");
            }
            other => panic!("{other:?}"),
        }
    }

    /// **The subject comes from the target, and the draft cannot move it.**
    ///
    /// The draft is seeded from the account so the form can say whose password
    /// it is about, which means its `name` and `host` are live fields sitting
    /// next to a statement that must not read them. This hands it a draft naming
    /// a *different* account — what a form that let those fields be edited would
    /// produce — and asserts the statement still names the one being reset.
    /// Reading them back would emit `ALTER USER 'somebody_else'@'%'`, which on an
    /// account that does not exist is an error the preview would blame on the
    /// server.
    #[test]
    fn a_reset_names_the_target_rather_than_the_draft() {
        let d = AccountDraft {
            name: "somebody_else".into(),
            host: "%".into(),
            kind: PrincipalKind::User,
            password: "hunter2".into(),
        };
        match account_change(&d, Some(&an_account())) {
            ddl::Change::SetAccountPassword(r) => {
                assert_eq!(r.account, an_account());
                assert_eq!(r.password, "hunter2");
            }
            other => panic!("{other:?}"),
        }
    }

    /// The composition, not the two halves: the change this form builds is the
    /// one the emitter turns into the statement the preview shows.
    #[test]
    fn the_reset_the_form_builds_emits_the_alter_for_that_account() {
        let d = AccountDraft {
            name: "ignored".into(),
            host: "ignored".into(),
            kind: PrincipalKind::User,
            password: "hunter2".into(),
        };
        let change = account_change(&d, Some(&an_account()));
        let cs = ddl::account("app", SqlDialect::MySql, change);
        assert_eq!(
            cs.emit(),
            ["ALTER USER 'app'@'10.0.0.%' IDENTIFIED BY 'hunter2';"]
        );
        // And nothing that leaves the preview carries it.
        let (clean, redacted) = cs.without_secrets();
        assert!(redacted);
        assert!(!clean.emit().iter().any(|s| s.contains("hunter2")));
    }
}

#[cfg(test)]
mod anchor_gate {
    /// What must not appear: the switcher, used for **anything** these doors
    /// decide.
    ///
    /// `ctx.read_only` joined the list, and that is the whole of `R1-L2-01`.
    /// This gate's own message used to end *"`read_only` is the one field that
    /// stays live — it is the refusal, not the address"*, which read as a
    /// distinction and was a hole: the browser's buttons are lit from the
    /// *target's* flag and nothing closes the browser when the tree switches
    /// connection, so a live refusal answered about a different connection than
    /// the one that lit the button. Address or refusal, a door launched from a
    /// captured target asks that target.
    const FORBIDDEN: &[&str] = &["ctx.conn_id", "ctx.dialect", "ctx.read_only"];
    /// What must appear instead, once per door.
    const REQUIRED: &[&str] = &["conn_id: from.conn_id,", "dialect: from.dialect,"];
    /// **The doors this file has**, as a number the gates below count against.
    /// A fourth `open_for_*` raises it here and nowhere else — and raising it
    /// without writing the two lines the gates look for is what they exist to
    /// refuse. It went from two to three when `open_for_reset` landed, and both
    /// gates caught it.
    const DOORS: usize = 3;

    #[test]
    fn no_account_form_takes_its_address_from_the_switcher() {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("account_editor.rs"),
        )
        .expect("account_editor.rs");
        // Comments quote both spellings — including this module's own prose, if
        // it ever moves out of a `#[cfg(test)]` item — so scan production code.
        let body = crate::source_gate::production_code(&src);

        // The floor, which is the failure mode a source gate is most prone to:
        // a rename would leave nothing to look for and pass silently.
        assert!(
            // The floor moved with the fix: these doors called `edit_ctx` for
            // `read_only` alone, and that was the defect, so they call it for
            // nothing now. What must still be *here* is the question asked in
            // its place.
            body.contains("door_read_only("),
            "the launchers no longer ask `door_read_only` — rewrite this gate \
             rather than deleting it: the refusal still has to be about the \
             connection the browser is showing, not the one the tree is on"
        );
        for want in REQUIRED {
            assert_eq!(
                body.matches(want).count(),
                DOORS,
                "`{want}` should appear once in each of the {DOORS} doors — did \
                 one of them stop reading the browser's captured target?"
            );
        }
        for bad in FORBIDDEN {
            assert!(
                !body.contains(bad),
                "`{bad}` is back in account_editor.rs: a form's address must come \
                 from the browser's captured `UsersTarget`, not from whichever \
                 connection the switcher points at when the button is pressed. \
                 That is true of the refusal as well as the address: see \
                 `door_read_only`."
            );
        }
    }

    /// And the refusal is untouched by all of it: every door still guards
    /// itself in the step that launches, which is `read_only_door_gate`'s
    /// rule for all fifteen. Asserted here too because this fix moved the two
    /// lines that gate finds them by.
    #[test]
    fn every_door_still_refuses_a_read_only_connection_first() {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("account_editor.rs"),
        )
        .expect("account_editor.rs");
        let body = crate::source_gate::production_code(&src);
        // The pair `read_only_door_gate` finds a door by, in this file's
        // spelling — see its `SUBJECTS`. Both terms are the *target's* answer
        // now, so a door cannot refuse on one connection and stamp another's
        // flag into the modal it opens.
        assert_eq!(body.matches("if door_read_only {").count(), DOORS);
        assert_eq!(body.matches("read_only: door_read_only,").count(), DOORS);
        // And the switcher's spelling is gone from both positions, which is
        // what `FORBIDDEN` asserts from the other side.
        assert_eq!(body.matches("if ctx.read_only {").count(), 0);
        assert_eq!(body.matches("read_only: ctx.read_only,").count(), 0);
    }
}
