//! *Find usages*: every place the statement under the click writes a name —
//! a table alias, a CTE, an output column's alias — as `intel::usages_at`
//! binds it, listed by line. A row jumps the editor to it.
//!
//! Raised from the SQL editor's context menu, like *Rename alias* beside it,
//! and the same names: what this lists is exactly what a rename would write.

use std::rc::Rc;

use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;

use crate::theme;
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, dismiss_layer, focus_root_with_ring,
    modal_footer, modal_pad_h, modal_title_owned, modal_w, panel_style,
};

/// What raises the list.
#[derive(Clone)]
pub struct UsagesRequest {
    /// The name, as its definition writes it.
    pub name: String,
    /// `(start, end, line, text)` per usage — the line 1-based, its text
    /// trimmed (`intel::usage_lines`).
    pub rows: Vec<(usize, usize, usize, String)>,
    /// Select `start..end` in the editor and bring it into view.
    pub jump: Rc<dyn Fn(usize, usize)>,
    /// Gives the keyboard back to the editor, however the list closes.
    pub back: Option<Rc<dyn Fn()>>,
}

fn panel_w() -> f64 {
    modal_w(560.0)
}

pub(crate) fn usages_overlay(open: RwSignal<Option<UsagesRequest>>) -> impl IntoView {
    dyn_container(
        move || open.with(Option::is_some),
        move |up| {
            let Some(req) = up.then(|| open.get_untracked()).flatten() else {
                return empty().into_any();
            };
            let ring = FocusRing::new();
            let root_ring = ring.clone();
            let back = req.back.clone();
            let close: Rc<dyn Fn()> = Rc::new(move || {
                open.set(None);
                if let Some(back) = &back {
                    back();
                }
            });
            let n = req.rows.len();
            let rows = v_stack_from_iter(req.rows.iter().cloned().map({
                let jump = req.jump.clone();
                let close = close.clone();
                move |(a, b, line, text)| {
                    let (jump, close) = (jump.clone(), close.clone());
                    h_stack((
                        text_line(format!("{line}"))
                            .style(|s| s.color(theme::text_dim()).min_width(theme::scaled(44.0))),
                        text_line(text).style(|s| s.color(theme::text()).min_width(0.0)),
                    ))
                    .on_click_stop(move |_| {
                        close();
                        jump(a, b);
                    })
                    .style(|s| {
                        s.width_full()
                            .items_center()
                            .gap(theme::scaled(8.0))
                            .padding_horiz(theme::scaled(8.0))
                            .padding_vert(theme::scaled(4.0))
                            .border_radius(4.0)
                            .hover(|s| s.background(theme::row_hover_soft()))
                    })
                }
            }))
            .style(|s| s.flex_col().width_full());
            let body = scroll(rows).style(|s| {
                s.width_full()
                    .max_height(theme::scaled(360.0))
                    .padding_horiz(modal_pad_h())
                    .padding_vert(theme::scaled(12.0))
            });
            let done = close.clone();
            let footer = modal_footer(action_button(
                "Close",
                ActionKind::Primary,
                true,
                ring.clone(),
                ACTION_TAB,
                move || done(),
            ));
            let panel = v_stack((
                modal_title_owned(
                    format!(
                        "{n} {} of {}",
                        if n == 1 { "usage" } else { "usages" },
                        req.name
                    ),
                    close.clone(),
                    root_ring.clone(),
                ),
                body,
                footer,
            ))
            .on_click_stop(|_| {})
            .style(|s| panel_style(s).width(panel_w()));
            let esc = close.clone();
            focus_root_with_ring(stack((dismiss_layer(move || close()), panel)), root_ring)
                .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| esc())
                .style(|s| {
                    s.size_full()
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

/// One line of monospace text, cut rather than wrapped.
fn text_line(t: String) -> impl IntoView {
    text(t).style(|s| {
        s.font_family(crate::consts::MONO_FAMILY.to_string())
            .font_size(theme::font_body())
            .text_ellipsis()
    })
}
