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
use schemaic_core::stats;
use schemaic_core::stats::{IndexStats, RowCount, TableStats};

use crate::i18n::{bytes as format_bytes, figure};

use crate::i18n::tr;
use crate::theme::{font_body, font_hint, font_label};
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, action_gap, autohide, dismiss_layer,
    focus_root_with_ring, in_ring_button, loading_dots, modal_body_h, modal_footer_split,
    modal_pad_h, modal_title_owned, modal_w, panel_style,
};
use crate::{
    ConnUi, DdlUi, OverlayUi, PropertiesState, PropertiesTarget, SchemaUi, ViewAlgoFn, icons, theme,
};

/// Everything the properties modal reaches out of the root bundle.
///
/// **This one is nearly all driver, and the reason is its footer.** The panel
/// itself only reads `overlay` and the loaded schema, but the footer offers
/// **Edit**, which routes to the table designer or the view editor — so it
/// wants what those two doors want (`ConnUi`, `SchemaUi`, `DdlUi` and the
/// `ViewAlgoFn` the view editor's door ends in) on top of the `overlay` and the
/// three count closures it already carries. Named one by one that is **eight**
/// parameters, past clippy's limit of seven; the gate's own rule and the lint
/// agree, so they are gathered here.
///
/// It said nine, in both copies of this paragraph, by counting `SchemaUi` in
/// each half of the sum — the panel's "loaded schema" and the doors' list are
/// the same field. The conclusion survives the arithmetic, which is exactly why
/// nothing caught it.
///
/// Holds no `Ui`, for `whole_ui_gate`'s reason: it is built by naming the
/// reads at its one call site.
#[derive(Clone)]
pub(crate) struct PropertiesCtx {
    /// The modal's own four signals — target, state, counting, count error.
    overlay: OverlayUi,
    /// The loaded schema, for the `TableInfo` the panel shows without a round
    /// trip, and for the two editor doors below.
    schema: SchemaUi,
    /// The connection registry — `edit_ctx` at the footer's Edit.
    conn: ConnUi,
    /// The editors the footer opens into.
    ddl: DdlUi,
    /// Fetch this table's statistics into `overlay.properties_state`.
    stats: Rc<dyn Fn(PropertiesTarget)>,
    /// The `COUNT(*)` the user asks for, and the stop for it. Separate from
    /// [`Self::stats`] because it is a full scan rather than part of opening.
    count_rows: Rc<dyn Fn(PropertiesTarget)>,
    count_cancel: Rc<dyn Fn()>,
    /// The view editor's door ends in this — see `view_editor::open_for_view`.
    view_algorithm: ViewAlgoFn,
}

impl PropertiesCtx {
    /// Gather the modal's reads where the root bundle is in reach.
    pub(crate) fn new(
        overlay: OverlayUi,
        schema: SchemaUi,
        conn: ConnUi,
        ddl: DdlUi,
        actions: &crate::SchemaActions,
    ) -> Self {
        Self {
            overlay,
            schema,
            conn,
            ddl,
            stats: actions.table_stats.clone(),
            count_rows: actions.count_rows.clone(),
            count_cancel: actions.count_cancel.clone(),
            view_algorithm: actions.view_algorithm.clone(),
        }
    }
}

/// Modal width. Wide enough that the index list's three columns (name,
/// cardinality, usage) sit on one line for an ordinary index name without the
/// row wrapping, which is what makes the list scannable at all.
fn panel_w() -> f64 {
    modal_w(640.0)
}
/// The label column of the detail list. One column so the values line up down
/// the panel — a ragged left edge on a list of eight facts reads as eight
/// unrelated lines — but as wide as the longest label this panel can show, in
/// the interface's language, up to under half the panel
/// ([`crate::widgets::fact_label_w`]).
fn label_w() -> f64 {
    let labels = [
        tr!("props-columns"),
        tr!("props-indexes"),
        tr!("props-foreign-keys"),
        tr!("props-type"),
        tr!("props-engine"),
        tr!("props-row-format"),
        tr!("props-collation"),
        tr!("props-next-auto-increment"),
        tr!("props-created"),
        tr!("props-updated"),
        tr!("props-comment"),
    ];
    crate::widgets::fact_label_w(&labels, theme::scaled(132.0), panel_w() * 0.45)
}
/// Height of the storage-breakdown bar.
fn bar_h() -> f64 {
    theme::scaled(8.0)
}
/// Gap between the rows of a section's list.
fn row_gap() -> f64 {
    theme::scaled(4.0)
}
/// The Indexes section's wider row gap — see the note at its call site.
fn index_row_gap() -> f64 {
    row_gap() + theme::scaled(3.0)
}
/// Gap between an index's name and the facts about it. Wide enough to read as a
/// separation rather than a run-on, without being the fixed gutter a column
/// would impose — see [`index_row`].
fn index_fact_gap() -> f64 {
    theme::scaled(20.0)
}

/// Open the properties modal for one object. The fetch is kicked off by the
/// modal itself, so every entry point is this one call.
///
/// `conn_id` is passed rather than read from the active connection because the
/// two can differ: a query tab keeps the connection it was opened on, so the
/// results toolbar's entry describes a table on *that* server even while the
/// switcher points at another one — and the fetch keys on this
/// (`db_for(target.conn_id)`).
/// Takes the bundle rather than [`PropertiesCtx`]: this door only writes the
/// modal's own signals, so gathering four fetches for it would be four `Rc`
/// clones spent on nothing — `users_view::open_for_server`'s case again.
pub(crate) fn open_for_table(
    o: OverlayUi,
    conn_id: u64,
    database: &str,
    schema: Option<&str>,
    table: &str,
    is_view: bool,
) {
    o.properties_state.set(PropertiesState::Loading);
    o.properties_counting.set(false);
    o.properties_count_err.set(None);
    o.properties.set(Some(PropertiesTarget {
        conn_id,
        database: database.to_string(),
        schema: schema.map(str::to_string),
        table: table.to_string(),
        is_view,
    }));
}

pub(crate) fn properties_overlay(ctx: PropertiesCtx) -> impl IntoView {
    let target = ctx.overlay.properties;
    let state = ctx.overlay.properties_state;
    let counting = ctx.overlay.properties_counting;
    let count_err = ctx.overlay.properties_count_err;

    dyn_container(
        move || target.get(),
        move |open| {
            let Some(t) = open else {
                return empty().into_any();
            };
            let ctx = ctx.clone();
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
                let fetch = ctx.stats.clone();
                let t = t.clone();
                create_effect(move |_| (fetch)(t.clone()));
            }

            // The structural half comes from the schema already in memory —
            // column count, collation, comment, and (for a view) its definition.
            // No round trip, and it is what gives a view something to show on an
            // engine that publishes no statistics for one.
            let info = crate::table_designer::loaded_table(
                ctx.schema,
                &t.database,
                t.schema.as_deref(),
                &t.table,
            );

            let body = {
                let (t, info, ctx, ring) = (t.clone(), info.clone(), ctx.clone(), ring.clone());
                dyn_container(
                    move || (state.get(), counting.get(), count_err.get()),
                    move |(st, busy, err)| {
                        stats_body(&t, info.as_ref(), st, busy, err, &ctx, ring.clone())
                    },
                )
                .style(|s| s.width_full().flex_col())
            };
            let title = modal_title_owned(
                tr!("props-title", table = t.display()),
                close.clone(),
                ring.clone(),
            );
            let footer = footer(
                &ctx,
                t.clone(),
                info.clone(),
                state,
                close.clone(),
                ring.clone(),
            );

            let panel = v_stack((title, body, footer))
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
    ctx: &PropertiesCtx,
    ring: FocusRing,
) -> AnyView {
    // **Collected as `Option`s and filtered, never as an `empty()` placeholder.**
    // The stack has a 16px gap, and floem gaps an empty child like any other — an
    // absent section left a 16px hole, which is why Indexes used to float away
    // from the pair above it whenever the vacuum note had nothing to say.
    let sections: Vec<AnyView> = match state {
        PropertiesState::Loading => {
            return container(loading_dots(
                tr!("props-reading"),
                theme::text_dim,
                font_body,
            ))
            .style(|s| {
                s.height(theme::scaled(200.0))
                    .width_full()
                    .items_center()
                    .justify_center()
            })
            .into_any();
        }
        PropertiesState::Failed(e) => [
            Some(note_line(icons::TRIANGLE_ALERT, theme::error, e)),
            structure_section(target, info),
            // **The state with least to show is not the state to withhold the
            // one figure still obtainable from.** A `COUNT(*)` needs no
            // catalogue, which is `count_row`'s whole argument and is made three
            // times in this module — and this was the one arm of four that did
            // not call it, so the doc was a false statement about the code under
            // it. It was also the only state in which `CountHint::Error` could
            // never appear, so a count error raised in another state and then
            // followed by a failed re-fetch was swallowed.
            count_row(target, None, counting, count_err, ctx, ring),
        ]
        .into_iter()
        .flatten()
        .collect(),
        PropertiesState::Loaded(stats) if !stats.has_any() => [
            Some(note_line(
                icons::CIRCLE_QUESTION,
                theme::text_dim,
                if target.is_view {
                    tr!("props-no-stats-view")
                } else {
                    tr!("props-no-stats-table")
                },
            )),
            structure_section(target, info),
            // **A timestamp is not a statistic, and it is still a fact.**
            // `has_any` stopped counting `created`/`updated` so a MySQL view —
            // whose `CREATE_TIME` is the one column `information_schema.TABLES`
            // fills for a view — would stop claiming published figures; this
            // arm then rendered without `options_section` and dropped the
            // timestamp itself, which was the only thing the server had said
            // about the object. `options_section` has its own emptiness guard,
            // so on an engine that publishes nothing at all it adds nothing.
            options_section(&stats, info),
            partition_section(info),
            count_row(target, None, counting, count_err, ctx, ring),
        ]
        .into_iter()
        .flatten()
        .collect(),
        PropertiesState::Loaded(stats) => [
            Some(headline(&stats)),
            count_row(target, Some(&stats), counting, count_err, ctx, ring),
            storage_section(&stats),
            structure_section(target, info),
            options_section(&stats, info),
            partition_section(info),
            vacuum_note(&stats),
            index_section(&stats),
            freshness_note(&stats),
        ]
        .into_iter()
        .flatten()
        .collect(),
    };

    let content =
        v_stack_from_iter(sections).style(|s| s.flex_col().gap(theme::scaled(16.0)).width_full());
    autohide(scroll(
        container(content).style(|s| s.width_full().padding(modal_pad_h())),
    ))
    .style(|s| s.width_full().max_height(modal_body_h(460.0)))
    .into_any()
}

/// The two figures the panel exists for, at title weight: how many rows, and how
/// much disk. Each says *nothing* rather than `0` when the engine didn't report
/// it.
fn headline(stats: &TableStats) -> AnyView {
    let rows = stats.row_count();
    // Both halves from the model: the figure's own label, and the caption that
    // qualifies it — the same word the copied Markdown uses, which is the point of
    // asking rather than deciding here.
    let row_tile = tile(
        rows.map(|r| figure(&r.label()))
            .unwrap_or_else(|| "—".to_string()),
        row_caption_text(stats),
    );
    let size_tile = tile(
        stats
            .total_bytes()
            .map(format_bytes)
            .unwrap_or_else(|| "—".to_string()),
        tr!("props-on-disk"),
    );
    h_stack((row_tile, size_tile))
        .style(|s| s.width_full().gap(theme::scaled(28.0)))
        .into_any()
}

/// One headline figure over its caption.
fn tile(value: String, caption: String) -> AnyView {
    v_stack((
        text(value).style(|s| {
            // **One scaled term, not a scaled one plus a literal.** `font_title()
            // + 8.0` is 22 at Normal and 30 at 160%, where the composition wants
            // 35 — the one figure the panel exists to show, under-growing while
            // its caption grows — and 19 at 80% where it wants 18.
            s.font_size(theme::scaled_font(22.0))
                .font_bold()
                .color(theme::text())
        }),
        text(caption).style(|s| s.font_size(font_hint()).color(theme::text_faint())),
    ))
    .style(|s| s.flex_col().gap(theme::scaled(2.0)))
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
    ctx: &PropertiesCtx,
    ring: FocusRing,
) -> Option<AnyView> {
    let counted = stats.is_some_and(|s| s.exact_rows.is_some());
    // The four states are the model's (`stats::count_row_state`), including the one
    // that removes the whole row rather than leaving a blank band where the control
    // used to be.
    let (offer, hint_kind) = stats::count_row_state(counted, counting, count_err.is_some())?;
    let control: Option<AnyView> = if offer == stats::CountOffer::Done {
        // Nothing to press any more, and `None` rather than an `empty()`: the row
        // has a 10px gap, which an empty child would leave as a phantom indent
        // in front of the hint.
        None
    } else if offer == stats::CountOffer::Running {
        // **The spinner needs a way out.** The scan is unbounded and holds a
        // connection, so "wait or close the panel" is not the only answer it should
        // have: Cancel stops it on the server. It takes `COUNT_TAB` because the two
        // are mutually exclusive — the ring holds whichever of them exists.
        let stop = ctx.count_cancel.clone();
        let press = stop.clone();
        let face = h_stack((
            loading_dots(tr!("props-counting"), theme::text_dim, font_label),
            text(tr!("dialog-cancel")).style(|s| {
                s.font_size(font_label())
                    .color(theme::text_dim())
                    .padding_horiz(theme::scaled(8.0))
                    .padding_vert(theme::scaled(4.0))
                    .border(1.0)
                    .border_color(theme::control_border())
                    .border_radius(6.0)
                    .hover(|s| s.color(theme::text()).background(theme::control_hover()))
            }),
        ))
        .style(|s| s.items_center().gap(theme::scaled(10.0)))
        .on_click_stop(move |_| (stop)());
        Some(in_ring_button(
            face,
            ring.clone(),
            COUNT_TAB,
            true,
            6.0,
            move || (press)(),
        ))
    } else {
        let run = ctx.count_rows.clone();
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
            text(tr!("props-count-rows")).style(|s| s.font_size(font_label())),
        ))
        .style(|s| {
            s.items_center()
                .flex_shrink(0.0_f32)
                .gap(theme::scaled(7.0))
                .padding_horiz(theme::scaled(8.0))
                .padding_vert(theme::scaled(4.0))
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

    // One line beside the button, ellipsized where the modal is narrow, the
    // whole of it in the tooltip — it clipped mid-word before.
    let hint: Option<AnyView> = match hint_kind {
        Some(stats::CountHint::Error) => Some(
            crate::widgets::ellipsized_text(count_err.clone().unwrap_or_default(), |s| {
                s.font_size(font_body()).color(theme::error())
            })
            .into_any(),
        ),
        // The warning belongs *before* the press, not after.
        Some(stats::CountHint::Slow) => Some(
            crate::widgets::ellipsized_text(tr!("props-count-slow"), |s| {
                s.font_size(font_body()).color(theme::text_faint())
            })
            .into_any(),
        ),
        // Counted: nothing left to say. The caption under the headline already
        // reads "rows (counted)", so a second line asserting the same thing is
        // just a line.
        None => None,
    };

    let parts: Vec<AnyView> = control.into_iter().chain(hint).collect();
    if parts.is_empty() {
        return None;
    }
    Some(
        h_stack_from_iter(parts)
            .style(|s| s.items_center().gap(theme::scaled(10.0)).width_full())
            .into_any(),
    )
}

/// Where the bytes went: a proportional bar over a legend.
fn storage_section(stats: &TableStats) -> Option<AnyView> {
    let (data, index, free) = stats.storage_split()?;
    let seg = |share: f64, color: fn() -> Color| {
        empty().style(move |s| {
            s.height(bar_h())
                .width_pct(share * 100.0)
                .background(color())
        })
    };
    let bar = h_stack((
        seg(data, theme::accent),
        seg(index, theme::key_index),
        seg(free, theme::border),
    ))
    .style(|s| s.width_full().height(bar_h()).border_radius(bar_h() / 2.0))
    // The segments are square; the container's radius is what rounds the two
    // outer ends, so it has to clip them.
    .clip();

    let mut legend: Vec<AnyView> = vec![
        swatch(theme::accent, tr!("props-storage-data"), stats.data_bytes),
        swatch(
            theme::key_index,
            tr!("props-storage-indexes"),
            stats.index_bytes,
        ),
    ];
    // Free space is MySQL's `DATA_FREE`, and it is only worth a legend entry when
    // there is some — the threshold is the model's (`shows_free`), shared with the
    // copied Markdown.
    if stats.shows_free() {
        legend.push(swatch(
            theme::border,
            tr!("props-storage-free"),
            stats.free_bytes,
        ));
    }

    Some(
        v_stack((
            bar,
            h_stack_from_iter(legend).style(|s| s.gap(theme::scaled(18.0)).items_center()),
        ))
        // The bar is the one element with no text baseline to align to, so it
        // needs its own breathing room on top of the stack's gap; the swatches
        // stay tight under it, which is what makes them read as its legend.
        .style(|s| {
            s.flex_col()
                .gap(theme::scaled(9.0))
                .width_full()
                .margin_top(theme::scaled(5.0))
                .margin_bottom(theme::scaled(5.0))
        })
        .into_any(),
    )
}

/// One legend entry: a colour chip, a name, and the figure it stands for.
fn swatch(color: fn() -> Color, label: String, bytes: Option<u64>) -> AnyView {
    h_stack((
        empty().style(move |s| {
            s.width(theme::scaled(9.0))
                .height(theme::scaled(9.0))
                .border_radius(2.0)
                .flex_shrink(0.0_f32)
                .background(color())
        }),
        text(label).style(|s| s.font_size(font_hint()).color(theme::text_faint())),
        text(bytes.map(format_bytes).unwrap_or_else(|| "—".into()))
            .style(|s| s.font_size(font_hint()).color(theme::text_dim())),
    ))
    .style(|s| s.items_center().gap(theme::scaled(6.0)))
    .into_any()
}

/// What the loaded schema knows: columns, keys, and a view's definition. Present
/// even when the statistics fetch failed, because it costs no round trip.
fn structure_section(target: &PropertiesTarget, info: Option<&TableInfo>) -> Option<AnyView> {
    let info = info?;
    let mut rows: Vec<AnyView> = Vec::new();
    let n = info.columns.len();
    rows.push(detail(
        tr!("props-columns"),
        tr!("props-count-columns", n = n),
    ));
    let n = info.indexes.len();
    if n > 0 {
        rows.push(detail(
            tr!("props-indexes"),
            tr!("props-count-indexes", n = n),
        ));
    }
    let n = info.foreign_keys.len();
    if n > 0 {
        rows.push(detail(
            tr!("props-foreign-keys"),
            tr!("props-count-keys", n = n),
        ));
    }
    rows.push(detail(
        tr!("props-type"),
        if target.is_view {
            tr!("props-type-view")
        } else {
            tr!("props-type-table")
        },
    ));
    Some(section(tr!("props-section-structure"), rows))
}

/// The table options the engine reports back — read here, edited in the
/// designer.
fn options_section(stats: &TableStats, info: Option<&TableInfo>) -> Option<AnyView> {
    let mut rows: Vec<AnyView> = Vec::new();
    if let Some(e) = &stats.engine {
        rows.push(detail(tr!("props-engine"), e.clone()));
    }
    if let Some(f) = &stats.row_format {
        rows.push(detail(tr!("props-row-format"), f.clone()));
    }
    if let Some(c) = info.and_then(|i| i.collation.clone()) {
        rows.push(detail(tr!("props-collation"), c));
    }
    if let Some(a) = stats.auto_increment {
        rows.push(detail(
            tr!("props-next-auto-increment"),
            figure(&RowCount::Exact(a).label()),
        ));
    }
    if let Some(c) = &stats.created {
        rows.push(detail(tr!("props-created"), c.clone()));
    }
    if let Some(u) = &stats.updated {
        rows.push(detail(tr!("props-updated"), u.clone()));
    }
    if let Some(c) = info.and_then(|i| i.comment.clone()) {
        rows.push(detail(tr!("props-comment"), c));
    }
    if rows.is_empty() {
        return None;
    }
    Some(section(tr!("props-section-options"), rows))
}

/// How the table is partitioned, or which table it is a partition of
/// ([`TableInfo::partition_facts`]). `None` for a table that is neither.
fn partition_section(info: Option<&TableInfo>) -> Option<AnyView> {
    let lines = info?.partition_facts(PARTITIONS_LISTED);
    if lines.is_empty() {
        return None;
    }
    let rows = lines
        .iter()
        .map(partition_line_text)
        .map(|(label, value)| crate::widgets::fact_row(label, value, label_w))
        .collect();
    Some(section(tr!("props-section-partitioning"), rows))
}

/// One partitioning row, `(label, value)` — core's
/// `schema::PartitionLine::english` in the interface's language. A
/// partition's own row is labelled by its name, which is data.
fn partition_line_text(line: &schemaic_core::schema::PartitionLine) -> (String, String) {
    use schemaic_core::schema::PartitionLine as L;
    match line {
        L::Of(t) => (tr!("props-partition-of"), t.clone()),
        L::Bound(b) => (tr!("props-partition-bound"), b.clone()),
        L::By(k) => (tr!("props-partitioned-by"), k.clone()),
        L::Part { name, bound } => (
            name.clone(),
            bound.clone().unwrap_or_else(|| "—".to_string()),
        ),
        L::More(n) => {
            let n = *n;
            (String::new(), tr!("props-partitions-more", n = n))
        }
    }
}

/// How many partitions the panel names before it says how many more there are.
const PARTITIONS_LISTED: usize = 24;

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
        // Rounded here, as `{pct:.0}` did: Fluent would print the fraction.
        tr!("props-vacuum-note", pct = format!("{pct:.0}")),
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
    Some(section_with_gap(
        tr!("props-section-indexes"),
        rows,
        index_row_gap,
    ))
}

fn index_row(idx: &IndexStats) -> AnyView {
    let unused = idx.is_unused();
    let is_primary = idx.is_primary;
    // Which figures are worth printing, and in what words, is the model's
    // decision (`stats::index_facts`) — the counted zero that prints nothing and
    // the absent count that says so out loud are the difference between "drop this
    // index" and "nobody was counting", and they were rules held in this view.
    let facts: Vec<String> = stats::index_fact_list(idx)
        .into_iter()
        .map(index_fact_text)
        .collect();

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
        text(idx.name.clone()).style(|s| s.font_size(font_body()).color(theme::text())),
    ))
    .style(|s| {
        s.items_center()
            .gap(theme::scaled(6.0))
            .flex_shrink(0.0_f32)
    });

    // **Not a column.** These sit right after the name they describe, because
    // the facts differ per index and lining them up down a fixed gutter left a
    // ragged trench beside short names without buying any comparison — the
    // figures aren't commensurable the way the detail lists' values are.
    let detail =
        text(facts.join(" · ")).style(|s| s.font_size(font_body()).color(theme::text_faint()));

    let flag: Option<AnyView> = unused.then(|| {
        h_stack((
            icons::icon(icons::TRIANGLE_ALERT, 12.0)
                .style(|s| s.flex_shrink(0.0_f32).color(theme::plan_warn())),
            // Worded as an observation with its window attached, because that is
            // all the counter can support: it resets when the server does, and a
            // nightly job's index looks identical to a dead one. The sentence is
            // the model's, so the exported Markdown says it too.
            text(tr!("props-index-unused"))
                .style(|s| s.font_size(font_body()).color(theme::plan_warn())),
        ))
        .style(|s| s.items_center().gap(theme::scaled(5.0)))
        .into_any()
    });

    let row = h_stack((name, detail))
        .style(|s| s.items_center().gap(index_fact_gap()).width_full())
        .into_any();
    v_stack_from_iter(std::iter::once(row).chain(flag))
        .style(|s| s.flex_col().gap(theme::scaled(2.0)).width_full())
        .into_any()
}

/// The caption under the row figure — `TableStats::row_caption` in the
/// interface's language.
fn row_caption_text(stats: &TableStats) -> String {
    match stats.row_count() {
        Some(rc) if rc.is_estimate() => tr!("props-rows-estimated"),
        Some(_) => tr!("props-rows-counted"),
        None => tr!("props-rows-not-reported"),
    }
}

/// The staleness caveat — [`stats::Freshness::note`] in the interface's
/// language.
fn freshness_note(stats: &TableStats) -> Option<AnyView> {
    freshness_text(&stats.freshness)
        .map(|n| note_line(icons::CIRCLE_QUESTION, theme::text_faint, n))
}

/// [`Freshness::note`]'s words. The timestamp is the server's own text, as
/// core prints it; the age is `stats::span` in the interface's units.
///
/// [`Freshness::note`]: schemaic_core::stats::Freshness::note
fn freshness_text(f: &stats::Freshness) -> Option<String> {
    use stats::Freshness;
    match f {
        Freshness::Unknown => None,
        Freshness::Analyzed(None) => Some(tr!("props-never-analysed")),
        Freshness::Analyzed(Some(when)) => Some(tr!("props-analysed-at", when = when.as_str())),
        Freshness::CachedUpTo(0) => Some(tr!("props-read-live")),
        &Freshness::CachedUpTo(secs) => Some(tr!(
            "props-cached-up-to",
            age = crate::i18n::staleness_age(stats::span(secs)),
            secs = secs.to_string()
        )),
    }
}

/// One of an index's facts — [`IndexFact::message`] in the interface's
/// language; the size and the counts are core's figures as it prints them.
///
/// [`IndexFact::message`]: schemaic_core::stats::IndexFact::message
fn index_fact_text(fact: stats::IndexFact) -> String {
    index_fact_in(fact, crate::i18n::number_marks())
}

/// [`index_fact_text`] in the given marks, so a test can ask for German's.
/// **Every one of the three figures is remarked**: the cardinality arrives
/// already printed in English (`~1.25k`), and passed through raw it was the
/// one figure on the line a German reader took for ~125k.
fn index_fact_in(fact: stats::IndexFact, marks: schemaic_core::text::NumberMarks) -> String {
    use schemaic_core::text::remark;
    use stats::IndexFact;
    match fact {
        IndexFact::Size(b) => remark(&stats::format_bytes(b), marks),
        IndexFact::Cardinality(c) => {
            tr!("props-index-cardinality", cardinality = remark(&c, marks))
        }
        IndexFact::Scans(n) => tr!(
            "props-index-scans",
            n = n,
            shown = remark(&RowCount::Exact(n).label(), marks)
        ),
        IndexFact::NotCounted => tr!("props-index-not-counted"),
    }
}

/// A titled group of `label: value` rows. The heading is the app's form-section
/// heading (`widgets::form_section`), so a group here reads at the same weight as
/// **General** in Settings rather than inventing a third heading style.
/// This panel's rows at its own gap — see [`crate::widgets::fact_section`],
/// which is the shared view and where the reasoning lives.
fn section(title: String, rows: Vec<AnyView>) -> AnyView {
    section_with_gap(title, rows, row_gap)
}

/// [`section`] for a group whose rows need more air than the shared [`row_gap()`]
/// — the index list, whose rows are taller and less uniform than a detail list's.
fn section_with_gap(title: String, rows: Vec<AnyView>, gap: fn() -> f64) -> AnyView {
    crate::widgets::fact_section(title, rows, gap)
}

/// One `label: value` row of a section, at this panel's label column.
fn detail(label: String, value: String) -> AnyView {
    crate::widgets::fact_row(label, value, label_w)
}

/// An icon-led sentence — a caveat, a warning, or an engine's limitation.
fn note_line(icon: &'static str, color: fn() -> Color, message: String) -> AnyView {
    crate::widgets::fact_note(icon, color, message)
}

/// Copy on the left; the handoff to the editor and Close on the right.
fn footer(
    ctx: &PropertiesCtx,
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
    //
    // **The read-only clause was true of this comment and not of the menu it
    // names** until the term was added there (B11.2-L2-01), and this gate is a
    // build-time `bool` besides: the panel outlives a connection being marked
    // read-only from the status bar, and the button would stay live. What makes
    // it so in both directions is the refusal inside `open_for_table` /
    // `open_for_view`; this decides only what the button *says*.
    let conn_ctx = crate::table_designer::edit_ctx(ctx.conn);
    let editable_view = crate::view_editor::is_editable_view(info.as_ref());
    let can_edit = !conn_ctx.read_only
        && info.as_ref().is_some_and(|i| {
            if target.is_view {
                editable_view
            } else {
                // Not a sequence or a foreign table, which no menu offers the
                // designer either.
                !i.columns.is_empty() && crate::table_designer::designable(i)
            }
        });
    let is_view = target.is_view;
    let edit = {
        let (ctx, t, close) = (ctx.clone(), target.clone(), close.clone());
        move || {
            (close)();
            if t.is_view {
                crate::view_editor::open_for_view(
                    ctx.conn,
                    ctx.schema,
                    ctx.ddl,
                    &ctx.view_algorithm,
                    &t.database,
                    t.schema.as_deref(),
                    &t.table,
                );
            } else {
                crate::table_designer::open_for_table(
                    ctx.conn,
                    ctx.schema,
                    ctx.ddl,
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
            tr!("props-copy"),
            ActionKind::Quiet,
            true,
            ring.clone(),
            ACTION_TAB,
            copy,
        ),
        h_stack((
            action_button(
                if is_view {
                    tr!("tree-edit-view")
                } else {
                    tr!("tree-edit-table")
                },
                ActionKind::Neutral,
                can_edit,
                ring.clone(),
                ACTION_TAB + 10,
                edit,
            ),
            action_button(
                tr!("dialog-close"),
                ActionKind::Primary,
                true,
                ring,
                ACTION_TAB + 20,
                move || (done)(),
            ),
        ))
        .style(|s| s.gap(action_gap())),
    )
    .into_any()
}

/// The Count-rows button's place in the focus ring — before the footer's
/// actions, which is where it sits on screen.
const COUNT_TAB: u32 = 10;

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every partitioning row reads in English exactly as core's**, the
    /// count of the unlisted ones for one and for several.
    #[test]
    fn english_says_every_partition_line_as_core_does() {
        use schemaic_core::schema::PartitionLine as L;
        for line in [
            L::Of("public.measurements".into()),
            L::Bound("FOR VALUES FROM (1) TO (2)".into()),
            L::By("RANGE (at)".into()),
            L::Part {
                name: "m_2024".into(),
                bound: Some("FOR VALUES IN (1)".into()),
            },
            L::Part {
                name: "m_default".into(),
                bound: None,
            },
            L::More(1),
            L::More(3),
        ] {
            assert_eq!(partition_line_text(&line), line.english(), "{line:?}");
        }
    }

    /// **The staleness caveat, an index's facts and its unused flag read in
    /// English exactly as core says them.**
    #[test]
    fn english_says_every_stats_note_as_core_does() {
        use stats::Freshness;
        for f in [
            Freshness::Unknown,
            Freshness::Analyzed(None),
            Freshness::Analyzed(Some("2026-10-19 14:02:11+02".into())),
            Freshness::CachedUpTo(0),
            Freshness::CachedUpTo(45),
            Freshness::CachedUpTo(86_400),
        ] {
            assert_eq!(freshness_text(&f), f.note(), "{f:?}");
        }
        for scans in [None, Some(0), Some(1), Some(12_345)] {
            let ix = IndexStats {
                name: "ix".into(),
                bytes: Some(16_384),
                cardinality: Some(1_200),
                scans,
                ..IndexStats::default()
            };
            let ours: Vec<String> = stats::index_fact_list(&ix)
                .into_iter()
                .map(index_fact_text)
                .collect();
            assert_eq!(ours, stats::index_facts(&ix), "{scans:?}");
        }
        // **Every figure on an index's line is in the interface's marks**,
        // the cardinality as well as the size and the scans.
        let german = schemaic_core::text::NumberMarks {
            decimal: ',',
            group: '.',
        };
        let card = stats::index_fact_list(&IndexStats {
            name: "ix".into(),
            cardinality: Some(1_250),
            ..IndexStats::default()
        })
        .into_iter()
        .find(|f| matches!(f, stats::IndexFact::Cardinality(_)))
        .expect("a cardinality fact");
        let shown = index_fact_in(card, german);
        assert!(shown.contains("1,25k"), "{shown}");
        assert_eq!(tr!("props-index-unused"), stats::unused_note());
        for (rows, exact_rows) in [(None, None), (Some(40), None), (Some(40), Some(42))] {
            let t = TableStats {
                rows,
                exact_rows,
                ..TableStats::default()
            };
            assert_eq!(
                row_caption_text(&t),
                t.row_caption(),
                "{rows:?} {exact_rows:?}"
            );
        }
    }
}
