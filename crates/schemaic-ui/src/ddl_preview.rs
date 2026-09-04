//! The DDL preview: what is about to change, what it costs, and the exact SQL.
//!
//! **Nothing in Schemaic runs generated DDL without going through here.** The
//! designer, Create table, and every context-menu shortcut all end at this
//! modal, so there's one place that shows the statements and one place that says
//! what they destroy. The escape hatch is deliberate too — "Open in editor"
//! drops the script into a query tab, because a generated `ALTER` the user can't
//! read and adjust is exactly the thing that makes people distrust a schema
//! editor.
//!
//! Applying is the app's job (`SchemaActions::run_ddl`, off the UI thread); this
//! module owns the panel and the outcome states.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;

use schemaic_core::intel::SqlDialect;
use schemaic_core::text::plural;

use crate::widgets::{
    ACTION_TAB, ActionKind, ExitAction, FocusRing, action_button, action_button_icon, action_gap,
    autohide, exit_action, focus_root_with_ring, form_section, form_section_owned,
    modal_footer_split, modal_h, modal_pad_h, modal_title_owned, modal_w, panel_style,
};
use crate::{DdlOutcome, DdlPreview, DdlRunRequest, FieldCfg, Ui, edit_field, icons, theme};

fn panel_w() -> f64 {
    modal_w(660.0)
}
const PANEL_H: f64 = 560.0;
/// The SQL box's height before it scrolls. Deep enough that a typical `ALTER`
/// with a handful of clauses is visible whole.
const SQL_ROWS: usize = 16;

/// Close **every** editor behind the preview.
///
/// One function because there are five of them and the two sites that had to
/// clear them cleared two. After a successful Apply, pressing Close remounted
/// the trigger, function or object editor from the untouched pre-apply draft,
/// with the same change count — and Preview → Apply then re-ran a plan the
/// server had already applied (a second `CREATE TRIGGER`, a rename whose source
/// is gone). On the "Open in editor" path it was worse: the script landed in a
/// query tab and the modal immediately painted over the tab the user had just
/// been sent to.
///
/// A sixth editor added later is a one-line change here rather than a bug in two
/// places, which is what `tests::close_editors_clears_every_editor`
/// is guarding.
///
/// **One exception: a function plan doesn't close the trigger editor it was
/// opened from.** The function modal exists to serve the trigger one — a
/// PostgreSQL trigger has no body, only a function to call — so `open_function`
/// deliberately leaves the trigger target set and the trigger overlay renders
/// nothing while the function modal is up. Clearing both meant the documented
/// middle step (fill in a trigger, press **New function**, Apply) silently
/// destroyed the half-written trigger, and "Open in editor" did the same without
/// applying anything at all.
///
/// The routine editor being open *is* the signal that the plan came from it: it
/// renders over the trigger modal, so nothing else can reach Preview while it is.
pub(crate) fn close_editors(d: crate::DdlUi) {
    close_peers(d, d.routine.get_untracked().is_some());
}

/// Clear every editor target — **the one list**, called both by
/// [`close_editors`] above and by each editor's own `open`, which clears its
/// peers before setting itself.
///
/// Those `open`s used to keep their own hand-written lists, and they had
/// **drifted**: the table designer cleared only the view editor and the view
/// editor only the designer, while `object_editor`, `routine_editor` and
/// `trigger_editor` cleared four apiece. Each of those `set(None)` lines carries
/// the same comment — "each overlay knows only its own flag, so two open would
/// paint two panels" — and a partial list is exactly that bug with the comment
/// still attached. Adding the event editor made it six flags maintained by hand
/// in five places, which is the point at which one list is the only version that
/// stays true.
///
/// `keep_trigger` is the single exception, and it belongs to the caller rather
/// than to this list: see [`close_editors`] for why a function plan leaves the
/// trigger form it was opened from standing.
pub(crate) fn close_peers(d: crate::DdlUi, keep_trigger: bool) {
    d.designer.set(None);
    d.view.set(None);
    if !keep_trigger {
        d.trigger.set(None);
    }
    d.routine.set(None);
    d.object.set(None);
    d.event.set(None);
    d.database.set(None);
    d.account.set(None);
    d.grant.set(None);
    // **The drafts go with the targets.** `account_draft` is app-lifetime, so
    // clearing only `d.account` left the plaintext password sitting in a signal
    // for the rest of the process — after Cancel and after Apply alike, and
    // reachable by anything that reads the bundle. The form re-seeds itself from
    // its target on open, so there is nothing to keep.
    d.account_draft.set(Default::default());
    d.grant_draft.set(Default::default());
}

/// Close the preview and drop the script with it.
///
/// The one door, because there are two `set(None)` sites and a third would
/// otherwise have to remember: `d.sql` is app-lifetime and held the last plan's
/// script for the life of the process. It is [`ChangeSet::export_script`]'s
/// output rather than the real statement, so this is defence in depth rather
/// than the only line — which is the reason it is a one-line helper and not a
/// larger piece of machinery.
///
/// [`ChangeSet::export_script`]: schemaic_core::ddl::ChangeSet::export_script
pub(crate) fn close_preview(d: crate::DdlUi) {
    d.preview.set(None);
    d.sql.set(String::new());
}

/// Open the preview on a change set. `from_designer` decides where Cancel goes.
pub(crate) fn open_preview(ui: &Ui, preview: DdlPreview) {
    let d = ui.ddl;
    // The script, so what the box shows is what Copy and "Open in editor" hand
    // over. Apply still sends `statements` on the wire, where `DELIMITER` — a
    // client directive the server has never heard of — must not appear.
    d.sql.set(preview.script.clone());
    d.sql_rows.set(SQL_ROWS);
    d.error.set(None);
    d.applied.set(false);
    d.applying.set(false);
    d.generation.update(|g| *g += 1);
    d.preview.set(Some(preview));
}

/// Build a preview from a change set — the one conversion every caller uses, so
/// the summaries, the warnings and the SQL can't come from different places.
pub(crate) fn preview_of(
    conn_id: u64,
    database: &str,
    subject: impl Into<String>,
    cs: &schemaic_core::ddl::ChangeSet,
    read_only: bool,
) -> DdlPreview {
    DdlPreview {
        conn_id,
        database: database.to_string(),
        // **Read off the changes, not off the caller.** Every path into this
        // function would otherwise have to remember to say, and the one that
        // forgot would send a `DROP DATABASE` down the in-database route — where
        // PostgreSQL refuses it and MySQL runs it on a connection pointed at the
        // database it just removed.
        scope: if cs.changes.iter().any(schemaic_core::ddl::is_server_level) {
            crate::DdlScope::Server
        } else {
            crate::DdlScope::Database
        },
        subject: subject.into(),
        // Off the change set, like `scope` above — but a *different* question:
        // see `DdlPreview::qualified`. An account is server-wide and takes the
        // in-database runner, so the two answers differ for exactly this case.
        qualified: !cs.changes.iter().any(schemaic_core::ddl::is_server_level)
            && !cs.changes.iter().all(schemaic_core::ddl::is_account_change),
        changes: cs.changes.iter().map(|c| c.summary()).collect(),
        destructive: cs.destructive(),
        risk_heading: cs.risk_heading(),
        withheld: cs.unsupported(),
        statements: cs.emit(),
        // **`export_script`, not `editor_script`.** This field is what Copy and
        // Open in editor hand over, and both put it somewhere durable — the
        // clipboard, and a query tab the session file writes to `tabs.json` in
        // the clear. A `CREATE USER … IDENTIFIED BY 'hunter2'` has no business
        // in either. The *preview* renders `statements`, which is unchanged and
        // is the statement that runs. Read off the change set, like `scope`
        // below, so a third exit from this modal inherits the rule instead of
        // having to remember it.
        script: cs.export_script(),
        read_only,
        // Off the change set, like `scope` above and for the same reason: a
        // caller that had to remember to say is a caller that will one day
        // forget.
        dialect: cs.dialect,
    }
}

/// Build a preview from a whole **multi-object plan** — [`preview_of`]'s
/// counterpart for a schema comparison, which is about many objects at once.
///
/// [`DdlPreview`] needed nothing added for this: every field it holds was
/// already flat text, and [`SchemaPlan`] answers each one as the concatenation
/// of its sets' answers. So the plan reaches the same modal, the same Apply and
/// the same `Db::run_ddl` as one table's designer edit, which is the point — a
/// second apply path is the thing the DDL invariant exists to prevent.
///
/// **`scope` and `qualified` are read off every set's changes**, not the first
/// one's and not the caller's, for the reason [`preview_of`] gives: the path
/// that had to remember is the path that forgets. A comparison produces neither
/// a server-level nor an account change today — it compares objects *within* a
/// database — but asking the plan rather than assuming that is what keeps the
/// answer right if it ever grows one.
///
/// `subject` names the plan rather than an object, since there is no single
/// object to name; the caller passes something like "12 objects".
///
/// [`SchemaPlan`]: schemaic_core::compare::SchemaPlan
pub(crate) fn preview_of_plan(
    conn_id: u64,
    database: &str,
    subject: impl Into<String>,
    plan: &schemaic_core::compare::SchemaPlan,
    read_only: bool,
) -> DdlPreview {
    let server_level = plan
        .sets
        .iter()
        .flat_map(|s| s.changes.iter())
        .any(schemaic_core::ddl::is_server_level);
    DdlPreview {
        conn_id,
        database: database.to_string(),
        scope: if server_level {
            crate::DdlScope::Server
        } else {
            crate::DdlScope::Database
        },
        subject: subject.into(),
        // **Never qualified.** `qualified` asks "does `subject` live *in*
        // `database`, so the title may write `database.subject`?" — and a
        // plan's subject is a count, not an object name, so the answer is no
        // whatever the changes are. Reading it off the changes the way
        // `preview_of` does produced `Apply changes to My MariaDB · shop.12
        // objects`: true of every field it was derived from, and nonsense.
        // Each *statement* is still qualified, and `summaries` names the object
        // per line.
        qualified: false,
        changes: plan.summaries(),
        destructive: plan.destructive(),
        risk_heading: plan.risk_heading(),
        // The same refusal a single set gets: non-empty disables Apply and is
        // re-checked inside `apply`, so a plan that can't be expressed in full
        // is not applied in part.
        withheld: plan.unsupported(),
        statements: plan.emit(),
        // **`export_script`, the same field `preview_of` fills that way**, and
        // for the same reason: this is what Copy and Open in editor hand over,
        // and both put it somewhere durable. It happens to equal
        // `editor_script` for a comparison, which produces no account change —
        // but the rule is enforced by calling the scrubbing function, not by a
        // comment observing that today there is nothing to scrub.
        script: plan.export_script(),
        read_only,
        dialect: plan.dialect,
    }
}

/// Send a **container** change — a database or a namespace — to the preview.
///
/// The counterpart of [`preview_change`] for the four changes that have no
/// table: `ddl::server_level` builds the set, and [`preview_of`] reads the scope
/// back off it, so a caller here cannot get the run path wrong by forgetting to
/// say which one it wanted.
///
/// `database` is what the plan is *about*, which under [`crate::DdlScope::Server`]
/// is deliberately not the database the statements run on — see
/// [`crate::DdlRunRequest::database`].
pub(crate) fn preview_container(
    ui: &Ui,
    database: &str,
    subject: &str,
    change: schemaic_core::ddl::Change,
) {
    let ctx = crate::table_designer::edit_ctx(ui);
    let cs = schemaic_core::ddl::server_level(subject, ctx.dialect, change);
    open_preview(
        ui,
        preview_of(ctx.conn_id, database, subject, &cs, ctx.read_only),
    );
}

/// Send an **account** change — a create, a drop, a grant or a revoke — to the
/// preview.
///
/// The counterpart of [`preview_container`] for the six changes that have no
/// table *and* are not server-level: `ddl::account` builds the set, and
/// [`preview_of`] reads the scope back off it, which for these is
/// [`crate::DdlScope::Database`]. That is not an accident of the default — see
/// `ddl::is_account_change` for why a PostgreSQL grant has to run in the
/// database whose catalogue holds the object it names.
///
/// `database` is therefore load-bearing here in a way it is not for a container:
/// it is the database the statements actually run on, and it is the one the
/// browser was already showing privileges for.
/// **Built against the connection the plan was raised on**, not against
/// whichever the switcher points at now. `conn_id`, `dialect` and `read_only`
/// come from the captured `AccountTarget`/`GrantTarget`/`UsersTarget` — which is
/// what those fields are *for*, and they were carried and never read while this
/// re-derived all three from the live `edit_ctx`. A form opened on MySQL and
/// previewed after a switch to PostgreSQL was emitted at the wrong dialect, and
/// the wrong connection's read-only flag decided whether Apply was offered.
pub(crate) fn preview_account(
    ui: &Ui,
    on: AccountPlanTarget,
    subject: &str,
    change: schemaic_core::ddl::Change,
) {
    let cs = schemaic_core::ddl::account(subject, on.dialect, change);
    open_preview(
        ui,
        preview_of(on.conn_id, &on.database, subject, &cs, on.read_only),
    );
}

/// Which server an account plan is for — captured where the plan is raised.
///
/// A struct rather than three more parameters because all three come from one
/// place and have to stay together: taking them individually is how one call
/// site comes to pass the live connection's `read_only` beside the target's
/// `conn_id`.
#[derive(Clone, Debug)]
pub(crate) struct AccountPlanTarget {
    pub conn_id: u64,
    pub database: String,
    pub dialect: SqlDialect,
    pub read_only: bool,
}

impl From<&crate::AccountTarget> for AccountPlanTarget {
    fn from(t: &crate::AccountTarget) -> Self {
        Self {
            conn_id: t.conn_id,
            database: t.database.clone(),
            dialect: t.dialect,
            read_only: t.read_only,
        }
    }
}

impl From<&crate::GrantTarget> for AccountPlanTarget {
    fn from(t: &crate::GrantTarget) -> Self {
        Self {
            conn_id: t.conn_id,
            database: t.database.clone(),
            dialect: t.dialect,
            read_only: t.read_only,
        }
    }
}

/// Send **one** change straight to the preview, skipping the designer — how
/// every context-menu shortcut works. Same modal, same warnings, same Apply: the
/// shortcut saves the designer, not the review.
pub(crate) fn preview_change(
    ui: &Ui,
    database: &str,
    table: &str,
    schema: Option<&str>,
    change: schemaic_core::ddl::Change,
) {
    let ctx = crate::table_designer::edit_ctx(ui);
    let cs = schemaic_core::ddl::single(table, schema, ctx.dialect, change);
    open_preview(
        ui,
        preview_of(
            ctx.conn_id,
            database,
            schemaic_core::schema::display_name(schema, table),
            &cs,
            ctx.read_only,
        ),
    );
}

/// Send an **AI proposal** to the preview, or say why it can't go there.
///
/// The same modal, the same warnings, the same Apply the designer reaches: a
/// proposal is a draft like any other by the time it gets here, which is the
/// whole point of `core::propose` handing back a `TableDraft` rather than SQL.
/// Nothing on this path runs anything — the user still reads the plan and clicks
/// Apply.
///
/// The `Err` is shown on the proposal card itself rather than in a modal. Every
/// one of them is the model being wrong about the table, and the card is where
/// the user can see what it asked for and tell it what it got wrong.
///
/// The table comes from [`crate::table_designer::loaded_table`] — the one funnel
/// every editor seeds from, and it refuses while a re-introspection is in
/// flight. That refusal matters more here than anywhere: the model may have read
/// the table minutes ago, and building a draft on a stale `TableInfo` is how an
/// `ALTER` comes to restate an old column definition and silently revert a
/// change that has already landed.
pub(crate) fn preview_proposal(
    ui: &Ui,
    database: &str,
    proposal: &schemaic_core::propose::Proposal,
) -> Result<(), String> {
    let ctx = crate::table_designer::edit_ctx(ui);
    let Some(loaded) = crate::table_designer::loaded_schema(ui, database) else {
        return Err(format!(
            "{} isn't loaded in {database} right now — open the database in the schema tree, or \
             wait for a refresh to finish, and try again.",
            proposal.table
        ));
    };
    // **The same resolver `propose_table_change` uses.** The tool tells the
    // model its change is valid against one table; this is what the user is
    // offered, and the two reading the JSON by different rules is how those
    // could be different tables.
    let info =
        schemaic_core::propose::resolve_target(&loaded, proposal).map_err(|e| e.to_string())?;
    // The table as the *server* spells it, not as the proposal wrote it: the
    // resolver accepts `sales.orders` and an explicit `schema`, so the subject
    // has to come off what was found.
    let subject = schemaic_core::schema::display_name(info.schema.as_deref(), &info.name);
    let draft =
        schemaic_core::propose::apply(info, proposal, ctx.dialect).map_err(|e| e.to_string())?;
    // The flavour the schema was actually introspected with — see `db_flavour`.
    // The MySQL emitter's `ALTER TABLE` path reads it, so taking the dialect
    // alone would give this path a different plan than the designer's for the
    // very same change.
    let target = schemaic_core::ddl::Target::new(
        ctx.dialect,
        crate::table_designer::db_flavour(ui, database),
    );
    let cs = schemaic_core::ddl::diff(info, &draft, target);
    if cs.is_empty() {
        return Err(format!(
            "{subject} already looks like that — there is nothing to change."
        ));
    }
    open_preview(
        ui,
        preview_of(ctx.conn_id, database, subject, &cs, ctx.read_only),
    );
    Ok(())
}

/// A bullet line in the change list.
fn change_line(label: String) -> impl IntoView {
    h_stack((
        text("•").style(|s| {
            s.color(theme::text_faint())
                .font_size(theme::font_body())
                .width(theme::scaled(12.0))
                .flex_shrink(0.0_f32)
        }),
        text(label).style(|s| {
            s.color(theme::text())
                .font_size(theme::font_body())
                .flex_grow(1.0_f32)
                .min_width(0.0)
        }),
    ))
    .style(|s| {
        s.flex_row()
            .items_start()
            .width_full()
            .margin_bottom(theme::scaled(3.0))
    })
}

/// The destructive block. Present ⇒ this plan takes something away, and the
/// wording says what rather than "are you sure".
fn risk_block(heading: &'static str, risks: Vec<String>) -> impl IntoView {
    let empty_block = risks.is_empty();
    v_stack((
        h_stack((
            icons::icon(icons::TRIANGLE_ALERT, 15.0)
                .style(|s| s.color(theme::error()).flex_shrink(0.0_f32)),
            // **The change set's, not a literal here.** See
            // `ChangeSet::risk_heading`: a revoke's own sentence says it is
            // undone by granting it back, and it appeared under "This can't be
            // undone" two entries away from `DROP USER`.
            text(heading).style(|s| {
                s.color(theme::error())
                    .font_size(theme::font_body())
                    .font_bold()
            }),
        ))
        .style(|s| {
            s.flex_row()
                .items_center()
                .gap(theme::scaled(7.0))
                .margin_bottom(theme::scaled(5.0))
        }),
        v_stack_from_iter(risks.into_iter().map(|r| {
            text(r).style(|s| {
                s.color(theme::text())
                    .font_size(theme::font_body())
                    .width_full()
                    .margin_bottom(theme::scaled(2.0))
            })
        })),
    ))
    .style(move |s| {
        let s = s
            .flex_col()
            .width_full()
            .padding(theme::scaled(10.0))
            .border(1.0)
            .border_color(theme::error())
            .border_radius(6.0)
            .background(theme::error().multiply_alpha(0.08));
        if empty_block { s.hide() } else { s }
    })
}

/// The withheld block. Present ⇒ this engine has no statement for part of the
/// plan, so the SQL below is **less** than the change list above it.
///
/// It is a block of its own rather than a line in the risk list because it says
/// the opposite thing: the risk block warns about what will happen, this one
/// says what won't. Apply refuses while it is showing — half an edit is not a
/// smaller version of the edit.
fn withheld_block(withheld: Vec<String>) -> impl IntoView {
    let empty_block = withheld.is_empty();
    v_stack((
        h_stack((
            icons::icon(icons::TRIANGLE_ALERT, 15.0)
                .style(|s| s.color(theme::accent()).flex_shrink(0.0_f32)),
            text("This engine can't express part of this plan").style(|s| {
                s.color(theme::accent())
                    .font_size(theme::font_body())
                    .font_bold()
            }),
        ))
        .style(|s| {
            s.flex_row()
                .items_center()
                .gap(theme::scaled(7.0))
                .margin_bottom(theme::scaled(5.0))
        }),
        v_stack_from_iter(withheld.into_iter().map(|w| {
            text(w).style(|s| {
                s.color(theme::text())
                    .font_size(theme::font_body())
                    .width_full()
                    .margin_bottom(theme::scaled(2.0))
            })
        })),
        text("Nothing is applied while this is listed.").style(|s| {
            s.color(theme::text_faint())
                .font_size(theme::font_body())
                .margin_top(theme::scaled(4.0))
        }),
    ))
    .style(move |s| {
        let s = s
            .flex_col()
            .width_full()
            .padding(theme::scaled(10.0))
            .border(1.0)
            .border_color(theme::accent())
            .border_radius(6.0)
            .background(theme::accent().multiply_alpha(0.08));
        if empty_block { s.hide() } else { s }
    })
}

/// Hand the plan to the app, and fold the outcome back into the modal.
fn apply(ui: Ui) {
    let d = ui.ddl;
    let Some(p) = d.preview.get_untracked() else {
        return;
    };
    // **Asked live, not read off the stamp.** `p.read_only` was copied into the
    // preview when it was *built*, so flipping the connection read-only from
    // the status bar while a `DROP DATABASE` plan was on screen did not stop
    // Apply — the modal that now routes `DROP DATABASE`/`DROP SCHEMA` is
    // exactly where that costs most. `script_view::policy` reads the flag at
    // the moment Run is pressed, and two destructive modals must not answer the
    // same question two different ways. The stamp stays, because the note
    // beside the footer is about the preview as opened; the *guard* is this.
    let read_only = ui.conn.connections.with_untracked(|cs| {
        cs.iter()
            .find(|c| c.id == p.conn_id)
            .is_some_and(|c| c.read_only)
    }) || p.read_only;
    if !crate::widgets::accept_launch(d.applying.get_untracked(), read_only) {
        return;
    }
    // The guard belongs on the action, not only on the disabled button — the
    // same rule the write guard follows. `statements` here is short of what the
    // change list promised, so running it would apply half an edit.
    if !p.withheld.is_empty() {
        return;
    }
    d.applying.set(true);
    d.error.set(None);
    let opened = d.generation.get_untracked();
    (ui.schema_actions.run_ddl)(
        DdlRunRequest {
            conn_id: p.conn_id,
            database: p.database.clone(),
            scope: p.scope,
            statements: p.statements.clone(),
        },
        Rc::new(move |res| {
            // The modal was closed and reopened on something else while this ran
            // — reporting into that would claim a different plan succeeded.
            if d.generation.get_untracked() != opened {
                return;
            }
            d.applying.set(false);
            match res {
                DdlOutcome::Applied => {
                    d.applied.set(true);
                    // The draft behind this is now the server's state, so
                    // whichever editor opened it has nothing left to show.
                    close_editors(d);
                }
                DdlOutcome::Failed(e) => d.error.set(Some(e)),
                // The user answered "Cancel" to a question the apply raised, so
                // the modal goes back to where it was — nothing to report, and
                // an error banner would name a failure that never happened.
                DdlOutcome::Declined => {}
            }
        }),
    );
}

/// The DDL preview modal. Absolutely positioned over the workspace when
/// `ui.ddl.preview` is `Some`.
/// The connection this plan runs against, by name.
///
/// Falls back to the id rather than to nothing: a title that silently drops the
/// connection is the state this exists to end.
fn connection_label(ui: &Ui, conn_id: u64) -> String {
    ui.conn.connections.with_untracked(|list| {
        list.iter()
            .find(|c| c.id == conn_id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| format!("connection {conn_id}"))
    })
}

/// The preview modal's title bar.
///
/// **A server-level plan has no database to qualify against**, and the subject
/// *is* the database — so the usual `db.subject` would read `.schemaic_probe`, a
/// qualifier with nothing on its left.
///
/// **And an account is not in a database either.** `DdlScope` answers which
/// *runner* a plan takes, and an account change deliberately takes the ordinary
/// in-database one (`ddl::is_account_change` — a PostgreSQL grant has to run in
/// the database whose catalogue holds the object). It inherited the qualifier as
/// a side effect, so `CREATE USER 'app'@'%'` — server-wide on both engines —
/// was titled `shop.app@%`, which reads as scoped and is not. `qualified` is the
/// separate question: what the plan is *about*, read off the change set by
/// `preview_of`, not off which connection carries it.
///
/// A free function so the sentence has a test; it was a `match` inside a
/// `dyn_container` child.
pub(crate) fn preview_title(connection: &str, p: &DdlPreview) -> String {
    if p.qualified {
        format!(
            "Apply changes to {connection} · {}.{}",
            p.database, p.subject
        )
    } else {
        format!("Apply changes to {connection} · {}", p.subject)
    }
}

pub(crate) fn ddl_preview_overlay(ui: Ui) -> impl IntoView {
    let d = ui.ddl;
    // Closing returns to the designer when it's still open behind — the draft is
    // untouched, so Cancel here means "not yet", not "throw it away".
    //
    // While an apply is in flight, the exit **depends on the engine**, and that
    // is the change. It used to refuse unconditionally, on the argument that
    // `run_ddl` was handed a fresh token nothing held and that on MySQL each
    // statement has already committed — so there is nothing to cancel, `d.error`
    // is the only reader of "statement 3 of 5 failed, 2 already stuck", and
    // closing would leave a half-migrated table with no indication at all. The
    // second half of that is still exactly right, and MySQL still refuses.
    //
    // The first half was not. PostgreSQL and SQLite roll a plan back as a whole
    // (`ddl::ddl_rolls_back_as_a_whole`), so a Stop there leaves the database as
    // it was and has nothing to orphan — and the range put *Refresh view* behind
    // this modal, which is a single statement that can run for hours (measured
    // 15 s on a toy matview) with `lock_timeout` bounding only the lock, not the
    // rebuild. Refusing every exit over that is a trap, not a guard.
    let exit_dialect = move || {
        d.preview
            .with_untracked(|p| p.as_ref().map(|p| p.dialect))
            .unwrap_or(SqlDialect::MySql)
    };
    let cancel_apply = ui.schema_actions.clone();
    // An `Rc` rather than a bare closure: it now holds the actions bundle, so it
    // is no longer `Copy` and the three exits share one.
    let exit: Rc<dyn Fn()> = Rc::new(move || {
        let cancellable = schemaic_core::ddl::ddl_rolls_back_as_a_whole(exit_dialect());
        match exit_action(d.applying.get_untracked(), cancellable) {
            ExitAction::Close => close_preview(d),
            ExitAction::Cancel => (cancel_apply.ddl_cancel)(),
            ExitAction::Ignore => {}
        }
    });

    dyn_container(
        move || (d.preview.get().is_some(), d.applied.get()),
        move |(open, applied)| {
            if !open {
                return empty().into_any();
            }
            let exit = exit.clone();
            let ui = ui.clone();
            let Some(p) = d.preview.get_untracked() else {
                return empty().into_any();
            };
            // Read before `ui` is moved into the footer's closures.
            let title = preview_title(&connection_label(&ui, p.conn_id), &p);

            // The script box, then the footer. The box is read-only, but it is
            // the thing this modal exists to be *read*, and Tab is how a keyboard
            // reaches it to scroll and select; the footer follows it at
            // `ACTION_TAB`. Apply is deliberately reachable only by Tab-ing to it
            // — there is no default Enter anywhere in these modals, and this is
            // the button that most earns that: the plan behind it is an
            // irreversible `ALTER`.
            let ring = FocusRing::new();
            let root_ring = ring.clone();

            let body: AnyView = if applied {
                container(
                    v_stack((
                        text(format!(
                            "Applied {} statement{} to {}.",
                            p.statements.len(),
                            if p.statements.len() == 1 { "" } else { "s" },
                            p.subject
                        ))
                        .style(|s| s.color(theme::text()).font_size(theme::font_body())),
                        text("The schema has been refreshed.").style(|s| {
                            s.color(theme::text_dim())
                                .font_size(theme::font_label())
                                .margin_top(theme::scaled(6.0))
                        }),
                    ))
                    .style(|s| s.flex_col()),
                )
                .style(|s| s.padding_vert(theme::scaled(10.0)))
                .into_any()
            } else {
                let n = p.changes.len();
                v_stack((
                    // The count *is* the heading — a bare "Changes" above the list
                    // and a "2 changes" below it said the same thing twice, once
                    // in each direction.
                    form_section_owned(format!("{n} {}", plural(n, "Change", "Changes"))),
                    v_stack_from_iter(p.changes.iter().cloned().map(change_line))
                        .style(|s| s.flex_col().width_full()),
                    risk_block(p.risk_heading, p.destructive.clone())
                        .style(|s| s.margin_top(theme::scaled(14.0))),
                    withheld_block(p.withheld.clone()).style(|s| s.margin_top(theme::scaled(14.0))),
                    form_section("SQL").style(|s| s.margin_top(theme::scaled(18.0))),
                    // Read-only, but a real editor field: the script is meant to
                    // be read and selected, and it's the same widget the rest of
                    // the app uses for text. Monospace, because this is the one
                    // place the user reads generated SQL closely — aligned
                    // columns are how a stray clause gets spotted before Apply.
                    edit_field(
                        d.sql,
                        FieldCfg {
                            multiline: true,
                            no_wrap: true,
                            read_only: true,
                            mono: true,
                            font_size: theme::font_body,
                            max_rows: Some(d.sql_rows),
                            focus: Some((ring.clone(), 10)),
                            ..Default::default()
                        },
                    )
                    .style(|s| s.width_full()),
                ))
                .style(|s| s.flex_col().gap(theme::scaled(8.0)).width_full())
                .into_any()
            };

            let err = dyn_container(
                move || d.error.get(),
                move |e| match e {
                    None => empty().into_any(),
                    Some(e) => text(e)
                        .style(|s| {
                            s.color(theme::error())
                                .font_size(theme::font_body())
                                .max_width(580.0)
                                .margin_top(theme::scaled(12.0))
                        })
                        .into_any(),
                },
            );

            // The script's own actions — neither of them answers the question the
            // footer is asking, so they sit recessed at the far left rather than
            // in the Back/Apply pair.
            let ui_side = ui.clone();
            let ring_side = ring.clone();
            let side = dyn_container(
                move || (d.applying.get(), d.applied.get()),
                move |(busy, applied)| {
                    let ring = ring_side.clone();
                    let Some(p) = d.preview.get_untracked().filter(|_| !applied) else {
                        return empty().into_any();
                    };
                    // The *script*, not the wire statements: both of these hand
                    // the plan to something that splits on `;`.
                    let sql = p.script.clone();
                    let open_sql = sql.clone();
                    // The database the plan would have been applied to — the tab
                    // has to be the one this script belongs in, or "Open in
                    // editor" hands you an `ALTER` aimed somewhere else.
                    //
                    // **A server-level plan binds the tab to no database at
                    // all**, and the question is the scope rather than whether
                    // the name happens to be blank. A `CREATE DATABASE` has no
                    // database and so reads as empty either way; a `DROP
                    // DATABASE` carries its *target* here — the one database the
                    // statement must not be run from — so keying on emptiness
                    // handed the user a tab bound to the database the script
                    // drops, where PostgreSQL answers `cannot drop the currently
                    // open database`.
                    //
                    // **What `None` buys, stated honestly, because this comment
                    // used to claim more.** It stops the tab being bound to the
                    // *target*. It does not guarantee the run happens elsewhere:
                    // an unbound tab runs in the connection's own configured
                    // database (`Db::open`'s fallback), and if that database is
                    // the one being dropped, PostgreSQL refuses with exactly the
                    // message above. That is a loud, correct refusal naming the
                    // problem, and the fix is one the user can make — switch the
                    // tab's database — which is why the escape hatch does not
                    // try to pick a database on their behalf out of a list it
                    // does not have here.
                    let open_db = match p.scope {
                        crate::DdlScope::Server => None,
                        crate::DdlScope::Database => {
                            Some(p.database.clone()).filter(|d| !d.is_empty())
                        }
                    };
                    let open_query = ui_side.tab_actions.open_query.clone();
                    h_stack((
                        action_button_icon(
                            "Copy",
                            icons::COPY,
                            ActionKind::Quiet,
                            !busy,
                            ring.clone(),
                            ACTION_TAB,
                            move || {
                                let _ = floem::Clipboard::set_contents(sql.clone());
                            },
                        ),
                        // The escape hatch: the generated script, in a tab, where
                        // it can be read, edited and run like anything else.
                        action_button_icon(
                            "Open in editor",
                            icons::FILE_PEN_LINE,
                            ActionKind::Quiet,
                            !busy,
                            ring.clone(),
                            ACTION_TAB + 10,
                            move || {
                                (open_query)(open_sql.clone(), open_db.clone());
                                close_preview(d);
                                close_editors(d);
                            },
                        ),
                    ))
                    .style(|s| s.flex_row().items_center().gap(action_gap()))
                    .into_any()
                },
            );

            let ring_actions = ring.clone();
            let footer_exit = exit.clone();
            let actions = dyn_container(
                move || (d.applying.get(), d.applied.get()),
                move |(busy, applied)| {
                    let ui = ui.clone();
                    let ring = ring_actions.clone();
                    let exit = footer_exit.clone();
                    if applied {
                        return action_button(
                            "Close",
                            ActionKind::Primary,
                            true,
                            ring,
                            ACTION_TAB + 20,
                            {
                                let exit = exit.clone();
                                move || exit()
                            },
                        );
                    }
                    let p = match d.preview.get_untracked() {
                        Some(p) => p,
                        None => return empty().into_any(),
                    };
                    // **The footer says what pressing it does**, which is what
                    // makes an enabled button during an apply an answer rather
                    // than a trapdoor — the same rule the export modal's footer
                    // follows. While a *stoppable* apply runs it reads Stop, in
                    // Danger; where the engine cannot roll the plan back it
                    // stays disabled, because there is nothing to stop and the
                    // half-applied report would have nowhere to go.
                    let stoppable =
                        busy && schemaic_core::ddl::ddl_rolls_back_as_a_whole(p.dialect);
                    h_stack((
                        // "Back" only when there's somewhere to go back *to*. A
                        // context-menu shortcut opens this modal with nothing
                        // behind it, where Back would point at nowhere.
                        action_button(
                            if stoppable {
                                "Stop"
                            } else if d.designer.get_untracked().is_some()
                                || d.view.get_untracked().is_some()
                            {
                                "Back"
                            } else {
                                "Cancel"
                            },
                            if stoppable {
                                ActionKind::Danger
                            } else {
                                ActionKind::Neutral
                            },
                            !busy || stoppable,
                            ring.clone(),
                            ACTION_TAB + 20,
                            {
                                let exit = exit.clone();
                                move || exit()
                            },
                        ),
                        action_button(
                            if busy { "Applying…" } else { "Apply" },
                            // A destructive plan's affirmative action wears the
                            // colour of what it does. Same place and same weight
                            // as an ordinary Apply — only the fill differs — since
                            // this is the last thing between the user and an
                            // irreversible statement.
                            if p.destructive.is_empty() {
                                ActionKind::Primary
                            } else {
                                ActionKind::Danger
                            },
                            !busy
                                && !p.read_only
                                && !p.statements.is_empty()
                                && p.withheld.is_empty(),
                            ring,
                            ACTION_TAB + 30,
                            move || apply(ui.clone()),
                        ),
                    ))
                    .style(|s| s.flex_row().items_center().gap(action_gap()))
                    .into_any()
                },
            );

            // A read-only connection blocks the write, and says so where the
            // disabled button is rather than leaving it unexplained.
            let read_only_note = text("This connection is read-only.").style(move |s| {
                let s = s
                    .color(theme::plan_warn())
                    .font_size(theme::font_label())
                    .margin_right(theme::scaled(12.0));
                if d.preview.get().is_some_and(|p| p.read_only) && !d.applied.get() {
                    s
                } else {
                    s.hide()
                }
            });

            let close_x: Rc<dyn Fn()> = exit.clone();
            let panel = v_stack((
                // **The title names where the plan is going, not just what it is
                // about.** The modal has always carried `conn_id` and
                // `database` and printed neither — which was survivable while
                // every plan came from a tree row the user had clicked, and
                // stopped being so when a proposal card became a second author.
                modal_title_owned(title, close_x, root_ring.clone()),
                autohide(scroll(v_stack((body, err)).style(|s| {
                    s.flex_col()
                        .width_full()
                        .padding_horiz(modal_pad_h())
                        .padding_vert(theme::scaled(18.0))
                })))
                .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0)),
                modal_footer_split(
                    side,
                    h_stack((read_only_note, actions)).style(|s| s.flex_row().items_center()),
                ),
            ))
            .on_click_stop(|_| {})
            .style(|s| panel_style(s).width(panel_w()).height(modal_h(PANEL_H)));

            focus_root_with_ring(container(panel), root_ring)
                .on_key_down(Key::Named(NamedKey::Escape), |_| true, {
                    let exit = exit.clone();
                    move |_| exit()
                })
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
        if d.preview.get().is_some() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// A `DdlUi` with every signal freshly made, for the tests in this crate that
/// need one.
///
/// Every editor signal is written out, so a test fails to compile rather than
/// silently pass when a seventh one is added. `pub(crate)` because
/// `account_editor`'s opener tests need the same bundle and a second copy is how
/// the two come to disagree about what an editor is.
#[cfg(test)]
pub(crate) fn test_ddl_ui(scope: floem::reactive::Scope) -> crate::DdlUi {
    crate::DdlUi {
        designer: scope.create_rw_signal(None),
        draft: scope.create_rw_signal(Default::default()),
        tab: scope.create_rw_signal(crate::DesignerTab::Table),
        selected: scope.create_rw_signal(0),
        rev: scope.create_rw_signal(0),
        view: scope.create_rw_signal(None),
        view_draft: scope.create_rw_signal(Default::default()),
        view_rows: scope.create_rw_signal(14),
        trigger: scope.create_rw_signal(None),
        trigger_draft: scope.create_rw_signal(Default::default()),
        routine: scope.create_rw_signal(None),
        routine_draft: scope.create_rw_signal(Default::default()),
        routine_body: scope.create_rw_signal(String::new()),
        routine_source_pending: scope.create_rw_signal(false),
        routine_body_stale: scope.create_rw_signal(false),
        event: scope.create_rw_signal(None),
        event_draft: scope.create_rw_signal(Default::default()),
        event_body: scope.create_rw_signal(String::new()),
        event_source_pending: scope.create_rw_signal(false),
        event_body_stale: scope.create_rw_signal(false),
        functions: scope.create_rw_signal(Vec::new()),
        database: scope.create_rw_signal(None),
        database_draft: scope.create_rw_signal(Default::default()),
        account: scope.create_rw_signal(None),
        account_draft: scope.create_rw_signal(Default::default()),
        grant: scope.create_rw_signal(None),
        grant_draft: scope.create_rw_signal(Default::default()),
        roles: scope.create_rw_signal(Vec::new()),
        object: scope.create_rw_signal(None),
        object_draft: scope.create_rw_signal(Default::default()),
        object_errors: scope.create_rw_signal(Vec::new()),
        object_rev: scope.create_rw_signal(0),
        preview: scope.create_rw_signal(None),
        sql: scope.create_rw_signal(String::new()),
        sql_rows: scope.create_rw_signal(16),
        applying: scope.create_rw_signal(false),
        error: scope.create_rw_signal(None),
        applied: scope.create_rw_signal(false),
        generation: scope.create_rw_signal(0),
        session: scope.create_rw_signal(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use floem::reactive::Scope;
    use schemaic_core::intel::SqlDialect;

    use super::test_ddl_ui as ddl_ui;

    /// After a successful Apply — and after "Open in editor" — **no** editor may
    /// still be holding its pre-apply draft.
    ///
    /// Both sites used to clear `designer` and `view` only, though this range
    /// added three more overlays. Pressing Close then remounted the trigger,
    /// function or object editor with the same change count, and Preview →
    /// Apply re-ran a plan the server had already applied.
    #[test]
    fn close_editors_clears_every_editor() {
        let scope = Scope::new();
        let d = ddl_ui(scope);

        // Only the signals matter here, not what is in them — but they carry no
        // `Default`, so each is seeded with the smallest real target.
        d.designer.set(Some(crate::DesignerTarget {
            conn_id: 1,
            database: "db".into(),
            flavour: Default::default(),
            schema: None,
            dialect: SqlDialect::MySql,
            current: None,
            tables: Vec::new(),
            read_only: false,
        }));
        d.view.set(Some(crate::ViewTarget {
            conn_id: 1,
            database: "db".into(),
            schema: None,
            dialect: SqlDialect::MySql,
            current: None,
            read_only: false,
        }));
        d.trigger.set(Some(crate::TriggerTarget {
            conn_id: 1,
            database: "db".into(),
            schema: None,
            table: "t".into(),
            dialect: SqlDialect::MySql,
            is_view: false,
            current: Vec::new(),
            read_only: false,
        }));
        d.object.set(Some(crate::ObjectTarget {
            conn_id: 1,
            database: "db".into(),
            schema: None,
            dialect: SqlDialect::Postgres,
            current: None,
            dependents: Vec::new(),
            read_only: false,
        }));
        d.event.set(Some(crate::EventTarget {
            conn_id: 1,
            database: "db".into(),
            dialect: SqlDialect::MySql,
            current: None,
            read_only: false,
        }));
        d.database.set(Some(crate::DatabaseTarget {
            conn_id: 1,
            kind: crate::ContainerKind::Database,
            database: None,
            dialect: SqlDialect::MySql,
            read_only: false,
        }));
        d.account.set(Some(crate::AccountTarget {
            conn_id: 1,
            database: "db".into(),
            dialect: SqlDialect::MySql,
            read_only: false,
        }));
        d.grant.set(Some(crate::GrantTarget {
            conn_id: 1,
            database: "db".into(),
            account: an_account(),
            dialect: SqlDialect::MySql,
            read_only: false,
        }));
        // The drafts too: `account_draft` is app-lifetime and holds the
        // plaintext password, so leaving it set is the secret outliving the form
        // by the rest of the process.
        d.account_draft.set(schemaic_core::users::AccountDraft {
            name: "app".into(),
            password: "hunter2".into(),
            ..Default::default()
        });
        d.grant_draft.set(schemaic_core::users::GrantDraft {
            role: "r".into(),
            ..Default::default()
        });

        close_editors(d);

        assert!(d.designer.get_untracked().is_none(), "designer");
        assert!(d.view.get_untracked().is_none(), "view");
        assert!(d.trigger.get_untracked().is_none(), "trigger");
        assert!(d.object.get_untracked().is_none(), "object");
        assert!(d.event.get_untracked().is_none(), "event");
        assert!(d.database.get_untracked().is_none(), "database");
        assert!(d.account.get_untracked().is_none(), "account");
        assert!(d.grant.get_untracked().is_none(), "grant");
        assert_eq!(
            d.account_draft.get_untracked(),
            Default::default(),
            "the password outlived the form it was typed into"
        );
        assert_eq!(d.grant_draft.get_untracked(), Default::default(), "grant");

        scope.dispose();
    }

    /// The account fixture the two lists above share.
    fn an_account() -> schemaic_core::users::Principal {
        schemaic_core::users::from_mysql_rows(&[schemaic_core::users::MyUserRow {
            user: "app".into(),
            host: "%".into(),
            ..Default::default()
        }])
        .remove(0)
    }

    /// **The one editor a close must leave standing.** A PostgreSQL trigger has
    /// no body, only a function to call, so the function modal is opened *from*
    /// a half-filled trigger form and `open_function` deliberately leaves the
    /// trigger target set. Clearing both destroyed that form on Apply — and on
    /// "Open in editor", which applies nothing at all.
    #[test]
    fn a_function_plan_leaves_the_trigger_editor_behind_it_standing() {
        let scope = Scope::new();
        let d = ddl_ui(scope);
        d.trigger.set(Some(crate::TriggerTarget {
            conn_id: 1,
            database: "db".into(),
            schema: None,
            table: "t".into(),
            dialect: SqlDialect::Postgres,
            is_view: false,
            current: Vec::new(),
            read_only: false,
        }));
        d.routine.set(Some(crate::RoutineTarget {
            conn_id: 1,
            database: "db".into(),
            dialect: SqlDialect::Postgres,
            current: None,
            read_only: false,
        }));

        close_editors(d);

        assert!(
            d.routine.get_untracked().is_none(),
            "the editor the plan came from still closes"
        );
        assert!(
            d.trigger.get_untracked().is_some(),
            "the trigger form it was opened from is what the function is for"
        );
        scope.dispose();
    }

    /// **The same invariant read from the other end.** `close_editors` must
    /// clear every editor; `ddl_editors_up` must *see* every editor — it is what
    /// gives the whole DDL overlay group its box, and a modal missing from it
    /// opens into zero by zero and paints nothing.
    ///
    /// The event editor shipped absent from it, which is why this test exists
    /// beside the one above rather than being folded into it: two lists, one
    /// rule, and a new editor has to be added to both.
    ///
    /// Each target is raised **alone**, so a list that happens to contain some
    /// other signal can't carry a missing one.
    #[test]
    fn every_editor_raises_the_group_that_gives_it_a_box() {
        let scope = Scope::new();
        let d = ddl_ui(scope);
        let up = crate::modals::ddl_editors_up(d);
        assert!(!up(), "nothing open");

        // Plain `fn` pointers over the `Copy` bundle rather than boxed closures:
        // none of these captures anything, and the array is the list of editors
        // the test is about.
        type Raise = (&'static str, fn(crate::DdlUi));
        let raise: [Raise; 9] = [
            ("account", |d| {
                d.account.set(Some(crate::AccountTarget {
                    conn_id: 1,
                    database: "db".into(),
                    dialect: SqlDialect::MySql,
                    read_only: false,
                }))
            }),
            ("grant", |d| {
                d.grant.set(Some(crate::GrantTarget {
                    conn_id: 1,
                    database: "db".into(),
                    account: an_account(),
                    dialect: SqlDialect::MySql,
                    read_only: false,
                }))
            }),
            ("designer", |d| {
                d.designer.set(Some(crate::DesignerTarget {
                    conn_id: 1,
                    database: "db".into(),
                    flavour: Default::default(),
                    schema: None,
                    dialect: SqlDialect::MySql,
                    current: None,
                    tables: Vec::new(),
                    read_only: false,
                }))
            }),
            ("view", |d| {
                d.view.set(Some(crate::ViewTarget {
                    conn_id: 1,
                    database: "db".into(),
                    schema: None,
                    dialect: SqlDialect::MySql,
                    current: None,
                    read_only: false,
                }))
            }),
            ("trigger", |d| {
                d.trigger.set(Some(crate::TriggerTarget {
                    conn_id: 1,
                    database: "db".into(),
                    schema: None,
                    table: "t".into(),
                    dialect: SqlDialect::MySql,
                    is_view: false,
                    current: Vec::new(),
                    read_only: false,
                }))
            }),
            ("routine", |d| {
                d.routine.set(Some(crate::RoutineTarget {
                    conn_id: 1,
                    database: "db".into(),
                    dialect: SqlDialect::MySql,
                    current: None,
                    read_only: false,
                }))
            }),
            ("object", |d| {
                d.object.set(Some(crate::ObjectTarget {
                    conn_id: 1,
                    database: "db".into(),
                    schema: None,
                    dialect: SqlDialect::Postgres,
                    current: None,
                    dependents: Vec::new(),
                    read_only: false,
                }))
            }),
            ("event", |d| {
                d.event.set(Some(crate::EventTarget {
                    conn_id: 1,
                    database: "db".into(),
                    dialect: SqlDialect::MySql,
                    current: None,
                    read_only: false,
                }))
            }),
            ("database", |d| {
                d.database.set(Some(crate::DatabaseTarget {
                    conn_id: 1,
                    kind: crate::ContainerKind::Database,
                    database: None,
                    dialect: SqlDialect::MySql,
                    read_only: false,
                }))
            }),
        ];

        for (name, set) in raise {
            set(d);
            assert!(up(), "{name} is open and the group says nothing is");
            close_editors(d);
            // `close_editors` deliberately leaves the trigger form standing when
            // the plan came from the routine editor above it, which is not the
            // case here — nothing raised a routine.
            d.trigger.set(None);
            assert!(!up(), "{name} closed and the group still says something is");
        }

        scope.dispose();
    }
    /// **`preview_of` decides which runner a plan takes and which refresh
    /// follows it**, and it is a pure `(&ChangeSet, …) -> DdlPreview`. Its two
    /// ingredients were tested in isolation and the composition was not: a
    /// caller that had to *say* which scope it wanted is one that will one day
    /// say the wrong thing, and a `DROP DATABASE` down the in-database route
    /// runs on a connection pointed at the database it just removed.
    #[test]
    fn a_previews_scope_and_dialect_come_off_the_change_set() {
        use schemaic_core::ddl::{Change, DatabaseDraft};

        let server = schemaic_core::ddl::server_level(
            "shop",
            SqlDialect::Postgres,
            Change::DropDatabase {
                name: "shop".into(),
            },
        );
        let p = preview_of(1, "shop", "shop", &server, false);
        assert_eq!(p.scope, crate::DdlScope::Server);
        assert_eq!(p.dialect, SqlDialect::Postgres);

        // And a create, whose `database` is empty rather than its target — the
        // emptiness test this replaced would have read the two the same way.
        let create = schemaic_core::ddl::server_level(
            "shop",
            SqlDialect::MySql,
            Change::CreateDatabase(Box::new(DatabaseDraft::blank("shop"))),
        );
        let p = preview_of(1, "", "shop", &create, false);
        assert_eq!(p.scope, crate::DdlScope::Server);
        assert_eq!(p.dialect, SqlDialect::MySql);

        // An ordinary in-database plan is the other half, and the one a wrong
        // answer here would send down the server-level runner.
        let table =
            schemaic_core::ddl::single("orders", None, SqlDialect::MySql, Change::TruncateTable);
        let p = preview_of(1, "shop", "orders", &table, false);
        assert_eq!(p.scope, crate::DdlScope::Database);
        assert_eq!(p.dialect, SqlDialect::MySql);
    }

    /// **Which runner a plan takes and what it is *about* are two questions**,
    /// and the title asked the first. An account change takes the in-database
    /// runner deliberately — a PostgreSQL grant has to run in the database whose
    /// catalogue holds the object — and is nonetheless server-wide, so
    /// `CREATE USER 'app'@'%'` was titled `shop.app@%`, which reads as scoped
    /// and is not.
    #[test]
    fn only_a_plan_that_lives_in_a_database_is_qualified_by_one() {
        use schemaic_core::ddl::{Change, DatabaseDraft};

        // A table: in the database, and qualified by it.
        let table =
            schemaic_core::ddl::single("orders", None, SqlDialect::MySql, Change::TruncateTable);
        let p = preview_of(1, "shop", "orders", &table, false);
        assert!(p.qualified);
        assert_eq!(
            preview_title("My MariaDB", &p),
            "Apply changes to My MariaDB · shop.orders"
        );

        // An account: the in-database *runner*, and no database to be in.
        let account = schemaic_core::ddl::account(
            "app@%",
            SqlDialect::MySql,
            Change::CreateAccount(Box::new(schemaic_core::users::AccountDraft {
                name: "app".into(),
                ..Default::default()
            })),
        );
        let p = preview_of(1, "shop", "app@%", &account, false);
        assert_eq!(
            p.scope,
            crate::DdlScope::Database,
            "the runner is unchanged"
        );
        assert!(!p.qualified);
        assert_eq!(
            preview_title("My MariaDB", &p),
            "Apply changes to My MariaDB · app@%"
        );

        // A container: neither, and the arm that already existed — the subject
        // *is* the database, so a qualifier would have nothing on its left.
        let create = schemaic_core::ddl::server_level(
            "shop",
            SqlDialect::MySql,
            Change::CreateDatabase(Box::new(DatabaseDraft::blank("shop"))),
        );
        let p = preview_of(1, "", "shop", &create, false);
        assert!(!p.qualified);
        assert_eq!(
            preview_title("My MariaDB", &p),
            "Apply changes to My MariaDB · shop"
        );
    }

    /// A schema comparison's whole plan, through the same modal.
    fn compare_plan(
        left: schemaic_core::schema::DbSchema,
        right: schemaic_core::schema::DbSchema,
    ) -> schemaic_core::compare::SchemaPlan {
        schemaic_core::compare::SchemaComparison::of(&left, &right, SqlDialect::MySql)
            .plan(|_| true)
    }

    fn one_table(name: &str, cols: &[&str]) -> schemaic_core::schema::DbSchema {
        schemaic_core::schema::DbSchema {
            tables: vec![schemaic_core::schema::TableInfo {
                name: name.to_string(),
                columns: cols
                    .iter()
                    .map(|c| schemaic_core::schema::ColumnInfo {
                        name: c.to_string(),
                        type_name: "int".to_string(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    /// **A plan's subject is a count, not an object**, so no database qualifies
    /// it. Derived from the changes the way `preview_of` does it, the answer was
    /// `true` — every input true, the title nonsense: `shop.12 objects`.
    #[test]
    fn a_plan_is_never_qualified_by_the_database_it_runs_in() {
        let plan = compare_plan(one_table("gone", &["id"]), Default::default());
        let p = preview_of_plan(1, "shop", "1 object", &plan, false);
        assert!(!p.qualified);
        assert_eq!(
            preview_title("My MariaDB", &p),
            "Apply changes to My MariaDB · 1 object"
        );
        // The runner is still the in-database one: a comparison compares
        // objects *within* a database and produces no server-level change.
        assert_eq!(p.scope, crate::DdlScope::Database);
    }

    /// Every field of the preview comes off the plan, and the two text lists
    /// name their objects — the modal's own title cannot, so they must.
    #[test]
    fn a_plans_preview_reads_every_field_off_the_plan() {
        let plan = compare_plan(one_table("gone", &["id"]), one_table("fresh", &["id"]));
        let p = preview_of_plan(7, "shop", "2 objects", &plan, false);
        assert_eq!(p.conn_id, 7);
        assert_eq!(p.database, "shop");
        assert_eq!(p.statements, plan.emit());
        assert_eq!(p.script, plan.export_script());
        assert_eq!(p.withheld, plan.unsupported());
        assert_eq!(p.dialect, SqlDialect::MySql);
        assert!(!p.read_only);
        // A drop and a create, each line saying which object it is about.
        assert!(
            p.changes.iter().any(|c| c.starts_with("gone — ")),
            "{:?}",
            p.changes
        );
        assert!(
            p.changes.iter().any(|c| c.starts_with("fresh — ")),
            "{:?}",
            p.changes
        );
        assert!(
            p.destructive.iter().all(|d| d.starts_with("gone — ")),
            "only the drop is destructive, and it says so by name: {:?}",
            p.destructive
        );
        assert_eq!(p.risk_heading, "This can't be undone");
    }

    /// An empty plan asks the modal for nothing, and must not answer `true` to
    /// a question about accounts by accident — `all()` over no changes is
    /// vacuously true, which is what made the old `qualified` derivation flip
    /// for a reason that had nothing to do with accounts.
    #[test]
    fn an_empty_plan_previews_as_empty_and_unqualified() {
        let plan = schemaic_core::compare::SchemaPlan::default();
        let p = preview_of_plan(1, "shop", "0 objects", &plan, false);
        assert!(!p.qualified);
        assert_eq!(p.scope, crate::DdlScope::Database);
        assert!(p.statements.is_empty());
        assert!(p.changes.is_empty());
        assert!(p.destructive.is_empty());
        assert!(p.withheld.is_empty());
    }

    /// Read-only travels through unchanged: it is what disables Apply, and
    /// `apply` re-reads the live flag on top of it.
    #[test]
    fn a_plans_preview_carries_read_only_through() {
        let plan = compare_plan(one_table("gone", &["id"]), Default::default());
        assert!(preview_of_plan(1, "shop", "1 object", &plan, true).read_only);
    }

    /// **Whether the preview's exits may stop an apply is the engine's
    /// question**, and the modal used to answer `false` for all three. The two
    /// wrong answers cost very differently, which is why this is pinned per
    /// engine rather than left to the exit's `bool`.
    #[test]
    fn only_an_engine_that_rolls_a_plan_back_may_have_its_apply_stopped() {
        use crate::widgets::{ExitAction, exit_action};
        use schemaic_core::ddl::ddl_rolls_back_as_a_whole;

        // MySQL: each statement has already committed, so a Stop would orphan
        // the "3 of 5 failed, 2 already stuck" report the modal is the only
        // reader of. This arm must keep refusing.
        assert!(!ddl_rolls_back_as_a_whole(SqlDialect::MySql));
        assert_eq!(
            exit_action(true, ddl_rolls_back_as_a_whole(SqlDialect::MySql)),
            ExitAction::Ignore
        );

        // PostgreSQL and SQLite wrap the plan, so a Stop leaves the database as
        // it was — and `Refresh view` behind this modal is a single statement
        // that can run for hours.
        for d in [SqlDialect::Postgres, SqlDialect::Sqlite] {
            assert!(ddl_rolls_back_as_a_whole(d), "{d:?}");
            assert_eq!(
                exit_action(true, ddl_rolls_back_as_a_whole(d)),
                ExitAction::Cancel,
                "{d:?}"
            );
        }

        // Nothing in flight closes on every engine, as it always did.
        for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            assert_eq!(
                exit_action(false, ddl_rolls_back_as_a_whole(d)),
                ExitAction::Close,
                "{d:?}"
            );
        }
    }
}
