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

use schemaic_core::connection::Connection;
use schemaic_core::intel::SqlDialect;
use schemaic_core::text::plural;

use crate::widgets::{
    ACTION_TAB, ActionKind, ExitAction, FocusRing, action_button, action_button_icon, action_gap,
    autohide, exit_action, focus_root_with_ring, form_section, form_section_owned,
    modal_footer_split, modal_h, modal_pad_h, modal_title_owned, modal_w, panel_style,
};
use crate::{
    ConnUi, DdlFn, DdlOutcome, DdlPreview, DdlRunRequest, DdlUi, FieldCfg, edit_field, icons, theme,
};

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

/// Is one of the editors [`close_peers`] enumerates standing behind the
/// preview?
///
/// **What the exit button's word depends on.** The label was a hand-spelled
/// two-name test — `designer.is_some() || view.is_some()` — while the canonical
/// list of what stacks under the preview sat in `close_peers` just above, nine
/// entries long. So seven of the nine editors got **Cancel** on a button that
/// does not cancel: `exit` resolves to [`close_preview`], which writes only
/// `preview` and `sql`, so whichever editor signal is still `Some` re-renders
/// with the draft intact. `object_editor` and `database_editor` both carry the
/// comment "Cancel there returns here with the draft intact", which is the code
/// asserting the button is a Back while the button said Cancel.
///
/// The same drift class this file already names once — "One list, in
/// `ddl_preview`; five hand-written copies had already drifted" — so the answer
/// is one predicate rather than a third spelling.
///
/// **What holds it and `close_peers` together is a test, not the compiler.**
/// `tests::every_editor_reaches_the_three_lists_that_must_know_about_it` raises
/// each of the nine targets *alone* and asserts `close_editors`,
/// `modals::ddl_editors_up` and this all see it — three hand-written
/// nine-entry lists, agreeing because something checks, which is the most that
/// is available without making the nine one enumerable thing.
pub(crate) fn has_editor_behind(d: crate::DdlUi) -> bool {
    d.designer.get_untracked().is_some()
        || d.view.get_untracked().is_some()
        || d.trigger.get_untracked().is_some()
        || d.routine.get_untracked().is_some()
        || d.object.get_untracked().is_some()
        || d.event.get_untracked().is_some()
        || d.database.get_untracked().is_some()
        || d.account.get_untracked().is_some()
        || d.grant.get_untracked().is_some()
}

/// Close the preview and drop the script with it.
///
/// The one door, because there are two `set(None)` sites and a third would
/// otherwise have to remember: `d.sql` is app-lifetime and would otherwise hold
/// the last plan's SQL for the life of the process.
///
/// **This clear is load-bearing, not defence in depth.** It was the latter while
/// the box showed [`ChangeSet::export_script`]'s redacted copy; it now holds
/// [`ChangeSet::emit`]'s statements, which for an account plan carry the real
/// password — see [`open_preview`] for why the box shows those.
///
/// [`ChangeSet::export_script`]: schemaic_core::ddl::ChangeSet::export_script
/// [`ChangeSet::emit`]: schemaic_core::ddl::ChangeSet::emit
pub(crate) fn close_preview(d: crate::DdlUi) {
    d.preview.set(None);
    d.sql.set(String::new());
}

/// Open the preview on a change set.
///
/// Takes the `DdlUi` rather than the whole [`crate::Ui`] because that is all it uses,
/// and because a bundle of 250 signals is not constructible in a test — which
/// is what left `the_sql_box_shows_the_statement_that_runs` unwritten while the
/// box showed the wrong text.
pub(crate) fn open_preview(d: crate::DdlUi, preview: DdlPreview) {
    // **The statements, not the script.** This module's own contract is "one
    // place that shows the statements and one place that says what they
    // destroy", and the box read `export_script` — which substitutes
    // `PUT-THE-PASSWORD-HERE` for an account's secret and prepends a header
    // about *a copy*. So the last confirmation surface before an irreversible
    // statement displayed text that would not run, and for the one case where
    // the emitted password is *wrong* (a `*` in it survives the masked field's
    // character diff mangled) there was no surface anywhere that showed it.
    //
    // Copy and "Open in editor" still hand over `p.script`: both put the text
    // somewhere durable — a clipboard, and a query tab `tabs.json` writes in the
    // clear — and that is the distinction `export_script` was written to draw.
    // A `DELIMITER` wrapper belongs to those two for the same reason; the wire
    // has never heard of it and neither has this box.
    d.sql.set(preview.statements.join("\n\n"));
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
        // A single object's edit is about that object: it is either in the plan
        // or there is no plan. Nothing can be left out of it.
        omitted: Vec::new(),
        statements: cs.emit(),
        // **`export_script`, not `editor_script`.** This field is what Copy and
        // Open in editor hand over, and both put it somewhere durable — the
        // clipboard, and a query tab the session file writes to `tabs.json` in
        // the clear. A `CREATE USER … IDENTIFIED BY 'hunter2'` has no business
        // in either. The *preview* renders `statements`, which is unchanged and
        // is the statement that runs — true again as of
        // `the_sql_box_shows_the_statement_that_runs`, and false for as long as
        // `open_preview` seeded the box from this field instead. Read off the
        // change set, like `scope` below, so a third exit from this modal
        // inherits the rule instead of having to remember it.
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
        // **The disclosure this modal was missing.** A comparison can hold
        // differences no plan can carry — on MySQL, every routine, trigger and
        // event, whose bodies arrive escape-mangled — and they have no tick-box,
        // so nothing the user does adds them. Before this, the last surface
        // before an irreversible Apply said "1 object" over a three-difference
        // comparison and reported "Applied N statements to 1 object".
        omitted: plan.omitted.clone(),
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
/// **Built against the connection the plan was raised on**, not against
/// whichever the switcher points at now — the same rule [`preview_account`]
/// states, and this is where it was missing. `database` comes off the target
/// too: for a namespace it is the database the plan runs in, and for a database
/// it is the empty string [`crate::DdlScope::Server`] wants.
pub(crate) fn preview_container(
    ui: DdlUi,
    on: PlanTarget,
    subject: &str,
    change: schemaic_core::ddl::Change,
) {
    open_preview(ui, container_preview(&on, subject, change));
}

/// The plan itself, with no `Ui` in reach — so the claim that it is built
/// against the *target* can be asserted rather than argued.
pub(crate) fn container_preview(
    on: &PlanTarget,
    subject: &str,
    change: schemaic_core::ddl::Change,
) -> DdlPreview {
    let cs = schemaic_core::ddl::server_level(subject, on.dialect, change);
    preview_of(on.conn_id, &on.database, subject, &cs, on.read_only)
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
    ui: DdlUi,
    on: PlanTarget,
    subject: &str,
    change: schemaic_core::ddl::Change,
) {
    let cs = schemaic_core::ddl::account(subject, on.dialect, change);
    open_preview(
        ui,
        preview_of(on.conn_id, &on.database, subject, &cs, on.read_only),
    );
}

/// Which server a plan is for — captured where the plan is **raised**, not read
/// back where it is previewed.
///
/// A struct rather than four more parameters because they come from one place
/// and have to stay together: taking them individually is how one call site
/// comes to pass the live connection's `read_only` beside the target's
/// `conn_id`.
///
/// **Both plan kinds that have no table take one.** It was the account plans'
/// alone while [`preview_container`] re-derived all four from the live
/// `edit_ctx` — so a *Create database* form filled in on MySQL and previewed
/// after a switch to PostgreSQL was emitted at PostgreSQL's dialect, against
/// PostgreSQL's `conn_id`, and Apply created the database on the wrong server.
/// Nothing closes a DDL editor on a connection switch, and the two container
/// menu entries reach the preview through a confirmation dialog, which is a
/// second window for the switch to happen in.
#[derive(Clone, Debug)]
pub(crate) struct PlanTarget {
    pub conn_id: u64,
    pub database: String,
    pub dialect: SqlDialect,
    pub read_only: bool,
}

impl From<&crate::AccountTarget> for PlanTarget {
    fn from(t: &crate::AccountTarget) -> Self {
        Self {
            conn_id: t.conn_id,
            database: t.database.clone(),
            dialect: t.dialect,
            read_only: t.read_only,
        }
    }
}

impl From<&crate::DatabaseTarget> for PlanTarget {
    fn from(t: &crate::DatabaseTarget) -> Self {
        Self {
            conn_id: t.conn_id,
            // Where the resulting plan runs: a namespace is created **in** its
            // database, a database is created on a server-level connection that
            // names none. The empty string is not a placeholder standing in for
            // a real value — under `crate::DdlScope::Server` the field is what
            // the run must *avoid*, and there is nothing to avoid when nothing
            // exists yet. (This was `database_editor::plan_database`, called at
            // the one site that is now this conversion.)
            database: t.database.clone().unwrap_or_default(),
            dialect: t.dialect,
            read_only: t.read_only,
        }
    }
}

impl From<&crate::GrantTarget> for PlanTarget {
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
///
/// **It reads the live switcher on purpose**, unlike [`preview_container`] —
/// which is why `conn` is named in the signature rather than hidden inside a
/// root bundle. A context-menu shortcut is answered in the same gesture that
/// raised it, so there is no window in which the user could switch connection
/// between the two; the `PlanTarget` forms exist for the plans that *are*
/// answered later.
pub(crate) fn preview_change(
    conn: crate::ConnUi,
    ddl: DdlUi,
    database: &str,
    table: &str,
    schema: Option<&str>,
    change: schemaic_core::ddl::Change,
) {
    let ctx = crate::table_designer::edit_ctx(conn);
    let cs = schemaic_core::ddl::single(table, schema, ctx.dialect, change);
    open_preview(
        ddl,
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
    conn: crate::ConnUi,
    schema: crate::SchemaUi,
    ddl: DdlUi,
    database: &str,
    proposal: &schemaic_core::propose::Proposal,
) -> Result<(), String> {
    let ctx = crate::table_designer::edit_ctx(conn);
    let Some(loaded) = crate::table_designer::loaded_schema(schema, database) else {
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
        crate::table_designer::db_flavour(schema.db_nodes, database),
    );
    let cs = schemaic_core::ddl::diff(info, &draft, target);
    if cs.is_empty() {
        return Err(format!(
            "{subject} already looks like that — there is nothing to change."
        ));
    }
    open_preview(
        ddl,
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

/// The omitted block. Present ⇒ the comparison this plan came from holds
/// differences the plan does not carry, and never could.
///
/// **It discloses and does not refuse**, which is the whole difference between
/// it and [`withheld_block`] and the reason they are two blocks. Withheld means
/// half an edit, and half an edit is not a smaller edit — Apply refuses until
/// the tick causing it is cleared. This means whole objects sat out: what is
/// below is complete and correct, the user has no tick to clear (these objects
/// have none), and refusing would make a comparison unusable on MySQL the
/// moment one routine differs. So it is stated in the muted colour, above the
/// statements, and Apply stays live.
fn omitted_block(omitted: Vec<String>) -> impl IntoView {
    let empty_block = omitted.is_empty();
    v_stack((
        h_stack((
            icons::icon(icons::CIRCLE_DOT, 15.0)
                .style(|s| s.color(theme::text_faint()).flex_shrink(0.0_f32)),
            text("Not included in this migration").style(|s| {
                s.color(theme::text())
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
        v_stack_from_iter(omitted.into_iter().map(|w| {
            text(w).style(|s| {
                s.color(theme::text())
                    .font_size(theme::font_body())
                    .width_full()
                    .margin_bottom(theme::scaled(2.0))
            })
        })),
        text("Applying leaves these objects as they are.").style(|s| {
            s.color(theme::text_faint())
                .font_size(theme::font_body())
                .margin_top(theme::scaled(4.0))
        }),
    ))
    .style(move |s| {
        s.flex_col()
            .width_full()
            .apply_if(empty_block, |s| s.display(floem::style::Display::None))
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

/// Is this plan refused — the connection read-only **now**, or the preview
/// stamped read-only when it was built?
///
/// **One expression for three readers**, because they were not reading the same
/// thing. `apply` asks the live flag (and must: flipping the connection
/// read-only from the status bar while a `DROP DATABASE` plan is on screen is
/// exactly where a stale answer costs most), while the footer's *enable* term
/// and its "This connection is read-only" note both read only the stamp. So the
/// flag could be flipped with the plan open and Apply stayed lit, said nothing,
/// and silently did nothing when pressed — a disabled-looking action would at
/// least have been honest, and the note that exists to explain it stayed hidden.
///
/// The stamp is still a term: a preview built while read-only says so for its
/// whole life, which is what the note beside the footer is about.
pub(crate) fn plan_read_only(
    conns: &[schemaic_core::connection::Connection],
    p: &DdlPreview,
) -> bool {
    p.read_only || schemaic_core::connection::read_only_of(conns, p.conn_id)
}

/// Hand the plan to the app, and fold the outcome back into the modal.
fn apply(d: DdlUi, conn: ConnUi, run_ddl: DdlFn) {
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
    let read_only = conn.connections.with_untracked(|cs| plan_read_only(cs, &p));
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
    (run_ddl)(
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
/// `d.preview` is `Some`.
/// The connection this plan runs against, by name.
///
/// Falls back to the id rather than to nothing: a title that silently drops the
/// connection is the state this exists to end.
/// **Takes the one signal it reads, not the whole `Ui`.** It touched 1 of 36
/// fields and encoded a real decision — the `connection N` fallback — which
/// could not be tested at all while constructing a 36-field bundle inside a
/// Floem scope was the price of calling it. The decision itself is now
/// `connection::label_of`, in core with its test.
fn connection_label(connections: RwSignal<Vec<Connection>>, conn_id: u64) -> String {
    connections.with_untracked(|list| schemaic_core::connection::label_of(list, conn_id))
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

/// May an apply in flight be stopped — from the preview's dialect, or from
/// there being no preview.
///
/// **`None` answers for itself.** The engine is what the *capability* is read
/// off, so with nothing open there is no engine to ask, and the honest answer is
/// "not cancellable" rather than whatever `SqlDialect::MySql` happens to say
/// today. That was the shape here, and it worked only by coincidence: MySQL's
/// answer is `false`, which is the conservative direction. MySQL 8's atomic DDL
/// is exactly the refinement `ddl_rolls_back_as_a_whole` exists to absorb, and
/// the day it lands the absent case would have started reporting "cancellable"
/// with nothing in the diff naming this modal.
fn exit_cancellable(dialect: Option<SqlDialect>) -> bool {
    dialect.is_some_and(schemaic_core::ddl::ddl_rolls_back_as_a_whole)
}

pub(crate) fn ddl_preview_overlay(
    d: DdlUi,
    conn: ConnUi,
    run_ddl: DdlFn,
    ddl_cancel: Rc<dyn Fn()>,
    open_query: Rc<dyn Fn(String, Option<String>)>,
) -> impl IntoView {
    // The connection list, for the footer's two live read-only reads — see
    // [`plan_read_only`].
    let conns = conn.connections;
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
    //
    // **The capability is defaulted, not the engine.** With no preview open
    // there is no engine to ask, and standing `SqlDialect::MySql` in for one
    // made this guard's correctness rest on a coincidence: `ddl_rolls_back_as_a
    // _whole(MySql)` happens to be `false`, which is the conservative answer
    // wanted for the absent case. MySQL 8's atomic DDL is exactly the kind of
    // refinement that predicate exists to absorb, and the day it lands the
    // no-preview case would start reporting "cancellable" with nothing in the
    // diff naming this modal. A constant in place of a capability is the rule's
    // second clause, and it leaves no comparison to grep for.
    let cancellable =
        move || exit_cancellable(d.preview.with_untracked(|p| p.as_ref().map(|p| p.dialect)));
    let cancel_apply = ddl_cancel.clone();
    // An `Rc` rather than a bare closure: it now holds a cancel action, so it is
    // no longer `Copy` and the three exits share one.
    let exit: Rc<dyn Fn()> = Rc::new(move || {
        let cancellable = cancellable();
        match exit_action(d.applying.get_untracked(), cancellable) {
            ExitAction::Close => close_preview(d),
            ExitAction::Cancel => (cancel_apply)(),
            ExitAction::Ignore => {}
        }
    });

    dyn_container(
        move || (d.preview.with(Option::is_some), d.applied.get()),
        move |(open, applied)| {
            if !open {
                return empty().into_any();
            }
            let exit = exit.clone();
            let run_ddl = run_ddl.clone();
            let Some(p) = d.preview.get_untracked() else {
                return empty().into_any();
            };
            // Read before the bundles are moved into the footer's closures.
            let title = preview_title(&connection_label(conn.connections, p.conn_id), &p);

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
                    omitted_block(p.omitted.clone()).style(|s| s.margin_top(theme::scaled(14.0))),
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
                                .max_width(theme::scaled(580.0))
                                .margin_top(theme::scaled(12.0))
                        })
                        .into_any(),
                },
            );

            // The script's own actions — neither of them answers the question the
            // footer is asking, so they sit recessed at the far left rather than
            // in the Back/Apply pair.
            let open_query_side = open_query.clone();
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
                    let open_query = open_query_side.clone();
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
                // **Read-only is in the key, not read in the builder.** A
                // `dyn_container` builder is not a tracking scope (floem 0.2), so
                // a flag read inside it is frozen at the rebuild that happened to
                // last run — which is how the enable term came to answer about
                // the connection as it was when the plan was stamped. It is the
                // *live* answer, through the same `plan_read_only` `apply` asks.
                move || {
                    (
                        d.applying.get(),
                        d.applied.get(),
                        d.preview
                            .get()
                            .is_some_and(|p| conns.with(|cs| plan_read_only(cs, &p))),
                    )
                },
                move |(busy, applied, read_only)| {
                    let run_ddl = run_ddl.clone();
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
                        // behind it, where Back would point at nowhere. Asked
                        // through `has_editor_behind`, so the word and
                        // `close_peers`' list are one definition — spelled here
                        // it named two of the nine and called the other seven
                        // Cancel, on a button that returns with the draft
                        // intact.
                        action_button(
                            if stoppable {
                                "Stop"
                            } else if has_editor_behind(d) {
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
                                && !read_only
                                && !p.statements.is_empty()
                                && p.withheld.is_empty(),
                            ring,
                            ACTION_TAB + 30,
                            move || apply(d, conn, run_ddl.clone()),
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
                // The live answer too — a style closure *is* a tracking scope, so
                // this one only ever needed the right expression. Flipping the
                // flag with the plan open now says so instead of leaving a dead
                // Apply unexplained.
                let blocked = d
                    .preview
                    .get()
                    .is_some_and(|p| conns.with(|cs| plan_read_only(cs, &p)));
                if blocked && !d.applied.get() {
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
        if d.preview.with(Option::is_some) {
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

    /// A plan on connection `id`, stamped with `stamped_read_only`.
    fn plan(id: u64, stamped_read_only: bool) -> DdlPreview {
        let cs = schemaic_core::ddl::ChangeSet {
            dialect: SqlDialect::MySql,
            flavour: Default::default(),
            schema: None,
            table: String::new(),
            changes: Vec::new(),
        };
        preview_of(id, "db", "orders", &cs, stamped_read_only)
    }

    fn conn(id: u64, read_only: bool) -> schemaic_core::connection::Connection {
        schemaic_core::connection::Connection {
            id,
            name: format!("conn {id}"),
            db_type: "MySQL".to_string(),
            host: "localhost".to_string(),
            port: 3306,
            user: "root".to_string(),
            password: String::new(),
            file: String::new(),
            database: String::new(),
            ssh: Default::default(),
            tls: Default::default(),
            color: None,
            prominent_color: false,
            read_only,
            environment: Default::default(),
            ai_data: None,
        }
    }

    /// **The connection as it is now, not as it was when the plan was stamped.**
    ///
    /// `apply` already asked the live flag — flipping a connection read-only
    /// from the status bar with a `DROP DATABASE` plan on screen is exactly
    /// where a stale answer costs most — but the footer's *enable* term and the
    /// "This connection is read-only" note beside it both read the stamp. So the
    /// flag could be flipped with the plan open and Apply stayed lit, said
    /// nothing, and did nothing when pressed, while the note that exists to
    /// explain a dead Apply stayed hidden.
    #[test]
    fn a_connection_flipped_read_only_refuses_a_plan_stamped_writable() {
        let p = plan(7, false);
        assert!(!super::plan_read_only(&[conn(7, false)], &p), "the premise");
        assert!(
            super::plan_read_only(&[conn(7, true)], &p),
            "the flag was flipped after the plan was stamped and nothing saw it"
        );
    }

    /// And the stamp is still a term in its own right: a plan built while the
    /// connection was read-only says so for its whole life, which is what the
    /// note beside the footer is about.
    #[test]
    fn a_plan_stamped_read_only_stays_refused() {
        let p = plan(7, true);
        assert!(super::plan_read_only(&[conn(7, false)], &p));
        assert!(super::plan_read_only(&[conn(7, true)], &p));
    }

    /// **The id is half the question.** A tab keeps the connection it was opened
    /// on, so the plan's own connection is the one to ask about — another
    /// connection going read-only must not refuse it, and the plan's connection
    /// having been deleted must not silently make it writable either.
    #[test]
    fn only_the_plans_own_connection_is_asked() {
        let p = plan(7, false);
        assert!(
            !super::plan_read_only(&[conn(9, true), conn(7, false)], &p),
            "another connection's flag refused this plan"
        );
        // Gone from the list: nothing says read-only, and the stamp is what is
        // left to answer with.
        assert!(!super::plan_read_only(&[conn(9, true)], &p));
        assert!(super::plan_read_only(&[], &plan(7, true)));
    }

    /// **With no preview there is no engine to ask**, and the exit guard must
    /// say so itself rather than borrowing an invented one's answer.
    ///
    /// The second assertion is what makes the first one mean anything: a test
    /// written only against the outcome would be green against the defect,
    /// because `ddl_rolls_back_as_a_whole(MySql)` is `false` today and that is
    /// the answer the absent case wants. **This cannot fail against the shape it
    /// replaces while that stays true** — the day it changes is the day the two
    /// disagree, and it is written to be red then rather than to be red now.
    #[test]
    fn an_exit_with_no_preview_is_not_cancellable_on_anyones_behalf() {
        assert!(!super::exit_cancellable(None));
        for dialect in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            assert_eq!(
                super::exit_cancellable(Some(dialect)),
                schemaic_core::ddl::ddl_rolls_back_as_a_whole(dialect),
                "a preview that is open reads the capability off its own engine ({dialect:?})"
            );
        }
        // And the outcome the guard produces from it: nothing is applying, so
        // the exit closes either way — which is why the line above is the check
        // that matters.
        assert_eq!(
            super::exit_action(false, super::exit_cancellable(None)),
            super::ExitAction::Close
        );
    }

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
            sibling_triggers: Vec::new(),
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
            resetting: None,
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

    /// **The SQL box shows what Apply sends, and it did not.** `open_preview`
    /// seeded the box from `export_script`, which substitutes
    /// `PUT-THE-PASSWORD-HERE` for the secret and prepends a three-line header
    /// about *a copy* — so the last confirmation surface before an irreversible
    /// statement displayed a statement that would not run, and a user reading
    /// the modal at its word would conclude the account was about to be created
    /// with the literal placeholder as its password.
    ///
    /// It matters most for the case that motivated it: a password containing
    /// `*` reaches `CREATE USER … IDENTIFIED BY` mangled (the masked field
    /// reconstructs it from a character diff, and `*` is the mask), and while
    /// the box redacted it there was **no surface anywhere** on which the real
    /// emitted secret appeared — the field re-masks to the right length and the
    /// preview substituted the placeholder. Showing `statements` puts it on
    /// screen exactly once, in a modal, and nowhere durable: Copy and "Open in
    /// editor" still hand over `p.script`, which is the distinction
    /// `export_script` was written to draw.
    ///
    /// Through `open_preview`, not over the two `ChangeSet` methods: the defect
    /// was entirely in *which of them the box reads*, so a test of either one
    /// alone passes against the bug.
    #[test]
    fn the_sql_box_shows_the_statement_that_runs() {
        let scope = Scope::new();
        let d = ddl_ui(scope);
        let cs = schemaic_core::ddl::ChangeSet {
            table: String::new(),
            schema: None,
            dialect: SqlDialect::MySql,
            flavour: Default::default(),
            changes: vec![schemaic_core::ddl::Change::CreateAccount(Box::new(
                schemaic_core::users::AccountDraft {
                    name: "app".into(),
                    host: "%".into(),
                    password: "hunter2".into(),
                    ..Default::default()
                },
            ))],
        };
        let p = preview_of(1, "db", "app@%", &cs, false);
        // The premise: the two really do differ, or the test proves nothing.
        assert!(
            p.script.contains("PUT-THE-PASSWORD-HERE") && !p.script.contains("hunter2"),
            "the copy is redacted: {}",
            p.script
        );
        open_preview(d, p);
        let shown = d.sql.get_untracked();
        assert!(
            shown.contains("hunter2"),
            "the box must show what Apply sends: {shown}"
        );
        assert!(
            !shown.contains("PUT-THE-PASSWORD-HERE"),
            "…and not the copy's placeholder: {shown}"
        );
        // Closing still drops it — the box now holds the real secret, so that
        // clear is load-bearing rather than defence in depth.
        close_preview(d);
        assert!(d.sql.get_untracked().is_empty());
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
            sibling_triggers: Vec::new(),
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

    /// **The same invariant read from the other end**, now for two consumers.
    /// `close_editors` must clear every editor; `ddl_editors_up` must *see*
    /// every editor — it is what gives the whole DDL overlay group its box, and
    /// a modal missing from it opens into zero by zero and paints nothing — and
    /// [`has_editor_behind`] must see every editor too, because it is what
    /// decides whether the preview's exit button says **Back** or **Cancel**.
    ///
    /// The event editor shipped absent from `ddl_editors_up`, and the exit label
    /// shipped naming two of the nine, which is why this test exists beside the
    /// one above rather than being folded into it: three lists, one rule, and a
    /// new editor has to be added to all of them.
    ///
    /// Each target is raised **alone**, so a list that happens to contain some
    /// other signal can't carry a missing one.
    #[test]
    fn every_editor_reaches_the_three_lists_that_must_know_about_it() {
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
                    resetting: None,
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
                    sibling_triggers: Vec::new(),
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
            assert!(
                has_editor_behind(d),
                "{name} is open and the preview's exit button would say Cancel — \
                 but Cancel there returns to {name} with its draft intact"
            );
            close_editors(d);
            // `close_editors` deliberately leaves the trigger form standing when
            // the plan came from the routine editor above it, which is not the
            // case here — nothing raised a routine.
            d.trigger.set(None);
            assert!(!up(), "{name} closed and the group still says something is");
            assert!(
                !has_editor_behind(d),
                "{name} closed and the exit button would still offer to go Back to it"
            );
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

    /// **The two fields a `SchemaPlan` adds over a `ChangeSet`, exercised.**
    /// Every other test here builds a plan whose `unsupported()` is empty and
    /// whose `cycles` is false, so `assert_eq!(p.withheld, plan.unsupported())`
    /// compares `[] == []` and the risk heading is never asked the question the
    /// flag exists to answer.
    #[test]
    fn a_cycle_in_the_plan_reaches_the_previews_risk_block() {
        let fk = |to: &str| schemaic_core::schema::ForeignKeyInfo {
            name: format!("fk_{to}"),
            columns: vec!["other".to_string()],
            ref_table: to.to_string(),
            ref_columns: vec!["id".to_string()],
            ..Default::default()
        };
        let mut right = one_table("a", &["id", "other"]);
        right.tables[0].foreign_keys = vec![fk("b")];
        let mut b = one_table("b", &["id", "other"]).tables.remove(0);
        b.foreign_keys = vec![fk("a")];
        right.tables.push(b);

        let plan = compare_plan(Default::default(), right);
        assert!(plan.cycles, "two mutually referencing creates");
        let p = preview_of_plan(1, "shop", "2 objects", &plan, false);
        // A warning, not a refusal: the statements are all there.
        assert!(p.withheld.is_empty());
        assert!(!p.statements.is_empty());
        assert_eq!(p.risk_heading, "This can't be undone");
        assert!(
            p.destructive.iter().any(|d| d.contains("cycle")),
            "{:?}",
            p.destructive
        );
    }

    /// **A withheld plan reaches the block that refuses Apply**, with the line
    /// naming the object whose tick has to be cleared. Nothing here had ever
    /// been built with a non-empty `unsupported()`.
    #[test]
    fn a_withheld_plan_previews_as_withheld_and_names_its_object() {
        let lossy = || schemaic_core::schema::IndexInfo {
            name: "ix_expr".to_string(),
            lossy: true,
            ..Default::default()
        };
        let mut left = one_table("city", &["id"]);
        left.tables[0].indexes = vec![lossy()];
        let mut right = one_table("city", &["id", "name"]);
        right.tables[0].indexes = vec![lossy()];
        let plan = schemaic_core::compare::SchemaComparison::of(&left, &right, SqlDialect::Sqlite)
            .plan(|_| true);

        let p = preview_of_plan(1, "shop", "1 object", &plan, false);
        assert_eq!(p.withheld, plan.unsupported());
        assert!(!p.withheld.is_empty(), "a lossy index cannot be rebuilt");
        assert!(
            p.withheld.iter().all(|w| w.starts_with("city — ")),
            "{:?}",
            p.withheld
        );
    }

    /// **What the plan could not carry reaches the modal separately from what
    /// it cannot express.** A differing MySQL trigger is not in the plan and
    /// never can be, so `withheld` stays empty and Apply stays live — and the
    /// omitted block is the only thing between the user and an "Applied N
    /// statements to 1 object" over a two-difference comparison.
    #[test]
    fn what_the_plan_left_out_is_disclosed_without_refusing_apply() {
        let left = one_table("city", &["id"]);
        let mut right = one_table("city", &["id", "name"]);
        right.tables[0].triggers = vec![schemaic_core::schema::TriggerInfo {
            name: "t_ins".to_string(),
            table: "city".to_string(),
            action: schemaic_core::schema::TriggerAction::Body("BEGIN SET @a = 1; END".to_string()),
            ..Default::default()
        }];
        let plan = compare_plan(left, right);

        let p = preview_of_plan(1, "shop", "1 object", &plan, false);
        assert_eq!(p.omitted, plan.omitted);
        assert!(
            p.omitted.iter().any(|o| o.contains("t_ins")),
            "{:?}",
            p.omitted
        );
        // Not `withheld`: what *is* in the plan is complete, and there is no
        // tick to clear — so Apply is not refused.
        assert!(p.withheld.is_empty());
        assert!(!p.statements.is_empty());
    }

    /// A designer edit is about one object; there is nothing it can leave out.
    #[test]
    fn a_single_objects_preview_has_nothing_omitted() {
        let cs = schemaic_core::ddl::single(
            "orders",
            None,
            SqlDialect::MySql,
            schemaic_core::ddl::Change::TruncateTable,
        );
        assert!(
            preview_of(1, "shop", "orders", &cs, false)
                .omitted
                .is_empty()
        );
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
/// **Which connection a plan with no table is built against.**
#[cfg(test)]
mod plan_target_tests {
    use super::*;
    use schemaic_core::ddl::Change;

    fn on(conn_id: u64, dialect: SqlDialect, read_only: bool) -> PlanTarget {
        PlanTarget {
            conn_id,
            database: String::new(),
            dialect,
            read_only,
        }
    }

    /// The plan carries the **target's** connection, dialect and read-only flag,
    /// not the switcher's.
    ///
    /// This one cannot be made to fail against the unfixed tree: there was no
    /// function to call — `preview_container` read `edit_ctx(ui)` inline and had
    /// no target to be given one. That is what the source gate below is for; it
    /// asserts the shape of the fix rather than its output, and it *does* fail.
    #[test]
    fn a_container_plan_is_built_against_the_target_it_was_raised_on() {
        let p = container_preview(
            &on(1, SqlDialect::MySql, false),
            "staging",
            Change::CreateDatabase(Box::new(schemaic_core::ddl::DatabaseDraft {
                name: "staging".into(),
                charset: Some("utf8mb4".into()),
                ..Default::default()
            })),
        );
        assert_eq!(p.conn_id, 1);
        assert_eq!(p.dialect, SqlDialect::MySql);
        assert!(!p.read_only);
        // The charset the MySQL form collected is in the statement, which is the
        // user-visible half of building at the wrong dialect: PostgreSQL has no
        // clause for it and the plan would have gone out without it.
        assert!(
            p.statements.iter().any(|s| s.contains("utf8mb4")),
            "{:?}",
            p.statements
        );
    }

    /// And the read-only flag is the target's too — the switcher's would decide
    /// whether Apply is offered for a connection the form was never on.
    #[test]
    fn a_container_plan_carries_the_targets_read_only_flag() {
        let p = container_preview(
            &on(4, SqlDialect::Postgres, true),
            "shop",
            Change::DropSchema {
                name: "shop".into(),
            },
        );
        assert!(p.read_only);
        assert_eq!(p.conn_id, 4);
    }

    /// **The gate.** `preview_container` and `container_preview` must not read
    /// the live switcher — the whole finding was that the first one did, while
    /// `preview_account` one function below took a target precisely to stop it
    /// and said so in its doc.
    ///
    /// Scoped to those two functions rather than to the file: `preview_change`
    /// and `preview_proposal` are about the connection the user is looking at
    /// and read `edit_ctx` correctly, so a file-wide gate would either fail on
    /// them or be written loosely enough to pass on anything.
    #[test]
    fn the_container_preview_does_not_read_the_live_connection() {
        let src =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ddl_preview.rs"))
                .expect("this file");
        let body = crate::source_gate::production_code(&src);
        for name in ["fn preview_container(", "fn container_preview("] {
            let at = body.find(name).unwrap_or_else(|| panic!("{name} is gone"));
            let end = crate::source_gate::item_end(&body, at)
                .unwrap_or_else(|| panic!("{name} has no end"));
            assert!(
                !body[at..end].contains("edit_ctx"),
                "{name} reads the live connection switcher; the plan has to be \
                 built against the `PlanTarget` it was raised on"
            );
        }
    }
}
