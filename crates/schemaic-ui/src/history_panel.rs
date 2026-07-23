//! The Query History panel: a per-connection, newest-first list of executed
//! statements on the right column (same chrome as the AI / Terminal panels).
//!
//! Each row previews the SQL (whitespace-collapsed, wrapped to ~3 lines then
//! clipped) with its database + relative run time; clicking opens the full query
//! in a new tab (`open_query`). The title carries a trash-2 that clears the
//! *current connection's* history. The list is filtered from the app-wide
//! `history.entries` signal by the active connection, so switching connections
//! shows only that connection's queries.

use std::rc::Rc;

use floem::prelude::*;
use floem::reactive::create_memo;

use schemaic_core::db_color::DbColorRule;
use schemaic_core::history::{self, HistoryEntry};

use crate::theme::{FONT_BODY, FONT_LABEL};
use crate::widgets::{autohide, section_title, toolbar_icon};
use crate::{Ui, db_color_dot, icons, theme};

/// Current wall-clock time, unix millis (for relative "x ago" labels).
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn history_panel(ui: Ui) -> impl IntoView {
    let entries = ui.history.entries;
    let active_conn = ui.conn.active_conn;
    let right_w = ui.layout.right_w;
    let open_history = ui.history_actions.open.clone();
    let clear = ui.history_actions.clear.clone();
    let db_colors = ui.db_colors;

    // The active connection's entries, newest-first (already stored that way).
    let visible = create_memo(move |_| {
        let conn = active_conn.get();
        entries.with(|v| {
            v.iter()
                .filter(|e| e.conn_id == conn)
                .cloned()
                .collect::<Vec<_>>()
        })
    });

    let list = dyn_container(
        move || visible.get(),
        move |rows| {
            if rows.is_empty() {
                return text("No queries yet.")
                    .style(|s| {
                        s.font_size(14.0)
                            .color(theme::text_muted())
                            .padding_top(10.0)
                            .padding_left(12.0)
                    })
                    .into_any();
            }
            let now = now_millis();
            let oh = open_history.clone();
            let items = rows
                .into_iter()
                .map(move |e| history_row(e, now, oh.clone(), db_colors))
                .collect::<Vec<_>>();
            v_stack_from_iter(items)
                .style(|s| s.flex_col().width_full())
                .into_any()
        },
    )
    .style(|s| s.flex_col().width_full());

    let scrolled =
        autohide(scroll(list)).style(|s| s.flex_grow(1.0_f32).width_full().min_height(0.0));

    // Title row: "QUERY HISTORY" left; a trash-2 (clear) right.
    let trash = toolbar_icon(icons::TRASH_2, 5.0, 7.0, || true, move || (clear)());
    let title_row = h_stack((section_title("QUERY HISTORY"), trash))
        .style(|s| s.width_full().flex_row().items_start().justify_between());

    v_stack((title_row, scrolled)).style(move |s| {
        s.width(right_w.get())
            .flex_shrink(0.0_f32)
            .height_full()
            .flex_col()
            .background(theme::bg_panel())
            .border_left(1.0)
            .border_color(theme::border())
    })
}

/// One history row: SQL preview (≤3 wrapped lines, clipped) over a
/// database + relative-time footer. Clicking opens the full SQL in a new tab.
fn history_row(
    entry: HistoryEntry,
    now: u64,
    open_history: Rc<dyn Fn(HistoryEntry)>,
    db_colors: RwSignal<Vec<DbColorRule>>,
) -> impl IntoView {
    // Full entry for the click handler — restores SQL + database + tab name.
    let entry_click = entry.clone();
    let preview = history::preview(&entry.sql);
    let db = entry.database.clone().unwrap_or_else(|| "—".to_string());
    let when = history::relative_time(entry.ts, now);
    // Key for the DB-identity dot (only drawn when this run's database has a colour).
    let dot_conn = entry.conn_id;
    let dot_db = entry.database.clone();

    // ~3 lines: FONT_BODY (13) × 1.4 line-height × 3, clipped.
    let max_h = (FONT_BODY as f64) * 1.4 * 3.0;

    let preview_view = text(preview)
        .style(move |s| {
            s.font_size(FONT_BODY)
                .color(theme::text())
                .line_height(1.4)
                .width_full()
                .max_height(max_h)
        })
        .clip();

    // Database name + its identity dot as a tight group, so the footer's `gap(8)`
    // applies only between this group and the timestamp — the dot's spacing from
    // the name is then purely its own `margin_left`.
    let db_group = h_stack((
        text(db).style(|s| {
            s.font_size(FONT_LABEL)
                .color(theme::text_dim())
                .min_width(0.0)
        }),
        // Identity dot next to the database name (colour set in the schema tree).
        db_color_dot(
            db_colors,
            move || dot_db.clone().map(|d| (dot_conn, d)),
            5.0,
            0.0,
            1.0,
        ),
    ))
    .style(|s| s.items_center().min_width(0.0));
    let footer = h_stack((
        db_group,
        empty().style(|s| s.flex_grow(1.0_f32)),
        text(when).style(|s| {
            s.font_size(FONT_LABEL)
                .color(theme::text_faint())
                .flex_shrink(0.0_f32)
        }),
    ))
    .style(|s| s.items_center().width_full().gap(8.0));

    // The originating tab's custom name (if any) on its own line below the footer,
    // as a small capsule that hugs its text (wrapped in a row so it doesn't stretch
    // full-width). Unnamed tabs add no extra row.
    let named = entry.tab_name.clone().filter(|n| !n.trim().is_empty());
    let inner: floem::AnyView = match named {
        Some(n) => {
            let capsule = text(n).style(|s| {
                s.font_size(FONT_LABEL)
                    .color(theme::text())
                    .padding_horiz(7.0)
                    .padding_vert(3.0)
                    .background(theme::capsule_bg())
                    .border_radius(4.0)
                    .flex_shrink(0.0_f32)
            });
            // +2px over the v_stack's 4px gap → 6px between the table and the name.
            let name_row = h_stack((capsule,)).style(|s| s.width_full().margin_top(2.0));
            v_stack((preview_view, footer, name_row))
                .style(|s| s.flex_col().width_full().gap(4.0))
                .into_any()
        }
        None => v_stack((preview_view, footer))
            .style(|s| s.flex_col().width_full().gap(4.0))
            .into_any(),
    };

    container(inner)
        .on_click_stop(move |_| (open_history)(entry_click.clone()))
        .style(|s| {
            s.width_full()
                .padding_horiz(12.0)
                .padding_vert(9.0)
                .border_bottom(1.0)
                .border_color(theme::border())
                .hover(|s| s.background(theme::row_hover()))
        })
}
