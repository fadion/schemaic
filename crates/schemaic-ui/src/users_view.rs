//! The **Users and privileges** browser: which accounts this server knows, and
//! what each of them is allowed to do.
//!
//! Opened from the SCHEMA gear by setting `overlay.users`; an effect then asks
//! the app for the server's accounts, which land in `overlay.users_state`.
//! Picking one asks for that account's privileges, into `overlay.users_grants`.
//!
//! **Server-level, like the gear it opens from.** An account belongs to the
//! server rather than to a database, so this modal has no object and takes none
//! — which is also why the gear is its home: a connection whose tree already
//! fills the panel has no blank space to right-click, and no database row would
//! be the right one to right-click anyway.
//!
//! **Two panes, because the two halves answer different questions.** The list is
//! *who exists* — scannable, filterable, with the server's own accounts sorted
//! to the bottom. The detail pane is *what one of them may do*, and it shows the
//! engine's own `GRANT` statements rather than a table this app invents: on
//! MySQL that is literally what `SHOW GRANTS` returned, and on PostgreSQL
//! `users::pg_grant_statements` reassembles the same sentences from the
//! catalogue. One rendering, so the two engines cannot disagree about what a
//! privilege is called, and so the form that grants and revokes offers the same
//! words this list reads back.
//!
//! **Read-only, and it says what it cannot see.** PostgreSQL keeps object
//! privileges in the catalogue of the database that holds the object, so one
//! connection answers for one database; the note under the statements is that
//! sentence, and it is there because a privilege screen that is quietly partial
//! is the one way this feature can mislead.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::text::plural;
use schemaic_core::users::{Grants, Principal, PrincipalKind, WriteGate};

use schemaic_core::intel::SqlDialect;

use crate::theme::{font_body, font_hint};
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, action_gap, autohide, dismiss_layer,
    focus_root_with_ring, in_ring_button, modal_footer_split, modal_pad_h, modal_title_owned,
    modal_w, panel_style,
};
use crate::{FieldCfg, GrantsState, Ui, UsersState, UsersTarget, edit_field, icons, theme};

/// Modal width. The grant pane holds SQL, and a MySQL grant on a namespaced
/// table (`` GRANT SELECT ON `warehouse`.`shipment_line` TO `app`@`%` ``) is
/// the length that decides this: narrower and the ordinary statement wraps, and
/// a wrapped list of statements stops looking like a list.
fn panel_w() -> f64 {
    modal_w(880.0)
}

/// **A fixed height, like every other modal here**, and for the same reason
/// `database_editor::PANEL_H` is one: both panes are scrolls with
/// `flex_grow(1)`, and inside an auto-height parent there is no free space for
/// them to grow into, so the panel would resolve to a title bar sitting on a
/// footer. Set so a server with a couple of dozen accounts fills it without the
/// list feeling cramped; `modal_h` caps it against short windows.
const PANEL_H: f64 = 560.0;

/// The account list's width. Wide enough for `some_service_account@10.0.0.%`,
/// which is the shape a real MySQL deployment's names take, without stealing
/// from the pane that holds SQL.
fn list_w() -> f64 {
    theme::scaled(260.0)
}

/// The gap between account rows — the 5px Manage Connections' list uses, so two
/// lists of names in the same app are spaced the same.
fn row_gap() -> f64 {
    theme::scaled(5.0)
}

/// How far a row's glyph sits in from the column's edge. A row is full width so
/// its selected background spans the column, which means the inset is padding on
/// the row rather than on the column — the same 12px `menu_item_style` uses, and
/// the column the `+ New account` row's icon lands on.
fn row_inset() -> f64 {
    theme::scaled(12.0)
}

/// The label column of the attributes list, fixed so the values line up — a
/// ragged left edge on a column of facts reads as unrelated lines.
fn label_w() -> f64 {
    theme::scaled(124.0)
}

// ── opening ──────────────────────────────────────────────────────────────────

/// Open the browser for one server.
///
/// The fetch is kicked off by the modal itself, so every entry point is this one
/// call — and every signal it reads is reset here rather than on close, so a
/// second opening can never flash the previous server's accounts while the new
/// list is in flight.
///
/// `database` is the one PostgreSQL's per-database privileges will be read from;
/// `None` (no database selected) still answers for roles and for the
/// cluster-wide half. There is no read-only refusal at this door, unlike the
/// schema editors': browsing accounts writes nothing.
pub(crate) fn open_for_server(ui: &Ui, conn_id: u64, database: Option<&str>) {
    let ctx = crate::table_designer::edit_ctx(ui);
    ui.overlay.users_state.set(UsersState::Loading);
    ui.overlay.users_selected.set(None);
    ui.overlay.users_grants.set(GrantsState::Idle);
    ui.overlay.users_filter.set(String::new());
    ui.overlay.users.set(Some(UsersTarget {
        conn_id,
        database: database.map(str::to_string),
        dialect: ctx.dialect,
    }));
}

pub(crate) fn users_overlay(ui: Ui) -> impl IntoView {
    let target = ui.overlay.users;
    let state = ui.overlay.users_state;
    let filter = ui.overlay.users_filter;
    let selected = ui.overlay.users_selected;
    let grants = ui.overlay.users_grants;

    // **Nothing renders while an account form or the DDL preview is up.** Those
    // are raised *from here* and are painted in an earlier group of the modal
    // layer, so a browser that stayed on screen would be painted on top of the
    // very form it opened. This is the pairing every editor in the crate already
    // has with the preview, one level up: Cancel over there returns here with
    // the list intact.
    let account_open = ui.ddl.account;
    let grant_open = ui.ddl.grant;
    let preview_open = ui.ddl.preview;
    let hidden = move || {
        account_open.get().is_some() || grant_open.get().is_some() || preview_open.get().is_some()
    };

    // **The fetch keys on the target alone**, and lives above the container
    // rather than inside it. The container's key is `(target, hidden)`, and
    // `hidden` moves whenever a form or the preview opens *or closes* — so an
    // effect created in the child ran again on every one of those transitions:
    // a full `fetch_principals` (a fresh connection, through the SSH tunnel if
    // there is one, plus an unbounded `SELECT … FROM mysql.user`) for pressing
    // Cancel, and two for an Apply. The comment that stood here said "runs once
    // per opening: the closure reads no signal", which was true of the closure
    // and not of the scope it was created in.
    {
        let fetch = ui.schema_actions.principals.clone();
        create_effect(move |prev: Option<Option<UsersTarget>>| {
            let t = target.get();
            // `create_effect` has no equality check of its own, so the compare
            // is here: this must not re-fetch when the signal is written with
            // the value it already held.
            if prev.as_ref() != Some(&t)
                && let Some(t) = t.clone()
            {
                (fetch)(t);
            }
            t
        });
    }

    dyn_container(
        move || (target.get(), hidden()),
        move |(open, hidden)| {
            let Some(t) = open.filter(|_| !hidden) else {
                return empty().into_any();
            };
            let ui = ui.clone();
            let close: Rc<dyn Fn()> = Rc::new(move || {
                target.set(None);
                state.set(UsersState::Loading);
                selected.set(None);
                grants.set(GrantsState::Idle);
                filter.set(String::new());
            });
            let ring = FocusRing::new();

            // **The footer belongs to the right column, not to the modal.** The
            // list column runs the full height of the body — the shape Manage
            // Connections has — so a footer spanning both would cut across it
            // and put the count under a list it is not about.
            // **Asked once for the whole browser.** Two callers asking
            // independently is two places for the answer to drift — one leaving
            // `+ New account` live while `Privileges`/`Drop` are dimmed, with
            // nothing to say which is right — and it re-walks the connection
            // list for an answer that cannot differ between them.
            let gate = write_gate(&ui, &t);
            let right = v_stack((
                detail_pane(&ui, &t, gate, ring.clone()),
                footer(&ui, &t, state, filter, grants, close.clone(), ring.clone()),
            ))
            .style(|s| s.flex_grow(1.0_f32).min_width(0.0).height_full().flex_col());
            let body = h_stack((list_pane(&ui, &t, gate, ring.clone()), right)).style(|s| {
                s.width_full()
                    .flex_grow(1.0_f32)
                    .min_height(0.0)
                    .flex_row()
                    .items_start()
            });

            let title = modal_title_owned(
                "Users and privileges".to_string(),
                close.clone(),
                ring.clone(),
            );

            let panel = v_stack((title, body)).on_click_stop(|_| {}).style(|s| {
                panel_style(s)
                    .background(theme::bg_panel())
                    .width(panel_w())
                    .height(crate::widgets::modal_h(PANEL_H))
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
        if target.get().is_some() && !hidden() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

// ── the list ─────────────────────────────────────────────────────────────────

fn list_pane(ui: &Ui, target: &UsersTarget, gate: WriteGate, ring: FocusRing) -> AnyView {
    let state = ui.overlay.users_state;
    let filter = ui.overlay.users_filter;
    let selected = ui.overlay.users_selected;
    let grants = ui.overlay.users_grants;
    let fetch = ui.schema_actions.grants.clone();
    let row_target = target.clone();

    let field = container(
        edit_field(
            filter,
            FieldCfg {
                placeholder: "Search accounts",
                clearable: true,
                focus: Some((ring.clone(), 10)),
                ..Default::default()
            },
        )
        .style(|s| s.width_full()),
    )
    // The rows below have no padding of their own to spare — a selected row's
    // background spans the column — so the inset the field needs is here, at the
    // same 12px the rows put their glyph on.
    .style(|s| s.width_full().padding_horiz(row_inset()));

    // **Keyed on the list alone — not on the selection, and not on the filter.**
    //
    // Picking a row changes one row's background, and keying on `selected` would
    // throw every row away and build them all again to do it. The selected row's
    // background is a *reactive style* reading `selected` instead, which is what
    // floem restyles without rebuilding — see `account_row`.
    //
    // **The filter is now the same trick.** It was part of this key, so one
    // keystroke discarded every row view and constructed new ones: at the ~1,000
    // accounts a shared server has, measured in the review at >=13 ms of a
    // 16.7 ms frame, and >=55 ms at 5,000 — dominated by re-parsing each row's
    // SVG glyph through `usvg`. Each row now decides for itself whether it is
    // filtered out, in its own style, and a row that is out takes `display:none`
    // — so it costs no gap and no layout, and the icon is parsed once per
    // account for the life of the modal rather than once per keystroke.
    let list = dyn_container(
        move || state.get(),
        move |st| match st {
            UsersState::Loading => {
                list_note(icons::CLOCK, theme::text_faint, "Loading accounts".into())
            }
            UsersState::Unsupported => list_note(
                icons::CIRCLE_QUESTION,
                theme::text_faint,
                "This connection's engine has no user accounts.".into(),
            ),
            UsersState::Failed(e) => list_note(icons::TRIANGLE_ALERT, theme::error, e),
            UsersState::Loaded(list) => {
                let accounts = std::rc::Rc::new(list.list);
                let mut rows: Vec<AnyView> = accounts
                    .iter()
                    .map(|p| {
                        account_row(
                            p.clone(),
                            selected,
                            grants,
                            fetch.clone(),
                            row_target.clone(),
                            filter,
                        )
                    })
                    .collect();

                // **The empty state is a row of its own**, hidden while anything
                // matches. It is the one part that genuinely depends on the
                // needle — it quotes it — so it is the only thing here rebuilt
                // per keystroke, and it is one note rather than a list.
                let anything_shown = {
                    let accounts = accounts.clone();
                    move |needle: &str| {
                        accounts
                            .iter()
                            .any(|p| schemaic_core::users::matches(p, needle))
                    }
                };
                let shown_style = anything_shown.clone();
                rows.push(
                    dyn_container(
                        move || filter.get(),
                        move |needle| {
                            if anything_shown(&needle) {
                                return crate::widgets::nothing().into_any();
                            }
                            list_note(
                                icons::CIRCLE_QUESTION,
                                theme::text_faint,
                                if needle.trim().is_empty() {
                                    "This server reports no accounts.".into()
                                } else {
                                    format!("No account matches \u{201c}{}\u{201d}.", needle.trim())
                                },
                            )
                        },
                    )
                    // The container is hidden too, not just its child: an empty
                    // flex child still claims the stack's gap.
                    .style(move |s| {
                        if shown_style(&filter.get()) {
                            s.hide()
                        } else {
                            s.width_full()
                        }
                    })
                    .into_any(),
                );

                // **Under the list, the way the privileges pane renders
                // `Grants::note`.** A list that is silently partial is the one
                // way this feature can mislead, and the footer's "1 account" is
                // exactly as confident about a denied read as about a server
                // with one account.
                if let Some(n) = list.note {
                    rows.push(list_note(icons::CIRCLE_QUESTION, theme::text_faint, n));
                }
                v_stack_from_iter(rows)
                    .style(|s| s.flex_col().width_full().gap(row_gap()))
                    .into_any()
            }
        },
    )
    .style(|s| s.width_full().flex_col());

    v_stack((
        field,
        autohide(scroll(list).style(|s| {
            s.width_full()
                .flex_grow(1.0_f32)
                .min_height(0.0)
                .margin_top(theme::scaled(10.0))
        })),
        new_account_row(ui, target, gate, ring),
    ))
    .style(|s| {
        // **Full height, and nothing cuts across it.** The footer used to span
        // both columns and put a rule through this one just above `+ New account`;
        // it now belongs to the right column, so the only line here is the one
        // that separates the two — which Manage Connections' list column has for
        // the same reason.
        s.width(list_w())
            .flex_shrink(0.0_f32)
            .height_full()
            .flex_col()
            .padding_vert(theme::scaled(10.0))
            .border_right(1.0)
            .border_color(theme::border())
    })
    .into_any()
}

/// A [`note`] standing in the account list, indented onto the same column the
/// rows start on.
///
/// The rows carry [`row_inset`] as their own padding — a selected row's
/// background has to span the column, so the column carries none — and a note
/// dropped in beside them would otherwise start hard against the edge.
fn list_note(icon: &'static str, color: fn() -> floem::peniko::Color, message: String) -> AnyView {
    container(note(icon, color, message))
        // `min_width(0)` so the note inside can shrink and wrap rather than
        // pushing this container past the column — see `widgets::fact_note`.
        .style(|s| s.width_full().min_width(0.0).padding_horiz(row_inset()))
        .into_any()
}

/// One account. Clicking it selects it *and* asks for its privileges — the two
/// are one action, because a selected row with an empty detail pane is a state
/// nobody wants to look at.
fn account_row(
    p: Principal,
    selected: RwSignal<Option<Principal>>,
    grants: RwSignal<GrantsState>,
    fetch: Rc<dyn Fn(UsersTarget, Principal)>,
    target: UsersTarget,
    filter: RwSignal<String>,
) -> AnyView {
    let display = p.display();
    // The name this row matches on, resolved once. `users::matches` would
    // re-derive it from the `Principal` on every restyle — hover included — and
    // the whole point of filtering here is that it costs no allocation.
    let searchable = display.clone();
    // The comparison the reactive style makes, resolved once: a `Principal` is a
    // handful of `String`s and this runs on every restyle of every row.
    let me = p.clone();
    let dim = p.system;
    let kind = p.kind;

    h_stack((
        icons::icon(
            match kind {
                PrincipalKind::User => icons::USER,
                PrincipalKind::Role => icons::USERS,
            },
            13.0,
        )
        .style(|s| s.flex_shrink(0.0_f32)),
        text(display).style(move |s| {
            let s = s.font_size(font_body()).text_ellipsis().min_width(0.0);
            // A server-owned account stays dimmer than the rest in **every**
            // state, which is why this colour is set here and the row's own
            // colour below is not allowed to overrule it.
            if dim { s.color(theme::text_faint()) } else { s }
        }),
    ))
    // Inline rather than a bound closure: floem's handler takes a `&Event` under
    // a higher-ranked lifetime, and a `let click = move |_| …` binding infers one
    // concrete lifetime and fails to satisfy it.
    .on_click_stop(move |_| {
        selected.set(Some(p.clone()));
        grants.set(GrantsState::Loading);
        (fetch)(target.clone(), p.clone());
    })
    // **The connection list's affordances, not affordances of its own.** Two
    // lists of names in the same app: resting text is `conn_list_text`, hover
    // brightens the *text* with no background, and selected is that same bright
    // text on a full-width `conn_list_sel_bg`. Copied from
    // `connection_form`'s row rather than approximated, so the two cannot drift.
    //
    // **No pointer cursor** — the app keeps the arrow on everything but a text
    // input and a genuine hyperlink; see the UI conventions in
    // `docs/architecture.md`.
    .style(move |s| {
        // **`with`, not `get`.** This closure runs for every row on every
        // restyle — hover included — and `get` clones the whole `Principal`,
        // its attribute `Vec<(String, String)>` and all, to answer one equality
        // test. On a server with a couple of hundred accounts that is a couple
        // of hundred heap allocations per frame while the pointer moves down
        // the list.
        // **Filtered out is `display:none`**, so the row costs no layout and no
        // gap — an empty flex child would still claim the stack's spacing. This
        // is what makes the filter a restyle instead of a rebuild; see the
        // container above.
        if !schemaic_core::users::matches_display(&searchable, &filter.get()) {
            return s.hide();
        }
        let mine = selected.with(|sel| sel.as_ref() == Some(&me));
        let s = s
            .width_full()
            .flex_row()
            .items_center()
            .gap(theme::scaled(7.0))
            .padding_horiz(row_inset())
            .padding_vert(theme::scaled(5.0));
        if mine {
            s.color(theme::conn_list_sel_text())
                .background(theme::conn_list_sel_bg())
        } else {
            s.color(theme::conn_list_text())
                .hover(|s| s.color(theme::conn_list_sel_text()))
        }
    })
    .into_any()
}

// ── the detail pane ──────────────────────────────────────────────────────────

fn detail_pane(ui: &Ui, target: &UsersTarget, gate: WriteGate, ring: FocusRing) -> AnyView {
    let selected = ui.overlay.users_selected;
    let grants = ui.overlay.users_grants;
    // The target's, not the active connection's — see [`UsersTarget::dialect`].
    let dialect = target.dialect;
    let ui = ui.clone();
    let target = target.clone();

    let body = dyn_container(
        move || (selected.get(), grants.get()),
        move |(who, st)| match who {
            None => note(
                icons::CIRCLE_QUESTION,
                theme::text_faint,
                "Pick an account to see what it may do.".into(),
            ),
            Some(p) => {
                // **Collected and filtered, never an `empty()` placeholder.** The
                // stack has a 16px gap and floem gaps an empty child like any
                // other, so an absent actions row left a hole between the name
                // and the attributes — the trap `properties::stats_body` states.
                let mut sections: Vec<AnyView> = vec![heading(&p)];
                sections.extend(actions_row(&ui, &target, gate, &p, ring.clone()));
                if !p.attributes.is_empty() {
                    sections.push(section(
                        "Attributes",
                        p.attributes
                            .iter()
                            .map(|(k, v)| detail(k.clone(), v.clone()))
                            .collect(),
                    ));
                }
                sections.push(grants_section(st, dialect));
                v_stack_from_iter(sections)
                    .style(|s| s.flex_col().width_full().gap(theme::scaled(16.0)))
                    .into_any()
            }
        },
    )
    .style(|s| s.width_full().flex_col());

    autohide(scroll(body).style(|s| {
        s.width_full()
            .flex_grow(1.0_f32)
            .min_height(0.0)
            .padding(modal_pad_h())
    }))
    .style(|s| s.flex_grow(1.0_f32).min_width(0.0).height_full().flex_col())
    .into_any()
}

/// The account's own name and what kind of thing it is.
fn heading(p: &Principal) -> AnyView {
    // **No database here**, deliberately. It would read as the scope of what
    // follows, and on MySQL that would be false — its grant tables are
    // server-wide and `SHOW GRANTS` answers for every database at once. The one
    // engine whose list *is* limited to a database says so in its own sentence,
    // under the statements, where the limit actually applies.
    let sub = if p.system {
        // Said once, at the top: the server owns this account, so what it may do
        // is not an administrator's choice.
        format!("{} · maintained by the server", p.kind.label())
    } else {
        p.kind.label().to_string()
    };
    v_stack((
        text(p.display()).style(|s| {
            s.font_size(theme::font_label())
                .font_bold()
                .color(theme::text())
        }),
        text(sub).style(|s| s.font_size(font_hint()).color(theme::text_faint())),
    ))
    .style(|s| s.flex_col().gap(theme::scaled(3.0)).width_full())
    .into_any()
}

/// The privileges, in whichever of the four states the per-account fetch is in.
/// How many `GRANT` statements the pane renders before it stops and says so.
///
/// Not a limit on the *answer* — `Grants::statements` is whole, Copy privileges
/// takes all of it, and the note below the list says how many there are. It is a
/// limit on what is built: each row lexes SQL and shapes a `RichText`, and a
/// role with privileges across a large schema runs to several hundred, all of
/// which re-shape on a theme switch.
///
/// Two hundred is well past what anyone reads down a scroll and well short of
/// where the cost is felt.
const STATEMENT_CAP: usize = 200;

fn grants_section(st: GrantsState, dialect: SqlDialect) -> AnyView {
    let rows: Vec<AnyView> = match st {
        GrantsState::Idle | GrantsState::Loading => {
            vec![note(
                icons::CLOCK,
                theme::text_faint,
                "Loading privileges".into(),
            )]
        }
        GrantsState::Failed(e) => vec![note(icons::TRIANGLE_ALERT, theme::error, e)],
        GrantsState::Loaded(Grants {
            statements,
            note: n,
        }) => {
            let mut rows: Vec<AnyView> = if statements.is_empty() {
                // Not an empty list: an account with no grants at all is a real
                // and interesting state, and a blank pane reads as a fetch that
                // silently failed.
                vec![note(
                    icons::CIRCLE_QUESTION,
                    theme::text_faint,
                    "This account holds no privileges.".into(),
                )]
            } else {
                statements
                    .iter()
                    .take(STATEMENT_CAP)
                    .map(|s| statement_row(s, dialect))
                    .collect()
            };
            // **What the cap left out, said rather than left off.** Every row
            // here lexes its SQL and shapes a `RichText`, and all of them
            // re-shape on a theme change; `pg_grant_statements`' own comment
            // documents the input as a role with privileges on every table of a
            // 500-table schema. A pane that silently stopped at the cap would be
            // the "quietly partial privilege screen" this whole feature is
            // written against, so the count is on screen and Copy still takes
            // the lot — it is the *rendering* that is capped, not the answer.
            if statements.len() > STATEMENT_CAP {
                rows.push(note(
                    icons::CIRCLE_QUESTION,
                    theme::text_faint,
                    format!(
                        "Showing the first {STATEMENT_CAP} of {} statements.                          Copy privileges takes all of them.",
                        statements.len()
                    ),
                ));
            }
            if let Some(n) = n {
                rows.push(note(icons::CIRCLE_QUESTION, theme::text_faint, n));
            }
            rows
        }
    };
    section("Privileges", rows)
}

/// One `GRANT` statement, rendered the way the app renders SQL everywhere it is
/// read rather than edited.
///
/// **The same call query history and the snippet library make** —
/// `widgets::highlight_sql_mono` over the editor's own lexer, `theme::preview_fg`
/// on a `theme::preview_bg` block. Those two colours are from the *editor's*
/// axis rather than the UI's, and are paired deliberately: `contrast.rs` tests
/// them against each other, so a block that took `preview_fg` on a UI background
/// would be untested for legibility. It also means a `GRANT` reads the same here
/// as it does in the tab it would be pasted into.
fn statement_row(sql: &str, dialect: SqlDialect) -> AnyView {
    container(
        crate::widgets::highlight_sql_mono(
            sql.to_string(),
            None,
            font_body,
            theme::preview_fg,
            1.4,
            dialect,
        )
        .style(|s| s.width_full().min_width(0.0)),
    )
    .style(|s| {
        s.width_full()
            .min_width(0.0)
            .padding_horiz(theme::scaled(8.0))
            .padding_vert(theme::scaled(6.0))
            .border_radius(theme::scaled(4.0))
            .background(theme::preview_bg())
    })
    .into_any()
}

// ── the write actions ────────────────────────────────────────────────────────

/// Can this browser write at all, and if not, why?
///
/// Three different refusals with three different remedies, which is why they are
/// three answers rather than one boolean: an engine with no accounts is nothing
/// the user can change, a read-only connection is a setting, and a connection
/// with no database selected is a query tab away. The last is the one that would
/// otherwise be invisible — an account change takes the ordinary in-database
/// route (`ddl::is_account_change`), so with nothing selected there is nowhere
/// to send it.
///
/// [`schemaic_core::users::WriteGate`], asked of this browser's target and of
/// the live connection.
///
/// The decision itself is in `core` — it is an ordering of four answers, which
/// is exactly the kind of thing an 860-line view is the wrong place to keep and
/// the reason it had no test.
fn write_gate(ui: &Ui, target: &UsersTarget) -> WriteGate {
    WriteGate::of(
        target.dialect,
        crate::table_designer::edit_ctx(ui).read_only,
        target.database.is_some(),
    )
}

/// **`+ New account`, at the foot of the list column** — the shape Manage
/// Connections' `New connection` row has, and in the same place: under the list
/// it adds to rather than beside the box that searches it, so the column reads
/// top to bottom as *find one, or make one*.
fn new_account_row(ui: &Ui, target: &UsersTarget, gate: WriteGate, ring: FocusRing) -> AnyView {
    if !gate.offered() {
        return crate::widgets::nothing().into_any();
    }
    let enabled = gate.enabled();
    let ui = ui.clone();
    let database = target.database.clone().unwrap_or_default();
    let open = move || {
        // The read-only refusal is inside `open_for_new`, so this launch is
        // guarded in the same step that launches it — the dimming says the
        // action is unavailable, this is what makes it so.
        crate::account_editor::open_for_new(&ui, &database);
    };
    let open_click = open.clone();
    in_ring_button(
        container(
            h_stack((
                icons::icon(icons::CIRCLE_PLUS, 16.0),
                text("New account").style(|s| s.font_size(font_body())),
            ))
            .style(move |s| {
                s.flex_row()
                    .items_center()
                    .gap(theme::scaled(8.0))
                    // Dimmed rather than absent while the connection is
                    // read-only or has no database selected — see [`WriteGate`].
                    .color(if enabled {
                        theme::accent()
                    } else {
                        theme::text_faint()
                    })
            }),
        )
        .on_click_stop(move |_| {
            if enabled {
                (open_click)();
            }
        })
        // Left-aligned at `menu_item_style`'s 12px inset, which is
        // [`row_inset`] — so the glyph starts on the same column as the account
        // icons above it rather than floating in the middle of the pane.
        .style(crate::widgets::menu_item_style),
        ring,
        11,
        enabled,
        // A full-width menu row, square like the list above it.
        0.0,
        open,
    )
}

/// **Grant** and **Drop**, under the selected account's name.
///
/// Beside the thing they act on rather than in the footer, because the footer's
/// actions are about the *modal* (Copy what is shown, Close it) and these are
/// about one account. A Drop in the footer would also sit one Tab away from
/// Close, which is the wrong pair of neighbours for an irreversible action.
fn actions_row(
    ui: &Ui,
    target: &UsersTarget,
    gate: WriteGate,
    p: &Principal,
    ring: FocusRing,
) -> Option<AnyView> {
    if !gate.offered() {
        return None;
    }
    // **Never offered for an account the server maintains.** Dropping
    // `mysql.sys` or `pg_monitor` breaks the server rather than the account, and
    // no privilege screen should make that one click away.
    if p.system {
        return Some(note(
            icons::CIRCLE_QUESTION,
            theme::text_faint,
            "This account belongs to the server, so Schemaic will not change it.".into(),
        ));
    }
    let enabled = gate.enabled();

    let grant_ui = ui.clone();
    let grant_target = target.clone();
    let grant_who = p.clone();
    let grant = action_button(
        "Privileges",
        ActionKind::Quiet,
        enabled,
        ring.clone(),
        // **The modal's own ring, in the fixed band above `+ New account`.**
        // These two used to make a `FocusRing::new()` of their own, with no
        // focus root stepping it — so clicking one focused it and Tab then
        // cycled the pair forever, with Escape the only way out. Reusing the
        // modal's ring at 1 and 2 would put them *before* the search field, so
        // they sit after the list's last fixed stop (11) and before the
        // footer's `ACTION_TAB`.
        12,
        move || {
            crate::account_editor::open_for_grant(
                &grant_ui,
                &grant_target.database.clone().unwrap_or_default(),
                &grant_who,
            );
        },
    );

    let drop_ui = ui.clone();
    let drop_target = target.clone();
    let drop_who = p.clone();
    let risk_dialect = target.dialect;
    // The connection the browser was opened on, like the dialect beside it —
    // the preview must be built for the server this account lives on, not for
    // whichever the switcher points at by the time the confirm is answered.
    // `read_only` is `false` because this button is only reachable through
    // `WriteGate::Allowed`, and the preview re-asks the live connection by
    // `conn_id` before it applies anything.
    let plan_conn_id = target.conn_id;
    let confirm = ui.overlay.confirm;
    let drop = action_button("Drop", ActionKind::Danger, enabled, ring, 13, move || {
        let ui = drop_ui.clone();
        let database = drop_target.database.clone().unwrap_or_default();
        let who = drop_who.clone();
        let change = schemaic_core::ddl::Change::DropAccount(Box::new(who.clone()));
        confirm.set(Some(crate::Confirm {
            title: format!("Drop {}", who.kind.label().to_lowercase()),
            // The plain-language cost is the change's own (`Change::risks`), so
            // this question and the preview's warning cannot drift into saying
            // different things about the same act — and the preview still stands
            // between the answer and the server.
            // The target's dialect, like the highlighting and the capability
            // half of `WriteGate` — the sentence is about the account on the
            // server this browser was opened on, not about whichever connection
            // the switcher points at.
            message: crate::overlays::risk_prompt(&change, risk_dialect),
            resolve: Rc::new(move |yes| {
                if yes {
                    crate::ddl_preview::preview_account(
                        &ui,
                        crate::ddl_preview::AccountPlanTarget {
                            conn_id: plan_conn_id,
                            database: database.clone(),
                            dialect: risk_dialect,
                            read_only: false,
                        },
                        &who.display(),
                        schemaic_core::ddl::Change::DropAccount(Box::new(who.clone())),
                    );
                }
            }),
        }));
    });

    let why = match gate {
        WriteGate::NoDatabase => Some(
            "Open a query tab on a database to change privileges — the statement has to run in \
             one.",
        ),
        WriteGate::ReadOnly => Some("This connection is read-only."),
        _ => None,
    };
    let mut rows: Vec<AnyView> = vec![
        h_stack((grant, drop))
            .style(|s| s.flex_row().items_center().gap(action_gap()))
            .into_any(),
    ];
    if let Some(why) = why {
        rows.push(note(icons::CIRCLE_QUESTION, theme::text_faint, why.into()));
    }
    Some(
        v_stack_from_iter(rows)
            .style(|s| {
                s.flex_col()
                    .items_start()
                    .gap(theme::scaled(6.0))
                    .width_full()
            })
            .into_any(),
    )
}

// ── shared bits ──────────────────────────────────────────────────────────────

/// This panel's rows at its own gap — see [`crate::widgets::fact_section`],
/// which is the shared view the properties modal wears too.
fn section(title: &'static str, rows: Vec<AnyView>) -> AnyView {
    crate::widgets::fact_section(title, rows, section_gap)
}

/// The gap between the rows of a section — wider than the account list's,
/// because these rows are sentences rather than names.
fn section_gap() -> f64 {
    theme::scaled(5.0)
}

/// One `label: value` row, at this panel's label column.
fn detail(label: String, value: String) -> AnyView {
    crate::widgets::fact_row(label, value, label_w)
}

/// An icon-led sentence — a state, a caveat, or an engine's limitation.
fn note(icon: &'static str, color: fn() -> floem::peniko::Color, message: String) -> AnyView {
    crate::widgets::fact_note(icon, color, message)
}

/// The count on the left; Refresh, Copy and Close on the right.
fn footer(
    ui: &Ui,
    target: &UsersTarget,
    state: RwSignal<UsersState>,
    filter: RwSignal<String>,
    grants: RwSignal<GrantsState>,
    close: Rc<dyn Fn()>,
    ring: FocusRing,
) -> AnyView {
    // **The way back to the truth.** Everything else here re-reads the server
    // only as a side effect of opening something, so a list that has gone stale
    // — a `DROP USER` applied from another client, or a fetch whose answer lost
    // a race — could only be corrected by closing the browser and reopening it.
    let refresh = {
        let fetch = ui.schema_actions.principals.clone();
        let t = target.clone();
        move || {
            state.set(UsersState::Loading);
            (fetch)(t.clone());
        }
    };
    let status = label(move || match state.get() {
        UsersState::Loaded(list) => {
            let needle = filter.get();
            let shown = schemaic_core::users::filter_indices(&list.list, &needle).len();
            let total = list.list.len();
            // "3 of 40 accounts" only while a filter is narrowing them — an
            // unfiltered list saying "40 of 40" invites a hunt for the missing
            // ones.
            if shown == total {
                format!("{total} {}", plural(total, "account", "accounts"))
            } else {
                format!("{shown} of {total} accounts")
            }
        }
        _ => String::new(),
    })
    .style(|s| s.font_size(font_hint()).color(theme::text_faint()));

    // Copy takes the statements the pane is showing — the redacted ones, since
    // redaction happens at the fetch. There is nothing here that could put a
    // password hash on the clipboard.
    let copy = move || {
        if let GrantsState::Loaded(g) = grants.get_untracked() {
            let _ = floem::Clipboard::set_contents(g.statements.join("\n"));
        }
    };
    // Always enabled, and a no-op before an account is picked — the same bargain
    // the properties modal's Copy makes. The alternative is a button whose
    // enabled state is a `dyn_container` over the fetch, which would rebuild its
    // focus-ring registration on every selection.
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
                "Copy privileges",
                ActionKind::Quiet,
                true,
                ring.clone(),
                ACTION_TAB + 1,
                copy,
            ),
            action_button(
                "Close",
                ActionKind::Neutral,
                true,
                ring,
                ACTION_TAB + 2,
                move || close(),
            ),
        ))
        .style(|s| s.flex_row().items_center().gap(action_gap())),
    )
    .into_any()
}
