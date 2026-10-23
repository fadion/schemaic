//! The **Import Connections** modal: a review list of everything the machine's
//! other database clients already know, plus a field for a URL that was pasted.
//!
//! Raised from Manage Connections, and painted above it — the list it is about
//! is the one behind it, and closing this returns to it.
//!
//! **Nothing here is written until Import is pressed**, and the whole modal is
//! built around that. `schemaic_core::conn_import` hands over *proposals*: rows
//! with no id, a suggested name, and a note where the source could not supply
//! something. What the user does on this screen is untick the ones they don't
//! want and read the ones that will need a password typed. That is why a row
//! that duplicates a saved connection is shown but not ticked, rather than
//! hidden — a stale saved copy is exactly the case where the import is the one
//! worth keeping, and only the user knows which.
//!
//! The parsing, the identity rule and the ordering all live in core, where they
//! are tested; the file-finding lives in the app, which is the only crate that
//! may touch a disk. This module is a view over their result and holds no
//! decision of its own.

use std::collections::HashSet;
use std::rc::Rc;

use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use schemaic_core::conn_import::{ImportNote, Imported, Skipped};
use schemaic_core::connection::AuthMode;

use crate::i18n::tr;
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, action_button_icon, autohide, field_button,
    focus_root_with_ring, form_hint, form_label_style, link_button, modal_h, modal_pad_h,
    modal_title_owned, modal_w, panel_style,
};
use crate::{ConnImportUi, FieldCfg, edit_field, icons, theme};

/// Tab stops, spaced by 10 so a control can be inserted between two without
/// renumbering — the convention `conn_form` uses.
const TAB_PASTE: u32 = 10;
const TAB_ADD: u32 = 20;
const TAB_FILE: u32 = 30;
const TAB_SCAN: u32 = 40;
const TAB_DOCKER: u32 = 45;
const TAB_ALL: u32 = 50;
const TAB_NONE: u32 = 60;

/// The modal. Absolutely positioned over the workspace while
/// `ui.conn.import.open` is true.
pub(crate) fn conn_import_overlay(
    imp: ConnImportUi,
    actions: Rc<crate::ConnActions>,
) -> impl IntoView {
    let open = imp.open;

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return crate::widgets::nothing();
            }
            let ring = FocusRing::new();
            let actions = actions.clone();
            let close: Rc<dyn Fn()> = Rc::new(move || open.set(false));
            let (close_x, close_esc) = (close.clone(), close.clone());

            // Four ways in, in the order they cost the user something: a URL
            // they already have, a file they can point at, the clients
            // installed here (the one that reads the filesystem), and the
            // containers Docker is running (the one that starts a program).
            // The review list appears **below all of them**, and only once one
            // of them has produced something.
            let body = v_stack((
                paste_row(imp, actions.add_pasted_url.clone(), ring.clone()),
                source_buttons(
                    imp,
                    actions.choose_import_file.clone(),
                    actions.scan_installed_clients.clone(),
                    actions.scan_docker.clone(),
                    ring.clone(),
                ),
                row_list(imp, ring.clone()),
                skipped_note(imp),
            ))
            .style(|s| s.flex_col().width_full().gap(theme::scaled(14.0)));

            let panel = v_stack((
                modal_title_owned(tr!("conn-import-title"), close_x, ring.clone()),
                autohide(scroll(body.style(|s| {
                    s.flex_col()
                        .width_full()
                        .padding_horiz(modal_pad_h())
                        .padding_vert(theme::scaled(16.0))
                })))
                .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0)),
                footer(imp, actions.import_chosen.clone(), close, ring.clone()),
            ))
            .on_click_stop(|_| {})
            .style(|s| panel_style(s).width(modal_w(560.0)).height(modal_h(520.0)));

            // **No click-away, on purpose**: the ticked list is the user's work
            // and nothing is written until Import, so a stray click must not
            // discard it — ✕ and Escape close it. See docs/architecture.md,
            // *which modals close on a backdrop click*.
            focus_root_with_ring(container(panel), ring)
                .on_key_down(
                    Key::Named(NamedKey::Escape),
                    |_| true,
                    move |_| (close_esc)(),
                )
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
        if open.get() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// The paste field and its Add button.
///
/// First on the screen, above the other two ways in, because it is the one that
/// always works: a scan finds what is installed here, and a URL out of a
/// colleague's message, a `.env` or a provider's dashboard is the case where
/// nothing is installed at all.
fn paste_row(imp: ConnImportUi, add: Rc<dyn Fn()>, ring: FocusRing) -> impl IntoView {
    let submit = add.clone();
    let ring_add = ring.clone();

    // On the default field surface, as every other form field is: it wore
    // `bg_editor` for no recorded reason, which made it the one lighter field in
    // the app (GitHub #19). *Add* sits inside it (`FieldCfg::button`), where it
    // can be neither taller nor shorter than the field.
    let field = edit_field(
        imp.paste,
        FieldCfg {
            placeholder: "postgres://user@host:5432/db",
            button: Some(Rc::new(move || {
                let add = add.clone();
                field_button(
                    tr!("conn-import-add"),
                    ring_add.clone(),
                    TAB_ADD,
                    move || (add)(),
                )
            })),
            // Monospace, because what goes in it is a URL: the punctuation that
            // decides where the host stops and the database starts is what the
            // reader is checking, and proportional type is where a `:` and a `/`
            // stop being distinguishable at a glance.
            mono: true,
            focus: Some((ring.clone(), TAB_PASTE)),
            autofocus: true,
            on_submit: Some(Rc::new(move || (submit)())),
            ..Default::default()
        },
    )
    .style(|s| s.width_full().min_width(0.0));

    // A hint that turns into the parse error, in the same place: the field is
    // where the answer is needed, and a message that appears somewhere else
    // moves the layout under the pointer that is about to press Add again.
    let message = dyn_container(
        move || imp.paste_error.get(),
        move |err| match err {
            Some(e) => text(e)
                .style(|s| {
                    s.font_size(theme::font_label())
                        .color(theme::error())
                        .width_full()
                })
                .into_any(),
            None => form_hint(tr!("conn-import-url-hint")).into_any(),
        },
    )
    .style(|s| s.width_full());

    v_stack((
        text(tr!("conn-import-url")).style(form_label_style),
        field,
        message,
    ))
    .style(|s| s.flex_col().width_full().gap(theme::scaled(6.0)))
}

/// The other two ways in: a file, and the clients installed here.
///
/// **Scanning is a button, not what opening the modal does.** The walk reads the
/// user's home directory, and a dialog that goes looking through it because it
/// was opened is doing something nobody asked for; this way the three sources
/// are three deliberate acts, and the modal opens instantly.
fn source_buttons(
    imp: ConnImportUi,
    choose: Rc<dyn Fn()>,
    scan: Rc<dyn Fn()>,
    docker: Rc<dyn Fn()>,
    ring: FocusRing,
) -> impl IntoView {
    let scanning = imp.scanning;
    let file_error = imp.file_error;
    let docker_note = imp.docker_note;

    // **Under the buttons, not under the paste field.** A failed *Choose a
    // file…* used to be written into `paste_error`, so a file button's failure
    // was rendered in the **Connection URL** field's error slot — and stayed
    // there over a good URL typed afterwards, because only a paste clears that
    // signal. See `ConnImportUi::file_error`.
    let message = dyn_container(
        move || file_error.get(),
        move |err| match err {
            Some(e) => text(e)
                .style(|s| {
                    s.font_size(theme::font_label())
                        .color(theme::error())
                        .width_full()
                })
                .into_any(),
            None => empty().into_any(),
        },
    );

    // Docker's answer when it is not a row — not installed, daemon down, nothing
    // running — in the label colour rather than the error one: "no database
    // containers are running" is an answer, and most of the others are about
    // the machine rather than a mistake.
    let docker_message = dyn_container(
        move || docker_note.get(),
        move |note| match note {
            Some(n) => form_hint(n).into_any(),
            None => empty().into_any(),
        },
    );

    v_stack((
        row_of_buttons(choose, scan, docker, scanning, imp.docker_busy, ring),
        message,
        docker_message,
    ))
    .style(|s| s.flex_col().width_full().gap(theme::scaled(6.0)))
}

fn row_of_buttons(
    choose: Rc<dyn Fn()>,
    scan: Rc<dyn Fn()>,
    docker: Rc<dyn Fn()>,
    scanning: RwSignal<bool>,
    docker_busy: RwSignal<bool>,
    ring: FocusRing,
) -> impl IntoView {
    let ring_docker = ring.clone();
    h_stack((
        action_button_icon(
            tr!("conn-import-choose-file"),
            icons::FILE,
            ActionKind::Quiet,
            true,
            ring.clone(),
            TAB_FILE,
            move || (choose)(),
        ),
        // Disabled *while* a scan is out, so a second press can't queue a second
        // walk whose result would land on top of the first's.
        //
        // **Quiet, like the other two.** It wore the accent fill (GitHub #18)
        // while it stood beside Choose a file alone; with Docker containers as
        // a third source, one lit button among three read as the odd one out
        // rather than the obvious one.
        dyn_container(
            move || scanning.get(),
            move |busy| {
                let (scan, ring) = (scan.clone(), ring.clone());
                action_button_icon(
                    if busy {
                        tr!("conn-import-scanning")
                    } else {
                        tr!("conn-import-scan")
                    },
                    icons::SEARCH,
                    ActionKind::Quiet,
                    !busy,
                    ring,
                    TAB_SCAN,
                    move || (scan)(),
                )
            },
        ),
        // One source among several, and the only one that starts a program —
        // disabled while it is out for the reason the scan is.
        dyn_container(
            move || docker_busy.get(),
            move |busy| {
                let (docker, ring) = (docker.clone(), ring_docker.clone());
                action_button_icon(
                    if busy {
                        tr!("conn-import-asking-docker")
                    } else {
                        tr!("conn-import-docker")
                    },
                    icons::DATABASE,
                    ActionKind::Quiet,
                    !busy,
                    ring,
                    TAB_DOCKER,
                    move || (docker)(),
                )
            },
        ),
    ))
    // Wraps: three sources side by side fit English and little else, and in a
    // longer language the third button ran past the modal's edge and was cut
    // in half. On a second line it stays whole.
    .style(|s| {
        s.items_center()
            .width_full()
            .flex_wrap(floem::style::FlexWrap::Wrap)
            .gap(theme::scaled(8.0))
    })
}

/// The review list — **built only once there is something to review.**
///
/// An empty bordered box with a sentence in it is a control that does nothing,
/// and it is the first thing on screen when the modal opens; a line of text
/// pointing at the three buttons above is what that space is worth.
fn row_list(imp: ConnImportUi, ring: FocusRing) -> impl IntoView {
    dyn_container(
        move || (imp.scanning.get(), imp.scanned.get(), imp.rows.get()),
        move |(scanning, scanned, rows)| {
            if rows.is_empty() {
                return form_hint(empty_message(scanning, scanned)).into_any();
            }
            // **Bounded**, and said so when it bites — see
            // `conn_import::ROW_VIEW_CAP`. The cap is on the views, not on the
            // import: `rows` still holds every connection and Import still
            // creates all of them.
            let overflow = row_overflow_note(rows.len());
            let list = v_stack_from_iter(
                rows.into_iter()
                    .enumerate()
                    .take(schemaic_core::conn_import::ROW_VIEW_CAP)
                    .map(move |(i, r)| import_row(imp.chosen, i, r)),
            )
            .style(|s| s.flex_col().width_full());
            let overflow_note = match overflow {
                Some(sentence) => form_hint(sentence).into_any(),
                None => crate::widgets::nothing(),
            };

            v_stack((
                found_header(imp, ring.clone()),
                autohide(scroll(list).style(|s| s.width_full())).style(|s| {
                    s.width_full()
                        .max_height(theme::scaled(230.0))
                        .background(theme::bg_editor())
                        .border(1.0)
                        .border_color(theme::border())
                        .border_radius(8.0)
                }),
                overflow_note,
            ))
            .style(|s| s.flex_col().width_full().gap(theme::scaled(8.0)))
            .into_any()
        },
    )
    .style(|s| s.flex_col().width_full())
}

/// What an empty list means, which is not one thing.
///
/// Pure and tested: before a scan the emptiness is an invitation, during one it
/// is progress, and after one it is the answer — and a screen that says
/// "Nothing found yet" in all three cases makes the scan button look dead.
fn empty_message(scanning: bool, scanned: bool) -> String {
    match (scanning, scanned) {
        (true, _) => tr!("conn-import-looking"),
        (false, true) => tr!("conn-import-none-found"),
        (false, false) => tr!("conn-import-nothing-yet"),
    }
}

/// How many were found, and the two links that tick them.
fn found_header(imp: crate::ConnImportUi, ring: FocusRing) -> impl IntoView {
    let found = move || tr!("conn-import-found", count = imp.rows.with(|r| r.len()));
    h_stack((
        label(found).style(|s| s.color(theme::text_dim()).font_size(theme::font_label())),
        empty().style(|s| s.flex_grow(1.0_f32)),
        // Links, not footer buttons: these adjust the list the user is reading,
        // they are not the decision the modal is asking about. That decision has
        // exactly two buttons and they are in the footer.
        link_button(
            tr!("conn-import-select-all"),
            theme::accent,
            ring.clone(),
            TAB_ALL,
            move || {
                let n = imp.rows.with_untracked(|r| r.len());
                imp.chosen.set((0..n).collect());
            },
        ),
        link_button(
            tr!("conn-import-select-none"),
            theme::text_muted,
            ring,
            TAB_NONE,
            move || imp.chosen.set(HashSet::new()),
        ),
    ))
    .style(|s| s.items_center().width_full().gap(theme::scaled(10.0)))
}

/// One proposed connection: a tick box, where it points, its engine and source,
/// and whatever is missing from it.
///
/// Nothing in it is rebuilt as the selection moves — the box changes by style
/// alone (see [`crate::widgets::check_box`]), which is the dump modal's
/// table-picker rule and holds for its reason: a row rebuilt under the pointer
/// takes itself apart mid-click.
fn import_row(chosen: RwSignal<HashSet<usize>>, index: usize, imp: Imported) -> impl IntoView {
    let is_in = move || chosen.with(|c| c.contains(&index));

    // Every note, joined: a row can be both already-saved and password-less, and
    // showing only the first would hide the one that decides whether to tick it.
    let notes = imp
        .notes
        .iter()
        .map(|&n| import_note_label(n))
        .collect::<Vec<_>>()
        .join(" · ");
    let alarming = imp.has(ImportNote::AlreadySaved)
        || imp
            .notes
            .iter()
            .any(|n| matches!(n, ImportNote::SignsInAs(_)));

    let detail = v_stack((
        text(imp.connection.name.clone()).style(|s| {
            s.font_size(theme::font_body())
                .color(theme::text())
                .text_ellipsis()
        }),
        text(row_target(&imp.connection)).style(|s| {
            s.font_family(crate::consts::MONO_FAMILY.to_string())
                .font_size(theme::font_hint())
                .color(theme::text_dim())
                .text_ellipsis()
        }),
        if notes.is_empty() {
            crate::widgets::nothing()
        } else {
            text(notes)
                .style(move |s| {
                    let s = s.font_size(theme::font_hint()).text_ellipsis();
                    if alarming {
                        s.color(theme::status_warn())
                    } else {
                        s.color(theme::text_muted())
                    }
                })
                .into_any()
        },
    ))
    .style(|s| {
        s.flex_col()
            .min_width(0.0)
            .flex_grow(1.0_f32)
            .gap(theme::scaled(2.0))
    });

    h_stack((
        crate::widgets::check_box(is_in),
        detail,
        // Engine then source, right-aligned: what it is, and who told us. Both
        // are one short word, and both are what the eye scans a mixed list by.
        capsule(schemaic_core::connection::engine_label(
            &imp.connection.db_type,
        )),
        capsule(imp.source.label().to_string()),
    ))
    .on_click_stop(move |_| {
        chosen.update(|c| {
            if !c.remove(&index) {
                c.insert(index);
            }
        });
    })
    .style(|s| {
        s.items_center()
            .width_full()
            .gap(theme::scaled(10.0))
            .padding_horiz(theme::scaled(11.0))
            .padding_vert(theme::scaled(9.0))
            .border_bottom(1.0)
            .border_color(theme::border())
            .hover(|s| s.background(theme::row_hover_soft()))
    })
}

/// What a row shows on its second line: where the connection points, as
/// compactly as it can be said without losing what distinguishes it.
///
/// Pure and tested. Deliberately **not** a `scheme://user@host/db` URL — this
/// project has no URL builder on purpose (see `connection.rs`), and one written
/// here to fill a label is what the next reader copies for something that
/// matters. On SQLite it is the file's own name, which is all `endpoint()` has
/// and all there is.
fn row_target(c: &schemaic_core::connection::Connection) -> String {
    let mut out = String::new();
    if !c.user.trim().is_empty() {
        out.push_str(c.user.trim());
        out.push('@');
    }
    out.push_str(&c.endpoint());
    if !c.database.trim().is_empty() {
        out.push('/');
        out.push_str(c.database.trim());
    }
    // A tunnel is part of where the row points: its SSH server is who sees
    // the traffic first, and an imported one is a host the user never typed.
    if c.ssh.enabled && schemaic_core::connection::is_networked(&c.db_type) {
        out.push_str(" via ");
        if !c.ssh.user.trim().is_empty() {
            out.push_str(c.ssh.user.trim());
            out.push('@');
        }
        out.push_str(&format!("{}:{}", c.ssh.host.trim(), c.ssh.port));
    }
    out
}

/// A small pill of dim text — the engine and the source, on the right of a row.
fn capsule(label: String) -> impl IntoView {
    container(text(label).style(|s| {
        s.font_size(theme::font_hint())
            .color(theme::text_dim())
            .text_ellipsis()
    }))
    .style(|s| {
        s.flex_shrink(0.0_f32)
            .items_center()
            .height(theme::scaled(18.0))
            .padding_horiz(theme::scaled(7.0))
            .border_radius(4.0)
            .background(theme::capsule_bg())
    })
}

/// What the sources held and this app cannot offer.
///
/// Shown rather than dropped: a user with twelve DataGrip data sources and four
/// rows here needs to know the other eight were Oracle, not that the import is
/// broken.
fn skipped_note(imp: ConnImportUi) -> impl IntoView {
    dyn_container(
        move || (imp.skipped.get(), imp.skipped_hidden.get()),
        move |(skipped, hidden)| match skipped_sentence(&skipped, hidden) {
            Some(sentence) => form_hint(sentence).into_any(),
            None => crate::widgets::nothing(),
        },
    )
    .style(|s| s.width_full())
}

/// How many entries were left out, and — while the list is short enough to read
/// — which.
///
/// Pure, and tested below: it is the one piece of this module that decides
/// anything, and "3 more" is exactly the kind of off-by-one that ships.
///
/// `None` for an empty list — the "nothing was skipped" case is the common one,
/// and making it a value the caller matches on is what stops a stray "0 entries
/// were not imported" ever reaching the screen.
/// `hidden` is what `conn_import::SKIPPED_CAP` kept out of the list. It counts
/// toward the total and nowhere else — the whole point of the cap is that those
/// entries were never worth keeping, and the whole point of the count is that a
/// large one is the signal the user needs.
fn skipped_sentence(skipped: &[Skipped], hidden: usize) -> Option<String> {
    /// Past this many, the names stop being a sentence and start being a list
    /// nobody reads.
    const NAMED: usize = 3;
    if skipped.is_empty() && hidden == 0 {
        return None;
    }
    let total = skipped.len() + hidden;
    let head = skipped
        .iter()
        .take(NAMED)
        .map(|s| format!("{} ({})", s.name, crate::i18n::skip_reason_shown(&s.reason)))
        .collect::<Vec<_>>()
        .join(", ");
    let rest = total.saturating_sub(NAMED);
    Some(if rest == 0 {
        tr!("conn-import-skipped", count = total, names = head)
    } else {
        tr!(
            "conn-import-skipped-more",
            count = total,
            names = head,
            rest = rest
        )
    })
}

/// Close, and the one button that writes.
fn footer(
    imp: ConnImportUi,
    run: Rc<dyn Fn()>,
    close: Rc<dyn Fn()>,
    ring: FocusRing,
) -> impl IntoView {
    // **Keyed on whether anything is selected, not on how many.**
    // `action_button` takes a plain `bool`, so the enabled state has to be read
    // in a `dyn_container` — an Import button left enabled over an empty
    // selection imports nothing while looking like it worked. But keying on the
    // *count* rebuilt both buttons on every single tick, and a rebuilt button
    // takes its focus-ring registration with it (the hazard `dump_view`'s footer
    // states, which is why that one keys on `is_empty()` too). The count is
    // needed for the label alone, so it gets its own inner container and the
    // buttons keep their identity across a tick.
    // The "Added N connections." line — shown only while nothing is selected,
    // which is exactly the window between an import finishing and the user
    // asking for another. Ticking a fresh row retires it, so the sentence can
    // never sit beside an enabled Import describing a *previous* press.
    //
    // **Its own row, above the buttons, wrapping.** It used to share the button
    // row as a `text_ellipsis()` slot about 350 px wide, and the sentence it
    // carries is ~190 characters — so the user read `Added 3 connections.
    // Read-only and the en…` and the warning the line exists to deliver, that
    // read-only and the environment badge are *not* carried over, never reached
    // the screen at all. The slot was sized for the sentence it was first given.
    //
    // That row is `modal_footer_noted`'s, not a `v_stack` handed to
    // `modal_footer`: its actions slot never shrinks and is as wide as its
    // content, so the sentence there went unwrapped and took the buttons off the
    // modal's right edge with it.
    //
    // **One decision, read twice.** What the row says and whether it is there
    // at all are both this memo's (`outcome_note`): they were two closures
    // re-deriving the same predicate — the content keyed on `done.filter(idle)`,
    // the `Display::None` on `idle && done.is_some()` — which had to agree for
    // the row to be neither an empty gap nor hidden over a sentence the user
    // needs, and nothing but reading both said they were one.
    let shown_note = floem::reactive::create_memo(move |_| {
        outcome_note(imp.chosen.with(|c| c.is_empty()), imp.done.get())
    });
    let note = dyn_container(
        move || shown_note.get(),
        move |note| match note {
            Some(d) => text(d)
                .style(|s| {
                    s.font_size(theme::font_label())
                        .color(theme::text_dim())
                        .width_full()
                        .min_width(0.0)
                })
                .into_any(),
            None => empty().into_any(),
        },
    )
    // Hidden while there is nothing to say, or the empty row still takes the
    // footer's gap.
    .style(move |s| {
        let hidden = shown_note.with(Option::is_none);
        s.apply_if(hidden, |s| s.display(floem::style::Display::None))
    });

    let bar = dyn_container(
        move || imp.chosen.with(|c| !c.is_empty()),
        move |any| {
            let (run, ring, close) = (run.clone(), ring.clone(), close.clone());
            h_stack((
                action_button(
                    tr!("dialog-close"),
                    ActionKind::Neutral,
                    true,
                    ring.clone(),
                    ACTION_TAB,
                    move || (close)(),
                ),
                // Its label counts, so the label is the reactive part; the
                // button around it is not rebuilt until `any` flips.
                crate::widgets::action_button_dyn(
                    move || import_label(imp.chosen.with(|c| c.len())),
                    ActionKind::Primary,
                    any,
                    ring,
                    ACTION_TAB + 10,
                    move || (run)(),
                ),
            ))
            .style(|s| s.items_center().gap(theme::scaled(8.0)))
            .into_any()
        },
    );

    crate::widgets::modal_footer_noted(note, bar)
}

/// The affirmative button's label, which counts what it is about to do.
///
/// It keeps the word "Import" at zero rather than going blank or reading
/// "Import 0": a disabled button still has to say what it *is*.
fn import_label(count: usize) -> String {
    match count {
        0 => tr!("conn-import-import"),
        n => tr!("conn-import-import-count", count = n),
    }
}

/// What a row's note says — `ImportNote::label`'s words, drawn here so they can
/// be translated; core keeps its English.
fn import_note_label(note: ImportNote) -> String {
    match note {
        ImportNote::NoPassword => tr!("conn-import-note-no-password"),
        ImportNote::AlreadySaved => tr!("conn-import-note-already-saved"),
        ImportNote::UnexpandedPath => tr!("conn-import-note-unexpanded-path"),
        ImportNote::PasswordFromPgpass => tr!("conn-import-note-pgpass"),
        ImportNote::PortAssumed => tr!("conn-import-note-port-assumed"),
        ImportNote::ExternalLogin => tr!("conn-import-note-external-login"),
        ImportNote::SignsInAs(mode) => match mode {
            AuthMode::Windows => tr!("conn-import-note-signs-in-windows"),
            AuthMode::AzureCli => tr!("conn-import-note-signs-in-entra"),
            AuthMode::AwsIam => tr!("conn-import-note-signs-in-aws"),
            AuthMode::GcpIam => tr!("conn-import-note-signs-in-gcp"),
            AuthMode::Password => tr!("conn-import-note-signs-in-password"),
        },
        ImportNote::ThroughSsh => tr!("conn-import-note-through-ssh"),
        ImportNote::RunsInitSql => tr!("conn-import-note-init-sql"),
    }
}

/// The footer's outcome line — the last import's "Added N connections." —
/// shown only while `idle`, nothing being selected, so it never sits beside an
/// enabled Import describing a previous press. `None` is both "no text" and
/// "no row": the footer keys its content and its `Display::None` on this one
/// answer.
fn outcome_note(idle: bool, done: Option<String>) -> Option<String> {
    done.filter(|_| idle)
}

/// The line under a review list too long to show whole — core's
/// `conn_import::row_overflow_sentence`, in the interface's language.
fn row_overflow_note(total: usize) -> Option<String> {
    use schemaic_core::conn_import::ROW_VIEW_CAP;
    let hidden = total.checked_sub(ROW_VIEW_CAP).filter(|n| *n > 0)?;
    Some(tr!(
        "conn-import-row-overflow",
        shown = ROW_VIEW_CAP,
        total = total,
        hidden = hidden
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemaic_core::conn_import::{SkipReason, parse_url};

    /// **The overflow line reads in English exactly as core says it** — the
    /// reasons in the skipped sentence have theirs in `i18n.rs`
    /// (`english_says_every_import_refusal_as_core_does`).
    #[test]
    fn english_says_the_row_overflow_as_core_does() {
        let cap = schemaic_core::conn_import::ROW_VIEW_CAP;
        for total in [0, cap, cap + 1, cap + 1_234] {
            assert_eq!(
                row_overflow_note(total),
                schemaic_core::conn_import::row_overflow_sentence(total),
                "{total}"
            );
        }
    }
    use schemaic_core::connection::Connection;

    /// The interface's words for a row's note are core's in English, for
    /// every note.
    #[test]
    fn english_words_every_import_note_as_core_does() {
        for note in [
            ImportNote::NoPassword,
            ImportNote::AlreadySaved,
            ImportNote::UnexpandedPath,
            ImportNote::PasswordFromPgpass,
            ImportNote::PortAssumed,
            ImportNote::ExternalLogin,
            ImportNote::SignsInAs(AuthMode::Windows),
            ImportNote::SignsInAs(AuthMode::AzureCli),
            ImportNote::SignsInAs(AuthMode::AwsIam),
            ImportNote::SignsInAs(AuthMode::GcpIam),
            ImportNote::SignsInAs(AuthMode::Password),
            ImportNote::ThroughSsh,
            ImportNote::RunsInitSql,
        ] {
            assert_eq!(import_note_label(note), note.label(), "{note:?}");
        }
    }

    /// The found count reads as it did in English before the catalogue — the
    /// skipped sentence and the Import button have tests of their own below.
    #[test]
    fn the_found_count_reads_as_it_did_in_english() {
        assert_eq!(tr!("conn-import-found", count = 1), "1 connection found");
        assert_eq!(tr!("conn-import-found", count = 0), "0 connections found");
        assert_eq!(tr!("conn-import-found", count = 12), "12 connections found");
    }

    fn skip(name: &str) -> Skipped {
        Skipped {
            name: name.to_string(),
            reason: SkipReason::UnsupportedEngine("oracle.16".to_string()),
        }
    }

    #[test]
    fn the_import_button_counts_what_it_will_do() {
        // It keeps the word at zero rather than reading "Import 0": a disabled
        // button still has to say what it *is*.
        assert_eq!(import_label(0), "Import");
        assert_eq!(import_label(1), "Import 1");
        assert_eq!(import_label(7), "Import 7");
    }

    /// The outcome line is there only once an import has finished and until
    /// the next row is ticked — one answer for its text and its row alike.
    #[test]
    fn the_outcome_line_shows_only_between_an_import_and_the_next_selection() {
        let added = || Some("Added 3 connections.".to_string());
        assert_eq!(outcome_note(true, added()), added());
        assert_eq!(
            outcome_note(false, added()),
            None,
            "a fresh tick retires it"
        );
        assert_eq!(outcome_note(true, None), None, "nothing imported yet");
        assert_eq!(outcome_note(false, None), None);
    }

    #[test]
    fn an_empty_list_says_which_kind_of_empty_it_is() {
        let before = empty_message(false, false);
        let during = empty_message(true, false);
        let after = empty_message(false, true);
        assert!(before.contains("Nothing found yet"));
        assert!(during.contains("Looking for"));
        assert!(after.starts_with("No connections found"));
        // The three must differ, or pressing Scan on a machine with no clients
        // installed leaves the screen identical and the button looks dead.
        assert_ne!(before, after);
        assert_ne!(during, after);
        // A scan still running wins over having finished an earlier one.
        assert_eq!(empty_message(true, true), during);
    }

    fn conn(user: &str, host: &str, port: u16, database: &str) -> Connection {
        let mut c = parse_url("mysql://h/d").expect("a base connection");
        c.user = user.to_string();
        c.host = host.to_string();
        c.port = port;
        c.database = database.to_string();
        c
    }

    #[test]
    fn a_rows_target_names_the_login_the_server_and_the_database() {
        assert_eq!(
            row_target(&conn("app", "db.example", 3306, "shop")),
            "app@db.example:3306/shop"
        );
    }

    #[test]
    fn a_target_leaves_out_what_the_source_did_not_say() {
        assert_eq!(
            row_target(&conn("", "db.example", 3306, "shop")),
            "db.example:3306/shop"
        );
        assert_eq!(
            row_target(&conn("app", "db.example", 3306, "")),
            "app@db.example:3306"
        );
        assert_eq!(
            row_target(&conn("", "db.example", 3306, "")),
            "db.example:3306"
        );
    }

    /// **A tunnel is part of where a row points**, so its SSH server is named
    /// on the same line: an imported row routed through somebody else's
    /// bastion read exactly like a direct one.
    #[test]
    fn a_tunnelled_target_names_its_ssh_server() {
        let mut c = conn("app", "db.example", 3306, "shop");
        c.ssh.enabled = true;
        c.ssh.user = "ops".into();
        c.ssh.host = "bastion.example".into();
        c.ssh.port = 2222;
        assert_eq!(
            row_target(&c),
            "app@db.example:3306/shop via ops@bastion.example:2222"
        );
        c.ssh.user.clear();
        c.ssh.port = 22;
        assert_eq!(
            row_target(&c),
            "app@db.example:3306/shop via bastion.example:22"
        );
        c.ssh.enabled = false;
        assert_eq!(row_target(&c), "app@db.example:3306/shop");
    }

    #[test]
    fn a_sqlite_target_is_the_files_name_and_nothing_else() {
        // No host, no port, no user, no database — `endpoint()` is the whole of
        // it, and `:0` would read as a misconfiguration.
        let c = parse_url("sqlite:///var/db/app.db").expect("a sqlite connection");
        assert_eq!(row_target(&c), "app.db");
    }

    #[test]
    fn a_target_never_becomes_a_url() {
        // The invariant this helper exists under: no `scheme://` builder, and
        // above all no password in it.
        let mut c = conn("app", "db.example", 3306, "shop");
        c.password = "s3cret".into();
        let t = row_target(&c);
        assert!(!t.contains("://"), "{t}");
        assert!(!t.contains("s3cret"), "{t}");
    }

    #[test]
    fn one_skipped_entry_is_named_in_the_singular() {
        assert_eq!(
            skipped_sentence(&[skip("Warehouse")], 0).as_deref(),
            Some("1 entry was not imported: Warehouse (unsupported engine (oracle.16)).")
        );
    }

    #[test]
    fn three_skipped_entries_are_all_named() {
        let s = skipped_sentence(&[skip("A"), skip("B"), skip("C")], 0).expect("a sentence");
        assert!(s.starts_with("3 entries were not imported: A ("), "{s}");
        assert!(s.contains("B ("), "{s}");
        assert!(s.ends_with("C (unsupported engine (oracle.16))."), "{s}");
        assert!(!s.contains("more"), "nothing was left unnamed: {s}");
    }

    #[test]
    fn past_three_the_rest_are_counted_not_listed() {
        let s = skipped_sentence(&[skip("A"), skip("B"), skip("C"), skip("D"), skip("E")], 0)
            .expect("a sentence");
        assert!(s.starts_with("5 entries were not imported: "), "{s}");
        assert!(s.ends_with(", and 2 more."), "{s}");
        assert!(!s.contains("D ("), "the fourth is counted, not named: {s}");
    }

    #[test]
    fn nothing_skipped_produces_no_sentence_at_all() {
        // The `None` is what the view matches on; a `Some("0 entries…")` here
        // would put a line on screen saying nothing went wrong.
        assert_eq!(skipped_sentence(&[], 0), None);
    }

    /// **The count is a claim about the file, not about the list.** `SKIPPED_CAP`
    /// stops the app keeping 140,000 `Skipped` structs from a shell history
    /// somebody pointed the picker at — and the one thing that must survive the
    /// cap is the number, because a huge number is exactly what tells the user
    /// they picked the wrong file.
    #[test]
    fn entries_the_cap_kept_out_are_still_counted() {
        let s = skipped_sentence(&[skip("A"), skip("B"), skip("C")], 139_997).expect("a sentence");
        assert!(
            s.starts_with("140000 entries were not imported: A ("),
            "{s}"
        );
        assert!(s.ends_with(", and 139997 more."), "{s}");
    }

    /// And a cap that swallowed *everything* still says so, rather than falling
    /// into the "nothing went wrong" arm.
    #[test]
    fn a_count_with_no_names_is_still_a_sentence() {
        let s = skipped_sentence(&[], 4).expect("a sentence");
        assert!(s.starts_with("4 entries were not imported"), "{s}");
    }
}
