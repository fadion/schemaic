//! The schema-compare modal: two databases side by side, and the migration
//! between them.
//!
//! Opened from a database row's context menu by setting `overlay.compare`,
//! which fixes the **left** side; the right one is picked here, and choosing it
//! is what asks `schema_actions.compare_fetch` for the two schemas. They land
//! in `overlay.compare_state` as a [`SchemaComparison`], and everything this
//! module draws is a read of that one value — the tree's rows, the counts in
//! each heading, the two sides of the diff pane, and the plan the footer
//! previews all come from `schemaic_core::compare`, which is where every
//! decision in this feature lives.
//!
//! **This view decides nothing.** It holds no opinion about what differs, in
//! what order the statements run, or what a plan withholds; asking it to would
//! be a second differ, which is the thing the DDL invariant exists to prevent.
//! What it owns is the reading: which groups are open, what the filter says,
//! which objects are ticked, and which one's text is on the right.
//!
//! **A MySQL body is not offered.** `information_schema` hands back a trigger's,
//! a routine's or an event's body with its escapes already resolved, so the
//! *comparison* is right — two mangled bodies compare equal — while a `CREATE`
//! built from one is not. Those entries are shown, and read, and cannot be
//! ticked: [`CompareEntry::needs_source`] is the flag, and the row says why.
//! Emitting them correctly means re-reading each body through
//! `Db::{trigger,routine,event}_source` first, which is the next piece of work
//! and not something to fake in the meantime.
//!
//! [`SchemaComparison`]: schemaic_core::compare::SchemaComparison
//! [`CompareEntry::needs_source`]: schemaic_core::compare::CompareEntry::needs_source

use std::collections::HashSet;
use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::{create_effect, create_memo};

use schemaic_core::compare::{
    CompareEntry, CompareKind, CompareRow, ObjectStatus, RowFilter, SchemaComparison, is_planned,
};
use schemaic_core::diff::{DiffTag, line_diff};

use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, autohide, check_box, dismiss_layer,
    focus_root_with_ring, in_ring_button, link_button, loading_dots, modal_body_h,
    modal_footer_split, modal_pad_h, modal_title_owned, modal_w, panel_style,
};
use crate::{CompareSide, CompareState, CompareTarget, FieldCfg, Ui, edit_field, theme};

/// Tab stops, top to bottom as the modal reads: the source picker, the filter
/// box, then the two selection links. **The footer's button is not here** — it
/// takes [`ACTION_TAB`], the band every modal's footer actions register in, so
/// a body that later grows `VALUE_TAB`-based rows cannot overtake it.
///
/// The identical-objects checkbox is deliberately not a stop, and nor are the
/// picker's own rows (including `‹ back`): a click-only row is the pattern
/// everywhere else in this app — the dump modal's table picker — and giving
/// these stops the others lack would be the odd one out rather than an
/// improvement.
const TAB_PICK: u32 = 1;
const TAB_FILTER: u32 = 2;
const TAB_ALL: u32 = 3;
const TAB_NONE: u32 = 4;

/// Wide enough for a tree and a diff pane beside it without either becoming a
/// column of wrapped fragments — the pane is showing SQL, which does not wrap.
fn panel_w() -> f64 {
    modal_w(1040.0)
}

/// How far the picker's name is inset, so its focus ring has room either side
/// of the text rather than hugging the glyphs.
///
/// **One number, read twice.** It also insets the "source of truth" caption
/// beneath it: the padding shifts the name right and the caption is a plain
/// text view with none, so the two sat on visibly different left edges. Two
/// separately-tuned literals would drift apart again the first time the ring's
/// breathing room changed.
fn picker_inset() -> f64 {
    theme::scaled(2.0)
}

/// Open the comparison on `database`, with no right-hand side chosen yet.
pub(crate) fn open_compare(ui: &Ui, conn_id: u64, database: &str) {
    reset(ui.overlay);
    ui.overlay.compare.set(Some(CompareTarget {
        left: CompareSide {
            conn_id,
            database: database.to_string(),
        },
        right: None,
    }));
}

/// Every signal this modal owns, back to its opening value.
///
/// **One door, for opening and closing both.** These are eight separate signals
/// and the two paths used to write their own subsets — a `show_same` reset on
/// open and not on close, which is the shape that leaves a reopened comparison
/// showing a filter nobody set. A single function cannot forget one of them
/// asymmetrically; it can only be wrong about all of them at once, which is a
/// thing a reader can see.
fn reset(o: crate::OverlayUi) {
    o.compare_state.set(CompareState::Idle);
    o.compare_selected.set(HashSet::new());
    o.compare_expanded.set(HashSet::new());
    o.compare_focus.set(None);
    o.compare_query.set(String::new());
    o.compare_show_same.set(false);
    o.compare_dbs.set(None);
    o.compare_dbs_err.set(None);
}

pub(crate) fn compare_overlay(ui: Ui) -> impl IntoView {
    let target = ui.overlay.compare;
    let state = ui.overlay.compare_state;

    dyn_container(
        move || target.get(),
        move |open| {
            let Some(t) = open else {
                return empty().into_any();
            };
            let ui = ui.clone();
            let o = ui.overlay;
            let close: Rc<dyn Fn()> = Rc::new(move || {
                reset(o);
                target.set(None);
            });
            let ring = FocusRing::new();

            // Ask for the two schemas. **Once per pair**, and the closure reads
            // no signal to make that true: `target` is what this whole branch
            // is keyed on, so picking a new right-hand side rebuilds it and the
            // fresh effect fires with the fresh pair — while a tick, a
            // keystroke or an opened group touches none of it. Reading
            // `target` in here instead would leave the old effect able to fire
            // on the same change that replaces it, and two round trips per
            // pick. `properties_overlay` captures its target for this reason.
            if t.right.is_some() {
                let fetch = ui.schema_actions.compare_fetch.clone();
                let t = t.clone();
                create_effect(move |_| (fetch)(t.clone()));
            }

            let head = sources_bar(ui.clone(), t.clone(), ring.clone());
            // **One place says what went wrong**, whichever step failed. A
            // listing that couldn't reach its server and a pair that can't be
            // compared are the same kind of news to the person reading, so a
            // failed listing takes the body rather than a red line wedged into
            // the picker — and the picker's rows stay clickable behind it.
            // Nothing is discarded doing it: `compare_state` is untouched by a
            // listing, so clicking another connection clears the error and the
            // comparison that was already on screen comes straight back.
            let body = dyn_container(move || (state.get(), o.compare_dbs_err.get()), {
                let (ui, ring) = (ui.clone(), ring.clone());
                move |(st, list_err)| match list_err {
                    Some(e) => failure(e).into_any(),
                    None => body_for(st, &ui, ring.clone()),
                }
            })
            .style(|s| s.width_full().flex_col().flex_grow(1.0_f32).min_height(0.0));

            let title =
                modal_title_owned("Compare schemas".to_string(), close.clone(), ring.clone());
            let footer = footer(ui.clone(), close.clone(), ring.clone());

            let panel = v_stack((title, head, body, footer))
                .on_click_stop(|_| {})
                .style(|s| {
                    panel_style(s)
                        .background(theme::bg_panel())
                        .width(panel_w())
                        .height(modal_body_h(620.0))
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

// ── the two sources ─────────────────────────────────────────────────────────

/// The header: what is being compared, and the control that picks the other
/// half. **Left is what changes, right is the source of truth** — the arrow
/// says so, because a comparison that reads the wrong way round generates a
/// migration that runs on the wrong server.
fn sources_bar(ui: Ui, t: CompareTarget, ring: FocusRing) -> impl IntoView {
    let target = ui.overlay.compare;
    let left_label = side_label(&ui, &t.left);

    // An inline list rather than the popup menu, which anchors itself to a
    // measured rect: this control sits in a fixed header where a plain
    // expand-in-place is both simpler and easier to scan, the way the import
    // modal's sheet picker is.
    //
    // **Two steps, connection then database, and that is not decoration.** The
    // obvious one-step list is every connection crossed with `schema.db_nodes`,
    // and it is wrong in both directions: `db_nodes` is always the *active*
    // connection's, so the cross product offers another server databases it may
    // not hold, and cannot offer one only that server has — the comparison this
    // feature exists for. So picking a connection asks *it* what it has.
    let picking = RwSignal::new(false);

    // **The name is the control.** A separate "change" link beside it made the
    // clickable thing the word rather than the thing it names, and left an
    // unchosen side reading "Choose a database" next to a "change" that had
    // nothing to change yet. The name is what a reader is already pointing at.
    let chosen_label = {
        let ui = ui.clone();
        let ring = ring.clone();
        let toggle = move || picking.update(|p| *p = !*p);
        in_ring_button(
            // **The click goes on the view, not into `in_ring_button`.** Its
            // `on_press` is wired to KeyDown alone — Enter and Space — so a
            // control that hands it the action and nothing else answers the
            // keyboard and ignores the mouse, which is what left this label
            // dead to a click. `link_button` puts `on_click_stop` on its own
            // container and passes the action down as well; this is that,
            // spelled out because the asymmetry is invisible from the call.
            container(
                label(move || match target.get().and_then(|t| t.right.clone()) {
                    Some(s) => side_label(&ui, &s),
                    None => "Choose a database".to_string(),
                })
                .style(|s| {
                    s.font_family(crate::consts::MONO_FAMILY.to_string())
                        .font_size(theme::font_body())
                }),
            )
            .on_click_stop(move |_| toggle()),
            ring,
            TAB_PICK,
            true,
            4.0,
            move || picking.update(|p| *p = !*p),
        )
        .style(|s| {
            // The colour on the parent, not the label: `color` is inherited, so
            // a child setting its own shadows the hover — `link_button`'s own
            // comment records that failure.
            s.padding_horiz(picker_inset())
                .color(theme::accent())
                .hover(|s| s.color(theme::text()))
        })
    };

    let o = ui.overlay;
    let list = {
        let ui = ui.clone();
        // The modal's own ring, like every other focusable here. A fresh
        // `FocusRing::new()` belongs to no focus root, so Tab cannot reach the
        // control and it paints no ring — invisible until clicked.
        let left_type = left_db_type(&ui, &t.left);
        dyn_container(
            move || (picking.get(), o.compare_dbs.get()),
            move |(open, listed)| {
                if !open {
                    return empty().into_any();
                }
                // A listing that failed is reported in the **body**, beside the
                // cross-engine refusal, not inside this list: an error line here
                // pushed the rows down and, when it was an arm of its own,
                // replaced the very rows that could retry it. The picker stays a
                // picker, and the one place a comparison says what went wrong
                // stays one place.
                let inner: AnyView = match listed {
                    // Step two: that connection's own databases.
                    Some((cid, dbs)) => {
                        let rows = v_stack_from_iter(dbs.into_iter().map(move |db| {
                            pick_row(db.clone(), theme::text, move || {
                                let db = db.clone();
                                target.update(|t| {
                                    if let Some(t) = t {
                                        t.right = Some(CompareSide {
                                            conn_id: cid,
                                            database: db.clone(),
                                        });
                                    }
                                });
                                picking.set(false);
                            })
                        }))
                        .style(|s| s.flex_col().width_full());
                        // Back is a row like the rest, so it reads at the list's
                        // own size. `link_button` hardcodes `font_hint`, which
                        // made it the one line in the list set smaller than the
                        // things it sits above.
                        v_stack((
                            pick_row("‹ back".to_string(), theme::text_muted, move || {
                                o.compare_dbs.set(None)
                            }),
                            rows,
                        ))
                        .style(|s| s.flex_col().width_full())
                        .into_any()
                    }
                    // Step one: which server. **Only servers of the same
                    // engine**, through `connection::same_engine` rather than a
                    // comparison of its own: MariaDB and MySQL are one engine
                    // and compare fine, and offering a PostgreSQL connection to
                    // a MySQL comparison is offering a click whose only possible
                    // outcome is the refusal below.
                    None => {
                        let list_dbs = ui.schema_actions.compare_list_dbs.clone();
                        let conns: Vec<_> = ui
                            .conn
                            .connections
                            .get()
                            .into_iter()
                            .filter(|c| {
                                schemaic_core::connection::same_engine(&c.db_type, &left_type)
                            })
                            .collect();
                        if conns.is_empty() {
                            // A `step` value and not an early `return`, so it
                            // stays inside the picker's frame.
                            note("No connection of this engine to compare against.").into_any()
                        } else {
                            v_stack_from_iter(conns.into_iter().map(move |c| {
                                let list_dbs = list_dbs.clone();
                                let id = c.id;
                                pick_row(c.name.clone(), theme::text, move || (list_dbs)(id))
                            }))
                            .style(|s| s.flex_col().width_full())
                            .into_any()
                        }
                    }
                };
                autohide(scroll(inner).style(|s| s.width_full()))
                    .style(|s| {
                        s.width_full()
                            .max_height(theme::scaled(180.0))
                            .margin_top(theme::scaled(6.0))
                            .padding(theme::scaled(4.0))
                            .background(theme::bg_deepest())
                            .border(1.0)
                            .border_color(theme::border())
                            .border_radius(5.0)
                    })
                    .into_any()
            },
        )
        .style(|s| s.flex_col().width_full())
    };

    v_stack((
        h_stack((
            side_chip(left_label, "will change"),
            // Pointing at the side that changes. A comparison read the wrong way
            // round generates a migration for the wrong server, so the direction
            // is stated rather than implied by which column it is in.
            text("←").style(|s| {
                s.font_size(theme::font_body())
                    .color(theme::text_muted())
                    .margin_horiz(theme::scaled(12.0))
            }),
            v_stack((
                chosen_label,
                text("source of truth").style(|s| {
                    s.font_size(theme::font_hint())
                        .color(theme::text_muted())
                        .padding_left(picker_inset())
                }),
            ))
            .style(|s| s.flex_col().gap(theme::scaled(2.0))),
        ))
        .style(|s| s.items_center().width_full()),
        list,
    ))
    .style(|s| {
        s.flex_col()
            .width_full()
            .padding_horiz(modal_pad_h())
            .padding_vert(theme::scaled(10.0))
            .border_bottom(1.0)
            .border_color(theme::border())
    })
}

/// One side, as a name over what will happen to it.
fn side_chip(label: String, note: &'static str) -> impl IntoView {
    v_stack((
        text(label).style(|s| {
            s.font_family(crate::consts::MONO_FAMILY.to_string())
                .font_size(theme::font_body())
                .color(theme::text())
        }),
        text(note).style(|s| s.font_size(theme::font_hint()).color(theme::text_muted())),
    ))
    .style(|s| s.flex_col().gap(theme::scaled(2.0)))
}

/// The left side's `db_type` label, which is what the picker filters the
/// connection list against. Empty when the connection has gone — an empty label
/// predates the field, and `same_engine` reads it as MySQL, which is the same
/// answer the connection form's own picker gives it.
fn left_db_type(ui: &Ui, left: &CompareSide) -> String {
    ui.conn.connections.with_untracked(|cs| {
        cs.iter()
            .find(|c| c.id == left.conn_id)
            .map(|c| c.db_type.clone())
            .unwrap_or_default()
    })
}

/// One clickable line of either picker step.
///
/// `color` is a parameter because `‹ back` is not one of the things being
/// picked — it is the way out of the list — and reads dimmer than the names it
/// sits above so the eye lands on those first.
fn pick_row(
    label: String,
    color: fn() -> floem::peniko::Color,
    on_pick: impl Fn() + 'static,
) -> impl IntoView {
    text(label)
        .on_click_stop(move |_| on_pick())
        .style(move |s| {
            s.width_full()
                .font_family(crate::consts::MONO_FAMILY.to_string())
                .font_size(theme::font_body())
                .color(color())
                .padding_horiz(theme::scaled(8.0))
                .padding_vert(theme::scaled(4.0))
                .border_radius(4.0)
                .hover(|s| s.background(theme::row_hover_soft()))
        })
}

/// `connection · database`, which is the only unambiguous way to name a side:
/// two connections routinely hold a database of the same name, and that pair is
/// exactly the comparison this feature is for.
fn side_label(ui: &Ui, side: &CompareSide) -> String {
    let conn = ui.conn.connections.with_untracked(|cs| {
        cs.iter()
            .find(|c| c.id == side.conn_id)
            .map(|c| c.name.clone())
    });
    match conn {
        Some(name) => format!("{name} · {}", side.database),
        None => side.database.clone(),
    }
}

// ── the body, in whichever state the fetch is in ────────────────────────────

fn body_for(st: CompareState, ui: &Ui, ring: FocusRing) -> AnyView {
    match st {
        CompareState::Idle => note("Choose a database to compare against.").into_any(),
        CompareState::Loading => container(loading_dots(
            "Reading both schemas",
            theme::text_muted,
            theme::font_body,
        ))
        .style(|s| s.padding(modal_pad_h()))
        .into_any(),
        CompareState::Failed(e) => failure(e).into_any(),
        CompareState::Ready(c) => ready_body(c, ui, ring).into_any(),
    }
}

/// Why the comparison can't be shown — a refused pair, or a server that
/// couldn't be reached. **One rendering for both**, so the two read the same and
/// neither has to be found in a different part of the modal.
fn failure(msg: String) -> impl IntoView {
    text(msg).style(|s| {
        s.font_size(theme::font_body())
            .color(theme::error())
            .padding(modal_pad_h())
    })
}

fn note(msg: &'static str) -> impl IntoView {
    text(msg).style(|s| {
        s.font_size(theme::font_body())
            .color(theme::text_muted())
            .padding(modal_pad_h())
    })
}

/// The tree on the left, the two sides' text on the right.
fn ready_body(c: Rc<SchemaComparison>, ui: &Ui, ring: FocusRing) -> impl IntoView {
    let o = ui.overlay;
    let tree = {
        let (c, o) = (c.clone(), o);
        dyn_container(
            move || {
                (
                    o.compare_query.get(),
                    o.compare_show_same.get(),
                    o.compare_expanded.get(),
                )
            },
            move |(query, show_same, expanded)| {
                let rows = c.rows(
                    RowFilter {
                        query: &query,
                        show_same,
                    },
                    &expanded,
                );
                if rows.is_empty() {
                    // **Which emptiness this is** — `SchemaComparison::rows`'
                    // own question, asked of the same filter so the two cannot
                    // disagree. It lived here as three inline arms nothing could
                    // reach, and one of them was wrong: a filter that *matched*,
                    // but matched only objects the two schemas agree on with
                    // *Include identical* off, was reported as "Nothing matches
                    // that filter" — a claim about the filter over a result the
                    // toggle beside it produced.
                    return note(
                        c.empty_reason(RowFilter {
                            query: &query,
                            show_same,
                        })
                        .message(),
                    )
                    .into_any();
                }
                v_stack_from_iter(rows.into_iter().map(|r| row_view(r, o)))
                    .style(|s| s.flex_col().width_full())
                    .into_any()
            },
        )
        .style(|s| s.flex_col().width_full())
    };

    let pane = {
        let c = c.clone();
        dyn_container(
            move || o.compare_focus.get(),
            move |focus| {
                let entry = focus
                    .as_deref()
                    .and_then(|k| c.entries.iter().find(|e| e.key() == k));
                match entry {
                    Some(e) => diff_pane(e).into_any(),
                    None => note("Select an object to see both sides.").into_any(),
                }
            },
        )
        .style(|s| s.flex_col().size_full())
    };

    // A foreign-key cycle means no creation order satisfies every reference, so
    // the plan below is in an order one statement will be refused for. It is
    // said here as well as in the preview's risk block, because this is where
    // someone decides what to tick.
    let cycle_note: AnyView = if c.cycles() {
        h_stack((
            crate::icons::icon(crate::icons::TRIANGLE_ALERT, 12.0)
                .style(|s| s.color(theme::plan_warn())),
            text(
                "These tables' foreign keys form a cycle — no creation order satisfies \
                 all of them, so a generated migration may be refused partway.",
            )
            .style(|s| {
                s.font_size(theme::font_hint())
                    .color(theme::text_dim())
                    .min_width(0.0)
                    .flex_shrink(1.0_f32)
            }),
        ))
        .style(|s| {
            s.items_center()
                .width_full()
                .gap(theme::scaled(6.0))
                .padding_horiz(modal_pad_h())
                .padding_vert(theme::scaled(6.0))
                .background(theme::plan_warn_bg())
        })
        .into_any()
    } else {
        empty().into_any()
    };

    v_stack((
        filter_bar(ui.clone(), c.clone(), ring),
        cycle_note,
        h_stack((
            autohide(scroll(tree).style(|s| s.width_full())).style(|s| {
                s.width(theme::scaled(400.0))
                    .height_full()
                    .flex_shrink(0.0_f32)
                    .padding(theme::scaled(4.0))
                    .border_right(1.0)
                    .border_color(theme::border())
            }),
            autohide(scroll(pane).style(|s| s.size_full())).style(|s| {
                s.flex_grow(1.0_f32)
                    .height_full()
                    .min_width(0.0)
                    .background(theme::bg_deepest())
            }),
        ))
        .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0)),
    ))
    .style(|s| s.flex_col().width_full().flex_grow(1.0_f32).min_height(0.0))
    .into_any()
}

/// The filter box, the identical-objects toggle, and the tick-everything pair.
fn filter_bar(ui: Ui, c: Rc<SchemaComparison>, ring: FocusRing) -> impl IntoView {
    let o = ui.overlay;
    let counts = c.counts();
    let mut summary = format!(
        "{} differ · {} only here · {} only there · {} identical",
        counts.differing, counts.only_left, counts.only_right, counts.same
    );
    // **The uncertain tally, on the one line that is about the whole
    // comparison.** `CompareEntry::uncertain` was drawn only as a per-row hint,
    // and an uncertain match is overwhelmingly an object that came out `Same` —
    // which the default `show_same: false` hides. So the flag's own case was
    // invisible: the tree said two schemas agreed about an index it had never
    // fully read.
    if counts.uncertain > 0 {
        summary.push_str(&format!(
            " · {} {} the comparison can't vouch for",
            counts.uncertain,
            schemaic_core::text::plural(counts.uncertain, "match", "matches")
        ));
    }

    // **`edit_field`, not a styled `text_input`.** Hand-rolling the box got the
    // app's field wrong in three ways at once — a lighter border, a hover state
    // nothing else here has, and text sitting high in the box because the
    // placeholder's position is derived from the same metrics the editor uses
    // and a bare `padding_vert` doesn't move it. This is the one field, and it
    // is the same one the users browser filters accounts with.
    let field = container(
        edit_field(
            o.compare_query,
            FieldCfg {
                placeholder: "Filter by name",
                clearable: true,
                focus: Some((ring.clone(), TAB_FILTER)),
                ..Default::default()
            },
        )
        .style(|s| s.width_full()),
    )
    .style(|s| s.width(theme::scaled(220.0)).flex_shrink(0.0_f32));

    let show_same = {
        let shown = move || o.compare_show_same.get();
        h_stack((
            check_box(shown),
            text("Include identical")
                .style(|s| s.font_size(theme::font_label()).color(theme::text_dim())),
        ))
        .on_click_stop(move |_| o.compare_show_same.update(|v| *v = !*v))
        // The extra margin is on the control, not added to the bar's `gap`:
        // this is the one seam in the row that needs more air — a toggle
        // butting up against the tally it changes — and widening the gap would
        // space the filter box and the two links to match.
        .style(|s| {
            s.items_center()
                .gap(theme::scaled(6.0))
                .margin_right(theme::scaled(10.0))
        })
    };

    // **"All" means what the filter is showing**, which is why the keys are
    // read at click time through `selectable_keys` rather than collected once
    // from the whole comparison: this link sits in the same bar as the filter
    // box, and one that reached past it ticked every difference in the
    // comparison — a plan over objects the user had just filtered away and
    // never looked at. `None` is symmetric — it clears only the ticks on rows
    // that are on screen, so a filtered-out selection is not silently thrown
    // away either.
    let keys_now = {
        let c = c.clone();
        move || {
            o.compare_query.with(|q| {
                c.selectable_keys(RowFilter {
                    query: q,
                    show_same: o.compare_show_same.get_untracked(),
                })
            })
        }
    };

    h_stack((
        field,
        show_same,
        text(summary).style(|s| {
            s.font_size(theme::font_hint())
                .color(theme::text_muted())
                .min_width(0.0)
                .flex_shrink(1.0_f32)
        }),
        empty().style(|s| s.flex_grow(1.0_f32).min_width(theme::scaled(8.0))),
        link_button("Select all", theme::accent, ring.clone(), TAB_ALL, {
            let keys_now = keys_now.clone();
            move || {
                let shown = keys_now();
                o.compare_selected.update(|sel| sel.extend(shown));
            }
        }),
        link_button("None", theme::text_muted, ring, TAB_NONE, {
            let keys_now = keys_now.clone();
            move || {
                let shown: HashSet<String> = keys_now().into_iter().collect();
                o.compare_selected
                    .update(|sel| sel.retain(|k| !shown.contains(k)));
            }
        }),
    ))
    .style(|s| {
        s.items_center()
            .width_full()
            .gap(theme::scaled(10.0))
            .padding_horiz(modal_pad_h())
            .padding_vert(theme::scaled(8.0))
            .border_bottom(1.0)
            .border_color(theme::border())
    })
}

// ── one row ─────────────────────────────────────────────────────────────────

fn row_view(row: CompareRow<'_>, o: crate::OverlayUi) -> AnyView {
    match row {
        CompareRow::Group {
            kind,
            counts,
            expanded,
        } => group_row(kind, counts, expanded, o).into_any(),
        CompareRow::Object(e) => object_row(e, o).into_any(),
    }
}

/// A kind's heading: a chevron, the plural noun, and what is under it.
fn group_row(
    kind: CompareKind,
    counts: schemaic_core::compare::CompareCounts,
    expanded: bool,
    o: crate::OverlayUi,
) -> impl IntoView {
    let label = format!(
        "{}{} ({})",
        kind.label(),
        schemaic_core::text::plural(counts.total(), "", "s"),
        counts.total()
    );
    let key = kind.label().to_string();
    h_stack((
        crate::icons::icon(
            if expanded {
                crate::icons::CHEVRON_DOWN
            } else {
                crate::icons::CHEVRON_RIGHT
            },
            10.0,
        )
        .style(|s| s.color(theme::text_muted())),
        // **No `font_weight` here.** `Weight::MEDIUM` was the only use of it in
        // the crate, and asking for a weight the loaded family has no face for
        // fell through to a symbol font: every heading rendered as a row of
        // unrelated glyphs, which is what an ASCII string looks like in
        // Wingdings. Nothing else in this app sets a weight, and a heading is
        // already told apart by its colour and its chevron.
        text(label).style(|s| s.font_size(theme::font_label()).color(theme::text_dim())),
    ))
    .on_click_stop(move |_| {
        o.compare_expanded.update(|set| {
            if !set.remove(&key) {
                set.insert(key.clone());
            }
        })
    })
    .style(|s| {
        s.items_center()
            .width_full()
            .gap(theme::scaled(6.0))
            .padding_horiz(theme::scaled(6.0))
            .padding_vert(theme::scaled(5.0))
            .border_radius(4.0)
            .hover(|s| s.background(theme::row_hover_soft()))
    })
}

/// The leading column both a tick and a warning sit in — the [`check_box`]'s
/// own 15px, centred, so a row's name starts in the same place either way.
fn lead_box(s: floem::style::Style) -> floem::style::Style {
    s.width(theme::scaled(15.0))
        .flex_shrink(0.0_f32)
        .items_center()
        .justify_center()
}

/// One object: its tick, its status, its name.
fn object_row(e: &CompareEntry, o: crate::OverlayUi) -> impl IntoView {
    let key = e.key();
    let blocked = e.needs_source();
    let ticked = {
        let key = key.clone();
        move || o.compare_selected.with(|s| s.contains(&key))
    };
    let focused = {
        let key = key.clone();
        move || o.compare_focus.with(|f| f.as_deref() == Some(key.as_str()))
    };

    // The tick, or — where a body would have to be re-read before it could be
    // emitted — nothing to tick and a reason in its place.
    //
    // Both are boxed to the check-box's own 15px so the names line up down the
    // list whichever a row has: the warning triangle is smaller, and left to
    // itself it shifted every blocked row's name a few pixels left of its
    // neighbours'.
    let lead: AnyView = if blocked {
        container(
            crate::icons::icon(crate::icons::TRIANGLE_ALERT, 12.0)
                .style(|s| s.color(theme::plan_warn())),
        )
        .style(lead_box)
        .into_any()
    } else {
        let toggle_key = key.clone();
        container(check_box(ticked))
            .on_click_stop(move |_| {
                let k = toggle_key.clone();
                o.compare_selected.update(|set| {
                    if !set.remove(&k) {
                        set.insert(k);
                    }
                })
            })
            .style(lead_box)
            .into_any()
    };

    let status = e.status;
    let hint = if blocked {
        Some("body must be re-read before this can be applied")
    } else if e.uncertain {
        Some("an index this model reads only in part — a match here isn't certain")
    } else {
        None
    };

    let focus_key = key.clone();
    let row = h_stack((
        lead,
        // Centred in its column, not left-aligned in it. A glyph parked at the
        // left of a 14px box sits hard against the tick with all the slack
        // falling on the name's side, which is the lopsided gap the eye picks
        // up in a long list.
        container(text(status_glyph(status)).style(move |s| {
            s.font_family(crate::consts::MONO_FAMILY.to_string())
                .font_size(theme::font_body())
                .color(status_color(status))
        }))
        .style(|s| {
            s.width(theme::scaled(12.0))
                .flex_shrink(0.0_f32)
                .items_center()
                .justify_center()
        }),
        text(e.label()).style(|s| {
            s.font_family(crate::consts::MONO_FAMILY.to_string())
                .font_size(theme::font_body())
                .color(theme::text())
                .min_width(0.0)
                .flex_shrink(1.0_f32)
        }),
    ))
    .style(|s| s.items_center().width_full().gap(theme::scaled(8.0)));

    // A plain match, not a `dyn_container`: `hint` is a constant by the time we
    // get here, so a container around it registered an updater that could never
    // fire and cost a `ViewId` and a child `Scope` per row — hundreds of them
    // built and disposed on every filter keystroke, since the tree rebuilds
    // wholesale.
    let hint_row: AnyView = match hint {
        Some(h) => text(h)
            .style(|s| {
                s.font_size(theme::font_hint())
                    .color(theme::text_muted())
                    .margin_left(theme::scaled(44.0))
            })
            .into_any(),
        None => empty().into_any(),
    };

    v_stack((row, hint_row))
        // **Guarded, because `set` never dedups.** Re-clicking the row that is
        // already selected notified without changing anything: the diff pane is
        // a `dyn_container` keyed on this signal, so it was torn down and
        // rebuilt — a fresh `line_diff` over both sides' DDL — and every row in
        // the tree restyled. The same feature guards its sibling signal in
        // `compare_list_dbs` for the same reason.
        .on_click_stop(move |_| {
            if o.compare_focus
                .with_untracked(|f| f.as_deref() != Some(focus_key.as_str()))
            {
                o.compare_focus.set(Some(focus_key.clone()));
            }
        })
        .style(move |s| {
            let s = s
                .flex_col()
                .width_full()
                .padding_horiz(theme::scaled(6.0))
                .padding_vert(theme::scaled(4.0))
                .margin_left(theme::scaled(14.0))
                .border_radius(4.0);
            // **The selected row keeps its own colour under the pointer.** A single
            // unconditional `hover` wins over the base background, so hovering the
            // selected row *dimmed* it to the hover tint — the one row in the list
            // that visibly lost its state by being pointed at. The selected arm
            // therefore restates its colour as the hover colour too.
            if focused() {
                s.background(theme::row_selected())
                    .hover(|s| s.background(theme::row_selected()))
            } else {
                s.hover(|s| s.background(theme::row_hover_soft()))
            }
        })
}

/// One character per status, so a long list reads down the column rather than
/// as a wall of words: `+` arrives, `−` goes, `~` changes.
fn status_glyph(s: ObjectStatus) -> &'static str {
    match s {
        ObjectStatus::OnlyRight => "+",
        ObjectStatus::OnlyLeft => "−",
        ObjectStatus::Differing => "~",
        ObjectStatus::Same => "=",
    }
}

fn status_color(s: ObjectStatus) -> floem::peniko::Color {
    match s {
        ObjectStatus::OnlyRight => theme::diff_add_marker(),
        ObjectStatus::OnlyLeft => theme::diff_del_marker(),
        ObjectStatus::Differing => theme::plan_warn(),
        ObjectStatus::Same => theme::text_muted(),
    }
}

// ── the diff pane ───────────────────────────────────────────────────────────

/// The two sides of one object, as a unified line diff.
///
/// [`line_diff`] over the two `CREATE` texts the comparison captured — the same
/// differ the inline AI edit preview reads, and no second opinion about what
/// changed. It is a *reading* of the object, not the plan: the statements come
/// from the change set, which is why nothing here is copyable as SQL.
fn diff_pane(e: &CompareEntry) -> impl IntoView {
    let heading = format!("{} {}", e.kind.label(), e.label());
    let lines = line_diff(&e.left_ddl, &e.right_ddl);
    let rows = v_stack_from_iter(lines.into_iter().map(|(tag, line)| {
        let mark = match tag {
            DiffTag::Equal => " ",
            DiffTag::Del => "−",
            DiffTag::Ins => "+",
        };
        h_stack((
            text(mark).style(move |s| {
                s.font_family(crate::consts::MONO_FAMILY.to_string())
                    .font_size(theme::font_body())
                    .color(match tag {
                        DiffTag::Equal => theme::text_muted(),
                        DiffTag::Del => theme::diff_del_marker(),
                        DiffTag::Ins => theme::diff_add_marker(),
                    })
                    .width(theme::scaled(12.0))
                    .flex_shrink(0.0_f32)
            }),
            text(line).style(|s| {
                s.font_family(crate::consts::MONO_FAMILY.to_string())
                    .font_size(theme::font_body())
                    .color(theme::text())
            }),
        ))
        // **The colour is looked up *inside* the closure.** A `Color` captured
        // out here is read once at build time, so switching theme repainted
        // every marker and every line of text — those call `theme::…()` in
        // their own closures — while the added and removed rows kept the old
        // palette's backgrounds until the pane happened to be rebuilt.
        // Invariant: themable colours reach a reactive style as `fn() -> Color`.
        .style(move |s| {
            let s = s
                .items_start()
                .width_full()
                .padding_horiz(theme::scaled(4.0));
            match tag {
                DiffTag::Equal => s,
                DiffTag::Del => s.background(theme::diff_del_bg()),
                DiffTag::Ins => s.background(theme::diff_add_bg()),
            }
        })
    }))
    .style(|s| s.flex_col().width_full());

    // **A `Same` row with a red-and-green pane under it needs a sentence.** On
    // SQLite the pane's text is `sqlite_master.sql` verbatim while the status
    // comes from the structured differ, so two tables that really do agree can
    // carry different whitespace, quoting or clause order — and every one of
    // those draws as a change under a row labelled identical. Nothing is hidden:
    // the difference is real and worth seeing. What was missing is the reading.
    let note: AnyView = if e.text_differs_though_same() {
        text(
            "These agree structurally — the difference below is in how the engine \
             stores the statement, and there is nothing to migrate.",
        )
        .style(|s| {
            s.font_size(theme::font_hint())
                .color(theme::text_muted())
                .width_full()
                .padding_horiz(theme::scaled(8.0))
                .padding_bottom(theme::scaled(6.0))
        })
        .into_any()
    } else {
        empty().into_any()
    };

    v_stack((
        text(heading).style(|s| {
            s.font_size(theme::font_label())
                .color(theme::text_dim())
                .padding_horiz(theme::scaled(8.0))
                .padding_vert(theme::scaled(6.0))
        }),
        note,
        rows,
    ))
    .style(|s| s.flex_col().width_full().padding_bottom(theme::scaled(8.0)))
}

// ── the footer ──────────────────────────────────────────────────────────────

/// What is ticked, and the one button that turns it into a plan.
///
/// Preview never applies: it builds a [`SchemaPlan`] and hands it to the same
/// DDL preview modal every other generated statement in this app goes through,
/// which is where Apply lives and where the write guard is.
///
/// [`SchemaPlan`]: schemaic_core::compare::SchemaPlan
fn footer(ui: Ui, close: Rc<dyn Fn()>, ring: FocusRing) -> impl IntoView {
    let o = ui.overlay;
    let state = o.compare_state;

    // The plan the tick-boxes describe, recomputed when either changes. A memo
    // rather than a call per render: the footer's status, the button's enabled
    // state and its click all ask the same question — and it has to be the
    // *same* question, `plan_of` included. Counting over a laxer predicate than
    // the one that builds the plan is how a confident "1 object · 3 statements"
    // ends up over a button that builds an empty plan and returns.
    // **Counted, not built.** This ran `plan_of` and then `emit()` on every
    // tick — cloning a `ChangeSet` per selected object and rendering the whole
    // migration's SQL synchronously on the UI thread, to display a number. Over
    // a few hundred objects that is the frame budget spent per keystroke of a
    // checkbox. The statement count went with it: the preview shows that, and
    // it cannot be had without emitting.
    //
    // `with` rather than `get` on the selection, so the `HashSet` is read in
    // place instead of cloned.
    let planned = create_memo(move |_| {
        let CompareState::Ready(c) = state.get() else {
            return 0usize;
        };
        o.compare_selected
            .with(|sel| c.differences().filter(|e| is_planned(e, sel)).count())
    });

    let status = label(move || schemaic_core::compare::selection_note(planned.get()))
        .style(|s| s.font_size(theme::font_hint()).color(theme::text_muted()));

    let preview = {
        let ui = ui.clone();
        let close = close.clone();
        // Keyed on a `Memo<bool>`, which only notifies when the value actually
        // changes. `dyn_container` has no equality check of its own, so keying
        // it on the count tore the button down and rebuilt it — losing its
        // focus-ring registration — on every tick rather than on the two
        // transitions that matter.
        let any = create_memo(move |_| planned.get() > 0);
        dyn_container(
            move || any.get(),
            move |enabled| {
                let (ui, close, ring) = (ui.clone(), close.clone(), ring.clone());
                action_button(
                    "Preview migration",
                    ActionKind::Primary,
                    enabled,
                    ring,
                    ACTION_TAB,
                    move || open_plan_preview(&ui, close.clone()),
                )
                .into_any()
            },
        )
    };

    modal_footer_split(status, preview)
}

/// The plan a selection describes, over [`is_planned`].
fn plan_of(c: &SchemaComparison, selected: &HashSet<String>) -> schemaic_core::compare::SchemaPlan {
    c.plan(|e| is_planned(e, selected))
}

/// Build the plan the ticks describe and send it to the DDL preview.
fn open_plan_preview(ui: &Ui, close: Rc<dyn Fn()>) {
    let CompareState::Ready(c) = ui.overlay.compare_state.get_untracked() else {
        return;
    };
    let Some(t) = ui.overlay.compare.get_untracked() else {
        return;
    };
    let sel = ui.overlay.compare_selected.get_untracked();
    let plan = plan_of(&c, &sel);
    if plan.is_empty() {
        return;
    }

    let read_only = ui.conn.connections.with_untracked(|cs| {
        cs.iter()
            .find(|c| c.id == t.left.conn_id)
            .is_some_and(|c| c.read_only)
    });
    let preview = crate::ddl_preview::preview_of_plan(
        t.left.conn_id,
        &t.left.database,
        // Named for the database it lands in, not just the connection: the
        // left-hand side is what these statements change, and this modal is the
        // last thing the user sees before they run.
        plan.subject_in(&t.left.database),
        &plan,
        read_only,
    );
    // The comparison closes behind the preview: the plan is a snapshot of what
    // was ticked, and leaving the tree open behind it invites editing a
    // selection the preview no longer reflects.
    (close)();
    crate::ddl_preview::open_preview(ui, preview);
}
