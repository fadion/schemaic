//! The scheduled-event editor — one modal for a MySQL `CREATE EVENT`.
//!
//! Modelled directly on [`crate::routine_editor`], which is the closest thing in
//! the app: an object with a body, a definer, a comment and a pile of session
//! state the statement has no clause for. Four things differ, and all four are
//! consequences of one fact — **MySQL alters an event in place**.
//!
//! * **No drop-and-create, anywhere.** `ALTER EVENT` reaches the schedule, the
//!   status, the comment, the definer, the name *and* the body, so every edit
//!   here is one statement restating only what changed. That is why a rename
//!   can't destroy the original the way a MySQL routine's can, and why this
//!   module has no `name_clash` warning about it.
//! * **The lazy `SHOW CREATE` is still not optional.**
//!   `information_schema.EVENTS.EVENT_DEFINITION` resolves the body's escapes,
//!   so an edit restated from it is refused over a quote nobody typed. Read when
//!   this opens, and applied to **both** sides of the diff so an event doesn't
//!   open already-changed.
//! * **The schedule is two shapes, not one nullable one.** `AT` takes a
//!   timestamp and nothing else; `EVERY` takes an interval and optional bounds.
//!   The control that switches between them is the *only* thing in this form
//!   that rebuilds part of itself, and it is keyed on the shape alone — never on
//!   the draft, which would tear a field down mid-keystroke.
//! * **Every timestamp field holds SQL, not a value.** `'2026-01-01 03:00:00'`
//!   arrives quoted from the catalogue and `CURRENT_TIMESTAMP + INTERVAL 1 HOUR`
//!   is as valid, which is the whole reason the model doesn't parse them. The
//!   fields are monospaced and their placeholders show the quotes.
//!
//! Everything else follows the routine editor's rules: the form is built once
//! per open, the footer and the actions are what re-render, and a new event's
//! namespace is inherited from wherever the modal was opened rather than chosen.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::ddl::{self, EventDraft};
use schemaic_core::schema::{EVENT_INTERVAL_UNITS, EventInfo, EventSchedule, EventStatus};

use crate::settings::focusable_toggle_row;
use crate::table_designer::{edit_ctx, focusable_owned_dropdown};
use crate::trigger_editor::unique_name;
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, action_gap, focus_root_with_ring, form_gap,
    form_section, form_setting, modal_footer_split, modal_h, modal_pad_h, modal_title_owned,
    modal_w, panel_style,
};
use crate::{
    EventSrcDoneFn, EventSrcRequest, EventTarget, FieldCfg, Ui, ddl_preview, edit_field,
    object_location, theme,
};

/// Matches the routine editor's: the two are siblings in every way that shows,
/// and a panel that changed size between them would read as a different app.
fn panel_w() -> f64 {
    modal_w(900.0)
}
const PANEL_H: f64 = 620.0;
fn field_w() -> f64 {
    theme::scaled(260.0)
}
/// The body box's height before it scrolls.
const BODY_ROWS: usize = 12;

/// The two labels the schedule switch offers, and the order it offers them in.
/// Recurring first, because it is what the overwhelming majority of events are
/// and what a blank draft starts as.
const SCHED_EVERY: &str = "Repeating";
const SCHED_AT: &str = "Once";

// ── opening ──────────────────────────────────────────────────────────────────

fn open(ui: &Ui, target: EventTarget, draft: EventDraft) {
    let d = ui.ddl;
    // A new editing session: any lazy fetch still in flight for the last one is
    // now for the wrong target and must not land.
    d.session.update(|g| *g += 1);
    // The Body field reads this rather than seeding itself from the draft, so
    // `fetch_source` can correct the text without rebuilding the modal.
    d.event_body.set(draft.info.body.clone());
    d.event_draft.set(draft);
    // Cleared here rather than on close, so the one path that raises them —
    // `fetch_source`, at the end of this function — is the only one that can
    // leave them raised.
    d.event_source_pending.set(false);
    d.event_body_stale.set(false);
    d.view_rows.set(BODY_ROWS);
    d.error.set(None);
    d.preview.set(None);
    // Each overlay knows only its own flag, so two open would paint two panels.
    // One list, in `ddl_preview` — five hand-written copies had already drifted.
    ddl_preview::close_peers(d, false);
    d.event.set(Some(target));
    fetch_source(ui);
}

/// Open the editor on an existing event.
pub(crate) fn open_for_event(ui: &Ui, database: &str, e: &EventInfo) {
    let ctx = edit_ctx(ui);
    open(
        ui,
        EventTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            dialect: ctx.dialect,
            current: Some(e.clone()),
            read_only: ctx.read_only,
        },
        EventDraft::from_info(e),
    );
}

/// Open the editor on a blank draft — Create event.
pub(crate) fn open_for_new(ui: &Ui, database: &str, schema: Option<&str>) {
    let ctx = edit_ctx(ui);
    open(
        ui,
        EventTarget {
            conn_id: ctx.conn_id,
            database: database.to_string(),
            dialect: ctx.dialect,
            current: None,
            read_only: ctx.read_only,
        },
        EventDraft::blank(
            unique_name(&taken_names(ui, database, schema), "new_event"),
            schema.map(str::to_string),
        ),
    );
}

/// The names already taken in this database, so a new event doesn't propose one
/// of them and a rename can be refused before it round-trips. Read off the
/// schema the tree is showing, so the proposal agrees with what the user can
/// see.
fn taken_names(ui: &Ui, database: &str, schema: Option<&str>) -> Vec<String> {
    ui.schema.db_nodes.with_untracked(|nodes| {
        let Some(node) = nodes.iter().find(|n| n.database == database) else {
            return Vec::new();
        };
        let crate::SchemaState::Loaded(s) = node.schema.get_untracked() else {
            return Vec::new();
        };
        s.events
            .iter()
            .filter(|e| e.schema.as_deref() == schema)
            .map(|e| e.name.clone())
            .collect()
    })
}

/// Read the event's body **as written**, on the one engine whose catalogue
/// resolves the escapes out of it.
///
/// The routine editor's `fetch_source`, with the same three rules and for the
/// same reasons: applied to `target.current` **and** the draft so the event
/// doesn't open already-changed; the body left alone if the user has already
/// typed; the session state — which nothing in this app edits, and which now
/// includes the `time_zone` the schedule is read in — patched either way.
///
/// The session guard is what makes a slow reply safe: the user can close this
/// modal and open another event while the read is in flight.
fn fetch_source(ui: &Ui) {
    let d = ui.ddl;
    let Some(target) = d.event.get_untracked() else {
        return;
    };
    let Some(current) = target.current.clone() else {
        // An event that doesn't exist yet has no source to read.
        return;
    };
    let session = d.session.get_untracked();
    let name = current.name.clone();
    d.event_source_pending.set(true);
    let done: EventSrcDoneFn = Rc::new(move |asked: String, src| {
        // A late reply for an event this modal is no longer editing. The flag
        // belongs to *that* session and was cleared with it.
        if d.session.get_untracked() != session {
            return;
        }
        if asked != name {
            return;
        }
        // Cleared even for a failed read: an account that may not see the
        // definition always gets `information_schema`'s body, so waiting past
        // this point would disable Preview for good.
        d.event_source_pending.set(false);
        let Some(src) = src else { return };
        let opened_with = d.event.with_untracked(|t| {
            t.as_ref()
                .and_then(|t| t.current.as_ref())
                .map(|c| c.body.clone())
                .unwrap_or_default()
        });
        // Read **before** `current` is corrected below — the body the editor
        // opened with is the only thing that can say whether the user has typed
        // since. `routine_source_outcome` is the shared decision; nothing about
        // it is specific to a routine.
        let outcome = d.event_draft.with_untracked(|dr| {
            ddl::routine_source_outcome(&opened_with, src.body.as_deref(), &dr.info.body)
        });
        d.event_draft.update(|dr| {
            src.apply_session_to(&mut dr.info);
            if outcome == ddl::SourceOutcome::Adopted {
                src.apply_body_to(&mut dr.info);
            }
        });
        // …and onto the screen, through the Body field's own signal, which
        // `edit_field` reconciles in place — caret and all.
        if outcome == ddl::SourceOutcome::Adopted
            && let Some(body) = src.body.clone()
        {
            d.event_body.set(body);
        }
        d.event_body_stale.set(outcome == ddl::SourceOutcome::Stale);
        // `current` — the left-hand side of every diff — unconditionally. The
        // overlay's key is a memo, so this patch no longer rebuilds the modal.
        d.event.update(|t| {
            if let Some(t) = t.as_mut()
                && let Some(cur) = t.current.as_mut()
            {
                src.apply_to(cur);
            }
        });
    });
    (ui.schema_actions.event_source.clone())(
        EventSrcRequest {
            conn_id: target.conn_id,
            database: target.database.clone(),
            name: current.name.clone(),
        },
        done,
    );
}

// ── bound controls ───────────────────────────────────────────────────────────

/// A field bound to one place in the event draft. Same contract as the routine
/// editor's: seeded once on build, and the effect writes back only on a genuine
/// change, so a rebuild can't read as an edit.
fn bound_field(
    ui: &Ui,
    initial: String,
    cfg: FieldCfg,
    apply: impl Fn(&mut EventDraft, &str) + 'static,
) -> AnyView {
    bound_field_on(ui, floem::reactive::create_rw_signal(initial), cfg, apply)
}

/// [`bound_field`] over a signal the **caller** owns — the Body, whose text a
/// late `SHOW CREATE` reply has to correct after the form is built.
fn bound_field_on(
    ui: &Ui,
    sig: RwSignal<String>,
    cfg: FieldCfg,
    apply: impl Fn(&mut EventDraft, &str) + 'static,
) -> AnyView {
    let draft = ui.ddl.event_draft;
    create_effect(move |prev: Option<String>| {
        let v = sig.get();
        if prev.is_some_and(|p| p != v) {
            draft.update(|d| apply(d, &v));
        }
        v
    });
    edit_field(sig, cfg).into_any()
}

fn bound_toggle(
    ui: &Ui,
    label: &'static str,
    hint: &'static str,
    initial: bool,
    ring: FocusRing,
    tabindex: u32,
    apply: impl Fn(&mut EventDraft, bool) + 'static,
) -> AnyView {
    let draft = ui.ddl.event_draft;
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

/// Rewrite the draft's schedule through a closure that sees the current one.
///
/// One helper because every schedule control does the same two-step — read the
/// shape, put a modified one back — and doing it inline in five places is how
/// one of them ends up writing the wrong arm.
fn edit_schedule(ui: &Ui, f: impl Fn(&mut EventSchedule) + 'static) {
    ui.ddl.event_draft.update(|d| f(&mut d.info.schedule));
}

// ── the form ─────────────────────────────────────────────────────────────────

/// Tab indices. One block per control, and the two *variable-length* blocks —
/// the schedule sub-form and the options list — get a decade of their own with
/// nothing above them until the next hundred.
///
/// **The spacing is the whole point, and it was wrong first.** `TAB_SCHED + 30`
/// was `60`, which was also `TAB_BODY`: two controls registered at one index,
/// and `FocusRing::register` inserts after an equal one, so the walk order fell
/// out of which happened to build first rather than out of the layout — Tab went
/// Starts → Body → Ends, back up the form. The schedule block is the one that
/// grows (a shape with more bounds adds another `+ 10`), so it is the one that
/// needs the room.
const TAB_NAME: u32 = 10;
const TAB_SHAPE: u32 = 20;
/// `30`–`99`: the schedule sub-form, which is per-shape and rebuilt.
const TAB_SCHED: u32 = 30;
const TAB_BODY: u32 = 100;
/// `200`–`999`: the options list, below [`crate::widgets::VALUE_TAB`].
const TAB_OPT: u32 = 200;

/// The `EVERY`/`AT` switch, plus whichever set of fields that shape needs.
///
/// **The one part of this form that rebuilds itself.** It is keyed on the shape
/// alone — a `bool` in this view's own scope — and never on the draft: a
/// draft-keyed field is torn down mid-keystroke, which is the bug the routine
/// editor's `open_key` memo exists for. Switching the shape is a click on a
/// dropdown, so no caret is lost when this one does rebuild.
fn schedule_form(ui: Ui, ring: FocusRing) -> AnyView {
    let d = ui.ddl.event_draft;
    let recurring =
        floem::reactive::create_rw_signal(!d.with_untracked(|dr| dr.info.schedule.is_one_shot()));

    let shape = {
        let ui = ui.clone();
        let sig = floem::reactive::create_rw_signal(
            if recurring.get_untracked() {
                SCHED_EVERY
            } else {
                SCHED_AT
            }
            .to_string(),
        );
        form_setting(
            "Runs",
            focusable_owned_dropdown(
                move || sig.get(),
                vec![SCHED_EVERY.to_string(), SCHED_AT.to_string()],
                field_w(),
                ring.clone(),
                TAB_SHAPE,
                move |label: String| {
                    if sig.get_untracked() == label {
                        return;
                    }
                    sig.set(label.clone());
                    let want_every = label == SCHED_EVERY;
                    // **The draft first, the flag second.** The fields below are
                    // rebuilt by the flag and seed themselves from the draft, so
                    // flipping the flag first would seed them from the shape
                    // that is being replaced.
                    edit_schedule(&ui, move |s| {
                        let is_every = !s.is_one_shot();
                        if is_every == want_every {
                            return;
                        }
                        // Nothing carries across: an interval and a timestamp
                        // are not the same value written two ways, and inventing
                        // one from the other would put a schedule in the draft
                        // that the user never chose. Each shape starts from its
                        // own default, which for `EVERY` is the one a blank
                        // event has.
                        *s = if want_every {
                            EventSchedule::default()
                        } else {
                            EventSchedule::At(String::new())
                        };
                    });
                    recurring.set(want_every);
                },
            ),
        )
        .into_any()
    };

    let fields = dyn_container(
        move || recurring.get(),
        move |every| {
            let ui = ui.clone();
            let ring = ring.clone();
            if !every {
                let at = d.with_untracked(|dr| match &dr.info.schedule {
                    EventSchedule::At(a) => a.clone(),
                    EventSchedule::Every { .. } => String::new(),
                });
                return form_setting(
                    "At",
                    bound_field(
                        &ui,
                        at,
                        FieldCfg {
                            // Quoted, because the field holds **SQL**: the
                            // catalogue's timestamp arrives as a literal and an
                            // expression is as welcome.
                            placeholder: "'2026-01-01 03:00:00'",
                            mono: true,
                            focus: Some((ring, TAB_SCHED)),
                            ..Default::default()
                        },
                        |d, v| {
                            let v = v.trim().to_string();
                            d.info.schedule = EventSchedule::At(v);
                        },
                    )
                    .style(move |s| s.width(field_w() * 1.6)),
                )
                .into_any();
            }

            let (value, unit, starts, ends) = d.with_untracked(|dr| match &dr.info.schedule {
                EventSchedule::Every {
                    value,
                    unit,
                    starts,
                    ends,
                } => (
                    value.clone(),
                    unit.clone(),
                    starts.clone().unwrap_or_default(),
                    ends.clone().unwrap_or_default(),
                ),
                EventSchedule::At(_) => (
                    "1".to_string(),
                    "DAY".to_string(),
                    String::new(),
                    String::new(),
                ),
            });

            // The units Schemaic proposes, **plus whatever this event already
            // uses** — the call `routine_editor`'s Language dropdown makes, and
            // for the same reason: a list that didn't carry this event's own
            // unit would show the right label over options that silently retime
            // it the moment the control is touched.
            let mut units: Vec<String> =
                EVENT_INTERVAL_UNITS.iter().map(|u| u.to_string()).collect();
            if !unit.trim().is_empty() && !units.iter().any(|u| u.eq_ignore_ascii_case(&unit)) {
                units.push(unit.clone());
            }
            let unit_sig = floem::reactive::create_rw_signal(unit);
            let unit_ui = ui.clone();
            let interval = form_setting(
                "Every",
                h_stack((
                    bound_field(
                        &ui,
                        value,
                        FieldCfg {
                            placeholder: "1",
                            mono: true,
                            focus: Some((ring.clone(), TAB_SCHED)),
                            ..Default::default()
                        },
                        |d, v| {
                            let v = v.trim().to_string();
                            if let EventSchedule::Every { value, .. } = &mut d.info.schedule {
                                *value = v;
                            }
                        },
                    )
                    .style(|s| s.width(theme::scaled(80.0))),
                    focusable_owned_dropdown(
                        move || unit_sig.get(),
                        units,
                        field_w() * 0.7,
                        ring.clone(),
                        TAB_SCHED + 10,
                        move |v: String| {
                            if unit_sig.get_untracked() == v {
                                return;
                            }
                            unit_sig.set(v.clone());
                            edit_schedule(&unit_ui, move |s| {
                                if let EventSchedule::Every { unit, .. } = s {
                                    unit.clone_from(&v);
                                }
                            });
                        },
                    ),
                ))
                .style(|s| s.flex_row().items_center().gap(8.0)),
            );

            let bound = |label: &'static str,
                         initial: String,
                         tab: u32,
                         set: fn(&mut EventSchedule, Option<String>)| {
                form_setting(
                    label,
                    bound_field(
                        &ui,
                        initial,
                        FieldCfg {
                            placeholder: "'2026-01-01 03:00:00'",
                            mono: true,
                            focus: Some((ring.clone(), tab)),
                            ..Default::default()
                        },
                        move |d, v| {
                            let v = v.trim();
                            set(&mut d.info.schedule, (!v.is_empty()).then(|| v.to_string()));
                        },
                    )
                    .style(move |s| s.width(field_w() * 1.6)),
                )
            };

            v_stack((
                interval,
                bound("Starts", starts, TAB_SCHED + 20, |s, v| {
                    if let EventSchedule::Every { starts, .. } = s {
                        *starts = v;
                    }
                }),
                bound("Ends", ends, TAB_SCHED + 30, |s, v| {
                    if let EventSchedule::Every { ends, .. } = s {
                        *ends = v;
                    }
                }),
            ))
            .style(|s| s.flex_col().gap(form_gap()).width_full())
            .into_any()
        },
    )
    .style(|s| s.flex_col().width_full());

    v_stack((shape, fields))
        .style(|s| s.flex_col().gap(form_gap()).width_full())
        .into_any()
}

fn event_form(ui: Ui, ring: FocusRing) -> AnyView {
    let d = ui.ddl.event_draft;
    let draft = d.get_untracked();

    let name = form_setting(
        "Name",
        bound_field(
            &ui,
            draft.info.name.clone(),
            FieldCfg {
                placeholder: "nightly_purge",
                focus: Some((ring.clone(), TAB_NAME)),
                ..Default::default()
            },
            |d, v| d.info.name = v.trim().to_string(),
        )
        .style(move |s| s.width(field_w())),
    );

    let body = form_setting(
        "Body",
        bound_field_on(
            &ui,
            ui.ddl.event_body,
            FieldCfg {
                placeholder: "DELETE FROM sessions WHERE expires_at < NOW()",
                mono: true,
                multiline: true,
                max_rows: Some(ui.ddl.view_rows),
                // Logical lines, so the box hugs its content on the first frame
                // instead of guessing from a width that hasn't settled.
                no_wrap: true,
                focus: Some((ring.clone(), TAB_BODY)),
                // It's a statement body: Tab indents. Escape leaves.
                tab_indents: true,
                ..Default::default()
            },
            |d, v| d.info.body = v.to_string(),
        )
        .style(|s| s.width_full()),
    );

    // ── options ──────────────────────────────────────────────────────────
    //
    // Every one of these is *carried*: `ALTER EVENT` restates only what the
    // plan names, so a field the form didn't offer is one nobody could fix
    // without dropping the event and writing it again.
    let mut options: Vec<AnyView> = Vec::new();

    // The status. Two entries, plus the replica state when that is what the
    // event is already in — offering `DISABLE ON SLAVE` as a free choice would
    // be offering a keyword MySQL 8.4 has removed, while hiding it from an event
    // that has it would show "Enabled" over an event that isn't.
    {
        let current = draft.info.status;
        let mut choices = vec![EventStatus::Enabled, EventStatus::Disabled];
        if !choices.contains(&current) {
            choices.push(current);
        }
        let labels: Vec<String> = choices.iter().map(|s| s.label().to_string()).collect();
        let sig = floem::reactive::create_rw_signal(current.label().to_string());
        options.push(
            form_setting(
                "Status",
                focusable_owned_dropdown(
                    move || sig.get(),
                    labels,
                    field_w(),
                    ring.clone(),
                    TAB_OPT,
                    move |label: String| {
                        if sig.get_untracked() == label {
                            return;
                        }
                        sig.set(label.clone());
                        if let Some(v) = choices.iter().copied().find(|s| s.label() == label) {
                            d.update(|dr| dr.info.status = v);
                        }
                    },
                ),
            )
            .into_any(),
        );
    }

    options.push(bound_toggle(
        &ui,
        "Preserve after it completes",
        "ON COMPLETION PRESERVE. Off is MySQL's default and means the server deletes \
         the event once its last run is past — which for a one-off is immediately \
         after it runs.",
        draft.info.preserve,
        ring.clone(),
        TAB_OPT + 10,
        |d, v| d.info.preserve = v,
    ));

    // Carried rather than offered as a free choice would be honest either way,
    // and a field is the honest one: an event has no caller, so this account's
    // rights are the only rights its body ever runs with.
    options.push(
        form_setting(
            "Definer",
            bound_field(
                &ui,
                draft.info.definer.clone().unwrap_or_default(),
                FieldCfg {
                    placeholder: "root@localhost",
                    mono: true,
                    focus: Some((ring.clone(), TAB_OPT + 20)),
                    ..Default::default()
                },
                |d, v| {
                    let v = v.trim();
                    d.info.definer = (!v.is_empty()).then(|| v.to_string());
                },
            )
            .style(move |s| s.width(field_w())),
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
                    focus: Some((ring.clone(), TAB_OPT + 30)),
                    ..Default::default()
                },
                |d, v| {
                    let v = v.trim();
                    d.info.comment = (!v.is_empty()).then(|| v.to_string());
                },
            )
            .style(move |s| s.width(field_w() * 1.6)),
        )
        .into_any(),
    );

    let mut rows: Vec<AnyView> = vec![form_section("Event").into_any(), name.into_any()];
    rows.push(
        form_section("Schedule")
            .style(|s| s.margin_top(4.0))
            .into_any(),
    );
    rows.push(schedule_form(ui.clone(), ring.clone()));
    rows.push(form_section("Body").style(|s| s.margin_top(4.0)).into_any());
    rows.push(body.into_any());
    rows.push(
        form_section("Options")
            .style(|s| s.margin_top(4.0))
            .into_any(),
    );
    rows.extend(options);
    v_stack_from_iter(rows)
        .style(|s| s.flex_col().gap(form_gap()).width_full())
        .into_any()
}

// ── the modal ────────────────────────────────────────────────────────────────

/// The change set the draft currently describes — the same call the preview
/// emits from, so the footer's count can't disagree with the SQL.
fn change_set(target: &EventTarget, draft: &EventDraft) -> ddl::ChangeSet {
    match &target.current {
        Some(cur) => ddl::diff_event(cur, draft, target.dialect),
        None => ddl::create_event(draft, target.dialect),
    }
}

/// The event editor. Absolutely positioned over the workspace when
/// `ui.ddl.event` is `Some`.
pub(crate) fn event_editor_overlay(ui: Ui) -> impl IntoView {
    let d = ui.ddl;
    let close = move || d.event.set(None);

    // **A memo, not the raw pair** — `overlay_open_key` carries the reason:
    // `fetch_source` patches `d.event` to correct `current`, and reading that
    // signal in the key would make every such patch tear this modal down and
    // rebuild it, dropping the caret when the reply landed mid-word.
    let open_key = crate::widgets::overlay_open_key(d.session, d.event, d.preview);

    dyn_container(
        move || open_key.get(),
        move |(_session, open, previewing)| {
            if !open || previewing {
                return empty().into_any();
            }
            let Some(target) = d.event.get_untracked() else {
                return empty().into_any();
            };
            let ui = ui.clone();
            let title = match &target.current {
                Some(e) => format!(
                    "Edit event {}.{}",
                    object_location(&target.database, e.schema.as_deref()),
                    e.name
                ),
                // A new event's namespace isn't chosen — it is *inherited* from
                // wherever the modal was opened, so the title is where it is
                // disclosed.
                None => format!(
                    "Create event in {}",
                    object_location(
                        &target.database,
                        d.event_draft
                            .with_untracked(|e| e.info.schema.clone())
                            .as_deref(),
                    )
                ),
            };

            // The form is built once per open, so one ring covers it.
            let ring = FocusRing::new();
            let root_ring = ring.clone();

            let body =
                crate::widgets::autohide(scroll(event_form(ui.clone(), ring.clone()).style(|s| {
                    s.width_full()
                        .padding_horiz(modal_pad_h())
                        .padding_vert(18.0)
                })))
                .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0));

            // Every event in this database, so the footer can see a rename
            // landing on one. Read once per open rather than per keystroke: it
            // is the same `db_nodes` snapshot the tree is drawing from, and the
            // reload that would change it runs after Apply, which closes this.
            let taken = taken_names(
                &ui,
                &target.database,
                d.event_draft
                    .with_untracked(|e| e.info.schema.clone())
                    .as_deref(),
            );
            let taken_status = taken.clone();
            let taken_actions = taken;

            // **The target comes from the signal, not from the capture above.**
            // `fetch_source` corrects `current` after the modal is up, and a
            // footer diffing the corrected draft against an uncorrected
            // `current` reports a change nobody made. Reading `d.event` here is
            // safe where reading it in the modal's key is not: this container
            // already rebuilds on every keystroke, and it holds the footer, not
            // the form — no caret lives inside it.
            let status = dyn_container(
                move || {
                    (
                        d.event.get(),
                        d.event_draft.get(),
                        (d.event_source_pending.get(), d.event_body_stale.get()),
                    )
                },
                move |(status_target, draft, (pending, stale))| {
                    let Some(status_target) = status_target else {
                        return empty().into_any();
                    };
                    let say = |m: String| {
                        text(m)
                            .style(|s| {
                                s.color(theme::error())
                                    .font_size(theme::font_label())
                                    .max_width(460.0)
                            })
                            .into_any()
                    };
                    // Said before the change count, because until the source
                    // lands the count is over `information_schema`'s resolved
                    // body rather than the event as written.
                    if pending {
                        return text("Reading the event's source…")
                            .style(|s| s.color(theme::text_faint()).font_size(theme::font_label()))
                            .into_any();
                    }
                    if stale {
                        return say("The event's real source arrived after you started typing, \
                             so this Body is built on the catalogue's copy — which \
                             isn't what the server holds. Close and reopen to edit it."
                            .to_string());
                    }
                    let errs = draft.validate();
                    if let Some(first) = errs.first() {
                        return say(first.clone());
                    }
                    if let Some(clash) =
                        draft.name_clash(status_target.current.as_ref(), &taken_status)
                    {
                        return say(clash);
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
            let actions = dyn_container(
                move || {
                    (
                        d.event.get(),
                        d.event_draft.get(),
                        (d.event_source_pending.get(), d.event_body_stale.get()),
                    )
                },
                move |(current, draft, (pending, stale))| {
                    let ui = preview_ui.clone();
                    let Some(target) = current else {
                        return empty().into_any();
                    };
                    let ring = ring_actions.clone();
                    let cs = change_set(&target, &draft);
                    // The same three refusals the routine editor makes, and here
                    // they cost a rejected statement rather than a lost object:
                    // `ALTER EVENT` is in place, so the worst case is an error
                    // over a quote nobody typed. Still refused, because that
                    // error is a worse way to find out than a footer.
                    let ready = !pending
                        && !stale
                        && draft.validate().is_empty()
                        && draft
                            .name_clash(target.current.as_ref(), &taken_actions)
                            .is_none()
                        && !cs.is_empty();
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
        // The same absolute placement every other DDL overlay takes.
        if d.event.get().is_some() && d.preview.get().is_none() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every tab index this form hands out, **in the order the controls read
    /// down the panel**.
    ///
    /// A third statement of the list, on the rule `MY_ROUTINE_COLUMNS` follows:
    /// the two that matter are the constants and the `focus:` arguments, and
    /// checking one against the other would only be checking whether the same
    /// hand wrote both. What this is compared against is the *layout*, which is
    /// the thing a tab order is supposed to follow.
    const TAB_ORDER: [u32; 10] = [
        TAB_NAME,
        TAB_SHAPE,
        TAB_SCHED,      // interval quantity, or the `AT` expression
        TAB_SCHED + 10, // interval unit
        TAB_SCHED + 20, // STARTS
        TAB_SCHED + 30, // ENDS
        TAB_BODY,
        TAB_OPT,      // status
        TAB_OPT + 10, // preserve
        TAB_OPT + 20, // definer
    ];

    /// **No two controls may share an index, and the walk must go down the
    /// panel.** `FocusRing::register` inserts *after* an equal index, so a
    /// collision doesn't error — it silently orders the two by which built
    /// first, which is the form's construction order and not its layout.
    ///
    /// This shipped broken: `TAB_SCHED + 30` (ENDS) was `60`, and so was
    /// `TAB_BODY`, so Tab went Starts → Body → Ends.
    #[test]
    fn no_two_controls_claim_the_same_tab_stop() {
        let mut seen: Vec<u32> = Vec::new();
        for (i, t) in TAB_ORDER.into_iter().enumerate() {
            assert!(
                !seen.contains(&t),
                "index {t} is claimed twice — control {i} collides"
            );
            if let Some(prev) = seen.last() {
                assert!(*prev < t, "control {i} at {t} walks back above {prev}");
            }
            seen.push(t);
        }
        // The last option (Comment) is the highest fixed stop, and it must stay
        // below the shared value-row block and the footer's actions.
        const {
            assert!(TAB_OPT + 30 < crate::widgets::VALUE_TAB);
            assert!(crate::widgets::VALUE_TAB < crate::widgets::ACTION_TAB);
        }
    }
}
