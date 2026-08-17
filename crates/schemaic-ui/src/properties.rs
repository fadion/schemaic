//! The table-properties modal: what a table costs, and how much of that to
//! believe.
//!
//! Opened from a Table or View row's context menu (and the results toolbar) by
//! setting `overlay.properties`; an effect then asks the app for the database's
//! statistics, which land in `overlay.properties_state`.
//!
//! **It shows what nothing else in the app does.** A table's *structure* already
//! has three surfaces — the tree, the designer and Generate DDL — so this one
//! deliberately doesn't repeat them: it is sizes, row counts, storage split and
//! index usage, plus the handful of table options that are observed rather than
//! edited. What it adds is the answer to "is this table big enough to worry
//! about", which was previously unanswerable in the GUI while the MCP
//! `describe_table` tool was already assembling a summary for the AI.
//!
//! **Every figure here is qualified.** The panel's hardest job is not laying out
//! numbers, it is not overstating them: an estimate prints with a `~`,
//! [`Freshness`](schemaic_core::stats::Freshness) says in words why it may be
//! stale, an index is called unused only when the server actually counted zero
//! scans, and **Count rows** is there for when the estimate isn't good enough.
//! That button runs an uncapped `COUNT(*)`, which is why it is a button.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::peniko::Color;
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::schema::TableInfo;
use schemaic_core::stats::{IndexStats, RowCount, TableStats, format_bytes};
use schemaic_core::text::plural;

use crate::theme::{FONT_BODY, FONT_HINT, FONT_LABEL, FONT_TITLE};
use crate::widgets::{
    ACTION_GAP, ACTION_TAB, ActionKind, FocusRing, MODAL_PAD_H, action_button, autohide,
    dismiss_layer, focus_root_with_ring, in_ring_button, loading_dots, modal_footer_split,
    modal_title_owned, panel_style,
};
use crate::{PropertiesState, PropertiesTarget, Ui, icons, theme};

/// Modal width. Wide enough that the index list's three columns (name,
/// cardinality, usage) sit on one line for an ordinary index name without the
/// row wrapping, which is what makes the list scannable at all.
const PANEL_W: f64 = 640.0;
/// The label column of the detail list. Fixed rather than measured so the values
/// line up down the panel — a ragged left edge on a list of eight facts reads as
/// eight unrelated lines.
const LABEL_W: f64 = 132.0;
/// Height of the storage-breakdown bar.
const BAR_H: f64 = 8.0;
/// Gap between the rows of a section's list.
const ROW_GAP: f64 = 4.0;
/// Gap between an index's name and the facts about it. Wide enough to read as a
/// separation rather than a run-on, without being the fixed gutter a column
/// would impose — see [`index_row`].
const INDEX_FACT_GAP: f64 = 20.0;

/// Open the properties modal for one object. The fetch is kicked off by the
/// modal itself, so every entry point is this one call.
pub(crate) fn open_for_table(
    ui: &Ui,
    database: &str,
    schema: Option<&str>,
    table: &str,
    is_view: bool,
) {
    ui.overlay.properties_state.set(PropertiesState::Loading);
    ui.overlay.properties_counting.set(false);
    ui.overlay.properties_count_err.set(None);
    ui.overlay.properties.set(Some(PropertiesTarget {
        conn_id: ui.conn.active_conn.get_untracked(),
        database: database.to_string(),
        schema: schema.map(str::to_string),
        table: table.to_string(),
        is_view,
    }));
}

pub(crate) fn properties_overlay(ui: Ui) -> impl IntoView {
    let target = ui.overlay.properties;
    let state = ui.overlay.properties_state;
    let counting = ui.overlay.properties_counting;
    let count_err = ui.overlay.properties_count_err;

    dyn_container(
        move || target.get(),
        move |open| {
            let Some(t) = open else {
                return empty().into_any();
            };
            let ui = ui.clone();
            let close: Rc<dyn Fn()> = Rc::new(move || {
                target.set(None);
                state.set(PropertiesState::Loading);
                counting.set(false);
                count_err.set(None);
            });
            let ring = FocusRing::new();

            // Ask for the statistics. Runs once per opening: the closure reads no
            // signal, and a fresh target rebuilds this whole branch.
            {
                let fetch = ui.schema_actions.table_stats.clone();
                let t = t.clone();
                create_effect(move |_| (fetch)(t.clone()));
            }

            // The structural half comes from the schema already in memory —
            // column count, collation, comment, and (for a view) its definition.
            // No round trip, and it is what gives a view something to show on an
            // engine that publishes no statistics for one.
            let info = crate::table_designer::loaded_table(
                &ui,
                &t.database,
                t.schema.as_deref(),
                &t.table,
            );

            let body = {
                let (t, info, ui, ring) = (t.clone(), info.clone(), ui.clone(), ring.clone());
                dyn_container(
                    move || (state.get(), counting.get(), count_err.get()),
                    move |(st, busy, err)| {
                        stats_body(&t, info.as_ref(), st, busy, err, &ui, ring.clone())
                    },
                )
                .style(|s| s.width_full().flex_col())
            };
            let title = modal_title_owned(
                format!("Properties — {}", t.display()),
                close.clone(),
                ring.clone(),
            );
            let footer = footer(
                ui.clone(),
                t.clone(),
                info.clone(),
                state,
                close.clone(),
                ring.clone(),
            );

            let panel = v_stack((title, body, footer))
                .on_click_stop(|_| {})
                .style(|s| panel_style(s).background(theme::bg_panel()).width(PANEL_W));

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
        if target.get().is_some() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// The scrolling body, in whichever of the four states the fetch is in.
fn stats_body(
    target: &PropertiesTarget,
    info: Option<&TableInfo>,
    state: PropertiesState,
    counting: bool,
    count_err: Option<String>,
    ui: &Ui,
    ring: FocusRing,
) -> AnyView {
    // **Collected as `Option`s and filtered, never as an `empty()` placeholder.**
    // The stack has a 16px gap, and floem gaps an empty child like any other — an
    // absent section left a 16px hole, which is why Indexes used to float away
    // from the pair above it whenever the vacuum note had nothing to say.
    let sections: Vec<AnyView> = match state {
        PropertiesState::Loading => {
            return container(loading_dots(
                "Reading statistics",
                theme::text_dim,
                FONT_BODY,
            ))
            .style(|s| s.height(200.0).width_full().items_center().justify_center())
            .into_any();
        }
        PropertiesState::Failed(e) => [
            Some(note_line(icons::TRIANGLE_ALERT, theme::error, e)),
            structure_section(target, info),
        ]
        .into_iter()
        .flatten()
        .collect(),
        PropertiesState::Unsupported => [
            Some(note_line(
                icons::CIRCLE_QUESTION,
                theme::text_dim,
                // Named as a property of the engine, not as a missing feature —
                // SQLite genuinely keeps no per-table size or row estimate.
                "This engine publishes no per-table statistics. \
                 The exact row count is still available below."
                    .to_string(),
            )),
            structure_section(target, info),
            count_row(target, None, counting, count_err, ui, ring),
        ]
        .into_iter()
        .flatten()
        .collect(),
        PropertiesState::Loaded(stats) if !stats.has_any() => [
            Some(note_line(
                icons::CIRCLE_QUESTION,
                theme::text_dim,
                if target.is_view {
                    "A view has no storage of its own, so the server reports no \
                     statistics for it."
                        .to_string()
                } else {
                    "The server reported no statistics for this table.".to_string()
                },
            )),
            structure_section(target, info),
            count_row(target, None, counting, count_err, ui, ring),
        ]
        .into_iter()
        .flatten()
        .collect(),
        PropertiesState::Loaded(stats) => [
            Some(headline(&stats)),
            count_row(target, Some(&stats), counting, count_err, ui, ring),
            storage_section(&stats),
            structure_section(target, info),
            options_section(&stats, info),
            vacuum_note(&stats),
            index_section(&stats),
            freshness_note(&stats),
        ]
        .into_iter()
        .flatten()
        .collect(),
    };

    let content = v_stack_from_iter(sections).style(|s| s.flex_col().gap(16.0).width_full());
    autohide(scroll(
        container(content).style(|s| s.width_full().padding(MODAL_PAD_H)),
    ))
    .style(|s| s.width_full().max_height(460.0))
    .into_any()
}

/// The two figures the panel exists for, at title weight: how many rows, and how
/// much disk. Each says *nothing* rather than `0` when the engine didn't report
/// it.
fn headline(stats: &TableStats) -> AnyView {
    let rows = stats.row_count();
    let row_tile = tile(
        rows.map(RowCount::label).unwrap_or_else(|| "—".to_string()),
        match rows {
            Some(rc) if rc.is_estimate() => "rows (estimated)".to_string(),
            Some(_) => "rows (counted)".to_string(),
            None => "rows — not reported".to_string(),
        },
    );
    let size_tile = tile(
        stats
            .total_bytes()
            .map(format_bytes)
            .unwrap_or_else(|| "—".to_string()),
        "on disk".to_string(),
    );
    h_stack((row_tile, size_tile))
        .style(|s| s.width_full().gap(28.0))
        .into_any()
}

/// One headline figure over its caption.
fn tile(value: String, caption: String) -> AnyView {
    v_stack((
        text(value).style(|s| {
            s.font_size(FONT_TITLE + 8.0)
                .font_bold()
                .color(theme::text())
        }),
        text(caption).style(|s| s.font_size(FONT_HINT).color(theme::text_faint())),
    ))
    .style(|s| s.flex_col().gap(2.0))
    .into_any()
}

/// The **Count rows** control, plus whatever the last count had to say.
///
/// Offered whatever the fetch returned, including on the engine that publishes
/// nothing — a `COUNT(*)` needs no catalogue. `None` once an exact count is in
/// hand and there is no error to report: pressing it again would re-scan the
/// table to print the same number, and the figure it produced is already in the
/// headline.
fn count_row(
    target: &PropertiesTarget,
    stats: Option<&TableStats>,
    counting: bool,
    count_err: Option<String>,
    ui: &Ui,
    ring: FocusRing,
) -> Option<AnyView> {
    let counted = stats.is_some_and(|s| s.exact_rows.is_some());
    let control: Option<AnyView> = if counted {
        // Nothing to press any more, and `None` rather than an `empty()`: the row
        // has a 10px gap, which an empty child would leave as a phantom indent
        // in front of the hint.
        None
    } else if counting {
        Some(loading_dots("Counting", theme::text_dim, FONT_LABEL).into_any())
    } else {
        let run = ui.schema_actions.count_rows.clone();
        // **Both halves, and they are not the same one.** `in_ring_button` binds
        // Space/Enter for the focus ring and nothing else — a face without its
        // own `on_click_stop` is a button the mouse cannot press, which is
        // exactly how this one shipped.
        let press = {
            let (run, t) = (run.clone(), target.clone());
            Rc::new(move || (run)(t.clone()))
        };
        let clicked = press.clone();
        let face = h_stack((
            icons::icon(icons::HASH, 14.0).style(|s| s.flex_shrink(0.0_f32)),
            text("Count rows").style(|s| s.font_size(FONT_LABEL)),
        ))
        .style(|s| {
            s.items_center()
                .gap(7.0)
                .padding_horiz(8.0)
                .padding_vert(4.0)
                .border(1.0)
                .border_color(theme::control_border())
                .border_radius(6.0)
                .color(theme::text_dim())
                .hover(|s| s.color(theme::text()).background(theme::control_hover()))
        })
        .on_click_stop(move |_| (clicked)());
        Some(in_ring_button(
            face,
            ring,
            COUNT_TAB,
            true,
            6.0,
            move || (press)(),
        ))
    };

    let hint: Option<AnyView> = match (&count_err, counted) {
        (Some(e), _) => Some(
            text(e.clone())
                .style(|s| s.font_size(FONT_BODY).color(theme::error()))
                .into_any(),
        ),
        // Counted: nothing left to say. The caption under the headline already
        // reads "rows (counted)", so a second line asserting the same thing is
        // just a line — and with the button gone too, the whole row goes with it
        // rather than leaving a blank band where the control used to be.
        (None, true) => None,
        // The warning belongs *before* the press, not after.
        (None, false) => Some(
            text("A full scan of the table. Slow on a large one.")
                .style(|s| s.font_size(FONT_BODY).color(theme::text_faint()))
                .into_any(),
        ),
    };

    let parts: Vec<AnyView> = control.into_iter().chain(hint).collect();
    if parts.is_empty() {
        return None;
    }
    Some(
        h_stack_from_iter(parts)
            .style(|s| s.items_center().gap(10.0).width_full())
            .into_any(),
    )
}

/// Where the bytes went: a proportional bar over a legend.
fn storage_section(stats: &TableStats) -> Option<AnyView> {
    let (data, index, free) = stats.storage_split()?;
    let seg = |share: f64, color: fn() -> Color| {
        empty().style(move |s| s.height(BAR_H).width_pct(share * 100.0).background(color()))
    };
    let bar = h_stack((
        seg(data, theme::accent),
        seg(index, theme::key_index),
        seg(free, theme::border),
    ))
    .style(|s| s.width_full().height(BAR_H).border_radius(BAR_H / 2.0))
    // The segments are square; the container's radius is what rounds the two
    // outer ends, so it has to clip them.
    .clip();

    let mut legend: Vec<AnyView> = vec![
        swatch(theme::accent, "Data", stats.data_bytes),
        swatch(theme::key_index, "Indexes", stats.index_bytes),
    ];
    // Free space is MySQL's `DATA_FREE`, and it is only worth a legend entry when
    // there is some — a permanent "Free 0 B" is noise on every other engine.
    if stats.free_bytes.is_some_and(|b| b > 0) {
        legend.push(swatch(theme::border, "Free", stats.free_bytes));
    }

    Some(
        v_stack((
            bar,
            h_stack_from_iter(legend).style(|s| s.gap(18.0).items_center()),
        ))
        // The bar is the one element with no text baseline to align to, so it
        // needs its own breathing room on top of the stack's gap; the swatches
        // stay tight under it, which is what makes them read as its legend.
        .style(|s| {
            s.flex_col()
                .gap(9.0)
                .width_full()
                .margin_top(5.0)
                .margin_bottom(5.0)
        })
        .into_any(),
    )
}

/// One legend entry: a colour chip, a name, and the figure it stands for.
fn swatch(color: fn() -> Color, label: &'static str, bytes: Option<u64>) -> AnyView {
    h_stack((
        empty().style(move |s| {
            s.width(9.0)
                .height(9.0)
                .border_radius(2.0)
                .flex_shrink(0.0_f32)
                .background(color())
        }),
        text(label).style(|s| s.font_size(FONT_HINT).color(theme::text_faint())),
        text(bytes.map(format_bytes).unwrap_or_else(|| "—".into()))
            .style(|s| s.font_size(FONT_HINT).color(theme::text_dim())),
    ))
    .style(|s| s.items_center().gap(6.0))
    .into_any()
}

/// What the loaded schema knows: columns, keys, and a view's definition. Present
/// even when the statistics fetch failed, because it costs no round trip.
fn structure_section(target: &PropertiesTarget, info: Option<&TableInfo>) -> Option<AnyView> {
    let info = info?;
    let mut rows: Vec<AnyView> = Vec::new();
    let n = info.columns.len();
    rows.push(detail(
        "Columns",
        format!("{n} {}", plural(n, "column", "columns")),
    ));
    let n = info.indexes.len();
    if n > 0 {
        rows.push(detail(
            "Indexes",
            format!("{n} {}", plural(n, "index", "indexes")),
        ));
    }
    let n = info.foreign_keys.len();
    if n > 0 {
        rows.push(detail(
            "Foreign keys",
            format!("{n} {}", plural(n, "key", "keys")),
        ));
    }
    rows.push(detail(
        "Type",
        if target.is_view { "View" } else { "Table" }.to_string(),
    ));
    Some(section("Structure", rows))
}

/// The table options the engine reports back — read here, edited in the
/// designer.
fn options_section(stats: &TableStats, info: Option<&TableInfo>) -> Option<AnyView> {
    let mut rows: Vec<AnyView> = Vec::new();
    if let Some(e) = &stats.engine {
        rows.push(detail("Engine", e.clone()));
    }
    if let Some(f) = &stats.row_format {
        rows.push(detail("Row format", f.clone()));
    }
    if let Some(c) = info.and_then(|i| i.collation.clone()) {
        rows.push(detail("Collation", c));
    }
    if let Some(a) = stats.auto_increment {
        rows.push(detail("Next auto-increment", RowCount::Exact(a).label()));
    }
    if let Some(c) = &stats.created {
        rows.push(detail("Created", c.clone()));
    }
    if let Some(u) = &stats.updated {
        rows.push(detail("Updated", u.clone()));
    }
    if let Some(c) = info.and_then(|i| i.comment.clone()) {
        rows.push(detail("Comment", c));
    }
    if rows.is_empty() {
        return None;
    }
    Some(section("Options", rows))
}

/// The dead-tuple warning, when PostgreSQL says there are enough of them for its
/// own autovacuum to agree.
fn vacuum_note(stats: &TableStats) -> Option<AnyView> {
    if !stats.needs_vacuum() {
        return None;
    }
    let pct = stats.dead_ratio().unwrap_or_default() * 100.0;
    Some(note_line(
        icons::TRIANGLE_ALERT,
        theme::plan_warn,
        format!(
            "{pct:.0}% of this table's row versions are dead and not yet reclaimed. \
             A VACUUM would return that space."
        ),
    ))
}

/// The index list — the part of the panel that can tell you something you didn't
/// already know.
fn index_section(stats: &TableStats) -> Option<AnyView> {
    if stats.indexes.is_empty() {
        return None;
    }
    let rows: Vec<AnyView> = stats.indexes.iter().map(index_row).collect();
    // Looser than the detail lists above it: an index row is a name plus a run of
    // facts (and sometimes a second line under it), so at the shared 4px the rows
    // ran together into one block instead of reading as a list.
    Some(section_with_gap("Indexes", rows, ROW_GAP + 3.0))
}

fn index_row(idx: &IndexStats) -> AnyView {
    let unused = idx.is_unused();
    let is_primary = idx.is_primary;
    let mut facts: Vec<String> = Vec::new();
    if let Some(b) = idx.bytes {
        facts.push(format_bytes(b));
    }
    if let Some(c) = idx.cardinality {
        facts.push(format!("cardinality {}", RowCount::Exact(c).label()));
    }
    match idx.scans {
        // "Never used" is the finding; the raw zero beside it would just repeat
        // itself. Anything else prints the count, because the *size* of the
        // number is the point ("9 scans" is nearly as interesting as none).
        Some(0) => {}
        Some(n) => facts.push(format!(
            "{} {}",
            RowCount::Exact(n).label(),
            plural(n as usize, "scan", "scans")
        )),
        // Absent, not zero — see `IndexStats::scans`. Said out loud, because a
        // blank here would read as "no scans".
        None => facts.push("usage not counted".to_string()),
    }

    let name = h_stack((
        icons::icon(
            if is_primary {
                icons::KEY_ROUND
            } else {
                icons::HASH
            },
            13.0,
        )
        .style(move |s| {
            s.flex_shrink(0.0_f32).color(if is_primary {
                theme::key_primary()
            } else {
                theme::key_index()
            })
        }),
        text(idx.name.clone()).style(|s| s.font_size(FONT_BODY).color(theme::text())),
    ))
    .style(|s| s.items_center().gap(6.0).flex_shrink(0.0_f32));

    // **Not a column.** These sit right after the name they describe, because
    // the facts differ per index and lining them up down a fixed gutter left a
    // ragged trench beside short names without buying any comparison — the
    // figures aren't commensurable the way the detail lists' values are.
    let detail =
        text(facts.join(" · ")).style(|s| s.font_size(FONT_BODY).color(theme::text_faint()));

    let flag: Option<AnyView> = unused.then(|| {
        h_stack((
            icons::icon(icons::TRIANGLE_ALERT, 12.0)
                .style(|s| s.flex_shrink(0.0_f32).color(theme::plan_warn())),
            // Worded as an observation with its window attached, because that is
            // all the counter can support: it resets when the server does, and a
            // nightly job's index looks identical to a dead one.
            text("never used since the server's counters were reset")
                .style(|s| s.font_size(FONT_BODY).color(theme::plan_warn())),
        ))
        .style(|s| s.items_center().gap(5.0))
        .into_any()
    });

    let row = h_stack((name, detail))
        .style(|s| s.items_center().gap(INDEX_FACT_GAP).width_full())
        .into_any();
    v_stack_from_iter(std::iter::once(row).chain(flag))
        .style(|s| s.flex_col().gap(2.0).width_full())
        .into_any()
}

/// The staleness caveat, verbatim from
/// [`Freshness::note`](schemaic_core::stats::Freshness::note).
fn freshness_note(stats: &TableStats) -> Option<AnyView> {
    stats
        .freshness
        .note()
        .map(|n| note_line(icons::CIRCLE_QUESTION, theme::text_faint, n))
}

/// A titled group of `label: value` rows. The heading is the app's form-section
/// heading (`widgets::form_section`), so a group here reads at the same weight as
/// **General** in Settings rather than inventing a third heading style.
fn section(title: &'static str, rows: Vec<AnyView>) -> AnyView {
    section_with_gap(title, rows, ROW_GAP)
}

/// [`section`] for a group whose rows need more air than the shared [`ROW_GAP`]
/// — the index list, whose rows are taller and less uniform than a detail list's.
fn section_with_gap(title: &'static str, rows: Vec<AnyView>, gap: f64) -> AnyView {
    v_stack((
        crate::widgets::form_section(title),
        v_stack_from_iter(rows).style(move |s| s.flex_col().gap(gap).width_full()),
    ))
    .style(|s| s.flex_col().gap(7.0).width_full())
    .into_any()
}

/// One `label: value` row of a section.
fn detail(label: &'static str, value: String) -> AnyView {
    h_stack((
        text(label).style(|s| {
            s.width(LABEL_W)
                .flex_shrink(0.0_f32)
                .font_size(FONT_BODY)
                .color(theme::text_faint())
        }),
        // `text_dim` — the app's form-label colour (`widgets::form_label_style`,
        // what "Font size" wears in Settings). Full `text()` made a column of
        // read-only facts shout louder than the editable fields elsewhere.
        text(value).style(|s| s.font_size(FONT_BODY).color(theme::text_dim())),
    ))
    .style(|s| s.items_start().gap(10.0).width_full())
    .into_any()
}

/// An icon-led sentence — a caveat, a warning, or an engine's limitation.
fn note_line(icon: &'static str, color: fn() -> Color, message: String) -> AnyView {
    h_stack((
        icons::icon(icon, 14.0)
            .style(move |s| s.flex_shrink(0.0_f32).color(color()).margin_top(1.0)),
        text(message).style(move |s| s.font_size(FONT_BODY).color(color())),
    ))
    .style(|s| s.items_start().gap(7.0).width_full())
    .into_any()
}

/// Copy on the left; the handoff to the editor and Close on the right.
fn footer(
    ui: Ui,
    target: PropertiesTarget,
    info: Option<TableInfo>,
    state: RwSignal<PropertiesState>,
    close: Rc<dyn Fn()>,
    ring: FocusRing,
) -> AnyView {
    let display = target.display();
    let copy = {
        let display = display.clone();
        move || {
            if let PropertiesState::Loaded(stats) = state.get_untracked() {
                let _ = floem::Clipboard::set_contents(stats.to_markdown(&display));
            }
        }
    };

    // The handoff, so the panel is a place you can act from rather than a
    // dead end. Gated exactly as the context menu's own entry is: an unloaded
    // schema has nothing to edit from, a read-only connection may not, and a
    // materialized view has no `CREATE OR REPLACE` to be edited with.
    let ctx = crate::table_designer::edit_ctx(&ui);
    let editable_view = crate::view_editor::is_editable_view(info.as_ref());
    let can_edit = !ctx.read_only
        && info.as_ref().is_some_and(|i| {
            if target.is_view {
                editable_view
            } else {
                !i.columns.is_empty()
            }
        });
    let is_view = target.is_view;
    let edit = {
        let (ui, t, close) = (ui.clone(), target.clone(), close.clone());
        move || {
            (close)();
            if t.is_view {
                crate::view_editor::open_for_view(&ui, &t.database, t.schema.as_deref(), &t.table);
            } else {
                crate::table_designer::open_for_table(
                    &ui,
                    &t.database,
                    t.schema.as_deref(),
                    &t.table,
                    crate::table_designer::DesignerFocus::Table,
                );
            }
        }
    };

    let done = close.clone();
    modal_footer_split(
        action_button(
            "Copy",
            ActionKind::Quiet,
            true,
            ring.clone(),
            ACTION_TAB,
            copy,
        ),
        h_stack((
            action_button(
                if is_view { "Edit view" } else { "Edit table" },
                ActionKind::Neutral,
                can_edit,
                ring.clone(),
                ACTION_TAB + 10,
                edit,
            ),
            action_button(
                "Close",
                ActionKind::Primary,
                true,
                ring,
                ACTION_TAB + 20,
                move || (done)(),
            ),
        ))
        .style(|s| s.gap(ACTION_GAP)),
    )
    .into_any()
}

/// The Count-rows button's place in the focus ring — before the footer's
/// actions, which is where it sits on screen.
const COUNT_TAB: u32 = 10;
