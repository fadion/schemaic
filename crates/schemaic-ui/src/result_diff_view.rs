//! **Compare results** — two results of one tab side by side as their
//! differences (`core::result_diff`): rows added, removed and changed,
//! matched by a key column, raised from a result chip's *Compare with*.
//!
//! Of the two results as they were read — a kept one is a snapshot, a live one
//! is what the panel holds when the comparison opens — and of their rows only:
//! nothing is fetched.

use std::rc::Rc;
use std::sync::Arc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;

use schemaic_core::model::ResultSet;
use schemaic_core::result_diff::{self, ResultDiff};

use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, autohide, dismiss_layer,
    focus_root_with_ring, modal_body_h, modal_footer_split, modal_pad_h, modal_title_owned,
    panel_style,
};
use crate::{OverlayUi, theme};

/// The two results the modal compares, and what each is called.
#[derive(Clone)]
pub struct DiffTarget {
    /// The earlier result — "removed" rows are its.
    pub left: Arc<ResultSet>,
    pub left_label: String,
    /// The later one — "added" rows are its.
    pub right: Arc<ResultSet>,
    pub right_label: String,
}

/// The key picker's entry for "no key — the whole row".
const WHOLE_ROW: &str = "Whole row";

/// The most rows of each kind the list draws; the rest are counted.
const SHOWN: usize = 500;

fn panel_w() -> f64 {
    theme::scaled(820.0)
}

pub(crate) fn result_diff_overlay(overlay: OverlayUi) -> impl IntoView {
    let target = overlay.result_diff;
    dyn_container(
        move || target.with(Option::is_some),
        move |open| {
            let Some(t) = open.then(|| target.get_untracked()).flatten() else {
                return empty().into_any();
            };
            let close: Rc<dyn Fn()> = Rc::new(move || target.set(None));
            let ring = FocusRing::new();
            let shared = result_diff::shared_columns(&t.left, &t.right);
            let key = RwSignal::new(
                result_diff::suggested_key(&t.left, &t.right)
                    .into_iter()
                    .next(),
            );
            let mut options = vec![WHOLE_ROW.to_string()];
            options.extend(shared.iter().cloned());
            let picker = crate::table_designer::focusable_owned_dropdown(
                move || key.get().unwrap_or_else(|| WHOLE_ROW.to_string()),
                options,
                || theme::scaled(220.0),
                ring.clone(),
                10,
                move |picked| key.set((picked != WHOLE_ROW).then_some(picked)),
            );
            let controls = h_stack((
                v_stack((
                    text("Match rows by").style(crate::widgets::form_label_style),
                    picker,
                ))
                .style(|s| s.flex_col().gap(theme::scaled(6.0))),
                text(format!("{}  →  {}", t.left_label, t.right_label)).style(|s| {
                    s.color(theme::text_dim())
                        .font_size(theme::font_label())
                        .margin_top(theme::scaled(22.0))
                        .min_width(0.0)
                }),
            ))
            .style(|s| s.gap(theme::scaled(16.0)).items_start().width_full());
            let body = {
                let t = t.clone();
                dyn_container(move || key.get(), move |k| diff_body(&t, k))
                    .style(|s| s.width_full())
            };
            let title =
                modal_title_owned("Compare results".to_string(), close.clone(), ring.clone());
            let done = close.clone();
            let footer = modal_footer_split(
                empty(),
                action_button(
                    "Close",
                    ActionKind::Primary,
                    true,
                    ring.clone(),
                    ACTION_TAB,
                    move || (done)(),
                ),
            );
            let panel = v_stack((
                title,
                v_stack((controls, body)).style(|s| {
                    s.flex_col()
                        .gap(theme::scaled(12.0))
                        .padding(modal_pad_h())
                        .width_full()
                }),
                footer,
            ))
            .on_click_stop(|_| {})
            .style(|s| {
                panel_style(s)
                    .background(theme::bg_panel())
                    .width(panel_w())
            });
            let esc = close.clone();
            focus_root_with_ring(stack((dismiss_layer(move || close()), panel)), ring)
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
        if target.with(Option::is_some) {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// The summary, the notes and the three lists, for `key`.
fn diff_body(t: &DiffTarget, key: Option<String>) -> AnyView {
    let key: Vec<String> = key.into_iter().collect();
    let d: ResultDiff = match result_diff::diff(&t.left, &t.right, &key) {
        Ok(d) => d,
        Err(e) => {
            return text(e.to_string())
                .style(|s| s.color(theme::error()).font_size(theme::font_body()))
                .into_any();
        }
    };
    let mut sections: Vec<AnyView> = vec![
        text(d.summary())
            .style(|s| s.font_size(theme::font_body()).color(theme::text()))
            .into_any(),
    ];
    let mut notes = Vec::new();
    if !d.only_left.is_empty() {
        notes.push(format!(
            "Only in {}: {}",
            t.left_label,
            d.only_left.join(", ")
        ));
    }
    if !d.only_right.is_empty() {
        notes.push(format!(
            "Only in {}: {}",
            t.right_label,
            d.only_right.join(", ")
        ));
    }
    if d.repeated_keys > 0 {
        notes.push(format!(
            "{} {} a key already taken on its side, and {} left out.",
            d.repeated_keys,
            schemaic_core::text::plural(d.repeated_keys, "row repeats", "rows repeat"),
            schemaic_core::text::plural(d.repeated_keys, "is", "are")
        ));
    }
    if t.left.truncated || t.right.truncated {
        notes.push("A result was capped: rows past its cap are not compared.".to_string());
    }
    for n in notes {
        sections.push(
            text(n)
                .style(|s| s.font_size(theme::font_hint()).color(theme::text_faint()))
                .into_any(),
        );
    }
    let list =
        |title: String, color: fn() -> floem::peniko::Color, lines: Vec<String>, total: usize| {
            let mut rows: Vec<AnyView> = lines
                .into_iter()
                .map(|l| {
                    text(l)
                        .style(move |s| {
                            s.font_family(crate::MONO_FAMILY.to_string())
                                .font_size(theme::font_label())
                                .color(color())
                                .min_width(0.0)
                        })
                        .into_any()
                })
                .collect();
            if total > SHOWN {
                rows.push(
                    text(format!("and {} more", total - SHOWN))
                        .style(|s| s.font_size(theme::font_hint()).color(theme::text_faint()))
                        .into_any(),
                );
            }
            crate::widgets::fact_section_owned(title, rows, || theme::scaled(4.0))
        };
    if !d.right_only.is_empty() {
        let lines = d
            .right_only
            .iter()
            .take(SHOWN)
            .map(|&r| format!("+ {}", result_diff::row_line(&t.right, r)))
            .collect();
        sections.push(list(
            format!("Added ({})", d.right_only.len()),
            theme::grid_edit_staged,
            lines,
            d.right_only.len(),
        ));
    }
    if !d.left_only.is_empty() {
        let lines = d
            .left_only
            .iter()
            .take(SHOWN)
            .map(|&r| format!("− {}", result_diff::row_line(&t.left, r)))
            .collect();
        sections.push(list(
            format!("Removed ({})", d.left_only.len()),
            theme::error,
            lines,
            d.left_only.len(),
        ));
    }
    if !d.changed.is_empty() {
        let lines = d
            .changed
            .iter()
            .take(SHOWN)
            .map(|c| format!("~ {}", result_diff::change_line(&t.left, &t.right, &key, c)))
            .collect();
        sections.push(list(
            format!("Changed ({})", d.changed.len()),
            theme::text,
            lines,
            d.changed.len(),
        ));
    }
    let content =
        v_stack_from_iter(sections).style(|s| s.flex_col().gap(theme::scaled(14.0)).width_full());
    autohide(scroll(
        container(content).style(|s| s.width_full().padding_bottom(theme::scaled(4.0))),
    ))
    .style(|s| s.width_full().max_height(modal_body_h(440.0)))
    .into_any()
}
