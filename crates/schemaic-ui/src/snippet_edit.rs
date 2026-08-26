//! The snippet editor: the one place a saved query's **body** can be changed.
//!
//! The panel's inline fields cover the two one-word edits (a name, an abbrev)
//! because those are the same act as renaming a tab. A body is not: it is SQL,
//! it is multi-line, and a 300px panel row is the wrong shape for it. So the
//! body gets the modal chrome every other editor here wears, with the name and
//! abbrev alongside it — editing all three in one place is what someone who
//! opened "Edit…" is asking for.
//!
//! It commits through the same three per-field actions the panel's inline fields
//! use (`rename` / `set_abbrev` / `set_body`), and only for the fields that
//! actually changed. There is deliberately no fourth "save everything" action:
//! two paths writing the same field is how the two drift.

use std::rc::Rc;

use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;

use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, action_gap, focus_root_with_ring, form_gap,
    form_setting, modal_footer, modal_h, modal_pad_h, modal_title_owned, modal_w, panel_style,
};
use crate::{FieldCfg, Ui, edit_field, theme};

fn panel_w() -> f64 {
    modal_w(680.0)
}
const PANEL_H: f64 = 560.0;
/// The body box's height before it scrolls. A snippet is usually shorter than a
/// view's `SELECT`, and the name/abbrev rows sit above it.
const BODY_ROWS: usize = 12;

// Opened by setting `OverlayUi::snippet_edit` — the app's `SnippetActions::edit`
// is the one caller, and a second "open me" helper here would be a second way to
// raise the same modal.

pub(crate) fn snippet_edit_overlay(ui: Ui) -> impl IntoView {
    let open = ui.overlay.snippet_edit;
    let library = ui.snippets.items;
    let close = move || open.set(None);

    dyn_container(
        move || open.get(),
        move |target| {
            let Some(id) = target else {
                return empty().into_any();
            };
            // The snippet as it is *now*, read once: the drafts below are what
            // the user edits, and rebuilding this modal mid-edit would take the
            // caret with it (the reason `overlay_open_key` exists for the DDL
            // editors — here the key is just the id, which does not change while
            // the modal is up).
            let Some(snip) = library.with_untracked(|v| v.iter().find(|s| s.id == id).cloned())
            else {
                return empty().into_any();
            };
            let actions = ui.snippet_actions.clone();

            let name = RwSignal::new(snip.name.clone());
            let abbrev = RwSignal::new(snip.abbrev.clone().unwrap_or_default());
            let body = RwSignal::new(snip.body.clone());
            let rows = RwSignal::new(BODY_ROWS);

            let ring = FocusRing::new();
            let root_ring = ring.clone();

            let form = v_stack((
                form_setting(
                    "Name",
                    edit_field(
                        name,
                        FieldCfg {
                            placeholder: "Snippet name",
                            focus: Some((ring.clone(), 10)),
                            ..Default::default()
                        },
                    )
                    .style(|s| s.width(theme::scaled(320.0))),
                ),
                form_setting(
                    "Expansion shortcut",
                    edit_field(
                        abbrev,
                        FieldCfg {
                            placeholder: "none",
                            mono: true,
                            focus: Some((ring.clone(), 20)),
                            ..Default::default()
                        },
                    )
                    .style(|s| s.width(theme::scaled(140.0))),
                ),
                form_setting(
                    "Query",
                    edit_field(
                        body,
                        FieldCfg {
                            multiline: true,
                            no_wrap: true,
                            mono: true,
                            font_size: theme::font_body,
                            max_rows: Some(rows),
                            placeholder: "SELECT …",
                            focus: Some((ring.clone(), 30)),
                            // It's SQL: Tab indents rather than leaving the
                            // field, the same bargain the view editor's body
                            // takes.
                            tab_indents: true,
                            ..Default::default()
                        },
                    )
                    .style(|s| s.width_full()),
                ),
            ))
            .style(|s| s.flex_col().width_full().gap(form_gap()));

            let body_view = crate::widgets::autohide(scroll(form.style(|s| {
                s.width_full()
                    .padding_horiz(modal_pad_h())
                    .padding_vert(theme::scaled(18.0))
            })))
            .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0));

            // Save writes only what changed, through the same per-field actions
            // the panel's inline edits use.
            let save = {
                let actions = actions.clone();
                let before = snip.clone();
                move || {
                    let typed_name = name.get_untracked().trim().to_string();
                    if !typed_name.is_empty() && typed_name != before.name {
                        (actions.rename)(id, typed_name);
                    }
                    let typed_abbrev = abbrev.get_untracked().trim().to_string();
                    let next_abbrev = (!typed_abbrev.is_empty()).then_some(typed_abbrev);
                    if next_abbrev != before.abbrev {
                        (actions.set_abbrev)(id, next_abbrev);
                    }
                    let typed_body = body.get_untracked();
                    if typed_body.trim() != before.body.trim() {
                        (actions.set_body)(id, typed_body);
                    }
                    open.set(None);
                }
            };

            // A snippet with no name or no body is not a snippet; everything
            // else is the user's business. Keyed on that emptiness rather than
            // on the text, so the buttons rebuild when Save turns on and *not*
            // on every keystroke — the caret lives in the form above, but a
            // rebuild per character is still work nobody asked for.
            let ring_actions = ring.clone();
            let save = Rc::new(save);
            let footer = modal_footer(dyn_container(
                move || !name.get().trim().is_empty() && !body.get().trim().is_empty(),
                move |ready: bool| {
                    let save = save.clone();
                    h_stack((
                        action_button(
                            "Cancel",
                            ActionKind::Neutral,
                            true,
                            ring_actions.clone(),
                            ACTION_TAB,
                            close,
                        ),
                        action_button(
                            "Save",
                            ActionKind::Primary,
                            ready,
                            ring_actions.clone(),
                            ACTION_TAB + 10,
                            move || (save)(),
                        ),
                    ))
                    .style(|s| s.flex_row().items_center().gap(action_gap()))
                    .into_any()
                },
            ));

            let close_x: Rc<dyn Fn()> = Rc::new(close);
            let panel = v_stack((
                modal_title_owned(
                    format!("Edit snippet — {}", snip.name),
                    close_x,
                    root_ring.clone(),
                ),
                body_view,
                footer,
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
        if open.get().is_some() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}
