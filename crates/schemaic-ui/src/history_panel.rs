//! The Query History panel: a per-connection, newest-first list of executed
//! statements on the right column (same chrome as the AI / Terminal panels).
//!
//! Each row previews the SQL (whitespace-collapsed, wrapped to ~3 lines then
//! clipped, syntax-coloured by the editor's own lexer on the editor's surface)
//! with its database + relative run time; clicking opens the full query in a new
//! tab (`open_query`). The title carries a trash-2 that clears the
//! *current connection's* history, behind a confirm. The list is filtered from the app-wide
//! `history.entries` signal by the active connection, so switching connections
//! shows only that connection's queries.

use std::rc::Rc;
use std::time::Duration;

use floem::prelude::*;
use floem::reactive::create_memo;

use schemaic_core::db_color::DbColorRule;
use schemaic_core::history::{self, HistoryEntry};
use schemaic_core::intel::SqlDialect;

use crate::consts::SEARCH_DEBOUNCE_MS;
use crate::theme::{font_body, font_label};
use crate::widgets::{
    MenuEntry, autohide, debounced, highlight_sql_mono, highlight_text, menu_panel_width,
    section_title, toolbar_icon,
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
    let connections = ui.conn.connections;
    let open_history = ui.history_actions.open.clone();
    let clear = ui.history_actions.clear.clone();
    let db_colors = ui.db_colors;
    let overlay = ui.overlay;
    let menus = crate::widgets::MenuFlags::of(&ui);

    // The row menu, raised at the pointer through the app-wide popup channel —
    // the same route the snippet library's rows take. Built here rather than
    // inside `history_row` so the row keeps taking only what it draws with:
    // `overlay` and `menus` are panel-wide and Copy, and threading them through
    // every row would say otherwise.
    let open_menu: Rc<dyn Fn(HistoryEntry)> = {
        let open = open_history.clone();
        let remove = ui.history_actions.remove.clone();
        Rc::new(move |entry: HistoryEntry| {
            menus.close_except(Some(crate::widgets::MenuId::Popup));
            overlay.popup_anchor.set(None);
            let entries = row_menu(&entry, &open, &remove);
            // Measured, not a constant — `popup_width` is the panel's
            // `min_width`, so a number picked by eye is a floor the rows can't
            // pull back in. Same reasoning as the snippet library's.
            overlay.popup_width.set(menu_panel_width(&entries));
            overlay.popup_menu.set(Some(entries));
        })
    };

    // Which lexer colours the previews. The list is filtered to the **active
    // connection**, so every row on screen ran against it and its dialect is the
    // right one for all of them — the same reasoning (and the same memo) the
    // snippet library uses, and for the same reason it reads the connection
    // rather than the active tab.
    let dialect = create_memo(move |_| {
        let cid = active_conn.get();
        connections
            .with(|cs| {
                cs.iter()
                    .find(|c| c.id == cid)
                    .map(|c| SqlDialect::from_db_type(&c.db_type))
            })
            .unwrap_or_default()
    });

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
    // The dialect rides along for the same reason: switching connections normally
    // changes `visible` as well, but two connections with no history between them
    // both yield an empty list, and the memo would hold the rows at the previous
    // engine's lexer.
    let list = dyn_container(
        move || (visible.get(), search.get(), dialect.get()),
        move |(rows, q, dialect)| {
            if rows.is_empty() {
                // Distinguish "nothing recorded" from "filtered everything out".
                let msg = if q.trim().is_empty() {
                    "No queries yet."
                } else {
                    "No matching queries."
                };
                return text(msg)
                    .style(|s| {
                        s.font_size(theme::scaled_font(14.0))
                            .color(theme::text_muted())
                            .padding_top(theme::scaled(10.0))
                            .padding_left(theme::scaled(12.0))
                    })
                    .into_any();
            }
            let now = now_millis();
            let oh = open_history.clone();
            let om = open_menu.clone();
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
                    let (oh, om, term) = (oh.clone(), om.clone(), term.clone());
                    std::iter::once(header).chain(
                        group
                            .into_iter()
                            .map(move |e| {
                                history_row(
                                    e,
                                    now,
                                    oh.clone(),
                                    om.clone(),
                                    db_colors,
                                    term.clone(),
                                    dialect,
                                )
                                .into_any()
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
    )
    // Named for what it clears: this panel lists one connection's queries, and a
    // bare "Clear history" beside a filtered list reads as "all of it".
    .tooltip(|| text("Clear this connection's history…").style(crate::widgets::tooltip_style));
    let title_row = h_stack((section_title("QUERY HISTORY"), trash))
        .style(|s| s.width_full().flex_row().items_start().justify_between());

    v_stack((
        title_row,
        // Same 5px above / 10px below the search box as the schema panel (≈15px
        // each visually); spacers (not margins) so the flex-grow scroll's height
        // stays exact (a sibling's vertical margin isn't subtracted → overflow).
        empty().style(|s| s.height(theme::scaled(5.0)).flex_shrink(0.0_f32)),
        history_search(search_input),
        empty().style(|s| s.height(theme::scaled(10.0)).flex_shrink(0.0_f32)),
        scrolled,
    ))
    .style(move |s| {
        s.width(crate::widgets::right_panel_w().get())
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
    .style(|s| {
        s.margin_left(theme::scaled(12.0))
            .margin_right(theme::scaled(12.0))
            .flex_shrink(0.0_f32)
    })
}

/// A recency group's header — `TODAY` and how many ran in it.
///
/// The same weight as the panel's own `section_title`, one step down in size: it
/// divides a list *inside* a section rather than naming one, and at equal size
/// the two read as competing titles. In the accent, which is where this list
/// differs from a section title — the bands are the only thing a long history
/// is scanned by, so they get the colour the eye already uses to find the start
/// of a thing (it is the same accent the AI panel names Claude's turns in). The
/// count rides along at 60%: it belongs to the band rather than beside it, and
/// at full strength two accents of equal weight compete across the row.
/// `first` is the topmost header in the list, and the only one that draws its own
/// top rule: every other one follows a row that already ends in the same 1px
/// border, and two of them stacked is a 2px seam at every group boundary but the
/// first — floem doesn't collapse adjacent borders.
fn group_header(bucket: history::Bucket, count: usize, first: bool) -> floem::AnyView {
    let label = text(bucket.label())
        .style(|s| s.font_size(font_label()).font_bold().color(theme::accent()));
    // **`text_dim`, not a faded accent.** The count has to recede from the bold
    // label beside it — two accents of equal weight compete across the row —
    // but it is also a number the reader is meant to read, and an alpha on the
    // accent got there by making it *dimmer than legible*: 2.32:1 in Light,
    // under AA and under the large/bold level both. This is the same colour the
    // AI panel's code-block actions use on this exact surface, where the gate
    // already holds it to `Body`.
    let n = text(count.to_string()).style(|s| {
        s.font_size(font_label())
            .color(theme::text_dim())
            .flex_shrink(0.0_f32)
    });
    h_stack((label, empty().style(|s| s.flex_grow(1.0_f32)), n))
        .style(move |s| {
            let s = s
                .width_full()
                .items_center()
                .padding_horiz(theme::scaled(12.0))
                .padding_vert(theme::scaled(8.0))
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

/// A history row's right-click menu: what the single click already does, and the
/// delete that has nowhere else to live.
///
/// **Delete is not behind the shared confirm** that the panel's trash button
/// uses. That one clears a whole connection's log at once; this removes the one
/// row under the pointer, and the statement comes back the next time it is run —
/// so a modal would cost more than the mistake.
fn row_menu(
    entry: &HistoryEntry,
    open: &Rc<dyn Fn(HistoryEntry)>,
    remove: &Rc<dyn Fn(HistoryEntry)>,
) -> Vec<MenuEntry> {
    let (open, open_entry) = (open.clone(), entry.clone());
    let (remove, remove_entry) = (remove.clone(), entry.clone());
    vec![
        MenuEntry::action("Open in new tab", move || (open)(open_entry.clone())),
        // The separator before the destructive entry, as in the snippet
        // library's row menu.
        MenuEntry::Separator,
        MenuEntry::action("Delete", move || (remove)(remove_entry.clone())),
    ]
}

/// One history row: the database + when it ran, the SQL preview (monospace,
/// syntax-coloured, ≤3 wrapped lines, clipped), then how the run went. Clicking
/// opens the full SQL in a new tab; right-clicking raises [`row_menu`].
#[allow(clippy::too_many_arguments)]
fn history_row(
    entry: HistoryEntry,
    now: u64,
    open_history: Rc<dyn Fn(HistoryEntry)>,
    open_menu: Rc<dyn Fn(HistoryEntry)>,
    db_colors: RwSignal<Vec<DbColorRule>>,
    term: Option<String>,
    dialect: SqlDialect,
) -> impl IntoView {
    // Full entry for the click handler — restores SQL + database + tab name.
    let entry_click = entry.clone();
    // And for the menu, which offers that same open plus the delete.
    let entry_menu = entry.clone();
    let preview = history::preview_for_highlight(&entry.sql, dialect);
    let db = entry.database.clone().unwrap_or_else(|| "—".to_string());
    let when = history::relative_time(entry.ts, now);
    // Key for the DB-identity dot (only drawn when this run's database has a colour).
    let dot_conn = entry.conn_id;
    let dot_db = entry.database.clone();

    // ~3 lines: font_body() (13) × 1.4 line-height × 3, clipped.

    // Monospace: it is SQL, and it is the same face the editor it came from and
    // the diff view use. Still `highlight_*`, so a search term stays marked, and
    // it is added after the syntax so what was typed stays the loudest thing on
    // the row.
    //
    // Syntax-coloured with the editor's own lexer, like the snippet library's
    // previews: this is the row's substance and the panel's one block of text,
    // and a long history read as a wall of grey without it. Identifiers, which
    // are most of a statement, stay in the base colour — only keywords, strings
    // and numbers carry colour, so the list still scans as a list.
    //
    // `theme::preview_fg` is that base, the same one the library's previews
    // take: the two panels sit in the same column and are read the same way, so
    // a brighter base here would make the colour that *is* meaningful — the
    // keywords — carry less of the difference than it does one panel over. It
    // comes from the editor axis because the token colours beside it do; a UI
    // colour on this surface is the mismatch `theme::preview_bg` describes.
    let preview_view = highlight_sql_mono(
        preview,
        term.clone(),
        font_body,
        theme::preview_fg,
        1.4,
        dialect,
    )
    .style(move |s| {
        s.width_full()
            // Three line boxes of `font_body()`, computed in the closure —
            // a captured height clips the preview at the old scale's three
            // lines while the SQL inside it grows.
            .max_height((font_body() as f64) * 1.4 * 3.0)
            // `preview` collapses a statement to one long line, so those
            // three lines are three *soft-wrapped* ones — and a `RichText`
            // wraps only when it is handed a width narrower than the line it
            // holds. Inside the row-direction stacks below (`.clip()`'s own
            // node, then the surface container) an auto width would resolve
            // to the whole statement instead; see the same pair in
            // `snippet_panel`, where adding the surface is what broke it.
            .min_width(0.0)
    })
    .clip()
    .style(|s| s.width_full().min_width(0.0));
    // On the **editor's** surface, not the panel's — `theme::preview_bg`, which
    // is where the reason lives. The snippet library takes the same one, and the
    // gate that keeps both there is `contrast.rs`'s cross-axis test.
    //
    // The padding replaces the 3px the text used to carry above and below, on
    // top of the stack's own 4px gap: the SQL was sitting tight against the two
    // label lines, which made consecutive rows read as one block.
    let preview_view = container(preview_view).style(|s| {
        s.width_full()
            .min_width(0.0)
            .margin_vert(theme::scaled(3.0))
            .padding_horiz(theme::scaled(7.0))
            .padding_vert(theme::scaled(5.0))
            .background(theme::preview_bg())
            .border_radius(5.0)
    });

    // Database name + its identity dot as a tight group, so the footer's `gap(8)`
    // applies only between this group and the timestamp — the dot's spacing from
    // the name is then purely its own `margin_left`.
    let db_group = h_stack((
        highlight_text(db, term.clone(), font_label, theme::text_dim, false, 1.0)
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
            s.font_size(font_label())
                .color(theme::text_faint())
                .flex_shrink(0.0_f32)
        }),
    ))
    .style(|s| s.items_center().width_full().gap(theme::scaled(8.0)));

    // What the run turned out to be, under the SQL: `5ms · 100 rows`, or
    // `4ms · Failed` in red. A success is not labelled — the row count *is* the
    // success, and a word saying so on every row would drown the one row that
    // failed. Absent entirely when the outcome is unknown (recorded before this
    // was tracked, cancelled, or a run the app didn't outlive), since every part
    // of the line would then be a guess.
    let outcome_row: Option<floem::AnyView> = {
        // The composition is `history::outcome_line`'s, in core beside the
        // `format_duration` it calls and under test; this only paints it.
        let failed = entry.outcome == history::Outcome::Failed;
        history::outcome_line(&entry).map(|lead| {
            let row = h_stack((
                text(lead).style(|s| {
                    s.font_size(font_label())
                        .color(theme::text_faint())
                        .min_width(0.0)
                }),
                // Only a failure says anything, and it is the only colour here.
                text(if failed { "Failed" } else { "" }).style(move |s| {
                    s.font_size(font_label())
                        .color(theme::error())
                        .flex_shrink(0.0_f32)
                }),
            ))
            .style(|s| s.items_center().width_full());
            row.into_any()
        })
    };

    // The originating tab's custom name (if any) on its own line below the footer,
    // as a small capsule that hugs its text (wrapped in a row so it doesn't stretch
    // full-width). Unnamed tabs add no extra row.
    let named = entry.tab_name.clone().filter(|n| !n.trim().is_empty());
    let name_row: Option<floem::AnyView> = named.map(|n| {
        let capsule =
            highlight_text(n, term.clone(), font_label, theme::text, false, 1.0).style(|s| {
                s.padding_horiz(theme::scaled(7.0))
                    .padding_vert(theme::scaled(3.0))
                    .background(theme::capsule_bg())
                    .border_radius(4.0)
                    .flex_shrink(0.0_f32)
            });
        // +2px over the v_stack's 4px gap → 6px between the table and the name.
        h_stack((capsule,))
            .style(|s| s.width_full().margin_top(theme::scaled(2.0)))
            .into_any()
    });
    // Both extra lines are optional and independent, so the stack is built from
    // what there is rather than from one arm per combination.
    let mut rows: Vec<floem::AnyView> = vec![heading.into_any(), preview_view.into_any()];
    rows.extend(outcome_row);
    rows.extend(name_row);
    let inner = floem::views::stack_from_iter(rows)
        .style(|s| s.flex_col().width_full().gap(theme::scaled(4.0)));

    container(inner)
        .on_click_stop(move |_| (open_history)(entry_click.clone()))
        .on_secondary_click_stop(move |_| (open_menu)(entry_menu.clone()))
        .style(|s| {
            s.width_full()
                .padding_horiz(theme::scaled(12.0))
                .padding_vert(theme::scaled(9.0))
                .border_bottom(1.0)
                .border_color(theme::border())
                .hover(|s| s.background(theme::row_hover_soft()))
        })
}
