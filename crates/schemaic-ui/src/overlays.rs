//! The app's absolutely-positioned popup overlays, all children of the workspace
//! root (so they position in window coords) and dismissed by the root pointer-down
//! handler: the connection switcher menu, the active-database menu, the schema
//! database-visibility / settings dropdowns, the schema right-click context menu,
//! the generic results-grid popup menu, the Find-Anywhere palette, and the editor
//! error modal. Each takes the `Ui` bundle and reads/writes its own overlay signal.

use std::collections::HashSet;
use std::rc::Rc;

use floem::AnyView;
use floem::event::EventListener;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::connection::Connection;
use schemaic_core::model::QueryState;
use schemaic_core::schema::SchemaState;

use crate::consts::{CHAT_PAD_H, CHAT_PAD_V, DB_MENU_W};
use crate::widgets::{
    MenuEntry, autohide, measure_text_px_at, menu_item_style, menu_panel, panel_style, window_size,
};
use crate::{
    ConnNode, CtxKind, PopupAnchor, RightPanel, Ui, icons, right_panel_allowed,
    schema_panel_allowed, search_box, status_color, theme,
};

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

            let panel = v_stack((
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
            });

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

            let panel = container(list).on_click_stop(|_| {}).style(move |s| {
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
            });

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
                        .min_width(170.0)
                        .padding_vert(6.0)
                        .font_size(theme::FONT_TITLE)
                })
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
                })
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
    let db_colors = ui.db_colors;
    let save_db_colors = ui.save_db_colors.clone();

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
                    // Set colour: preset swatches + Clear, stored per (active
                    // connection, database) and shown as a dot on the DB node,
                    // active-DB selector, and this database's query tabs.
                    let db_name = menu.name.clone();
                    let mut swatches: Vec<MenuEntry> = crate::CONN_COLOR_PRESETS
                        .iter()
                        .map(|(name, hex, cfn)| {
                            let dbc = db_colors;
                            let save = save_db_colors.clone();
                            let db = db_name.clone();
                            let hex = hex.to_string();
                            MenuEntry::action_icon(*name, (icons::DOT, *cfn), move || {
                                let cid = active_conn.get_untracked();
                                dbc.update(|r| {
                                    schemaic_core::db_color::upsert(r, cid, &db, Some(hex.clone()))
                                });
                                (save)();
                            })
                        })
                        .collect();
                    swatches.push(MenuEntry::Separator);
                    {
                        let dbc = db_colors;
                        let save = save_db_colors.clone();
                        let db = db_name.clone();
                        swatches.push(MenuEntry::action("None", move || {
                            let cid = active_conn.get_untracked();
                            dbc.update(|r| schemaic_core::db_color::upsert(r, cid, &db, None));
                            (save)();
                        }));
                    }
                    entries.push(MenuEntry::sub("Colour", swatches));
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
            menu_panel(entries, Rc::new(move || ctx.set(None)), 170.0).into_any()
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
    let popup_width = ui.overlay.popup_width;
    dyn_container(
        move || popup.get(),
        move |entries| match entries {
            None => empty().into_any(),
            Some(entries) => {
                // Width was set by the opener; an effect in `workspace` resets it to
                // the default when the popup closes, so the next menu gets 170.
                let w = popup_width.get_untracked();
                menu_panel(entries, Rc::new(move || popup.set(None)), w).into_any()
            }
        },
    )
    .style(move |s| {
        let Some(n) = popup.with(|p| p.as_ref().map(|e| e.len())) else {
            return s;
        };
        let (ww, wh) = window_size().get();
        let ph = n as f64 * 34.0 + 14.0;
        let pw = popup_width.get(); // matches the panel's min_width for edge flips
        match anchor.get() {
            // Status-bar segment menu: centre the panel horizontally on the anchor's
            // x-range and sit 5px above the status bar (FOOTER_H tall at the window
            // bottom), growing upward via `inset_bottom` so we needn't know its
            // height. Clamp horizontally so it never spills past a window edge.
            Some(PopupAnchor::AboveFooter(left, right)) => {
                let cx = (left + right) / 2.0;
                let x = if ww > 1.0 {
                    (cx - pw / 2.0).clamp(0.0, (ww - pw).max(0.0))
                } else {
                    (cx - pw / 2.0).max(0.0)
                };
                s.absolute()
                    .inset_left(x)
                    .inset_bottom(theme::FOOTER_H + 5.0)
            }
            // Toolbar dropdown (grid Copy): drop 5px below the icon, tucked under it
            // (left edge 40px left of the icon's right edge, so it overlaps the icon
            // like the schema/db menus); flip to right-aligned (right edge flush on
            // the icon) if it'd spill past the window's right edge, and flip upward if
            // it'd spill past the bottom. Real panel width → no drift.
            Some(PopupAnchor::BelowIcon(_left, right, bottom)) => {
                let open_x = right - 40.0;
                let x = if ww > 1.0 && open_x + pw > ww {
                    (right - pw).max(0.0)
                } else {
                    open_x.max(0.0)
                };
                let y = if wh > 1.0 && bottom + 5.0 + ph > wh {
                    (bottom - 5.0 - ph).max(0.0)
                } else {
                    bottom + 5.0
                };
                s.absolute().inset_left(x).inset_top(y)
            }
            // Cursor menus (right-click): open at the pointer. A right-edge flip
            // lands the menu's right edge ~3px from the cursor — mirroring the 3px
            // gap on the non-flipped side.
            None => {
                let (mx, my) = last_mouse.get_untracked();
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
                s.absolute().inset_left(x).inset_top(y)
            }
        }
    })
}

/// One actionable row in the palette — a table hit, a command, a command's
/// argument option, or a live result. `activate` does the whole thing (run the
/// action and close the overlay, or transition into argument entry).
#[derive(Clone)]
struct PaletteItem {
    primary: String,
    secondary: String,
    activate: Rc<dyn Fn()>,
    /// The full query string Tab completes to (and the ghost previews), or `None`
    /// for free-text/search rows with nothing to complete.
    complete: Option<String>,
    /// Substring of `primary` to bold-highlight (the matched search/filter term),
    /// or `None` for rows with no meaningful match to show.
    match_term: Option<String>,
}

/// Render `primary` with the first case-insensitive occurrence of `term` bolded +
/// tinted (`match_highlight`). Plain text when there's no term / no match.
fn highlighted_primary(primary: &str, term: &Option<String>) -> AnyView {
    let seg = |t: &str, hit: bool| {
        text(t.to_string()).style(move |s| {
            let s = s.font_size(14.0);
            if hit {
                s.color(theme::match_highlight()).font_bold()
            } else {
                s.color(theme::text())
            }
        })
    };
    let m = term.as_deref().filter(|t| !t.is_empty()).and_then(|t| {
        schemaic_core::text_ops::find_matches(primary, t)
            .first()
            .map(|&s| (s, s + t.len()))
    });
    match m {
        Some((start, end)) => h_stack((
            seg(&primary[..start], false),
            seg(&primary[start..end], true),
            seg(&primary[end..], false),
        ))
        .style(|s| s.flex_row().items_center())
        .into_any(),
        None => seg(primary, false).into_any(),
    }
}

/// `() -> [(value, label)]` — an argument-command's choice list.
type OptionsFn = Rc<dyn Fn() -> Vec<(String, String)>>;
/// `arg -> rows` — a free-text command's live results.
type ItemsFn = Rc<dyn Fn(&str) -> Vec<PaletteItem>>;

/// How a command consumes its argument. Every `run`/action closure already closes
/// the overlay, so `build_items` doesn't add that itself.
enum CmdArg {
    /// No argument — runs on Enter.
    Instant(Rc<dyn Fn()>),
    /// Pick one of `(value, label)`; `run(value)`.
    Options {
        list: OptionsFn,
        run: Rc<dyn Fn(String)>,
    },
    /// A number clamped to `[min, max]`; `run(n)`. `empty` handles a missing arg.
    Number {
        min: i64,
        max: i64,
        run: Rc<dyn Fn(i64)>,
        empty: Option<Rc<dyn Fn()>>,
    },
    /// Free text → its own result rows (a live search, or a single confirm row).
    Text(ItemsFn),
}

/// A command-palette entry. `name` is the canonical lowercase keyword the pure
/// parser matches (see `schemaic_core::palette`); `label`/`hint` are display.
struct Command {
    name: &'static str,
    label: &'static str,
    hint: &'static str,
    arg: CmdArg,
}

impl Command {
    fn takes_arg(&self) -> bool {
        !matches!(self.arg, CmdArg::Instant(_))
    }
}

/// Build the command registry. Every action closure captures `close` and closes
/// the overlay when it runs.
fn palette_commands(ui: &Ui, close: Rc<dyn Fn()>) -> Vec<Command> {
    let tabs = ui.tabs_ui.tabs;
    let active = ui.tabs_ui.active;
    let add_tab = ui.tab_actions.add_tab.clone();
    let close_tab = ui.tab_actions.close_tab.clone();
    let duplicate_tab = ui.tab_actions.duplicate_tab.clone();
    let run_all = ui.tab_actions.run_all.clone();
    let schema_visible = ui.layout.schema_visible;
    let right_panel = ui.layout.right_panel;
    let editor_font = ui.layout.editor_font;
    let soft_tabs = ui.layout.soft_tabs;
    let tab_width = ui.layout.tab_width;
    let word_wrap = ui.layout.word_wrap;
    let ui_theme = ui.layout.ui_theme;
    let editor_theme = ui.layout.editor_theme;
    let connections = ui.conn.connections;
    let active_conn = ui.conn.active_conn;
    let switch_conn = ui.conn_actions.switch_conn.clone();
    let entries = ui.history.entries;
    let hist_open = ui.history_actions.open.clone();
    let hist_clear = ui.history_actions.clear.clone();
    let ai_send = ui.ai_actions.send.clone();
    let term_input = ui.term_actions.input.clone();

    // The active tab (Copy) — for query-scoped commands.
    let active_tab = move || {
        let id = active.get_untracked();
        tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied())
    };
    // An instant command whose action `f` runs then closes the overlay.
    let instant = |f: Rc<dyn Fn()>, close: &Rc<dyn Fn()>| {
        let close = close.clone();
        CmdArg::Instant(Rc::new(move || {
            (f)();
            (close)();
        }))
    };

    vec![
        Command {
            name: "toggle panel",
            label: "Toggle Panel",
            hint: "schema · ai · terminal · query history",
            arg: CmdArg::Options {
                list: Rc::new(|| {
                    [
                        ("schema", "Schema"),
                        ("ai", "AI"),
                        ("terminal", "Terminal"),
                        ("history", "Query History"),
                    ]
                    .into_iter()
                    .map(|(v, l)| (v.to_string(), l.to_string()))
                    .collect()
                }),
                run: {
                    let close = close.clone();
                    Rc::new(move |v: String| {
                        match v.as_str() {
                            "schema" => {
                                if schema_panel_allowed() {
                                    schema_visible.update(|s| *s = !*s);
                                }
                            }
                            other => {
                                if right_panel_allowed() {
                                    let target = match other {
                                        "terminal" => RightPanel::Terminal,
                                        "history" => RightPanel::History,
                                        _ => RightPanel::Ai,
                                    };
                                    right_panel.update(|p| {
                                        *p = if *p == target {
                                            RightPanel::None
                                        } else {
                                            target
                                        }
                                    });
                                }
                            }
                        }
                        (close)();
                    })
                },
            },
        },
        Command {
            name: "new tab",
            label: "New Tab",
            hint: "",
            arg: instant(add_tab.clone(), &close),
        },
        Command {
            name: "duplicate tab",
            label: "Duplicate Tab",
            hint: "",
            arg: instant(
                {
                    let dup = duplicate_tab.clone();
                    Rc::new(move || {
                        if let Some(t) = active_tab() {
                            (dup)(t.id);
                        }
                    })
                },
                &close,
            ),
        },
        Command {
            name: "close tab",
            label: "Close Tab",
            hint: "",
            arg: instant(
                {
                    let close_tab = close_tab.clone();
                    Rc::new(move || {
                        if let Some(t) = active_tab() {
                            (close_tab)(t.id);
                        }
                    })
                },
                &close,
            ),
        },
        Command {
            name: "next tab",
            label: "Next Tab",
            hint: "",
            arg: instant(Rc::new(move || cycle_tab(tabs, active, 1)), &close),
        },
        Command {
            name: "previous tab",
            label: "Previous Tab",
            hint: "",
            arg: instant(Rc::new(move || cycle_tab(tabs, active, -1)), &close),
        },
        Command {
            name: "format code",
            label: "Format Code",
            hint: "",
            arg: instant(
                Rc::new(move || {
                    if let Some(t) = active_tab() {
                        let unit = if soft_tabs.get_untracked() {
                            " ".repeat(tab_width.get_untracked())
                        } else {
                            "\t".to_string()
                        };
                        let out =
                            schemaic_core::sqlfmt::format_sql(&t.query.get_untracked(), &unit);
                        t.query.set(out);
                    }
                }),
                &close,
            ),
        },
        Command {
            name: "run",
            label: "Run",
            hint: "run all statements",
            arg: instant(
                Rc::new(move || {
                    if let Some(t) = active_tab() {
                        let q = t.query.get_untracked();
                        let stmts: Vec<String> = schemaic_core::sql::statement_ranges(&q)
                            .into_iter()
                            .map(|(lo, hi)| q[lo..hi].to_string())
                            .filter(|s| !s.trim().is_empty())
                            .collect();
                        if !stmts.is_empty() {
                            (run_all)(stmts);
                        }
                    }
                }),
                &close,
            ),
        },
        Command {
            name: "go to line",
            label: "Go to Line",
            hint: "<number>",
            arg: CmdArg::Number {
                min: 1,
                max: i64::MAX,
                run: {
                    let close = close.clone();
                    Rc::new(move |n: i64| {
                        if let Some(t) = active_tab()
                            && let Some(off) = schemaic_core::text_ops::offset_of_line(
                                &t.query.get_untracked(),
                                n as usize,
                            )
                        {
                            t.jump_offset.set(Some(off));
                        }
                        (close)();
                    })
                },
                // No number → open the editor's Go-to-line popup.
                empty: Some({
                    let close = close.clone();
                    Rc::new(move || {
                        if let Some(t) = active_tab() {
                            t.goto_open.set(true);
                        }
                        (close)();
                    })
                }),
            },
        },
        Command {
            name: "history",
            label: "History",
            hint: "<search>",
            arg: CmdArg::Text({
                let close = close.clone();
                Rc::new(move |arg: &str| {
                    let conn = active_conn.get_untracked();
                    entries.with_untracked(|v| {
                        v.iter()
                            .filter(|e| {
                                e.conn_id == conn && schemaic_core::history::matches_query(e, arg)
                            })
                            .take(50)
                            .map(|e| {
                                let entry = e.clone();
                                let hist_open = hist_open.clone();
                                let close = close.clone();
                                PaletteItem {
                                    primary: schemaic_core::history::preview(&entry.sql),
                                    secondary: entry.database.clone().unwrap_or_default(),
                                    activate: Rc::new(move || {
                                        (hist_open)(entry.clone());
                                        (close)();
                                    }),
                                    complete: None,
                                    match_term: Some(arg.to_string()),
                                }
                            })
                            .collect()
                    })
                })
            }),
        },
        Command {
            name: "clear history",
            label: "Clear History",
            hint: "current connection",
            arg: instant(hist_clear.clone(), &close),
        },
        Command {
            name: "ai",
            label: "Ask AI",
            hint: "<prompt>",
            arg: CmdArg::Text({
                let close = close.clone();
                Rc::new(move |arg: &str| {
                    let arg = arg.trim();
                    if arg.is_empty() {
                        return vec![hint_item("Ask AI", "Type a prompt…")];
                    }
                    let prompt = arg.to_string();
                    let ai_send = ai_send.clone();
                    let close = close.clone();
                    vec![PaletteItem {
                        primary: "Ask AI".to_string(),
                        secondary: prompt.clone(),
                        activate: Rc::new(move || {
                            if right_panel_allowed() {
                                right_panel.set(RightPanel::Ai);
                            }
                            (ai_send)(prompt.clone());
                            (close)();
                        }),
                        complete: None,
                        match_term: None,
                    }]
                })
            }),
        },
        Command {
            name: "terminal",
            label: "Terminal",
            hint: "<command>",
            arg: CmdArg::Text({
                let close = close.clone();
                Rc::new(move |arg: &str| {
                    let arg = arg.trim();
                    if arg.is_empty() {
                        return vec![hint_item("Run in Terminal", "Type a command…")];
                    }
                    let cmd = arg.to_string();
                    let term_input = term_input.clone();
                    let close = close.clone();
                    vec![PaletteItem {
                        primary: "Run in Terminal".to_string(),
                        secondary: cmd.clone(),
                        activate: Rc::new(move || {
                            if right_panel_allowed() {
                                right_panel.set(RightPanel::Terminal);
                            }
                            (term_input)(format!("{cmd}\r").into_bytes());
                            (close)();
                        }),
                        complete: None,
                        match_term: None,
                    }]
                })
            }),
        },
        Command {
            name: "ui theme",
            label: "UI Theme",
            hint: "light · dark",
            arg: CmdArg::Options {
                list: Rc::new(|| {
                    theme::UiThemeKind::ALL
                        .into_iter()
                        .map(|k| (k.key().to_string(), k.label().to_string()))
                        .collect()
                }),
                run: {
                    let close = close.clone();
                    Rc::new(move |v: String| {
                        ui_theme.set(theme::UiThemeKind::from_key(&v));
                        (close)();
                    })
                },
            },
        },
        Command {
            name: "editor theme",
            label: "Editor Theme",
            hint: "<theme>",
            arg: CmdArg::Options {
                list: Rc::new(|| {
                    theme::EditorThemeKind::ALL
                        .into_iter()
                        .map(|k| (k.key().to_string(), k.label().to_string()))
                        .collect()
                }),
                run: {
                    let close = close.clone();
                    Rc::new(move |v: String| {
                        editor_theme.set(theme::EditorThemeKind::from_key(&v));
                        (close)();
                    })
                },
            },
        },
        Command {
            name: "font size",
            label: "Font Size",
            hint: "<8–32>",
            arg: CmdArg::Number {
                min: 8,
                max: 32,
                run: {
                    let close = close.clone();
                    Rc::new(move |n: i64| {
                        editor_font.set(n as f32);
                        (close)();
                    })
                },
                empty: None,
            },
        },
        Command {
            name: "increase font size",
            label: "Increase Font Size",
            hint: "",
            arg: instant(
                Rc::new(move || editor_font.update(|f| *f = (*f + 1.0).clamp(8.0, 32.0))),
                &close,
            ),
        },
        Command {
            name: "decrease font size",
            label: "Decrease Font Size",
            hint: "",
            arg: instant(
                Rc::new(move || editor_font.update(|f| *f = (*f - 1.0).clamp(8.0, 32.0))),
                &close,
            ),
        },
        Command {
            name: "indent style",
            label: "Indent Style",
            hint: "tabs · spaces",
            arg: CmdArg::Options {
                list: Rc::new(|| {
                    vec![
                        ("spaces".to_string(), "Spaces".to_string()),
                        ("tabs".to_string(), "Tabs".to_string()),
                    ]
                }),
                run: {
                    let close = close.clone();
                    Rc::new(move |v: String| {
                        soft_tabs.set(v == "spaces");
                        (close)();
                    })
                },
            },
        },
        Command {
            name: "indent width",
            label: "Indent Width",
            hint: "<1–8>",
            arg: CmdArg::Number {
                min: 1,
                max: 8,
                run: {
                    let close = close.clone();
                    Rc::new(move |n: i64| {
                        tab_width.set(n as usize);
                        (close)();
                    })
                },
                empty: None,
            },
        },
        Command {
            name: "toggle word wrap",
            label: "Toggle Word Wrap",
            hint: "",
            arg: instant(Rc::new(move || word_wrap.update(|w| *w = !*w)), &close),
        },
        Command {
            name: "switch connection",
            label: "Switch Connection",
            hint: "<connection>",
            arg: CmdArg::Options {
                list: Rc::new(move || {
                    connections.with(|cs| {
                        cs.iter()
                            .map(|c| (c.id.to_string(), c.name.clone()))
                            .collect()
                    })
                }),
                run: {
                    let close = close.clone();
                    Rc::new(move |v: String| {
                        if let Ok(id) = v.parse::<u64>() {
                            (switch_conn)(id);
                        }
                        (close)();
                    })
                },
            },
        },
    ]
}

/// Move the active tab by `step` (±1), wrapping around the strip order.
fn cycle_tab(tabs: RwSignal<Vec<crate::Tab>>, active: RwSignal<usize>, step: isize) {
    let ids: Vec<usize> = tabs.with_untracked(|v| v.iter().map(|t| t.id).collect());
    if ids.is_empty() {
        return;
    }
    let cur = active.get_untracked();
    let idx = ids.iter().position(|&x| x == cur).unwrap_or(0) as isize;
    let n = ids.len() as isize;
    let next = ((idx + step) % n + n) % n;
    active.set(ids[next as usize]);
}

/// A non-actionable informational row (Enter does nothing, palette stays open).
fn hint_item(primary: &str, secondary: &str) -> PaletteItem {
    PaletteItem {
        primary: primary.to_string(),
        secondary: secondary.to_string(),
        activate: Rc::new(|| {}),
        complete: None,
        match_term: None,
    }
}

/// Turn a parsed query into the list of rows to show. `caret_end` is pulsed by a
/// command→argument transition so the caret jumps to the end of the inserted text.
#[allow(clippy::too_many_arguments)]
fn build_items(
    parsed: schemaic_core::palette::Parsed,
    commands: &[Command],
    db_nodes: RwSignal<Vec<ConnNode>>,
    hidden: RwSignal<HashSet<String>>,
    open_table: &Rc<dyn Fn(String, String)>,
    close: &Rc<dyn Fn()>,
    query: RwSignal<String>,
    caret_end: RwSignal<u64>,
) -> Vec<PaletteItem> {
    use schemaic_core::palette::Parsed;
    match parsed {
        // Default table/column search (unchanged behaviour): open the table.
        Parsed::Search(q) => {
            let q = q.trim().to_lowercase();
            if q.is_empty() {
                return Vec::new();
            }
            find_matches(db_nodes, hidden, &q, 80)
                .into_iter()
                .map(|hit| {
                    let primary = match &hit.column {
                        Some(c) => format!("{}.{c}", hit.table),
                        None => hit.table.clone(),
                    };
                    // Ghost/Tab target: the matched name (column when it's a column
                    // hit, else the table). The ghost only paints when the query is a
                    // true prefix of it, so a mid-string match shows nothing.
                    let complete = hit.column.clone().unwrap_or_else(|| hit.table.clone());
                    let (db, table) = (hit.db.clone(), hit.table.clone());
                    let open_table = open_table.clone();
                    let close = close.clone();
                    PaletteItem {
                        primary,
                        secondary: hit.db,
                        activate: Rc::new(move || {
                            (open_table)(db.clone(), table.clone());
                            (close)();
                        }),
                        complete: Some(complete),
                        match_term: Some(q.clone()),
                    }
                })
                .collect()
        }
        // Command mode, still choosing: filter the command list. Instant commands
        // run on Enter; argument-commands transition into argument entry.
        Parsed::Filter(f) => {
            let f = f.trim().to_lowercase();
            commands
                .iter()
                .filter(|c| {
                    f.is_empty() || c.label.to_lowercase().contains(&f) || c.name.contains(&f)
                })
                .map(|c| {
                    // Tab/ghost target: the command name, plus a trailing space for
                    // argument-commands so the caret lands ready for the argument.
                    let complete = if c.takes_arg() {
                        format!(">{} ", c.name)
                    } else {
                        format!(">{}", c.name)
                    };
                    let activate: Rc<dyn Fn()> = match &c.arg {
                        CmdArg::Instant(run) => run.clone(),
                        // Argument-command: transition into argument entry (same as
                        // accepting the completion) and move the caret to the end.
                        _ => {
                            let s = complete.clone();
                            Rc::new(move || {
                                query.set(s.clone());
                                caret_end.update(|n| *n += 1);
                            })
                        }
                    };
                    PaletteItem {
                        primary: c.label.to_string(),
                        secondary: c.hint.to_string(),
                        activate,
                        complete: Some(complete),
                        match_term: Some(f.clone()),
                    }
                })
                .collect()
        }
        // A resolved argument-command: render its argument choices/results.
        Parsed::Command { name, arg } => {
            let Some(c) = commands.iter().find(|c| c.name == name) else {
                return Vec::new();
            };
            match &c.arg {
                CmdArg::Instant(run) => vec![PaletteItem {
                    primary: c.label.to_string(),
                    secondary: String::new(),
                    activate: run.clone(),
                    complete: None,
                    match_term: None,
                }],
                CmdArg::Options { list, run } => {
                    let a = arg.trim().to_lowercase();
                    list()
                        .into_iter()
                        .filter(|(v, l)| {
                            a.is_empty()
                                || l.to_lowercase().contains(&a)
                                || v.to_lowercase().contains(&a)
                        })
                        .map(|(v, l)| {
                            let run = run.clone();
                            let v2 = v.clone();
                            PaletteItem {
                                primary: l,
                                secondary: String::new(),
                                activate: Rc::new(move || (run)(v2.clone())),
                                // Tab fills the argument with this option's value.
                                complete: Some(format!(">{} {}", c.name, v)),
                                match_term: Some(a.clone()),
                            }
                        })
                        .collect()
                }
                CmdArg::Number {
                    min,
                    max,
                    run,
                    empty,
                } => {
                    let t = arg.trim();
                    if t.is_empty() {
                        return match empty {
                            Some(e) => vec![PaletteItem {
                                primary: c.label.to_string(),
                                secondary: "↵".to_string(),
                                activate: e.clone(),
                                complete: None,
                                match_term: None,
                            }],
                            None => vec![hint_item(c.label, c.hint)],
                        };
                    }
                    match t.parse::<i64>() {
                        Ok(n) => {
                            let clamped = n.clamp(*min, *max);
                            let run = run.clone();
                            vec![PaletteItem {
                                primary: c.label.to_string(),
                                secondary: format!("→ {clamped}"),
                                activate: Rc::new(move || (run)(clamped)),
                                complete: None,
                                match_term: None,
                            }]
                        }
                        Err(_) => vec![hint_item(c.label, "Enter a number")],
                    }
                }
                CmdArg::Text(f) => f(&arg),
            }
        }
    }
}

// Find Anywhere / command palette. No `>` prefix → table/column search; a `>`
// prefix enters command mode (see `schemaic_core::palette` + `palette_commands`).
pub(crate) fn find_overlay(ui: Ui) -> impl IntoView {
    let open = ui.overlay.find_open;
    let query = ui.overlay.find_query;
    let db_nodes = ui.schema.db_nodes;
    let hidden = ui.schema.hidden_dbs;
    let open_table = ui.tab_actions.open_table.clone();
    let ui_reg = ui.clone(); // for building the command registry per open

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let open_table = open_table.clone();
            let ui_reg = ui_reg.clone();
            // Custom box (not `text_input`) so we control Escape → close. Closing
            // also clears the query, so reopening Find starts blank and a stale
            // result list never flashes.
            let close: Rc<dyn Fn()> = Rc::new(move || {
                query.set(String::new());
                open.set(false);
            });

            // The command registry (rebuilt per open) + the names of its
            // argument-taking commands, which the pure parser needs.
            let commands = Rc::new(palette_commands(&ui_reg, close.clone()));
            let arg_names: Vec<&'static str> = commands
                .iter()
                .filter(|c| c.takes_arg())
                .map(|c| c.name)
                .collect();

            // Current rows + the keyboard-selected index, recomputed from the query.
            let items: RwSignal<Vec<PaletteItem>> = RwSignal::new(Vec::new());
            let selected = RwSignal::new(0usize);
            // Pulsed to move the caret to the end after a completion/transition.
            let caret_end = RwSignal::new(0u64);
            // Programmatic scroll target for keyboard nav (grid pattern: the scroll
            // reads this, its own on_scroll is owned by `autohide`).
            let list_scroll: RwSignal<Option<floem::kurbo::Point>> = RwSignal::new(None);
            {
                let commands = commands.clone();
                let open_table = open_table.clone();
                let close = close.clone();
                create_effect(move |_| {
                    let raw = query.get();
                    let parsed = schemaic_core::palette::parse(&raw, &arg_names);
                    items.set(build_items(
                        parsed,
                        &commands,
                        db_nodes,
                        hidden,
                        &open_table,
                        &close,
                        query,
                        caret_end,
                    ));
                    selected.set(0); // reset the cursor to the top on every new query
                });
            }

            // Activate the selected row (Enter or click).
            let open_sel: Rc<dyn Fn()> = Rc::new(move || {
                let act = items.with_untracked(|v| {
                    v.get(selected.get_untracked())
                        .map(|it| it.activate.clone())
                });
                if let Some(act) = act {
                    (act)();
                }
            });
            // Tab accepts the selected row's completion (the ghost): set the query
            // to it and move the caret to the end.
            let on_tab: Rc<dyn Fn()> = Rc::new(move || {
                let comp = items.with_untracked(|v| {
                    v.get(selected.get_untracked())
                        .and_then(|it| it.complete.clone())
                });
                if let Some(c) = comp {
                    query.set(c);
                    caret_end.update(|n| *n += 1);
                }
            });
            let on_up: Rc<dyn Fn()> =
                Rc::new(move || selected.update(|i| *i = i.saturating_sub(1)));
            let on_down: Rc<dyn Fn()> = Rc::new(move || {
                let n = items.with_untracked(|v| v.len());
                if n > 0 {
                    selected.update(|i| *i = (*i + 1).min(n - 1));
                }
            });

            let field = search_box(
                query,
                close.clone(),
                on_up,
                on_down,
                open_sel.clone(),
                on_tab,
                caret_end,
            );
            // Ghost completion: the dim tail of the selected row's `complete` beyond
            // what's typed, painted over the input right after the text — so Tab's
            // target is visible inline. Only when the typed text is a prefix of it.
            let ghost = dyn_container(
                move || {
                    let q = query.get();
                    let sel = selected.get();
                    items.with(|v| {
                        v.get(sel).and_then(|it| it.complete.clone()).and_then(|c| {
                            (c.len() > q.len() && c.to_lowercase().starts_with(&q.to_lowercase()))
                                .then(|| c[q.len()..].to_string())
                        })
                    })
                },
                move |g| match g {
                    Some(suffix) => text(suffix)
                        // Match the field's 1.46 line-height factor so the ghost
                        // glyph sits on the same baseline as the typed text (a
                        // default, tighter line box floated it ~4px too high). The
                        // placeholder colour keeps it a subtle hint, not competing
                        // with the typed text.
                        .style(|s| {
                            s.color(theme::placeholder())
                                .font_size(16.0)
                                .line_height(1.46)
                        })
                        .into_any(),
                    None => empty().into_any(),
                },
            )
            .style(move |s| {
                // Right after the typed text: box border (1) + horizontal padding +
                // the measured width of the query at the field's 16px font.
                let x = 1.0 + CHAT_PAD_H + measure_text_px_at(&query.get(), 16.0);
                s.absolute().inset_left(x).inset_top(1.0 + CHAT_PAD_V)
            })
            .pointer_events(|| false);
            let input = stack((field, ghost)).style(|s| s.width_full());

            // Suggestions appear only once something is typed (empty → just the box).
            let results = dyn_container(
                move || items.get(),
                move |list| {
                    if query.with_untracked(|q| q.is_empty()) {
                        return empty().into_any();
                    }
                    if list.is_empty() {
                        // Left-aligned like a normal result row (same padding), 13px.
                        return text("Nothing found")
                            .style(|s| {
                                s.color(theme::text_muted())
                                    .font_size(14.0)
                                    .padding_horiz(12.0)
                                    .padding_vert(9.0)
                            })
                            .into_any();
                    }
                    // Same look as the dropdown menus (menu_item_style): the primary
                    // label then a dim secondary, left-aligned. The keyboard-selected
                    // row is highlighted; click or Enter activates it.
                    let total = list.len();
                    v_stack_from_iter(list.into_iter().enumerate().map(move |(i, item)| {
                        let activate = item.activate.clone();
                        let row = h_stack((
                            highlighted_primary(&item.primary, &item.match_term),
                            text(item.secondary.clone())
                                .style(|s| s.color(theme::text_muted()).font_size(14.0)),
                        ))
                        .on_click_stop(move |_| {
                            selected.set(i);
                            (activate)();
                        })
                        .style(move |s| {
                            // +3px over menu_item_style's 6px vertical padding.
                            let s = menu_item_style(s).padding_vert(9.0);
                            if selected.get() == i {
                                s.background(theme::row_selected())
                            } else {
                                s
                            }
                        });
                        // Keep the keyboard-selected row in view. The ends scroll fully to
                        // the top / bottom (so the first row clears the input's 10px gap
                        // and the last row reaches the end); middle rows reveal minimally
                        // (deferred a tick so it clamps against settled layout).
                        let row_id = row.id();
                        create_effect(move |_| {
                            if selected.get() != i {
                                return;
                            }
                            if i == 0 {
                                list_scroll.set(Some(floem::kurbo::Point::ZERO));
                            } else if i + 1 == total {
                                list_scroll.set(Some(floem::kurbo::Point::new(0.0, 1.0e7)));
                            } else {
                                list_scroll.set(None);
                                floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                                    row_id.scroll_to(None);
                                });
                            }
                        });
                        row
                    }))
                    // Right gutter clears the floating scrollbar (3px edge inset +
                    // 6px handle) so a row's highlight stops just before it rather
                    // than running underneath.
                    .style(|s| s.flex_col().width_full().padding_right(10.0))
                    .into_any()
                },
            )
            // Fill the scroll's viewport width so the inner v_stack's `width_full`
            // (and each row's highlight) spans edge to edge, not just content width.
            .style(|s| s.width_full());

            // Panel: 400px input + 15px padding all around (→ 430px wide), results
            // below. Sizes to content; the results scroll caps its height.
            let panel = v_stack((
                input,
                autohide(scroll(results).scroll_to(move || list_scroll.get()))
                    // 10px gap here (not inside the content) so the scrollbar clears
                    // the input too, and the first row keeps the gap when scrolled up.
                    // Only when there's something to show — an empty query collapses
                    // the container, so the panel's padding stays even around the box.
                    .style(move |s| {
                        let s = s.width_full().max_height(360.0);
                        if query.with(|q| q.is_empty()) {
                            s
                        } else {
                            s.margin_top(10.0)
                        }
                    }),
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
