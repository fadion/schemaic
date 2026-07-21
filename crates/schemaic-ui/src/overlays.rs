//! The app's absolutely-positioned popup overlays, all children of the workspace
//! root (so they position in window coords) and dismissed by the root pointer-down
//! handler: the connection switcher menu, the active-database menu, the schema
//! database-visibility / settings dropdowns, the schema right-click context menu,
//! the generic results-grid popup menu, the Find-Anywhere palette, and the editor
//! error modal. Each takes the `Ui` bundle and reads/writes its own overlay signal.

use std::collections::HashSet;
use std::rc::Rc;

use floem::event::EventListener;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::connection::Connection;
use schemaic_core::model::QueryState;
use schemaic_core::schema::SchemaState;

use crate::consts::DB_MENU_W;
use crate::widgets::{
    MenuEntry, autohide, menu_enter, menu_item_style, menu_panel, panel_style, window_size,
};
use crate::{ConnNode, CtxKind, Ui, icons, search_box, status_color, theme};

// ===== moved from lib.rs (overlays) =====
pub(crate) fn conn_menu_overlay(ui: Ui) -> impl IntoView {
    let open = ui.conn.conn_menu_open;
    let connections = ui.conn.connections;
    let active_conn = ui.conn.active_conn;
    let conn_status = ui.conn.conn_status;
    let switch = ui.conn_actions.switch_conn.clone();
    let manage_open = ui.conn.manage_open;
    let select_conn = ui.conn_actions.select_conn.clone();

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let switch = switch.clone();
            let select_conn = select_conn.clone();

            let list = dyn_stack(
                move || connections.get(),
                |c: &Connection| c.id,
                move |c| {
                    let id = c.id;
                    let switch = switch.clone();
                    // Leading status dot in a fixed 14px slot. Only the active
                    // connection is health-checked, so others show a neutral dot.
                    let dot = container(icons::icon(icons::DOT, 6.0).style(move |s| {
                        let c = if active_conn.get() == id {
                            status_color(conn_status.get())
                        } else {
                            theme::text_dim()
                        };
                        s.color(c)
                    }))
                    .style(|s| {
                        s.width(14.0)
                            .flex_shrink(0.0_f32)
                            .items_center()
                            .justify_center()
                    });
                    // Truncate long names to 20 chars (+ ellipsis) so the row —
                    // and thus the fixed-width menu — never overflows past the
                    // panel edge; the endpoint stays fully visible on the right.
                    let name = c.name.clone();
                    let name = if name.chars().count() > 20 {
                        format!("{}…", name.chars().take(20).collect::<String>())
                    } else {
                        name
                    };
                    h_stack((
                        dot,
                        // Name in the connection-list text colour; the dot carries status.
                        text(name).style(|s| s.color(theme::conn_list_text())),
                        empty().style(|s| s.flex_grow(1.0_f32).min_width(20.0)),
                        text(c.endpoint())
                            .style(|s| s.color(theme::text_faint()).font_size(theme::FONT_LABEL)),
                    ))
                    .on_click_stop(move |_| {
                        (switch)(id);
                        open.set(false);
                    })
                    .style(menu_item_style)
                    .style(|s| s.padding_vert(8.0))
                },
            )
            .style(|s| s.flex_col());

            // Icon + label share the row's 8px gap (label sits 8px from the icon).
            let manage = h_stack((
                icons::icon(icons::SETTINGS, 16.0).style(|s| s.color(theme::accent())),
                text("Manage Connections").style(|s| s.color(theme::accent())),
            ))
            .on_click_stop(move |_| {
                (select_conn)(active_conn.get_untracked());
                manage_open.set(true);
                open.set(false);
            })
            .style(menu_item_style)
            .style(|s| s.padding_vert(8.0));

            let panel = menu_enter(
                v_stack((
                    list,
                    empty().style(|s| s.width_full().height(1.0).background(theme::border())),
                    manage,
                ))
                .on_click_stop(|_| {})
                .style(|s| {
                    panel_style(s)
                        .background(theme::bg_chrome())
                        .min_width(300.0)
                        .padding_vert(6.0)
                        .margin_left(36.0)
                        // 3px below the switcher button (which sits ~HEADER_H-7 down).
                        .margin_top(theme::HEADER_H - 4.0)
                        // Match the switcher button's size (the shell sets this, but
                        // overlays are siblings of the shell and don't inherit it).
                        .font_size(theme::FONT_TITLE)
                }),
            );

            // Transparent full-window layer: click outside the panel or Escape closes.
            container(panel)
                .keyboard_navigable()
                .request_focus(|| {})
                .on_key_down(
                    Key::Named(NamedKey::Escape),
                    |_| true,
                    move |_| open.set(false),
                )
                .on_click_stop(move |_| open.set(false))
                .style(|s| s.size_full().flex_col().items_start().justify_start())
                .into_any()
        },
    )
    .style(move |s| {
        if open.get() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

// The active-database menu, opened from the QUERY toolbar's DB selector. Lists
// the connection's databases (reactive), highlights the active one in the accent
// colour, and switches the active tab's database on click. Same look as the
// connection menu; right-aligned under the trigger via `active_db_anchor`.
pub(crate) fn active_db_menu_overlay(ui: Ui) -> impl IntoView {
    let open = ui.tabs_ui.active_db_menu_open;
    let db_nodes = ui.schema.db_nodes;
    let active_db = ui.tabs_ui.active_db;
    let set_db = ui.tab_actions.set_active_db.clone();
    let anchor = ui.tabs_ui.active_db_anchor;

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let set_db = set_db.clone();
            let list = dyn_stack(
                move || db_nodes.get(),
                |n: &ConnNode| n.id,
                move |n| {
                    let name = n.database.clone();
                    let name_active = name.clone();
                    let set_db = set_db.clone();
                    text(name.clone())
                        .on_click_stop(move |_| {
                            (set_db)(name.clone());
                            open.set(false);
                        })
                        .style(menu_item_style)
                        // Re-apply colour after `menu_item_style` (which sets the
                        // base text colour) so the active database stays accented.
                        .style(move |s| {
                            if active_db.get().as_deref() == Some(name_active.as_str()) {
                                s.color(theme::accent())
                            } else {
                                s
                            }
                        })
                },
            )
            .style(|s| s.flex_col());

            let panel = menu_enter(container(list).on_click_stop(|_| {}).style(move |s| {
                let a = anchor.get();
                panel_style(s)
                    .background(theme::bg_chrome())
                    .width(DB_MENU_W)
                    .padding_vert(6.0)
                    // Right edge aligns to the trigger's right edge. `a.y` is the
                    // button *box* bottom, which sits 3px below the chevron (the
                    // trigger's `padding_vert(3)`) — so anchoring flush here puts the
                    // popup ~3px under the glyph, matching the schema eye/settings menus.
                    .margin_left((a.x - DB_MENU_W).max(0.0))
                    .margin_top(a.y)
                    .font_size(theme::FONT_TITLE)
            }));

            container(panel)
                .keyboard_navigable()
                .request_focus(|| {})
                .on_key_down(
                    Key::Named(NamedKey::Escape),
                    |_| true,
                    move |_| open.set(false),
                )
                .on_click_stop(move |_| open.set(false))
                .style(|s| s.size_full().flex_col().items_start().justify_start())
                .into_any()
        },
    )
    .style(move |s| {
        if open.get() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

// The database-visibility dropdown (opened by the SCHEMA gear): every database
// with a check — green if visible, dim if hidden. Clicking a row toggles it and
// leaves the menu open (so several can be flipped at once). Same style as the
// connection menu, positioned 3px below the gear.
pub(crate) fn db_visibility_overlay(ui: Ui) -> impl IntoView {
    let open = ui.schema.db_menu_open;
    let db_nodes = ui.schema.db_nodes;
    let hidden = ui.schema.hidden_dbs;
    let toggle = ui.schema_actions.toggle_db_hidden.clone();

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let toggle = toggle.clone();
            let list = dyn_stack(
                move || db_nodes.get(),
                |c: &ConnNode| c.id,
                move |c| {
                    let db = c.database.clone();
                    let db_toggle = db.clone();
                    let db_state = db.clone();
                    let toggle = toggle.clone();
                    // No check icon — the row text itself carries the state:
                    // shown (enabled) is `db_toggle_on`, hidden (disabled) is dim.
                    text(c.name.clone())
                        .on_click_stop(move |_| (toggle)(db_toggle.clone()))
                        .style(menu_item_style)
                        .style(move |s| {
                            let c = if hidden.get().contains(&db_state) {
                                theme::db_toggle_off()
                            } else {
                                theme::db_toggle_on()
                            };
                            s.color(c).padding_vert(8.0)
                        })
                },
            )
            .style(|s| s.flex_col());

            // Just the panel (no full-window catcher — that would block a click
            // on the gear from switching menus). Dismissal is via the root-level
            // pointer-down handler; the panel absorbs its own pointer-downs so it
            // isn't closed while flipping items.
            menu_enter(
                v_stack((list,))
                    .keyboard_navigable()
                    .request_focus(|| {})
                    .on_key_down(
                        Key::Named(NamedKey::Escape),
                        |_| true,
                        move |_| open.set(false),
                    )
                    .on_event_stop(EventListener::PointerDown, |_| {})
                    .style(|s| {
                        panel_style(s)
                            .background(theme::bg_chrome())
                            .min_width(220.0)
                            .padding_vert(6.0)
                            .font_size(theme::FONT_TITLE)
                    }),
            )
            .into_any()
        },
    )
    // Positioned just below the SCHEMA eye (2nd-from-right icon).
    .style(move |s| {
        if open.get() {
            s.absolute()
                .inset_left(theme::SCHEMA_W - 50.0 - 30.0)
                .inset_top(theme::HEADER_H + 7.0 + 16.0 + 3.0)
        } else {
            s
        }
    })
}

// The SCHEMA settings dropdown (opened by the gear): a single "Refresh" action
// for now. Same style as the other dropdowns, dropped 3px below the gear.
pub(crate) fn schema_settings_overlay(ui: Ui) -> impl IntoView {
    let open = ui.schema.schema_menu_open;
    let refresh = ui.schema_actions.refresh_schema.clone();
    let collapse_all = ui.schema_actions.collapse_all.clone();

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let refresh = refresh.clone();
            let collapse_all = collapse_all.clone();
            let refresh_item = container(text("Refresh").style(|s| s.color(theme::text())))
                .on_click_stop(move |_| {
                    (refresh)();
                    open.set(false);
                })
                .style(menu_item_style)
                .style(|s| s.padding_vert(8.0));
            let collapse_item = container(text("Collapse all").style(|s| s.color(theme::text())))
                .on_click_stop(move |_| {
                    (collapse_all)();
                    open.set(false);
                })
                .style(menu_item_style)
                .style(|s| s.padding_vert(8.0));

            menu_enter(
                v_stack((refresh_item, collapse_item))
                    .keyboard_navigable()
                    .request_focus(|| {})
                    .on_key_down(
                        Key::Named(NamedKey::Escape),
                        |_| true,
                        move |_| open.set(false),
                    )
                    .on_event_stop(EventListener::PointerDown, |_| {})
                    .style(|s| {
                        panel_style(s)
                            .background(theme::bg_chrome())
                            .min_width(150.0)
                            .padding_vert(6.0)
                            .font_size(theme::FONT_TITLE)
                    }),
            )
            .into_any()
        },
    )
    // Just below the SCHEMA gear (rightmost icon).
    .style(move |s| {
        if open.get() {
            s.absolute()
                .inset_left(theme::SCHEMA_W - 24.0 - 30.0)
                .inset_top(theme::HEADER_H + 7.0 + 16.0 + 3.0)
        } else {
            s
        }
    })
}

// The schema right-click menu, anchored 3px below-right of the click. Rows vary
// by target kind; every kind ends with an "AI Explain" row (sparkles + prompt).
// Same styling as the other dropdowns.
pub(crate) fn context_menu_overlay(ui: Ui) -> impl IntoView {
    let ctx = ui.overlay.context_menu;
    let last_mouse = ui.overlay.last_mouse;
    let toggle_hidden = ui.schema_actions.toggle_db_hidden.clone();
    let open_table = ui.tab_actions.open_table.clone();
    let open_table_new = ui.tab_actions.open_table_new.clone();
    let tabs = ui.tabs_ui.tabs;
    let active_conn = ui.conn.active_conn;
    let open_query = ui.tab_actions.open_query.clone();
    let open_db_cli = ui.tab_actions.open_db_cli.clone();
    let refresh_db = ui.schema_actions.refresh_db.clone();
    let collapse_db = ui.schema_actions.collapse_db.clone();
    let ai_send = ui.ai_actions.send.clone();

    dyn_container(
        move || ctx.get(),
        move |menu| {
            let Some(menu) = menu else {
                return empty().into_any();
            };
            // Clipboard action for a string.
            let copy = |s: String| {
                move || {
                    let _ = floem::Clipboard::set_contents(s.clone());
                }
            };
            let mut entries: Vec<MenuEntry> = Vec::new();
            match menu.kind.clone() {
                CtxKind::Database => {
                    entries.push(MenuEntry::action("Copy name", copy(menu.name.clone())));
                    let th = toggle_hidden.clone();
                    let n = menu.name.clone();
                    entries.push(MenuEntry::action("Hide", move || (th)(n.clone())));
                    let rf = refresh_db.clone();
                    let dn = menu.name.clone();
                    entries.push(MenuEntry::action("Refresh", move || (rf)(dn.clone())));
                    let cd = collapse_db.clone();
                    let cn = menu.name.clone();
                    entries.push(MenuEntry::action("Collapse all", move || (cd)(cn.clone())));
                    let ocli = open_db_cli.clone();
                    let dbn = menu.name.clone();
                    entries.push(MenuEntry::action("Open in CLI", move || {
                        (ocli)(Some(dbn.clone()))
                    }));
                }
                CtxKind::Table {
                    database,
                    table,
                    ddl,
                } => {
                    let qualified = format!("{database}.{table}");
                    let refresh_database = database.clone();
                    // "Open": focus the tab already showing this table, else open one.
                    {
                        let ot = open_table.clone();
                        let db = database.clone();
                        let tbl = table.clone();
                        entries.push(MenuEntry::action("Open", move || {
                            (ot)(db.clone(), tbl.clone())
                        }));
                    }
                    // "Open in new tab" is only useful (and only shown) when a tab
                    // for this table is already open — otherwise it does exactly
                    // what "Open" does. Match connection too (H13): a same-named
                    // table under another connection isn't "this table".
                    let already_open = tabs.with_untracked(|v| {
                        v.iter().any(|t| {
                            t.source.get_untracked() == Some((database.clone(), table.clone()))
                                && t.conn_id.get_untracked() == active_conn.get_untracked()
                        })
                    });
                    if already_open {
                        let otn = open_table_new.clone();
                        let db = database.clone();
                        let tbl = table.clone();
                        entries.push(MenuEntry::action("Open in new tab", move || {
                            (otn)(db.clone(), tbl.clone())
                        }));
                    }
                    let oq = open_query.clone();
                    entries.push(MenuEntry::action("Generate DDL", move || {
                        let _ = floem::Clipboard::set_contents(ddl.clone());
                        (oq)(ddl.clone());
                    }));
                    // Group the two copy variants into a "Copy" submenu.
                    entries.push(MenuEntry::sub(
                        "Copy",
                        vec![
                            MenuEntry::action("Name", copy(menu.name.clone())),
                            MenuEntry::action("Qualified name", copy(qualified)),
                        ],
                    ));
                    let rf = refresh_db.clone();
                    entries.push(MenuEntry::action("Refresh", move || {
                        (rf)(refresh_database.clone())
                    }));
                }
                CtxKind::Field => {
                    entries.push(MenuEntry::action("Copy name", copy(menu.name.clone())));
                }
            }
            entries.push(MenuEntry::Separator);
            let ai = ai_send.clone();
            let prompt = menu.ai_prompt.clone();
            entries.push(MenuEntry::action_icon(
                "AI Explain",
                (icons::SPARKLES, theme::key_foreign),
                move || (ai)(prompt.clone()),
            ));

            // Dismissal is a root-level pointer-down handler (see `workspace`); the
            // panel absorbs its own pointer-downs so it isn't closed mid-click.
            menu_panel(entries, Rc::new(move || ctx.set(None))).into_any()
        },
    )
    // Anchor 3px below-right of the cursor (window coords tracked at root).
    .style(move |s| {
        if ctx.get().is_some() {
            let (mx, my) = last_mouse.get_untracked();
            s.absolute().inset_left(mx + 3.0).inset_top(my + 3.0)
        } else {
            s
        }
    })
}

/// Generic popup-menu overlay (the results-grid header/cell menus). Renders a
/// `menu_panel` from `ui.overlay.popup_menu` at the cursor, flipping the whole panel left
/// / up if it would spill past the window edge (the grid sits mid-window, unlike
/// the left-anchored schema menu). Submenus edge-flip themselves.
pub(crate) fn popup_menu_overlay(ui: Ui) -> impl IntoView {
    let popup = ui.overlay.popup_menu;
    let last_mouse = ui.overlay.last_mouse;
    let anchor = ui.overlay.popup_anchor;
    dyn_container(
        move || popup.get(),
        move |entries| match entries {
            None => empty().into_any(),
            Some(entries) => menu_panel(entries, Rc::new(move || popup.set(None))).into_any(),
        },
    )
    .style(move |s| {
        let Some(n) = popup.with(|p| p.as_ref().map(|e| e.len())) else {
            return s;
        };
        let (ww, wh) = window_size().get();
        let ph = n as f64 * 34.0 + 14.0;
        let (x, y) = match anchor.get() {
            // Toolbar dropdown: drop 5px below the icon, tucked under it (left edge
            // 40px left of the icon's right edge, so it overlaps the icon like the
            // schema/db menus); flip to right-aligned (right edge flush on the icon)
            // if it'd spill past the window's right edge. Real panel width → no drift.
            Some((_left, right, bottom, w)) => {
                let open_x = right - 40.0;
                let x = if ww > 1.0 && open_x + w > ww {
                    (right - w).max(0.0)
                } else {
                    open_x.max(0.0)
                };
                let y = if wh > 1.0 && bottom + 5.0 + ph > wh {
                    (bottom - 5.0 - ph).max(0.0)
                } else {
                    bottom + 5.0
                };
                (x, y)
            }
            // Cursor menus (right-click): open at the pointer. `pw` matches the
            // menu panels' actual `min_width(170)` (short labels never exceed it),
            // so a right-edge flip lands the menu's right edge ~3px from the cursor
            // — mirroring the 3px gap on the non-flipped side. An over-estimate here
            // is what pushed the flipped menu ~40px too far from the cursor.
            None => {
                let (mx, my) = last_mouse.get_untracked();
                let pw = 170.0;
                let x = if ww > 1.0 && mx + 3.0 + pw > ww {
                    (mx - pw - 3.0).max(0.0)
                } else {
                    mx + 3.0
                };
                let y = if wh > 1.0 && my + 3.0 + ph > wh {
                    (my - ph - 3.0).max(0.0)
                } else {
                    my + 3.0
                };
                (x, y)
            }
        };
        s.absolute().inset_left(x).inset_top(y)
    })
}

// Find Anywhere: fuzzy (substring) search over loaded tables (and their columns).
pub(crate) fn find_overlay(ui: Ui) -> impl IntoView {
    let open = ui.overlay.find_open;
    let query = ui.overlay.find_query;
    let db_nodes = ui.schema.db_nodes;
    let hidden = ui.schema.hidden_dbs;
    let open_table = ui.tab_actions.open_table.clone();

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let open_table = open_table.clone();
            // Custom box (not `text_input`) so we control Escape → close. Closing
            // also clears the query, so reopening Find starts blank and a stale
            // result list never flashes.
            let close: Rc<dyn Fn()> = Rc::new(move || {
                query.set(String::new());
                open.set(false);
            });

            // Current matches + the keyboard-selected row, shared by the list
            // render and the search box's Up/Down/Enter handlers (TODO).
            let hits: RwSignal<Vec<FindHit>> = RwSignal::new(Vec::new());
            let selected = RwSignal::new(0usize);
            create_effect(move |_| {
                let q = query.get().trim().to_lowercase();
                let m = if q.is_empty() {
                    Vec::new()
                } else {
                    find_matches(db_nodes, hidden, &q, 80)
                };
                hits.set(m);
                selected.set(0); // reset the cursor to the top on every new query
            });

            // Open the selected hit (Enter or click) — opens its table either way.
            let open_sel: Rc<dyn Fn()> = {
                let open_table = open_table.clone();
                let close = close.clone();
                Rc::new(move || {
                    let idx = selected.get_untracked();
                    let picked = hits
                        .with_untracked(|v| v.get(idx).map(|h| (h.db.clone(), h.table.clone())));
                    if let Some((db, table)) = picked {
                        (open_table)(db, table);
                        (close)();
                    }
                })
            };
            let on_up: Rc<dyn Fn()> =
                Rc::new(move || selected.update(|i| *i = i.saturating_sub(1)));
            let on_down: Rc<dyn Fn()> = Rc::new(move || {
                let n = hits.with_untracked(|v| v.len());
                if n > 0 {
                    selected.update(|i| *i = (*i + 1).min(n - 1));
                }
            });

            let input = search_box(query, close.clone(), on_up, on_down, open_sel.clone());

            // Suggestions appear only once something is typed (empty → just the box).
            let results = dyn_container(
                move || hits.get(),
                move |list| {
                    if query.with_untracked(|q| q.trim().is_empty()) {
                        return empty().into_any();
                    }
                    if list.is_empty() {
                        // Left-aligned like a normal result row (same padding), 13px.
                        return text("Nothing found")
                            .style(|s| {
                                s.color(theme::text_muted())
                                    .font_size(theme::FONT_BODY)
                                    .padding_horiz(12.0)
                                    .padding_vert(6.0)
                                    .margin_top(10.0)
                            })
                            .into_any();
                    }
                    let open_sel = open_sel.clone();
                    // Same look as the dropdown menus (menu_item_style): the name
                    // (`table` or `table.column`) then its database, dim,
                    // left-aligned. The keyboard-selected row is highlighted; click
                    // or Enter opens its table (and clears the search).
                    v_stack_from_iter(list.into_iter().enumerate().map(move |(i, hit)| {
                        let open_sel = open_sel.clone();
                        let name = match &hit.column {
                            Some(c) => format!("{}.{c}", hit.table),
                            None => hit.table.clone(),
                        };
                        h_stack((
                            text(name)
                                .style(|s| s.color(theme::text()).font_size(theme::FONT_BODY)),
                            text(hit.db.clone()).style(|s| {
                                s.color(theme::text_muted()).font_size(theme::FONT_BODY)
                            }),
                        ))
                        .on_click_stop(move |_| {
                            selected.set(i);
                            (open_sel)();
                        })
                        .style(move |s| {
                            let s = menu_item_style(s);
                            if selected.get() == i {
                                s.background(theme::row_selected())
                            } else {
                                s
                            }
                        })
                    }))
                    .style(|s| s.flex_col().margin_top(10.0))
                    .into_any()
                },
            );

            // Panel: 400px input + 15px padding all around (→ 430px wide), results
            // below. Sizes to content; the results scroll caps its height.
            let panel = v_stack((
                input,
                autohide(scroll(results)).style(|s| s.width_full().max_height(360.0)),
            ))
            .on_click_stop(|_| {})
            .style(|s| {
                panel_style(s)
                    .width(430.0)
                    .padding(15.0)
                    .margin_top(80.0)
                    .border_color(theme::modal_border())
            });

            // Top-anchored (command-palette style), #000 @ 50% backdrop, click-away closes.
            let close_esc = close.clone();
            container(panel)
                .on_key_down(
                    Key::Named(NamedKey::Escape),
                    |_| true,
                    move |_| (close_esc)(),
                )
                .on_click_stop(move |_| (close)())
                .style(|s| {
                    s.size_full()
                        .flex_col()
                        .items_center()
                        .justify_start()
                        .background(theme::modal_backdrop())
                })
                .into_any()
        },
    )
    .style(move |s| {
        if open.get() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// "View" modal for the editor error bar: same backdrop + panel chrome as
/// `find_overlay`, but no input — the active tab's full error, centered and
/// scrollable. Click-away or Escape closes.
pub(crate) fn error_modal_overlay(ui: Ui) -> impl IntoView {
    let open = ui.overlay.error_modal_open;
    let text_override = ui.overlay.error_modal_text;
    let tabs = ui.tabs_ui.tabs;
    let active = ui.tabs_ui.active;

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            // A commit error (grid) supplies its text directly; otherwise fall back
            // to the active tab's full query error (editor error bar).
            let msg = text_override
                .get_untracked()
                .or_else(|| {
                    tabs.with_untracked(|v| {
                        v.iter()
                            .find(|t| t.id == active.get_untracked())
                            .map(|t| t.results.get_untracked())
                    })
                    .and_then(|st| match st {
                        QueryState::Failed(m) => Some(m),
                        _ => None,
                    })
                })
                .unwrap_or_else(|| "No error.".to_string());

            // Fixed text width so the error wraps (a `scroll` gives its child
            // unbounded width otherwise). Must stay UNDER the scroll's content
            // area = panel 500 − 40 padding − 2 border = 458; wider triggers a
            // few-px horizontal scrollbar. `min_height` keeps the modal ~500×200
            // for short errors; it grows to `max_height` then scrolls if long.
            let panel = container(
                autohide(scroll(text(msg).style(|s| {
                    s.width(450.0)
                        .color(theme::error())
                        .font_size(theme::FONT_BODY)
                        .line_height(1.4)
                })))
                .style(|s| s.width_full().min_height(160.0).max_height(360.0)),
            )
            .on_click_stop(|_| {})
            .style(|s| {
                panel_style(s)
                    .width(500.0)
                    .padding(20.0)
                    .border_color(theme::modal_border())
            });

            // Closing clears the text override so the next open (e.g. the editor's
            // "View") falls back to the tab error again.
            let close = move || {
                open.set(false);
                text_override.set(None);
            };
            container(panel)
                .keyboard_navigable()
                .request_focus(|| {})
                .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| close())
                .on_click_stop(move |_| close())
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
        if open.get() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// One Find-Anywhere hit: a table (`column: None`) or a specific column within a
/// table (`column: Some`). Clicking either opens the table.
#[derive(Clone)]
struct FindHit {
    db: String,
    table: String,
    column: Option<String>,
}

fn find_matches(
    db_nodes: RwSignal<Vec<ConnNode>>,
    hidden: RwSignal<HashSet<String>>,
    q: &str,
    limit: usize,
) -> Vec<FindHit> {
    let mut out = Vec::new();
    let hidden = hidden.get_untracked();
    for node in db_nodes.get_untracked() {
        if hidden.contains(&node.database) {
            continue;
        }
        if let SchemaState::Loaded(schema) = node.schema.get_untracked() {
            for t in &schema.tables {
                // A table-name match lists the table itself; then each matching
                // column is listed as its own `table.column` hit.
                if q.is_empty() || t.name.to_lowercase().contains(q) {
                    out.push(FindHit {
                        db: node.database.clone(),
                        table: t.name.clone(),
                        column: None,
                    });
                    if out.len() >= limit {
                        return out;
                    }
                }
                if !q.is_empty() {
                    for c in &t.columns {
                        if c.name.to_lowercase().contains(q) {
                            out.push(FindHit {
                                db: node.database.clone(),
                                table: t.name.clone(),
                                column: Some(c.name.clone()),
                            });
                            if out.len() >= limit {
                                return out;
                            }
                        }
                    }
                }
            }
        }
    }
    out
}
