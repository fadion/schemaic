//! **The modal layer, and the four predicates that raise it.**
//!
//! Everything in the app that dims the window behind it hangs in one box, and
//! that box starts below the title bar. Which surface sits above which is a
//! tuple order, and it is *policy* — three of this layer's paint-order rules
//! were each bought with a bug — so it lives in a module of its own rather than
//! inside the 500-line view builder that used to hold it. [`modal_layer`] is
//! the layer; [`modal_backdrop_up`] is the question the layer's box and the
//! title-bar band must never answer differently, and `modal_backdrop_gate` at
//! the foot of this file is what keeps a new modal from being added outside
//! either one.
//!
//! The three group predicates ([`ddl_modals_up`], [`workspace_modals_up`],
//! [`settings_modals_up`]) are here because the wrappers that read them are:
//! each group exists to fit floem's 16-arity `ViewTuple` limit, and a group's
//! wrapper must fill the layer exactly when one of its members is open.

use floem::prelude::*;

use crate::connection_form::manage_modal;
use crate::erd_view::erd_overlay;
use crate::monitor_view::monitor_overlay;
use crate::overlays::{confirm_overlay, error_modal_overlay, find_overlay, tx_prompt_overlay};
use crate::plan_view::plan_overlay;
use crate::settings::{
    ai_settings_overlay, help_overlay, term_settings_overlay, theme_settings_overlay,
};
use crate::{
    DdlUi, Ui, account_editor, database_editor, ddl_preview, event_editor, import_view,
    object_editor, properties, routine_editor, table_designer, theme, trigger_editor, users_view,
    view_editor,
};

/// **Every modal, in one layer that starts below the title bar.**
///
/// "Modal" here is *what a surface does*, not what it is called. Find
/// Anywhere is a palette that closes on a click away, and it is in here
/// with the rest because it dims the window behind it exactly as they do
/// — and because it had the bug they had, in a form of its own: its
/// backdrop covered the title bar, so a press meant for the caption
/// buttons landed on the click-away instead and the only thing that
/// happened was the palette closing. The test is whether the surface
/// paints `theme::modal_backdrop()`; every view in the app that does is
/// in this layer.
///
/// A backdrop is `absolute().inset(0)` *against its parent*, so what this
/// wrapper is worth is where its box stops: `HEADER_H` down from the top.
/// The scrim, and every panel centred in it, is bounded by that — which
/// is what lets `WindowChrome::over_backdrop` lay a drag band across the
/// title bar without ever landing on a modal. Hoisting the band over a
/// full-window layer instead would have put it on top of whatever the
/// modal had up there: a 620px-tall panel centred in a 700px window
/// reaches into the top 40px, and its close × would have been answering
/// to the window's caption buttons.
///
/// The grouping pays for itself twice: the workspace root tuple this hangs
/// in was at floem's 16-arity limit, and five entries becoming one is the
/// room the next overlay will want.
///
/// **A modal that is not in here does not work at all** — its
/// `inset(0)` would resolve against this box while `modal_up` says
/// nothing is open, which is zero by zero. That is the guard: there is no
/// way to add a modal to the app, forget the predicate, and have it look
/// fine. Keep the two in step by construction, not by memory.
///
/// `modal_up` is passed in rather than computed here because the caller needs
/// the very same answer for the title-bar band, and the two must not be able to
/// disagree: a band without a backdrop dims a live header, a backdrop without a
/// band is the bug this whole layer exists to fix.
pub(crate) fn modal_layer(ui: Ui, modal_up: impl Fn() -> bool + Copy + 'static) -> impl IntoView {
    // Each group's wrapper asks its own question once, and gives its members
    // their box only while one of them is open — an always-full-window wrapper
    // would eat every click in the app beneath it.
    let ddl_modals_up = ddl_modals_up(&ui);
    let workspace_modals_up = workspace_modals_up(&ui);
    let settings_modals_up = settings_modals_up(&ui);
    let confirm_up = ui.overlay.confirm;
    stack((
        // First, so it keeps the place it had in the root stack: under every
        // modal. A palette raised over one would be painted behind it, and
        // that is the right way round — the modal is the thing being
        // answered.
        find_overlay(ui.clone()),
        // Right-click a connection → Delete raises a confirm, and it used to open
        // *behind* this modal, its backdrop dimming the panel still on top of it.
        // That was fixed by ordering the two here; the confirm now sits at the end
        // of this tuple, above every group, so the ordering is no longer this
        // entry's business — but the failure is worth remembering, because it is
        // the same one the DDL preview and the popup menu each hit.
        manage_modal(ui.clone()),
        // **Directly above Manage Connections**, which is what raises it: the
        // import modal is a question asked about the list behind it, and closing
        // it returns to that list. Same rule the confirm at the foot of this
        // tuple states — whatever can raise a question comes first.
        crate::connection_import::conn_import_overlay(ui.clone()),
        // Error modal + open-transaction prompt and the schema editors share one
        // tuple element, for the same 16-arity reason as monitor/ERD below (and
        // with the same fill-only-when-open wrapper, or it would eat every click).
        {
            let trigger_open = ui.ddl.trigger;
            let routine_open = ui.ddl.routine;
            let event_open = ui.ddl.event;
            let object_open = ui.ddl.object;
            let database_open = ui.ddl.database;
            let account_open = ui.ddl.account;
            let grant_open = ui.ddl.grant;
            let dump_open = ui.dump.target;
            let script_open = ui.script.target;
            let export_open = ui.export.target;
            stack((
                error_modal_overlay(ui.clone()),
                crate::snippet_edit::snippet_edit_overlay(ui.clone()),
                import_view::import_overlay(ui.clone()),
                // Export and Import-a-script share one tuple element: this stack
                // is at Floem's 16-arity `ViewTuple` limit, and the two are the
                // same journey in opposite directions — both open on a database
                // node and only ever one at a time.
                //
                // **The style is not decoration, it is the reason the modal is
                // in the right place.** A member overlay is
                // `absolute().inset(0)` *against its parent*, so nesting one a
                // level deeper re-parents it to this stack — and an unstyled
                // stack is an ordinary flow child of the layer, sized by its
                // content wherever it happens to sit. Shipped without this, the
                // Export panel rendered inside the schema tree's column,
                // clipped to it. Every grouped element here therefore fills the
                // layer exactly while one of its own members is open, which is
                // the same rule the trigger/routine/event group below states.
                stack((
                    crate::dump_view::dump_overlay(ui.clone()),
                    crate::script_view::script_overlay(ui.clone()),
                    // The grid export's progress modal joins this element rather
                    // than claiming one of its own — the stack is at Floem's
                    // 16-arity limit, and it belongs here on the merits: it is
                    // the dump modal's twin (same footer, same Stop, built beside
                    // it in `dump_view`), and the two cannot be up together,
                    // since a grid export and a schema-tree export are launched
                    // from different surfaces and each refuses a second run.
                    crate::dump_view::export_progress_overlay(ui.clone()),
                ))
                .style(move |s| {
                    if dump_open.get().is_some()
                        || script_open.get().is_some()
                        || export_open.get().is_some()
                    {
                        s.absolute().inset(0.0)
                    } else {
                        s
                    }
                }),
                table_designer::table_designer_overlay(ui.clone()),
                view_editor::view_editor_overlay(ui.clone()),
                // The trigger, routine and event editors share one tuple
                // element — this stack is at Floem's 16-arity `ViewTuple`
                // limit, and only one of the three is ever painted: the
                // trigger editor renders nothing while the routine editor it
                // opened is up, and every `open` here clears the other two
                // targets.
                stack((
                    trigger_editor::trigger_editor_overlay(ui.clone()),
                    routine_editor::routine_editor_overlay(ui.clone()),
                    event_editor::event_editor_overlay(ui.clone()),
                ))
                .style(move |s| {
                    if trigger_open.get().is_some()
                        || routine_open.get().is_some()
                        || event_open.get().is_some()
                    {
                        s.absolute().inset(0.0)
                    } else {
                        s
                    }
                }),
                // The object and database editors share one tuple element — this
                // stack is at Floem's 16-arity `ViewTuple` limit — and only one
                // of the two is ever painted, since every `open` here clears the
                // other's target. The wrapper must fill the layer while either
                // is up, or the member's own `inset(0)` resolves against a
                // zero-by-zero box and the modal renders nothing at all: the
                // failure the event editor shipped with, stated at
                // `ddl_editors_up`.
                stack((
                    object_editor::object_editor_overlay(ui.clone()),
                    database_editor::database_editor_overlay(ui.clone()),
                    account_editor::account_editor_overlay(ui.clone()),
                    account_editor::grant_editor_overlay(ui.clone()),
                ))
                .style(move |s| {
                    if object_open.get().is_some()
                        || database_open.get().is_some()
                        || account_open.get().is_some()
                        || grant_open.get().is_some()
                    {
                        s.absolute().inset(0.0)
                    } else {
                        s
                    }
                }),
                ddl_preview::ddl_preview_overlay(ui.clone()),
                // **Last in this group, because the DDL preview raises it.**
                // `run_ddl` asks about every open transaction on the connection
                // *before* applying (`tx::ddl_blocking_tabs`), and the preview is
                // still on screen while it asks — painted earlier, the question
                // sat entirely behind the preview's own backdrop, so an Apply
                // looked hung on "Applying…" with nothing to answer and no way to
                // reach it. Same rule as `manage_modal` above and the popup menu
                // below: whatever can raise a question comes first.
                tx_prompt_overlay(ui.clone()),
            ))
            .style(move |s| {
                if ddl_modals_up() {
                    s.absolute().inset(0.0)
                } else {
                    s
                }
            })
        },
        plan_overlay(ui.clone()),
        // Monitor + ER-diagram modals share one tuple element (the workspace stack
        // is at Floem's 16-arity `ViewTuple` limit). The wrapper must fill the
        // layer when either is open — so their own `.absolute().inset(0)` resolves
        // against it and the dim backdrop covers everything below the title bar —
        // but stay out-of-flow (zero-size) when both are closed, or it would
        // intercept every click meant for the app beneath it.
        stack((
            monitor_overlay(ui.clone()),
            erd_overlay(ui.clone()),
            properties::properties_overlay(ui.clone()),
            users_view::users_overlay(ui.clone()),
            // The binary-cell panel joins this group on the merits: like the
            // properties modal beside it, it is a question asked *about the
            // result on screen* rather than about the schema tree, and it is
            // raised from the grid's own cell menu.
            crate::blob_view::blob_overlay(ui.clone()),
            // Schema compare belongs here for the reason the ER diagram does:
            // it is a full-window reading of a database raised from the tree,
            // and it hands off to the DDL preview rather than writing anything
            // itself — so it sits *under* the preview's own group, which is
            // what lets Apply appear over it.
            crate::compare_view::compare_overlay(ui.clone()),
        ))
        .style(move |s| {
            if workspace_modals_up() {
                s.absolute().inset(0.0)
            } else {
                s
            }
        }),
        // The four settings/help modals share one tuple element — this stack is
        // at Floem's 16-arity `ViewTuple` limit, the same squeeze the trigger and
        // function editors are under above. They are mutually exclusive (each is
        // reached from a different chrome control, and each takes the window with
        // its own backdrop), and the wrapper fills only while one is open, or an
        // always-full-window box would eat every click in the app.
        stack((
            term_settings_overlay(ui.clone()),
            ai_settings_overlay(ui.clone()),
            theme_settings_overlay(ui.clone()),
            help_overlay(ui.clone()),
        ))
        .style(move |s| {
            if settings_modals_up() {
                s.absolute().inset(0.0)
            } else {
                s
            }
        }),
        // **The shared confirm, last, above every group.**
        //
        // A confirm is by definition raised *by* something already on screen, so
        // "whatever can raise a question comes first" — the rule `manage_modal`
        // and the DDL preview each state above — has exactly one stable answer
        // once there is more than one group: put the question above all of them.
        //
        // It used to live inside the DDL group, which is entry 3 of six, and the
        // Live Monitor is in the workspace group at 5: "Clear the log?" painted
        // *entirely* behind the monitor, with its focus root holding the keyboard
        // over a question nobody could see. Reordering the two groups only moves
        // which modal has the problem; this ends it for all of them, and it makes
        // the three "comes first" comments above local facts about their own
        // group rather than rules the next modal has to rediscover.
        //
        // Its own `absolute().inset(0)` needs a box to resolve against, hence the
        // wrapper — and the wrapper must be out of flow while nothing is asked,
        // or it would eat every click in the app.
        confirm_overlay(ui.clone()).style(move |s| {
            if confirm_up.get().is_some() {
                s.absolute().inset(0.0)
            } else {
                s
            }
        }),
    ))
    .style(move |s| {
        if modal_up() {
            s.absolute()
                .inset_top(theme::header_h())
                .inset_left(0.0)
                .inset_right(0.0)
                .inset_bottom(0.0)
        } else {
            s
        }
    })
}

/// The DDL/editor group's modals — is any of them up?
///
/// One list, read by the wrapper that gives them their box *and* by
/// [`modal_backdrop_up`]. Spelling it twice is how the two drift apart, and the
/// failure is silent in the worst direction: a modal the aggregate doesn't know
/// about opens with the title bar left live and undimmed over it.
///
/// **What belongs here is what is painted in this group**, which is the same
/// thing the wrapper's box is for. The shared confirm is not: it is its own entry
/// at the end of the layer, above every group, and so has its own term in
/// [`modal_backdrop_up`] exactly as `find`, `manage` and `plan` do.
fn ddl_modals_up(ui: &Ui) -> impl Fn() -> bool + Copy + 'static {
    let err_open = ui.overlay.error_modal_open;
    let tx_prompt = ui.overlay.tx_prompt;
    let import_open = ui.import.target;
    let dump_open = ui.dump.target;
    let script_open = ui.script.target;
    let export_open = ui.export.target;
    let snippet_edit = ui.overlay.snippet_edit;
    let editors = ddl_editors_up(ui.ddl);
    move || {
        err_open.get()
            || tx_prompt.get().is_some()
            || import_open.get().is_some()
            // Painted in this group, so it has to be in this list — the wrapper's
            // `inset(0)` resolves against a box this predicate keeps at zero by
            // zero otherwise, and the modal renders nothing at all.
            || dump_open.get().is_some()
            // The script loader shares Export's tuple element, so it is painted
            // in this group and has to be in this list for the same reason —
            // and it is the *second* signal in that element, which is the shape
            // most likely to be forgotten.
            || script_open.get().is_some()
            // The grid export's progress modal is the *third* signal in that
            // same tuple element, painted in this group — so it is in this list
            // for the reason the two above are, and it is the shape most likely
            // to be forgotten of all: a modal nobody opens deliberately, raised
            // by an export the user started somewhere else entirely.
            || export_open.get().is_some()
            // The snippet editor is painted in this group, so it has to be in
            // this list — the event editor shipped missing from exactly here and
            // rendered nothing at all, because the wrapper's `inset(0)` resolved
            // against a box this predicate was keeping at zero by zero.
            || snippet_edit.get().is_some()
            || editors()
    }
}

/// The schema-editing half of that list — **is any editor target set?**
///
/// Split out of [`ddl_modals_up`] so it can be *tested*: the aggregate itself
/// needs a whole [`Ui`], while this needs only the [`DdlUi`] bundle, which is
/// the same fixture `ddl_preview`'s tests already build for
/// `close_editors_clears_every_editor`. The two are one invariant read from
/// opposite ends — every editor must be in this list, and every editor must be
/// cleared by that one — and the event editor shipped absent from *this* half:
/// its overlay's `inset(0)` resolved against a box the aggregate was keeping at
/// zero by zero, so the modal painted nothing at all.
pub(crate) fn ddl_editors_up(d: DdlUi) -> impl Fn() -> bool + Copy + 'static {
    move || {
        d.designer.get().is_some()
            || d.view.get().is_some()
            || d.trigger.get().is_some()
            || d.routine.get().is_some()
            || d.object.get().is_some()
            || d.event.get().is_some()
            || d.database.get().is_some()
            // Both account forms are painted in this group, so both have to be
            // in this list — the failure `ddl_editors_up` states, and the shape
            // most likely to be forgotten is the *second* signal of a shared
            // tuple element, which the grant editor is.
            || d.account.get().is_some()
            || d.grant.get().is_some()
            || d.preview.get().is_some()
    }
}

/// The workspace group's modals — Live Monitor, the ER diagram, Properties, the
/// binary-cell panel, and the two that are not simply open or closed: the Users
/// and privileges browser, and schema compare. Both of those raise something
/// painted in an earlier group and so stop counting while it is up. See the
/// note in the body.
fn workspace_modals_up(ui: &Ui) -> impl Fn() -> bool + Copy + 'static {
    let mon_open = ui.overlay.monitor_open;
    let erd_open = ui.overlay.erd;
    let props_open = ui.overlay.properties;
    let users_open = ui.overlay.users;
    let blob_open = ui.blob.target;
    let compare_open = ui.overlay.compare;
    // **The browser counts only while it is the thing on screen.** It renders
    // nothing while one of the account forms or the DDL preview is up — those
    // are raised from it and painted in an earlier group — and a wrapper that
    // still filled the layer would be a transparent full-window box sitting on
    // top of the form, swallowing every click meant for it.
    let account_open = ui.ddl.account;
    let grant_open = ui.ddl.grant;
    let preview_open = ui.ddl.preview;
    move || {
        mon_open.get()
            || erd_open.get().is_some()
            || props_open.get().is_some()
            || blob_open.get().is_some()
            || (users_open.get().is_some()
                && account_open.get().is_none()
                && grant_open.get().is_none()
                && preview_open.get().is_none())
            // Compare takes the browser's `preview_open` clause for the same
            // reason: it raises the DDL preview, which is painted in an earlier
            // group. It closes itself on the way there today — a comparison
            // outlives the schema it describes by no more than one apply — so
            // this is the rule stated rather than a case that arises, and it is
            // here so that stops being load-bearing.
            || (compare_open.get().is_some() && preview_open.get().is_none())
    }
}

/// The settings/help group's modals.
fn settings_modals_up(ui: &Ui) -> impl Fn() -> bool + Copy + 'static {
    let term_open = ui.term.settings_open;
    let ai_open = ui.ai.settings_open;
    let theme_open = ui.layout.theme_settings_open;
    let help_open = ui.layout.help_open;
    move || term_open.get() || ai_open.get() || theme_open.get() || help_open.get()
}

/// Is a **full-window modal backdrop** on screen?
///
/// The three groups plus the three surfaces mounted on their own, which is every
/// view in the app that paints `theme::modal_backdrop()`. That is the test, and
/// it is a test about behaviour rather than about naming: **Find Anywhere is in
/// here** even though it is a palette that closes on a click away, because it
/// dims the window exactly as a modal does and so had exactly a modal's bug —
/// its backdrop covered the title bar, and a press aimed at the caption buttons
/// only closed the palette.
///
/// Menus are not here and must not be: they are shrink-wrapped to their panel,
/// they never cover the title bar, and raising the band over one would dim a
/// header the user can still use.
///
/// Two things read this and they must agree — the modal layer's box (which
/// starts below the title bar) and the band that then covers the title bar. If
/// only the first knew, the band would never appear; if only the second did,
/// it would dim a live header with no modal in sight.
pub(crate) fn modal_backdrop_up(ui: &Ui) -> impl Fn() -> bool + Copy + 'static {
    let find_open = ui.overlay.find_open;
    let manage_open = ui.conn.manage_open;
    // Its own term, like `manage_open`'s: it is a loose child of the layer with
    // no group wrapper, and it can outlive the modal that raised it — closing
    // Manage Connections while the import list is up must not take the backdrop
    // out from under it.
    let conn_import_open = ui.conn.import.open;
    let plan_open = ui.overlay.plan_open;
    // Its own entry in the layer, above every group — so its own term here, the
    // same way `find`, `manage` and `plan` each have one. It used to be counted
    // by `ddl_modals_up`, which is now only about that group's own box.
    let confirm = ui.overlay.confirm;
    let ddl = ddl_modals_up(ui);
    let workspace = workspace_modals_up(ui);
    let settings = settings_modals_up(ui);
    move || {
        find_open.get()
            || manage_open.get()
            || conn_import_open.get()
            || plan_open.get()
            || confirm.get().is_some()
            || ddl()
            || workspace()
            || settings()
    }
}

#[cfg(test)]
mod modal_backdrop_gate {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    /// **The claim this exists to hold up**, in the layer's own words: *"The test
    /// is whether the surface paints `theme::modal_backdrop()`; every view in the
    /// app that does is in this layer."*
    ///
    /// `07bda98` argued no test was needed, because a modal left out of the
    /// predicate gets a zero-by-zero box and does not open at all. That covers one
    /// direction. The other — a surface that paints a backdrop and is mounted
    /// **outside** the layer — resolves its `inset(0)` against the root, looks
    /// perfect, and silently restores the exact bug the layer was written to fix:
    /// the backdrop covers the title bar, the drag band never rises, and the window
    /// cannot be moved, minimised or closed, with nothing on screen saying why.
    /// Three of the layer's members are loose children with no group wrapper to
    /// remind anyone, so that is the shape the next overlay will take.
    ///
    /// Deliberately weak, like its three siblings (`widgets::popup_anchor_gate`,
    /// `menu_trigger_gate`, `menu_panel_gate`): it asserts *which files* paint a
    /// backdrop, not how many times each does. A count would fail on an innocent
    /// refactor and a gate that cries wolf gets deleted; the failure this catches is
    /// a backdrop appearing somewhere **new**, and a new place is a new file far
    /// more often than not. The floor below is what stops a rename making it pass by
    /// finding nothing.
    const PAINTS_A_BACKDROP: &[&str] = &[
        // In the layer's workspace group, beside `properties.rs`, raised by
        // `workspace_modals_up`'s `blob_open.get().is_some()` arm.
        "blob_view.rs",
        // In the layer's workspace group, beside `erd_view.rs`, raised by
        // `workspace_modals_up`'s `compare_open.get().is_some()` arm.
        "compare_view.rs",
        "connection_form.rs",
        // A loose child of the layer, directly above `connection_form.rs`'s
        // modal, which is what raises it.
        "connection_import.rs",
        // In the layer's DDL group, sharing `object_editor.rs`'s tuple element
        // (the stack is at floem's 16-arity limit and the two never open
        // together), raised by `ddl_editors_up`'s `d.database` arm.
        "database_editor.rs",
        "ddl_preview.rs",
        // In the layer's DDL group — **one file, two overlays**, sharing one
        // tuple element with `script_view.rs`: the Export panel, raised by
        // `ddl_modals_up`'s `dump_open` arm, and the grid export's
        // progress modal beside it, raised by its `export_open` arm. The second
        // is the shape this gate is weakest against, since the file was already
        // on the list before it existed.
        "dump_view.rs",
        "erd_view.rs",
        "event_editor.rs",
        "import_view.rs",
        "monitor_view.rs",
        "object_editor.rs",
        "overlays.rs",
        "plan_view.rs",
        "properties.rs",
        "routine_editor.rs",
        // In the layer's DDL group, sharing `dump_view.rs`'s tuple element (the
        // stack is at floem's 16-arity limit and the two never open together),
        // raised by `ddl_modals_up`'s `script_open.get().is_some()` arm.
        "script_view.rs",
        "settings.rs",
        // In the layer's DDL group, raised by `ddl_modals_up`'s
        // `snippet_edit.get().is_some()` arm.
        "snippet_edit.rs",
        // In the layer's DDL group, sharing `object_editor.rs`'s tuple element,
        // raised by `ddl_editors_up`'s `d.account` and `d.grant` arms — one file,
        // two overlays.
        "account_editor.rs",
        "table_designer.rs",
        "trigger_editor.rs",
        // In the layer's workspace group, raised by `workspace_modals_up`'s
        // `users_open.get().is_some()` arm.
        "users_view.rs",
        "view_editor.rs",
        // **The one deliberate exception**, and the reason the list is data rather
        // than a rule. `WindowChrome::over_backdrop` paints the same scrim across
        // the title bar *while* a modal is up, and it is mounted inside the
        // workspace root — after the modal layer, before the overlay menus — on
        // purpose: out at the window root it sat above the whole app and dimmed a
        // tall menu's first rows while answering their presses with a window drag.
        // It is not a modal and it is not in the layer.
        "window_chrome.rs",
    ];

    /// Where the colour itself is defined. Not paint sites.
    ///
    /// **`lib.rs` came off this list when the layer moved out of it.** It was
    /// excluded for prose that has since moved here, and `production_code`
    /// strips comments anyway — so the exclusion was never doing that job, while
    /// it *was* blinding the gate to the one file where a stray modal is most
    /// likely to be mounted: the workspace root tuple is there, and a backdrop
    /// added as a sibling of [`modal_layer`] rather than inside it is precisely
    /// the regression these two tests exist to catch.
    const NOT_A_PAINT_SITE: &[&str] = &["theme.rs", "themes.rs"];

    fn src_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
    }

    /// The file with its `#[cfg(test)]` module cut off, and with comment lines
    /// dropped — this very module quotes the rule, and `lib.rs` states it twice in
    /// prose, so a gate that counted comments would be measuring its own
    /// documentation.
    ///
    /// [`crate::source_gate`] owns the cut — see there for what the eleven
    /// hand-written copies of it all got wrong.
    use crate::source_gate::production_code;

    #[test]
    fn every_backdrop_in_the_crate_is_one_the_layer_knows_about() {
        let mut found: BTreeSet<String> = BTreeSet::new();
        let dir = std::fs::read_dir(src_dir()).expect("the crate's own src");
        for entry in dir {
            let path = entry.expect("a dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .expect("a file name")
                .to_string();
            if NOT_A_PAINT_SITE.contains(&name.as_str()) {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("a source file");
            if production_code(&src).contains("modal_backdrop()") {
                found.insert(name);
            }
        }

        let expected: BTreeSet<String> = PAINTS_A_BACKDROP.iter().map(|s| s.to_string()).collect();

        let unexpected: Vec<&String> = found.difference(&expected).collect();
        assert!(
            unexpected.is_empty(),
            "these files paint `modal_backdrop()` and are not on the list: {unexpected:?}\n\
             A backdrop mounted outside the modal layer resolves its `inset(0)` against \
             the root, looks perfect, and takes the title bar with it — the window can \
             then not be moved, minimised or closed. If the new site *is* in the layer, \
             add it to `PAINTS_A_BACKDROP` with a line saying which predicate raises it."
        );
        let gone: Vec<&String> = expected.difference(&found).collect();
        assert!(
            gone.is_empty(),
            "these files no longer paint `modal_backdrop()`: {gone:?} — the list is \
             stale, and a stale list is one that stops catching anything. Remove them."
        );
        // The floor: a rename of the colour fn would otherwise make this pass by
        // finding nothing at all, which is the failure mode a source gate is most
        // prone to.
        assert!(
            found.len() >= 15,
            "only {} files paint a backdrop — did `modal_backdrop` get renamed?",
            found.len()
        );
    }

    /// And the other direction, which `07bda98`'s "loud failure" argument covers
    /// and which is worth pinning next to it: every term of `modal_backdrop_up` is
    /// a predicate the layer also uses to size itself, so a modal in the layer with
    /// no term gets a zero box. All seven terms are named here — the three grouped
    /// predicates and the four signals the layer raises directly — so an eighth
    /// added without joining `modal_backdrop_up` fails.
    ///
    /// `confirm` is the seventh, and it arrived here late: hoisting the shared
    /// confirm out of the DDL group gave it its own entry in the layer and its
    /// own term in the predicate, and the array below was not extended with it —
    /// which is precisely the drift the paragraph above claims to catch. The list
    /// only guards what it names.
    #[test]
    fn the_predicate_names_every_group_the_layer_raises() {
        let src = std::fs::read_to_string(src_dir().join("modals.rs")).expect("modals.rs");
        let body = production_code(&src);
        let at = body
            .find("fn modal_backdrop_up(")
            .expect("modal_backdrop_up is gone — this gate is stale");
        let end = body[at..].find("\n}").expect("its end");
        let f = &body[at..at + end];
        // **The closure, not the whole function.** Binding a predicate and then not
        // `||`-ing it into the answer is exactly the mistake to catch, and it leaves
        // the binding's name in the body — so scanning the function would pass.
        let ret = f
            .find("move ||")
            .expect("the returned closure is gone — this gate is stale");
        let closure = &f[ret..];
        for term in [
            "ddl()",
            "workspace()",
            "settings()",
            "find_open.get()",
            "manage_open.get()",
            "conn_import_open.get()",
            "plan_open.get()",
            "confirm.get()",
        ] {
            assert!(
                closure.contains(term),
                "`modal_backdrop_up`'s answer no longer includes {term} — a modal \
                 that group raises would paint a backdrop the layer does not know is \
                 up, so the layer would not size itself and the title bar would stay \
                 under it"
            );
        }
    }
}

/// **Every `stack` in the layer is styled**, because a member overlay's
/// `absolute().inset(0)` resolves against *its parent*.
///
/// A group exists only to fit floem's 16-arity `ViewTuple` limit, so it is
/// natural to write one as a bare `stack((a, b))` and think nothing has changed.
/// Something has: the members are now one level deeper, and an unstyled stack is
/// an ordinary flow child of the layer, sized by its content wherever it happens
/// to sit. The Export and Import-a-script group shipped that way for one build
/// and painted its panel **inside the schema tree's column**, clipped to it.
///
/// The two existing groups both carried the fill-only-when-open style, and the
/// third did not — the drift a comment cannot catch and this can.
#[cfg(test)]
mod modal_group_gate {
    use std::path::{Path, PathBuf};

    fn this_file() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("modals.rs")
    }

    /// `modal_layer`'s body, from its signature to the closing brace in column 0.
    fn layer_body(src: &str) -> &str {
        let start = src
            .find("pub(crate) fn modal_layer(")
            .expect("the layer's signature is gone");
        let end = src[start..]
            .find("\n}\n")
            .map(|i| start + i)
            .expect("the layer's closing brace is gone");
        &src[start..end]
    }

    /// The byte index just past the `)` that closes the `stack(` beginning at
    /// `at`, by counting parentheses. Approximate by design — the same licence
    /// `menu_order_gate` takes — and safe here because the region holds no
    /// string literal carrying an unbalanced parenthesis.
    fn end_of_call(body: &str, at: usize) -> usize {
        let bytes = body.as_bytes();
        let open = at + "stack".len();
        let mut depth = 0usize;
        for (i, &b) in bytes.iter().enumerate().skip(open) {
            match b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return i + 1;
                    }
                }
                _ => {}
            }
        }
        panic!("unbalanced parentheses after the `stack(` at byte {at}");
    }

    #[test]
    fn every_stack_in_the_modal_layer_is_styled() {
        let src = std::fs::read_to_string(this_file()).expect("this file");
        let body = layer_body(&src);

        let mut found = 0usize;
        let mut unstyled: Vec<usize> = Vec::new();
        let mut at = 0usize;
        while let Some(i) = body[at..].find("stack((") {
            let i = at + i;
            // `h_stack((` / `v_stack((` are not groups of the layer.
            let is_bare = i == 0
                || !body.as_bytes()[i - 1].is_ascii_alphanumeric()
                    && body.as_bytes()[i - 1] != b'_';
            if is_bare {
                found += 1;
                let close = end_of_call(body, i);
                let rest = body[close..].trim_start();
                if !rest.starts_with(".style(") {
                    // Line number within the file, for a message that can be
                    // acted on without counting bytes.
                    let line = src[..src.find(body).unwrap_or(0) + i]
                        .bytes()
                        .filter(|&b| b == b'\n')
                        .count()
                        + 1;
                    unstyled.push(line);
                }
            }
            at = i + "stack((".len();
        }

        assert!(
            unstyled.is_empty(),
            "these `stack`s in `modal_layer` are not followed by `.style(…)`, at \
             lines {unstyled:?}.\nA group's members are `absolute().inset(0)` against \
             *the group*, so an unstyled one is a flow child sized by its content — the \
             panel then renders wherever that box lands (it rendered inside the schema \
             tree). Give it the fill-only-when-open style its siblings have: \
             `.style(|s| if <any member open> {{ s.absolute().inset(0.0) }} else {{ s }})`."
        );
        // And the scan is still finding them, or it passes by seeing nothing —
        // the vacuous-pass trap `menu_order_gate` fell into once.
        assert!(
            found >= 5,
            "only {found} `stack`s found in `modal_layer` — has it moved or been \
             renamed? The gate is then passing without checking anything."
        );
    }
}
