//! *Rename alias*: a name asked for first, and checked before any edit is made.
//!
//! It used to turn every occurrence into one multi-caret selection and let the
//! typing do the rename. Typed a keystroke at a time, the name was never
//! *finished* anywhere a check could stand, so nothing compared it with the
//! names the query already binds — and renaming the outer `o` to an inner
//! query's `i` silently rebinds a correlated `o.id`, which in a
//! `DELETE … WHERE EXISTS` removes every row or none (R3-L5-03). The decision is
//! `intel::alias_rename`'s; this file asks for the name and shows its refusal.

use std::rc::Rc;

use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use schemaic_core::intel::AliasRenameRefusal;

use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, action_gap, focus_root_with_ring, form_gap,
    form_setting, modal_footer, modal_pad_h, modal_title, modal_w, panel_style,
};
use crate::{FieldCfg, edit_field, theme};

/// What raises the dialog: the alias's name as its definition writes it, and
/// the editor's own rename, which re-asks `intel::alias_rename` against the
/// buffer as it is when Rename is pressed.
#[derive(Clone)]
pub struct AliasRenameRequest {
    pub name: String,
    pub apply: AliasRenameApply,
    /// Gives the keyboard back to the editor, however the dialog closes —
    /// its root's own hand-back finds no other root to give it to.
    pub back: Option<Rc<dyn Fn()>>,
}

/// The editor's rename of the alias to a name, or why it will not.
pub type AliasRenameApply = Rc<dyn Fn(&str) -> Result<(), AliasRenameRefusal>>;

fn panel_w() -> f64 {
    modal_w(420.0)
}

// Opened by setting `OverlayUi::alias_rename` — the editor's *Rename alias*
// entry is the one caller.

pub(crate) fn alias_rename_overlay(open: RwSignal<Option<AliasRenameRequest>>) -> impl IntoView {
    dyn_container(
        // Keyed on *whether* it is open: the request does not change while it
        // is up, and a rebuild would take the field's caret with it.
        move || open.with(Option::is_some),
        move |up| {
            let Some(req) = up.then(|| open.get_untracked()).flatten() else {
                return empty().into_any();
            };
            let name = RwSignal::new(req.name.clone());
            let refusal: RwSignal<Option<String>> = RwSignal::new(None);
            let ring = FocusRing::new();
            let root_ring = ring.clone();
            let back = req.back.clone();
            let close: Rc<dyn Fn()> = Rc::new(move || {
                open.set(None);
                if let Some(back) = &back {
                    back();
                }
            });

            let done = close.clone();
            let rename: Rc<dyn Fn()> = Rc::new(move || {
                let typed = name.get_untracked();
                match (req.apply)(&typed) {
                    Ok(()) => done(),
                    Err(why) => refusal.set(Some(why.message(&typed))),
                }
            });
            // A refusal answers the name it was given; editing the name
            // retracts it.
            floem::reactive::create_effect(move |prev: Option<()>| {
                name.track();
                if prev.is_some() {
                    refusal.set(None);
                }
            });

            let submit = rename.clone();
            let form = v_stack((
                form_setting(
                    "New name",
                    edit_field(
                        name,
                        FieldCfg {
                            placeholder: "Alias",
                            focus: Some((ring.clone(), 10)),
                            autofocus: true,
                            on_submit: Some(Rc::new(move || (submit)())),
                            on_escape: Some(close.clone()),
                            ..Default::default()
                        },
                    )
                    .style(|s| s.width_full()),
                ),
                dyn_container(
                    move || refusal.get(),
                    |why| match why {
                        Some(why) => label(move || why.clone())
                            .style(|s| {
                                s.color(theme::error())
                                    .font_size(theme::font_hint())
                                    .width_full()
                            })
                            .into_any(),
                        None => empty().into_any(),
                    },
                ),
            ))
            .style(|s| {
                s.flex_col()
                    .width_full()
                    .gap(form_gap())
                    .padding_horiz(modal_pad_h())
                    .padding_vert(theme::scaled(18.0))
            });

            // Keyed on the name's emptiness, so the buttons rebuild when
            // Rename turns on and not on every keystroke.
            let ring_actions = ring.clone();
            let cancel = close.clone();
            let footer = modal_footer(dyn_container(
                move || name.with(|n| !n.trim().is_empty()),
                move |ready: bool| {
                    let rename = rename.clone();
                    let cancel = cancel.clone();
                    h_stack((
                        action_button(
                            "Cancel",
                            ActionKind::Neutral,
                            true,
                            ring_actions.clone(),
                            ACTION_TAB,
                            move || cancel(),
                        ),
                        action_button(
                            "Rename",
                            ActionKind::Primary,
                            ready,
                            ring_actions.clone(),
                            ACTION_TAB + 10,
                            move || (rename)(),
                        ),
                    ))
                    .style(|s| s.flex_row().items_center().gap(action_gap()))
                    .into_any()
                },
            ));

            let panel = v_stack((
                modal_title("Rename alias", close.clone(), root_ring.clone()),
                form,
                footer,
            ))
            .on_click_stop(|_| {})
            .style(|s| panel_style(s).width(panel_w()));

            // **No click-away**, as the snippet editor: the name is applied only
            // by Rename, and ✕ and Escape close it.
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
        if open.with(Option::is_some) {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}
