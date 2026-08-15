//! The Query History panel: a per-connection, newest-first list of executed
//! statements on the right column (same chrome as the AI / Terminal panels).
//!
//! Each row previews the SQL (whitespace-collapsed, wrapped to ~3 lines then
//! clipped) with its database + relative run time; clicking opens the full query
//! in a new tab (`open_query`). The title carries a trash-2 that clears the
//! *current connection's* history, behind a confirm. The list is filtered from the app-wide
//! `history.entries` signal by the active connection, so switching connections
//! shows only that connection's queries.

use std::rc::Rc;
use std::time::Duration;

use floem::prelude::*;
use floem::reactive::create_memo;

use schemaic_core::db_color::DbColorRule;
use schemaic_core::history::{self, HistoryEntry};

use crate::consts::SEARCH_DEBOUNCE_MS;
use crate::theme::{FONT_BODY, FONT_LABEL};
use crate::widgets::{
    autohide, debounced, highlight_mono, highlight_text, section_title, toolbar_icon,
};
use crate::{FieldCfg, Ui, db_color_dot, edit_field, icons, theme};

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

    // Panel-local search filter (matched against SQL / database / tab name). Local
    // to this panel build — resets when the History panel is re-opened.
    // `search_input` is bound to the box (live); `search` is its debounced mirror
    // that drives filtering + highlighting, so a burst of typing re-filters once.
    let search_input = RwSignal::new(String::new());
    let search = debounced(search_input, Duration::from_millis(SEARCH_DEBOUNCE_MS));

    // The active connection's entries, newest-first (already stored that way),
    // narrowed to the search filter.
    let visible = create_memo(move |_| {
        let conn = active_conn.get();
        let q = search.get();
        entries.with(|v| {
            v.iter()
                .filter(|e| e.conn_id == conn && history::matches_query(e, &q))
                .cloned()
                .collect::<Vec<_>>()
        })
    });

    // Track the search term too (not just `visible`): typing more characters that
    // still match the same set leaves `visible` unchanged (the memo dedups), but the
    // highlight term must still update, so rebuild the rows when either changes.
    let list = dyn_container(
        move || (visible.get(), search.get()),
        move |(rows, q)| {
            if rows.is_empty() {
                // Distinguish "nothing recorded" from "filtered everything out".
                let msg = if q.trim().is_empty() {
                    "No queries yet."
                } else {
                    "No matching queries."
                };
                return text(msg)
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
            let term = {
                let t = q.trim();
                (!t.is_empty()).then(|| t.to_string())
            };
            // Grouped by when they ran, each group under its own header. The
            // headers scroll with the list rather than sticking: this list is a
            // few dozen rows, and a sticky header would need its own layer over
            // a scroll that already owns one for its autohiding bar.
            let items = history::group_by_recency(rows, now)
                .into_iter()
                .enumerate()
                .flat_map(|(gi, (bucket, group))| {
                    let header = group_header(bucket, group.len(), gi == 0);
                    let (oh, term) = (oh.clone(), term.clone());
                    std::iter::once(header).chain(
                        group
                            .into_iter()
                            .map(move |e| {
                                history_row(e, now, oh.clone(), db_colors, term.clone()).into_any()
                            })
                            .collect::<Vec<_>>(),
                    )
                })
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
    //
    // The trash sits a few pixels from the panel chrome and erases the whole
    // connection's history with no undo, so it asks first (the shared `Confirm`
    // modal, same as Drop/Truncate) and is inert when there's nothing to clear.
    // The count is the connection's *total* — the search box narrows the list, not
    // the delete.
    // The count goes through `history::count_conn` rather than an inline filter:
    // it is a promise about what the next click deletes, and `clear_conn` is what
    // fulfils it — a test pins the two together.
    let confirm = ui.overlay.confirm;
    let clearable = create_memo(move |_| {
        let conn = active_conn.get();
        entries.with(|v| history::count_conn(v, conn))
    });
    let trash = toolbar_icon(
        icons::TRASH_2,
        5.0,
        7.0,
        move || clearable.get() > 0,
        move || {
            let n = clearable.get_untracked();
            let clear = clear.clone();
            confirm.set(Some(crate::Confirm {
                title: "Clear query history".to_string(),
                message: format!(
                    "Delete {n} recorded {} for this connection? This can't be undone.",
                    schemaic_core::text::plural(n, "query", "queries")
                ),
                resolve: Rc::new(move |yes| {
                    if yes {
                        (clear)();
                    }
                }),
            }));
        },
    );
    let title_row = h_stack((section_title("QUERY HISTORY"), trash))
        .style(|s| s.width_full().flex_row().items_start().justify_between());

    v_stack((
        title_row,
        // Same 5px above / 10px below the search box as the schema panel (≈15px
        // each visually); spacers (not margins) so the flex-grow scroll's height
        // stays exact (a sibling's vertical margin isn't subtracted → overflow).
        empty().style(|s| s.height(5.0).flex_shrink(0.0_f32)),
        history_search(search_input),
        empty().style(|s| s.height(10.0).flex_shrink(0.0_f32)),
        scrolled,
    ))
    .style(move |s| {
        s.width(right_w.get())
            .flex_shrink(0.0_f32)
            .height_full()
            .flex_col()
            .background(theme::bg_panel())
            .border_left(1.0)
            .border_color(theme::border())
    })
}

// The history search box — same look/dimensions/placeholder as the schema tree's
// `schema_search`. Non-empty narrows the list by SQL / database / tab name.
fn history_search(filter: RwSignal<String>) -> impl IntoView {
    edit_field(
        filter,
        FieldCfg {
            placeholder: "Search…",
            background: theme::bg_chrome,
            clearable: true,
            ..Default::default()
        },
    )
    .style(|s| s.margin_left(12.0).margin_right(12.0).flex_shrink(0.0_f32))
}

/// A recency group's header — `TODAY` and how many ran in it.
///
/// The same weight and colour as the panel's own `section_title`, one step down
/// in size: it divides a list *inside* a section rather than naming one, and at
/// equal size the two read as competing titles.
/// `first` is the topmost header in the list, and the only one that draws its own
/// top rule: every other one follows a row that already ends in the same 1px
/// border, and two of them stacked is a 2px seam at every group boundary but the
/// first — floem doesn't collapse adjacent borders.
fn group_header(bucket: history::Bucket, count: usize, first: bool) -> floem::AnyView {
    let label = text(bucket.label()).style(|s| {
        s.font_size(FONT_LABEL)
            .font_bold()
            .color(theme::text_muted())
    });
    let n = text(count.to_string()).style(|s| {
        s.font_size(FONT_LABEL)
            .color(theme::text_faint())
            .flex_shrink(0.0_f32)
    });
    h_stack((label, empty().style(|s| s.flex_grow(1.0_f32)), n))
        .style(move |s| {
            let s = s
                .width_full()
                .items_center()
                .padding_horiz(12.0)
                .padding_vert(8.0)
                // A band, so the group it opens is legible as a group: a shade of
                // the panel rather than another colour, and not the hover — a
                // header painted in it would read as a hovered row.
                .background(theme::group_header_bg())
                .border_bottom(1.0)
                .border_color(theme::border());
            if first { s.border_top(1.0) } else { s }
        })
        .into_any()
}

/// One history row: the database + when it ran, the SQL preview (monospace, ≤3
/// wrapped lines, clipped), then how the run went. Clicking opens the full SQL
/// in a new tab.
fn history_row(
    entry: HistoryEntry,
    now: u64,
    open_history: Rc<dyn Fn(HistoryEntry)>,
    db_colors: RwSignal<Vec<DbColorRule>>,
    term: Option<String>,
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

    // Monospace: it is SQL, and it is the same face the editor it came from and
    // the diff view use. Still `highlight_*`, so a search term stays marked.
    // +3px above and below the code, on top of the stack's own 4px gap: the SQL
    // is the row's substance and was sitting tight against the two label lines,
    // which made consecutive rows read as one block.
    let preview_view = highlight_mono(preview, term.clone(), FONT_BODY, theme::text, 1.4)
        .style(move |s| {
            s.width_full()
                .max_height(max_h)
                .margin_top(3.0)
                .margin_bottom(3.0)
        })
        .clip();

    // Database name + its identity dot as a tight group, so the footer's `gap(8)`
    // applies only between this group and the timestamp — the dot's spacing from
    // the name is then purely its own `margin_left`.
    let db_group = h_stack((
        highlight_text(db, term.clone(), FONT_LABEL, theme::text_dim, false, 1.0)
            .style(|s| s.min_width(0.0)),
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
    // Above the SQL, not below it: the database and the age are what you scan a
    // history list by, and the statement under them is what you read once one of
    // them has caught your eye.
    let heading = h_stack((
        db_group,
        empty().style(|s| s.flex_grow(1.0_f32)),
        text(when).style(|s| {
            s.font_size(FONT_LABEL)
                .color(theme::text_faint())
                .flex_shrink(0.0_f32)
        }),
    ))
    .style(|s| s.items_center().width_full().gap(8.0));

    // What the run turned out to be, under the SQL: `5ms · 100 rows`, or
    // `4ms · Failed` in red. A success is not labelled — the row count *is* the
    // success, and a word saying so on every row would drown the one row that
    // failed. Absent entirely when the outcome is unknown (recorded before this
    // was tracked, cancelled, or a run the app didn't outlive), since every part
    // of the line would then be a guess.
    let outcome_row: Option<floem::AnyView> = match entry.outcome {
        history::Outcome::Unknown => None,
        outcome => {
            let failed = outcome == history::Outcome::Failed;
            // Duration first, then either the rows or the failure — the two are
            // exclusive: a run that failed produced nothing to count.
            let mut facts: Vec<String> = Vec::new();
            if let Some(ms) = entry.duration_ms {
                facts.push(history::format_duration(ms));
            }
            if let Some(n) = entry.rows.filter(|_| !failed) {
                // `200000+ rows` when the fetch stopped at the cap: that number
                // is what came back, not what the query returned, and only the
                // `+` says so once the grid is gone. Always plural there — the
                // count means "at least this many", so it is never one.
                if entry.rows_capped {
                    facts.push(format!("{n}+ rows"));
                } else {
                    let word = schemaic_core::text::plural(n as usize, "row", "rows");
                    facts.push(format!("{n} {word}"));
                }
            }
            // The trailing separator belongs to the facts, so a row with none of
            // them doesn't open with a stray "· ".
            let lead = if facts.is_empty() {
                String::new()
            } else if failed {
                format!("{} · ", facts.join(" · "))
            } else {
                facts.join(" · ")
            };
            let row = h_stack((
                text(lead).style(|s| {
                    s.font_size(FONT_LABEL)
                        .color(theme::text_faint())
                        .min_width(0.0)
                }),
                // Only a failure says anything, and it is the only colour here.
                text(if failed { "Failed" } else { "" }).style(move |s| {
                    s.font_size(FONT_LABEL)
                        .color(theme::error())
                        .flex_shrink(0.0_f32)
                }),
            ))
            .style(|s| s.items_center().width_full());
            Some(row.into_any())
        }
    };

    // The originating tab's custom name (if any) on its own line below the footer,
    // as a small capsule that hugs its text (wrapped in a row so it doesn't stretch
    // full-width). Unnamed tabs add no extra row.
    let named = entry.tab_name.clone().filter(|n| !n.trim().is_empty());
    let name_row: Option<floem::AnyView> = named.map(|n| {
        let capsule =
            highlight_text(n, term.clone(), FONT_LABEL, theme::text, false, 1.0).style(|s| {
                s.padding_horiz(7.0)
                    .padding_vert(3.0)
                    .background(theme::capsule_bg())
                    .border_radius(4.0)
                    .flex_shrink(0.0_f32)
            });
        // +2px over the v_stack's 4px gap → 6px between the table and the name.
        h_stack((capsule,))
            .style(|s| s.width_full().margin_top(2.0))
            .into_any()
    });
    // Both extra lines are optional and independent, so the stack is built from
    // what there is rather than from one arm per combination.
    let mut rows: Vec<floem::AnyView> = vec![heading.into_any(), preview_view.into_any()];
    rows.extend(outcome_row);
    rows.extend(name_row);
    let inner = floem::views::stack_from_iter(rows).style(|s| s.flex_col().width_full().gap(4.0));

    container(inner)
        .on_click_stop(move |_| (open_history)(entry_click.clone()))
        .style(|s| {
            s.width_full()
                .padding_horiz(12.0)
                .padding_vert(9.0)
                .border_bottom(1.0)
                .border_color(theme::border())
                .hover(|s| s.background(theme::row_hover_soft()))
        })
}
