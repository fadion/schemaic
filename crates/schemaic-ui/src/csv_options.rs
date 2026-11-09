//! *CSV read options*: the delimiter and header row an **Open data file**
//! view reads its CSV with, when DuckDB's sniffer guesses them wrong.
//!
//! It asks, and hands the answer on: the request's `apply` opens the view
//! editor on the view with its body rewritten (`data_file::csv_view_body`,
//! `view_editor::open_for_view_with_body`), and the editor's Preview SQL shows
//! the replace as for any other edit. So this dialog writes no DDL, and the
//! generated-DDL path stays the one it always is.

use std::rc::Rc;

use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use schemaic_core::data_file::{CsvDelimiter, CsvHeader, CsvReadOptions};

use crate::i18n::tr;
use crate::theme;
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, action_gap, focus_root_with_ring, form_gap,
    form_setting, modal_footer, modal_pad_h, modal_title, modal_w, panel_style, segmented_tabs,
};

/// What raises the dialog: the view, the options it reads with now, and what
/// to do with the options chosen.
#[derive(Clone)]
pub struct CsvOptionsRequest {
    pub view: String,
    pub options: CsvReadOptions,
    pub apply: Rc<dyn Fn(CsvReadOptions)>,
}

fn panel_w() -> f64 {
    modal_w(460.0)
}

/// A delimiter's label in the interface's language.
fn delimiter_label(d: CsvDelimiter) -> String {
    match d {
        CsvDelimiter::Auto => tr!("csv-options-detect"),
        CsvDelimiter::Comma => tr!("csv-options-comma"),
        CsvDelimiter::Semicolon => tr!("csv-options-semicolon"),
        CsvDelimiter::Tab => tr!("csv-options-tab"),
        CsvDelimiter::Pipe => tr!("csv-options-pipe"),
    }
}

/// A header choice's label in the interface's language.
fn header_label(h: CsvHeader) -> String {
    match h {
        CsvHeader::Auto => tr!("csv-options-detect"),
        CsvHeader::Yes => tr!("csv-options-yes"),
        CsvHeader::No => tr!("csv-options-no"),
    }
}

// Opened by setting `OverlayUi::csv_options` — the schema tree's *CSV read
// options* entry on an Open data file view is the caller.

pub(crate) fn csv_options_overlay(open: RwSignal<Option<CsvOptionsRequest>>) -> impl IntoView {
    dyn_container(
        move || open.with(Option::is_some),
        move |up| {
            let Some(req) = up.then(|| open.get_untracked()).flatten() else {
                return empty().into_any();
            };
            let delimiter = RwSignal::new(req.options.delimiter);
            let header = RwSignal::new(req.options.header);
            let ring = FocusRing::new();
            let root_ring = ring.clone();
            let close: Rc<dyn Fn()> = Rc::new(move || open.set(None));

            let form = v_stack((
                form_setting(
                    tr!("csv-options-delimiter"),
                    segmented_tabs(
                        CsvDelimiter::ALL
                            .into_iter()
                            .map(|d| (delimiter_label(d), d))
                            .collect(),
                        delimiter,
                    ),
                ),
                form_setting(
                    tr!("csv-options-header"),
                    segmented_tabs(
                        [CsvHeader::Auto, CsvHeader::Yes, CsvHeader::No]
                            .into_iter()
                            .map(|h| (header_label(h), h))
                            .collect(),
                        header,
                    ),
                ),
                label(|| tr!("csv-options-hint")).style(|s| {
                    s.color(theme::text_dim())
                        .font_size(theme::font_hint())
                        .width_full()
                }),
            ))
            .style(|s| {
                s.flex_col()
                    .width_full()
                    .gap(form_gap())
                    .padding_horiz(modal_pad_h())
                    .padding_vert(theme::scaled(18.0))
            });

            let apply = req.apply.clone();
            let done = close.clone();
            let cancel = close.clone();
            let footer = modal_footer(
                h_stack((
                    action_button(
                        tr!("dialog-cancel"),
                        ActionKind::Neutral,
                        true,
                        ring.clone(),
                        ACTION_TAB,
                        move || cancel(),
                    ),
                    action_button(
                        tr!("csv-options-continue"),
                        ActionKind::Primary,
                        true,
                        ring.clone(),
                        ACTION_TAB + 10,
                        move || {
                            let chosen = CsvReadOptions {
                                delimiter: delimiter.get_untracked(),
                                header: header.get_untracked(),
                            };
                            // Closed first: the editor it opens is a modal of
                            // its own, and two up at once paint two panels.
                            done();
                            (apply)(chosen);
                        },
                    ),
                ))
                .style(|s| s.flex_row().items_center().gap(action_gap())),
            );

            let panel = v_stack((
                modal_title(
                    tr!("csv-options-title", view = req.view.as_str()),
                    close.clone(),
                    root_ring.clone(),
                ),
                form,
                footer,
            ))
            .on_click_stop(|_| {})
            .style(|s| panel_style(s).width(panel_w()));

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
