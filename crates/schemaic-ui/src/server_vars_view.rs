//! The **Server variables** viewer: the active connection's settings, as the
//! engine reports them, in a read-only list with a search box.
//!
//! Opened from the SCHEMA gear, the tree's blank-space menu or the palette by
//! setting `overlay.server_vars`; an effect then asks the app for the list,
//! which lands in `overlay.server_vars_state`. The *Variables | Status* switch
//! is offered only where the engine has a status list
//! ([`schemaic_core::server_vars::supports_status`] — MySQL/MariaDB), and
//! switching is a fetch of the other list.
//!
//! **Server-level, like the Users browser beside it** in the gear: a setting
//! belongs to the server, not to a database, so the modal takes no object. On
//! SQLite the "server" is the library, and its settings are its PRAGMAs, read
//! from a fixed list that cannot reach one that acts
//! ([`schemaic_core::server_vars::SQLITE_PRAGMAS`]).
//!
//! **Read-only, and nothing here writes.** A `SET GLOBAL` or an `ALTER SYSTEM`
//! is a statement the user can type in the editor, through the write guard; a
//! value field here would be a second path to the server with no guard on it.
//!
//! **The list is capped at what gets drawn**, not at what is held: a MariaDB
//! server reports some six hundred variables, and the list is rebuilt on every
//! keystroke of the search. [`render_slice`](schemaic_core::server_vars::render_slice)
//! keeps the first [`RENDER_CAP`](schemaic_core::server_vars::RENDER_CAP) and
//! the line under them says how many more matched.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::intel::SqlDialect;
use schemaic_core::server_vars::{self, ServerVar, VarKind};
use schemaic_core::text::plural;

use crate::consts::MONO_FAMILY;
use crate::theme::{font_body, font_hint, font_label};
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, action_gap, autohide, dismiss_layer,
    focus_root_with_ring, modal_footer_split, modal_pad_h, modal_title_owned, modal_w, panel_style,
};
use crate::{FieldCfg, OverlayUi, VarsState, VarsTarget, edit_field, icons, theme};

/// Modal width. A PostgreSQL description runs to a sentence, and a MySQL value
/// can be a path or an `sql_mode` list; narrower, both wrap into a column of
/// fragments.
fn panel_w() -> f64 {
    modal_w(760.0)
}

/// A fixed height, for `users_view::PANEL_H`'s reason: the list is a scroll
/// with `flex_grow(1)`, which has nothing to grow into inside an auto-height
/// parent.
const PANEL_H: f64 = 560.0;

/// The name column's width — wide enough for
/// `innodb_buffer_pool_dump_at_shutdown` without the value column starting
/// at a different place on every row.
fn name_w() -> f64 {
    theme::scaled(300.0)
}

// ── what the viewer reads ────────────────────────────────────────────────────

/// Everything this modal reaches out of the root bundle: its own signals and
/// the one fetch. Built at its one call site, for `whole_ui_gate`'s reason.
#[derive(Clone)]
pub(crate) struct VarsCtx {
    overlay: OverlayUi,
    fetch: Rc<dyn Fn(VarsTarget, VarKind)>,
}

impl VarsCtx {
    pub(crate) fn new(overlay: OverlayUi, actions: &crate::SchemaActions) -> Self {
        Self {
            overlay,
            fetch: actions.server_vars.clone(),
        }
    }
}

// ── opening ──────────────────────────────────────────────────────────────────

/// Open the viewer on one connection.
///
/// Every signal the modal reads is reset here rather than on close, so a
/// second opening can never flash the previous server's settings while the new
/// list is in flight — `users_view::open_for_server`'s rule. The fetch is the
/// modal's own effect, so this door only writes signals.
pub(crate) fn open_for_server(overlay: OverlayUi, conn_id: u64, name: String, dialect: SqlDialect) {
    overlay.server_vars_state.set(VarsState::Loading);
    overlay.server_vars_kind.set(VarKind::Variables);
    overlay.server_vars_filter.set(String::new());
    overlay.server_vars.set(Some(VarsTarget {
        conn_id,
        name,
        dialect,
    }));
}

/// [`open_for_server`] on the active connection — the door every entry point
/// uses. A no-op with no connection selected, which the menus already grey
/// out.
pub(crate) fn open_for_active(conn: crate::ConnUi, overlay: OverlayUi) {
    let id = conn.active_conn.get_untracked();
    let Some((name, dialect)) = conn.connections.with_untracked(|cs| {
        schemaic_core::connection::by_id(cs, id)
            .map(|c| (c.name.clone(), SqlDialect::from_db_type(&c.db_type)))
    }) else {
        return;
    };
    open_for_server(overlay, id, name, dialect);
}

pub(crate) fn server_vars_overlay(ctx: VarsCtx) -> impl IntoView {
    let target = ctx.overlay.server_vars;
    let kind = ctx.overlay.server_vars_kind;
    let state = ctx.overlay.server_vars_state;
    let filter = ctx.overlay.server_vars_filter;

    // **The fetch keys on the target and the kind**, and lives above the
    // container rather than inside it — `users_view`'s rule: an effect created
    // in the child runs again on every rebuild of it.
    {
        let fetch = ctx.fetch.clone();
        create_effect(move |prev: Option<(Option<VarsTarget>, VarKind)>| {
            let now = (target.get(), kind.get());
            // `create_effect` has no equality check of its own.
            if prev.as_ref() != Some(&now)
                && let Some(t) = now.0.clone()
            {
                state.set(VarsState::Loading);
                (fetch)(t, now.1);
            }
            now
        });
    }

    let refetch = ctx.fetch.clone();
    dyn_container(
        move || target.get(),
        move |open| {
            let Some(t) = open else {
                return empty().into_any();
            };
            let close: Rc<dyn Fn()> = Rc::new(move || {
                target.set(None);
                state.set(VarsState::Loading);
                filter.set(String::new());
            });
            let ring = FocusRing::new();

            let title = modal_title_owned(
                format!("Server variables \u{2014} {}", t.name),
                close.clone(),
                ring.clone(),
            );

            let switch = if server_vars::supports_status(t.dialect) {
                kind_switch(kind).into_any()
            } else {
                empty().into_any()
            };

            let field = edit_field(
                filter,
                FieldCfg {
                    placeholder: "Search settings",
                    clearable: true,
                    focus: Some((ring.clone(), 10)),
                    ..Default::default()
                },
            )
            .style(|s| s.width_full());

            let top = v_stack((switch, field)).style(|s| {
                s.width_full()
                    .flex_col()
                    .gap(theme::scaled(8.0))
                    .padding_horiz(modal_pad_h())
                    .padding_top(theme::scaled(10.0))
                    .flex_shrink(0.0_f32)
            });

            let list = autohide(scroll(var_list(state, filter)).style(|s| {
                s.width_full()
                    .flex_grow(1.0_f32)
                    .min_height(0.0)
                    .margin_top(theme::scaled(10.0))
            }));

            let refresh = {
                let fetch = refetch.clone();
                let t = t.clone();
                move || {
                    state.set(VarsState::Loading);
                    (fetch)(t.clone(), kind.get_untracked());
                }
            };
            let footer = footer(state, filter, refresh, close.clone(), ring.clone());

            let panel = v_stack((title, top, list, footer))
                .on_click_stop(|_| {})
                .style(|s| {
                    panel_style(s)
                        .background(theme::bg_panel())
                        .width(panel_w())
                        .height(crate::widgets::modal_h(PANEL_H))
                        .flex_col()
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

/// *Variables | Status* — Server Activity's *Sessions | Locks* switch, the
/// same underlined labels. A switch is a fetch, which the modal's effect makes.
fn kind_switch(kind: RwSignal<VarKind>) -> impl IntoView {
    let tab = move |label: &'static str, which: VarKind| {
        text(label)
            .on_click_stop(move |_| {
                if kind.get_untracked() != which {
                    kind.set(which);
                }
            })
            .style(move |s| {
                let on = kind.get() == which;
                s.font_size(font_label())
                    .padding_vert(theme::scaled(3.0))
                    .margin_right(theme::scaled(14.0))
                    .cursor(floem::style::CursorStyle::Pointer)
                    .border_bottom(2.0)
                    .border_color(if on {
                        theme::accent()
                    } else {
                        floem::peniko::Color::TRANSPARENT
                    })
                    .color(if on { theme::text() } else { theme::text_dim() })
                    .hover(|s| s.color(theme::text()))
            })
    };
    h_stack((
        tab("Variables", VarKind::Variables),
        tab("Status", VarKind::Status),
    ))
    .style(|s| s.items_center())
}

/// The list, rebuilt on a new answer and on every keystroke of the search —
/// cheap, because only [`server_vars::RENDER_CAP`] rows are ever built.
fn var_list(state: RwSignal<VarsState>, filter: RwSignal<String>) -> impl IntoView {
    dyn_container(
        move || (state.get(), filter.get()),
        move |(st, needle)| match st {
            VarsState::Loading => message(
                icons::CLOCK,
                theme::text_faint,
                "Loading\u{2026}".to_string(),
            ),
            VarsState::Unsupported => message(
                icons::CIRCLE_QUESTION,
                theme::text_faint,
                "This connection's engine publishes no list of this kind.".to_string(),
            ),
            VarsState::Failed(e) => message(icons::TRIANGLE_ALERT, theme::error, e),
            VarsState::Loaded(vars) => {
                let (shown, more) = server_vars::render_slice(&vars, &needle);
                if shown.is_empty() {
                    return message(
                        icons::CIRCLE_QUESTION,
                        theme::text_faint,
                        "No matching settings.".to_string(),
                    );
                }
                let mut rows: Vec<AnyView> = shown.into_iter().map(var_row).collect();
                if more > 0 {
                    rows.push(message(
                        icons::CIRCLE_QUESTION,
                        theme::text_faint,
                        format!(
                            "{more} more {} \u{2014} narrow the search",
                            plural(more, "setting matches", "settings match")
                        ),
                    ));
                }
                v_stack_from_iter(rows)
                    .style(|s| s.flex_col().width_full())
                    .into_any()
            }
        },
    )
    .style(|s| s.width_full().flex_col())
}

/// One setting: the name in mono, the value beside it, and the engine's
/// description dim beneath the value when it has one.
fn var_row(v: &ServerVar) -> AnyView {
    let name = text(v.name.clone()).style(|s| {
        s.font_family(MONO_FAMILY.to_string())
            .font_size(font_body())
            .color(theme::text())
            .width(name_w())
            .flex_shrink(0.0_f32)
            .padding_right(theme::scaled(12.0))
    });
    let value = text(v.value.clone())
        .style(|s| s.font_size(font_body()).color(theme::text()).min_width(0.0));
    let right: AnyView = match v.detail.clone() {
        Some(d) => v_stack((
            value,
            text(d).style(|s| {
                s.font_size(font_hint())
                    .color(theme::text_faint())
                    .min_width(0.0)
            }),
        ))
        .style(|s| s.flex_col().min_width(0.0).flex_grow(1.0_f32))
        .into_any(),
        None => container(value)
            .style(|s| s.min_width(0.0).flex_grow(1.0_f32))
            .into_any(),
    };
    h_stack((name, right))
        .style(|s| {
            s.width_full()
                .flex_row()
                .items_start()
                .padding_horiz(modal_pad_h())
                .padding_vert(theme::scaled(4.0))
                .border_bottom(1.0)
                .border_color(theme::border())
        })
        .into_any()
}

/// An icon-led sentence standing in the list — a state or a count.
fn message(icon: &'static str, color: fn() -> floem::peniko::Color, text: String) -> AnyView {
    container(crate::widgets::fact_note(icon, color, text))
        .style(|s| {
            s.width_full()
                .min_width(0.0)
                .padding_horiz(modal_pad_h())
                .padding_vert(theme::scaled(6.0))
        })
        .into_any()
}

/// The count on the left; Refresh and Close on the right.
fn footer(
    state: RwSignal<VarsState>,
    filter: RwSignal<String>,
    refresh: impl Fn() + 'static,
    close: Rc<dyn Fn()>,
    ring: FocusRing,
) -> AnyView {
    let status = label(move || match state.get() {
        VarsState::Loaded(vars) => {
            let needle = filter.get();
            let total = vars.len();
            let shown = vars
                .iter()
                .filter(|v| server_vars::var_matches(v, &needle))
                .count();
            // "3 of 600" only while the search is narrowing them, for the
            // Users browser's reason.
            if shown == total {
                format!("{total} {}", plural(total, "setting", "settings"))
            } else {
                format!("{shown} of {total} settings")
            }
        }
        _ => String::new(),
    })
    .style(|s| s.font_size(font_hint()).color(theme::text_faint()));

    modal_footer_split(
        status,
        h_stack((
            action_button(
                "Refresh",
                ActionKind::Quiet,
                true,
                ring.clone(),
                ACTION_TAB,
                refresh,
            ),
            action_button(
                "Close",
                ActionKind::Neutral,
                true,
                ring,
                ACTION_TAB + 1,
                move || close(),
            ),
        ))
        .style(|s| s.flex_row().items_center().gap(action_gap())),
    )
    .into_any()
}
