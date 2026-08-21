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
use schemaic_core::schema::{SchemaState, TableSource};
use schemaic_core::skeleton::{delete_skeleton, insert_skeleton, update_skeleton};

use crate::consts::{CHAT_PAD_H, CHAT_PAD_V, DB_MENU_W};
use crate::widgets::{
    ACTION_TAB, CURSOR_MENU_GAP, MenuEntry, autohide, cursor_menu_pos, dialog_button, focus_root,
    measure_text_px_at, menu_item_style, menu_panel, menu_panel_height, panel_style, window_size,
};
use crate::{
    ConnNode, CtxKind, CtxMenu, PopupAnchor, RightPanel, TxChoice, Ui, icons, right_panel_allowed,
    schema_panel_allowed, search_box, theme,
};

/// Width of the schema tree's context menu (its panel's `min_width`), which is
/// also what its placement flips against.
const CTX_MENU_W: f64 = 170.0;

/// How far left of its icon a SCHEMA dropdown opens, so the panel overlaps the
/// glyph it belongs to rather than starting beside it.
const MENU_ICON_TUCK: f64 = 30.0;

/// Is the active connection read-only? Every schema-editing menu entry asks,
/// because a write it can't perform is shown dimmed rather than hidden — a
/// missing item reads as "not supported", a dimmed one as "not here".
fn conn_read_only(connections: &RwSignal<Vec<Connection>>, active_conn: RwSignal<u64>) -> bool {
    connections.with_untracked(|cs| {
        cs.iter()
            .find(|c| c.id == active_conn.get_untracked())
            .is_some_and(|c| c.read_only)
    })
}

/// The **Create** submenu a database or namespace node offers: `Table` and
/// `View` on both engines, plus PostgreSQL's `Type` / `Domain` / `Sequence`.
///
/// One submenu rather than five siblings, because the flat form put five rows
/// that all began with the same verb into the middle of a menu whose every
/// other entry is about the node itself — on PostgreSQL, most of the menu. The
/// verb moves to the parent row and the labels become the nouns, which is the
/// shape `Colour` here and `Copy` on a table node already use.
///
/// The three PostgreSQL objects are **absent on MySQL**, which has none of them
/// — the same call `trigger_editor`'s form makes about what an engine can't
/// express: hide it, rather than offer it and fail at apply. That is also why
/// they are hidden where every other schema-editing entry is merely *dimmed* on
/// a read-only connection: a missing entry reads as "not supported", a dimmed
/// one as "not here", and here both readings are true of a different engine.
///
/// The parent row is never itself dimmed — a [`MenuEntry::Sub`] has no disabled
/// state — so on a read-only connection it opens onto entries that are all
/// dimmed, which is the same thing the flat form said with the group it dimmed.
/// `None` when the engine offers nothing to create at all, so the caller leaves
/// the row out rather than showing a submenu that opens onto nothing.
fn create_submenu(
    ui: &Ui,
    database: &str,
    schema: Option<&str>,
    read_only: bool,
) -> Option<MenuEntry> {
    let dialect = crate::table_designer::edit_ctx(ui).dialect;
    let entries = create_children(dialect, read_only);
    if entries.is_empty() {
        return None;
    }
    let children = entries
        .into_iter()
        .map(|e| {
            let (ui, db, ns) = (ui.clone(), database.to_string(), schema.map(str::to_string));
            MenuEntry::action(e.label, move || match e.kind {
                CreateKind::Table => {
                    crate::table_designer::open_for_new(&ui, &db, ns.as_deref());
                }
                CreateKind::View => crate::view_editor::open_for_new(&ui, &db, ns.as_deref()),
                CreateKind::Object(kind) => {
                    crate::object_editor::open_for_new(&ui, &db, ns.as_deref(), kind);
                }
            })
            .disabled(e.disabled)
        })
        .collect();
    Some(MenuEntry::sub("Create", children))
}

/// Which of the per-object entries the schema tree's table/view menu offers **at
/// all** — as opposed to offering dimmed.
///
/// The distinction is the one [`create_children`] already draws: a **missing**
/// entry reads as "not supported", a **dimmed** one as "not here". For these
/// three the first is what's true of a view, so a view's menu simply doesn't
/// carry them; a read-only connection or a schema that hasn't loaded still dims,
/// because there the action is real and merely unavailable.
///
/// Separate from the menu builder so the rule can be asserted at all — the
/// builder needs a `Ui` — and because one of the three is **not uniform** and is
/// exactly the kind of thing a later edit flattens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct ObjectEntries {
    pub import: bool,
    pub triggers: bool,
    pub truncate: bool,
    /// Whether **Edit table** / **Edit view** is offered — false on an engine
    /// this build can't emit schema DDL for.
    pub edit: bool,
}

/// See [`ObjectEntries`]. `materialized` is only meaningful on PostgreSQL, which
/// is the only engine that has one.
pub(crate) fn object_entries(
    is_view: bool,
    dialect: schemaic_core::intel::SqlDialect,
    materialized: bool,
) -> ObjectEntries {
    // Three questions that every engine now answers yes to, each for its own
    // reason — SQLite designs a table by rebuilding it, edits a view by dropping
    // and re-creating it, and edits a trigger now that its `CREATE` text can be
    // read back into the model. They stay separate capabilities: what differs
    // between engines is no longer *whether* an editor opens but what each one
    // offers, which is the form's business rather than this menu's.
    let edits_views = schemaic_core::ddl::supports_view_editing(dialect);
    let edits_triggers = schemaic_core::ddl::supports_trigger_editing(dialect);
    ObjectEntries {
        // A view is not insertable, and owns no rows to delete.
        import: !is_view,
        // **Gated on the capability, like every sibling here.** It was the one
        // entry in this struct that wasn't, and Truncate is the entry that can
        // least afford it: on an engine with no arm for it, the menu offered a
        // red enabled item, asked "Delete all ~4.2m rows in orders?", and then
        // opened a preview whose script was empty and whose Apply was inert —
        // an irreversible question for something that was never going to happen.
        truncate: !is_view
            && schemaic_core::ddl::supports_change(
                dialect,
                &schemaic_core::ddl::Change::TruncateTable,
            ),
        // **A view really does carry triggers on two of the three engines** —
        // `INSTEAD OF` lives on PostgreSQL and on SQLite, where it is the only
        // way a view is written to at all — so this is not simply `!is_view`.
        // MySQL is the one that takes no trigger on a view. A *materialized*
        // view is excluded even on PostgreSQL: the server refuses outright
        // (`relation "mv" cannot have triggers`), which is the same call
        // `is_editable_view` makes.
        triggers: edits_triggers
            && (!is_view || (dialect != schemaic_core::intel::SqlDialect::MySql && !materialized)),
        // A table's designer, or a view's editor — the entry reads "Edit table"
        // or "Edit view" and they are not the same capability. The table half was
        // a literal `true`, left behind when the predicate it used to ask was
        // deleted; `supports_table_design` is that question restated as the one
        // the designer actually needs answered.
        edit: if is_view {
            edits_views
        } else {
            schemaic_core::ddl::supports_table_design(dialect)
        },
    }
}

/// What a **column** row's context menu offers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct FieldEntries {
    /// Whether **Edit column** is offered — it opens the designer, so it needs
    /// the whole schema-editing emitter, not just a statement for one drop.
    pub edit: bool,
    pub drop: bool,
}

/// What a **key / index** row's context menu offers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct KeyEntries {
    /// Whether the edit entry is offered — **Edit index**, **Edit foreign key**
    /// or **Edit primary key**, named for whichever the row is. Same designer as
    /// [`FieldEntries::edit`], opened on the section that holds the key (see
    /// [`schemaic_core::ddl::TableDraft::find_key`]).
    pub edit: bool,
    pub drop_foreign_key: bool,
    pub drop_index: bool,
}

/// See [`FieldEntries`]. Separate from the menu builder for the reason
/// [`object_entries`] is: so the rule can be asserted without a `Ui`.
///
/// The two questions really are different, which is why this asks twice.
/// **Edit** opens the designer, so it asks whether this engine can design a table
/// at all (`ddl::supports_table_design` — every engine can, SQLite by rebuilding
/// the table around a retype or a constraint). **Drop** is a shortcut with no
/// draft behind it, so it needs a statement for that one change, which is a
/// narrower thing to ask (`ddl::supports_change`).
/// `is_view` because **a view's columns are not the view's to edit.** The tree
/// renders a column row under a view exactly as it does under a table — the flag
/// only picks a different glyph — so without this the menu offers Edit column
/// and a red Drop for something that has neither, opens the *table* designer on
/// the view, and the refusal arrives from the server (`… is not BASE TABLE`, and
/// on SQLite a `DROP TABLE` on a view) rather than from the menu. It is the same
/// question [`object_entries`] one level up already asks.
pub(crate) fn field_entries(
    dialect: schemaic_core::intel::SqlDialect,
    is_view: bool,
) -> FieldEntries {
    FieldEntries {
        edit: !is_view && schemaic_core::ddl::supports_table_design(dialect),
        // The predicate reads the *shape* of the change, not its names — a
        // dropped column is expressible or not whatever it is called.
        drop: !is_view
            && schemaic_core::ddl::supports_change(
                dialect,
                &schemaic_core::ddl::Change::DropColumn {
                    name: String::new(),
                    type_name: String::new(),
                },
            ),
    }
}

/// See [`KeyEntries`]. `constraint` is the constraint an index backs, when it
/// backs one — SQLite can't drop those, because they are part of the table
/// definition rather than objects of their own.
///
/// `is_view` for the reason [`field_entries`] takes it: a view has no keys and
/// no indexes of its own, and every route out of these entries is the table
/// designer.
pub(crate) fn key_entries(
    dialect: schemaic_core::intel::SqlDialect,
    constraint: Option<&str>,
    is_view: bool,
) -> KeyEntries {
    use schemaic_core::ddl::{Change, supports_change, supports_table_design};
    KeyEntries {
        // The designer, which reaches what these shortcuts can't: dropping a
        // foreign key on SQLite is a rebuild, and the draft is what has one.
        edit: !is_view && supports_table_design(dialect),
        drop_foreign_key: !is_view
            && supports_change(
                dialect,
                &Change::DropForeignKey {
                    name: String::new(),
                },
            ),
        drop_index: !is_view
            && supports_change(
                dialect,
                &Change::DropIndex {
                    name: String::new(),
                    constraint: constraint.map(str::to_string),
                },
            ),
    }
}

/// What a Create entry makes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CreateKind {
    Table,
    View,
    Object(schemaic_core::ddl::ObjectKind),
}

/// One Create entry as data: its label, what it opens, and whether it is inert.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct CreateEntry {
    pub label: &'static str,
    pub kind: CreateKind,
    pub disabled: bool,
}

/// Which Create entries exist, and which are inert.
///
/// Two gates, and neither could be asserted while it lived inside a menu
/// builder that needs a `Ui`: **which entries exist** (the three PostgreSQL
/// objects are absent on MySQL, which has none of them) and **which are
/// disabled** (all of them, on a read-only connection). The second is a live
/// DDL path if it ever drifts.
///
/// The engine gate hides rather than dims, unlike everything else here: a
/// missing entry reads as "not supported" and a dimmed one as "not here", and
/// hiding is the same call `trigger_editor`'s per-engine form makes about what
/// an engine can't express.
pub(crate) fn create_children(
    dialect: schemaic_core::intel::SqlDialect,
    read_only: bool,
) -> Vec<CreateEntry> {
    use schemaic_core::ddl::ObjectKind;
    // Every engine can create a table. A view is a separate capability — see
    // `ddl::supports_view_editing` — and is absent rather than dimmed where the
    // emitter would write a statement the engine has no form of.
    let mut out = vec![CreateEntry {
        label: "Table",
        kind: CreateKind::Table,
        disabled: read_only,
    }];
    if schemaic_core::ddl::supports_view_editing(dialect) {
        out.push(CreateEntry {
            label: "View",
            kind: CreateKind::View,
            disabled: read_only,
        });
    }
    if dialect == schemaic_core::intel::SqlDialect::Postgres {
        out.extend(
            [
                (ObjectKind::Enum, "Type"),
                (ObjectKind::Domain, "Domain"),
                (ObjectKind::Sequence, "Sequence"),
            ]
            .into_iter()
            .map(|(kind, label)| CreateEntry {
                label,
                kind: CreateKind::Object(kind),
                disabled: read_only,
            }),
        );
    }
    out
}

// ===== moved from lib.rs (overlays) =====
pub(crate) fn conn_menu_overlay(ui: Ui) -> impl IntoView {
    let open = ui.conn.conn_menu_open;
    let connections = ui.conn.connections;
    let active_conn = ui.conn.active_conn;
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
                    // Leading identity dot in a fixed 14px slot — this
                    // connection's own colour, the same one on the switcher's
                    // outline and its tabs. It used to carry health, but only
                    // the active connection is ever checked, so every other row
                    // was neutral and the dot really only marked "this is the
                    // current one" — which the row's own highlight already says.
                    let dot_color = c
                        .color
                        .as_deref()
                        .and_then(theme::parse_hex)
                        .unwrap_or_else(theme::text_dim);
                    let dot =
                        container(icons::icon(icons::DOT, 6.0).style(move |s| s.color(dot_color)))
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
            // The click goes on a sibling behind the panel — see
            // `widgets::dismiss_layer` for why it must not be on the focus root.
            focus_root(stack((
                crate::widgets::dismiss_layer(move || open.set(false)),
                panel,
            )))
            .on_key_down(
                Key::Named(NamedKey::Escape),
                |_| true,
                move |_| open.set(false),
            )
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

    // **One predicate, read by the panel and by the layer it sits on.** Two
    // spellings of "is this menu showing" is what froze the app: the content said
    // `open && databases`, the style below said `open` alone, so clicking the
    // selector on a connection with nothing to list built no panel and no
    // dismiss layer — and still stretched this container over the whole window
    // (`inset(0.0)`). An absolute, transparent, handler-less sheet on top of
    // everything swallows every click, including the one on the selector that
    // would close it and the Escape handler that never mounted. The window
    // renders perfectly and answers nothing; the only way out is killing the
    // process, which is what a user had to do.
    let showing = move || open.get() && !db_nodes.with(|n| n.is_empty());
    // A flag no panel answers is also a flag nothing can clear, so it must not
    // survive: the databases can go away *while* the menu is open (a switch, a
    // failed reload) and `open` would sit `true` until some later load repopulated
    // the list and popped a menu nobody asked for.
    create_effect(move |_| {
        if db_nodes.with(|n| n.is_empty()) && open.get_untracked() {
            open.set(false);
        }
    });

    dyn_container(
        // Same rule as the schema eye: no databases, no dropdown.
        showing,
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

            focus_root(stack((
                crate::widgets::dismiss_layer(move || open.set(false)),
                panel,
            )))
            .on_key_down(
                Key::Named(NamedKey::Escape),
                |_| true,
                move |_| open.set(false),
            )
            .style(|s| s.size_full().flex_col().items_start().justify_start())
            .into_any()
        },
    )
    // `showing`, not `open` — see above. The window-wide sheet exists only when
    // there is a panel on it to dismiss.
    .style(move |s| {
        if showing() {
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
    let anchor = ui.schema.db_menu_anchor;
    let db_nodes = ui.schema.db_nodes;
    let hidden = ui.schema.hidden_dbs;
    let toggle = ui.schema_actions.toggle_db_hidden.clone();

    // Same flag hygiene as the active-database menu: a panel that cannot render
    // must not leave its flag set, or a later load pops a menu nobody asked for.
    // (This one is anchored to the eye rather than stretched over the window, so
    // it never swallowed the app the way that one did.)
    create_effect(move |_| {
        if db_nodes.with(|n| n.is_empty()) && open.get_untracked() {
            open.set(false);
        }
    });

    dyn_container(
        // Nothing to list → no panel. An empty dropdown is worse than none: it
        // reads as a broken menu rather than "this connection has no databases"
        // (true on a dead connection, and on a live one with nothing to show).
        move || open.get() && !db_nodes.with(|n| n.is_empty()),
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
                            let c = if hidden.with(|h| h.contains(&db_state)) {
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
            focus_root(v_stack((list,)))
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
    // Hung off the SCHEMA eye's own box, tucked 30px left of it so it overlaps the
    // icon like the other dropdowns.
    .style(move |s| {
        if open.get() {
            let a = anchor.get();
            s.absolute()
                .inset_left((a.x - MENU_ICON_TUCK).max(0.0))
                .inset_top(a.y + 3.0)
        } else {
            s
        }
    })
}

// The SCHEMA settings dropdown (opened by the gear): Refresh, Collapse all, and
// the size-column toggle. Same style as the other dropdowns, dropped 3px below
// the gear.
pub(crate) fn schema_settings_overlay(ui: Ui) -> impl IntoView {
    let open = ui.schema.schema_menu_open;
    let anchor = ui.schema.schema_menu_anchor;
    let refresh = ui.schema_actions.refresh_schema.clone();
    let collapse_all = ui.schema_actions.collapse_all.clone();
    let toggle_sizes = ui.schema_actions.toggle_table_sizes.clone();
    let sizes_on = ui.schema.table_sizes;
    // Kept whole for the capability check below: which rows this menu has depends
    // on the active connection, and the menu is rebuilt on each open, so it is read
    // there rather than captured here.
    let menu_ui = ui.clone();

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let ui = menu_ui.clone();
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

            // **The label carries the state, the way the eye menu's rows do** —
            // `db_toggle_on` when the column is showing. No check glyph: it would
            // need a reserved 13px gutter that reads as an empty box on every
            // other row of this menu, and on the off state as a missing tick.
            //
            // Off is plain `text()`, not the eye menu's faded `db_toggle_off`.
            // There, dim means *this database is hidden* — a state with a
            // consequence. Here off is just the resting state of a view mode, and
            // dimming it would make an ordinary menu row look disabled.
            //
            // **And it is offered only where the engine has sizes to show**
            // (`stats::supports_table_stats`, the capability the fetch itself is
            // guarded by). SQLite has none — no per-table row estimate outside an
            // `ANALYZE` sample, and per-table bytes need the `dbstat` module this
            // build omits — so the column stays empty whichever way the row is set,
            // and a row that visibly toggles while nothing changes is worse than no
            // row. Absent rather than disabled: there is nothing here for another
            // connection to enable. The *setting* is untouched — it is global and
            // persisted, so a MySQL connection comes back to whatever it was left at.
            let mut items: Vec<floem::AnyView> =
                vec![refresh_item.into_any(), collapse_item.into_any()];
            if schemaic_core::stats::supports_table_stats(
                crate::table_designer::edit_ctx(&ui).dialect,
            ) {
                let toggle_sizes = toggle_sizes.clone();
                items.push(
                    container(text("Show table sizes").style(move |s| {
                        s.color(if sizes_on.get() {
                            theme::db_toggle_on()
                        } else {
                            theme::text()
                        })
                    }))
                    .on_click_stop(move |_| (toggle_sizes)())
                    .style(menu_item_style)
                    .style(|s| s.padding_vert(8.0))
                    .into_any(),
                );
            }

            focus_root(v_stack_from_iter(items))
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
    // Hung off the SCHEMA gear's own box, same tuck as the eye's menu.
    .style(move |s| {
        if open.get() {
            let a = anchor.get();
            s.absolute()
                .inset_left((a.x - MENU_ICON_TUCK).max(0.0))
                .inset_top(a.y + 3.0)
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
    let erd = ui.overlay.erd;
    let last_mouse = ui.overlay.last_mouse;
    let toggle_hidden = ui.schema_actions.toggle_db_hidden.clone();
    let open_table = ui.tab_actions.open_table.clone();
    let open_table_new = ui.tab_actions.open_table_new.clone();
    let tabs = ui.tabs_ui.tabs;
    let active_conn = ui.conn.active_conn;
    let open_query = ui.tab_actions.open_query.clone();
    let open_db_cli = ui.tab_actions.open_db_cli.clone();
    let open_monitor = ui.tab_actions.open_monitor.clone();
    let refresh_db = ui.schema_actions.refresh_db.clone();
    let collapse_db = ui.schema_actions.collapse_db.clone();
    let ai_send = ui.ai_actions.send.clone();
    let right_panel = ui.layout.right_panel;
    let db_colors = ui.db_colors;
    let table_colors = ui.table_colors;
    let save_db_colors = ui.save_db_colors.clone();
    let db_favorites = ui.db_favorites;
    let save_db_favorites = ui.save_db_favorites.clone();
    let db_nodes = ui.schema.db_nodes;
    let connections = ui.conn.connections;
    let import_ui = ui.clone();

    // The entries for one target, built on demand. It is a closure rather than the
    // body of the `dyn_container` because the *placement* needs them too: how far
    // down the menu may open depends on how many rows it has, and that varies from
    // 4 to 14 by target kind. Called twice per open (once to place, once to
    // render), which is a dozen `Rc` clones.
    //
    // ── The order every arm below follows ─────────────────────────────────────
    // One skeleton, and each menu is a subsequence of it, so the same action is
    // in the same place whatever the row is:
    //
    //   1. Open        — what a double-click would have done
    //   2. Read        — Copy name, Copy qualified name, then what the node can
    //                    show you (Properties, Show diagram, Generate DDL),
    //                    closing with Refresh
    //   3. Tree state  — Favorite, Colour, Hide: the row, not the object
    //   4. Write       — Create / Edit / Import / Triggers, with the entries
    //                    that can't be taken back **last** inside the group and
    //                    coloured `theme::error`
    //   5. AI Explain  — appended to every menu, outside the `match`
    //
    // Group 4's rule is the load-bearing one: Drop is always the last thing in
    // its menu, so the row the cursor lands on after a right-click is never the
    // irreversible one. The key/index menu used to open straight onto `Drop
    // index`, and the column menu's `Drop` was the same colour as `Edit column`
    // above it.
    let build: Rc<dyn Fn(CtxMenu) -> Vec<MenuEntry>> = Rc::new(move |menu: CtxMenu| {
        {
            // Clipboard action for a string.
            let copy = |s: String| {
                move || {
                    let _ = floem::Clipboard::set_contents(s.clone());
                }
            };
            let mut entries: Vec<MenuEntry> = Vec::new();
            match menu.kind.clone() {
                CtxKind::Database { ddl } => {
                    // ── Open ──────────────────────────────────────────────────
                    let ocli = open_db_cli.clone();
                    let dbn = menu.name.clone();
                    entries.push(MenuEntry::action("Open in CLI", move || {
                        (ocli)(Some(dbn.clone()))
                    }));
                    entries.push(MenuEntry::Separator);
                    // ── Read: clipboard, then what the node can show you, then
                    // the two that only rearrange the tree under it ───────────
                    entries.push(MenuEntry::action("Copy name", copy(menu.name.clone())));
                    // The namespace script's database-level analog, and absent
                    // for the same reason it is: a schema that hasn't loaded has
                    // no script to give, and an entry that copies "" is worse
                    // than no entry.
                    if !ddl.is_empty() {
                        let oq = open_query.clone();
                        // This node *is* the database, so that is what the script
                        // is for — a `CREATE TABLE` script opened against another
                        // database is one Ctrl+Enter from building the tables
                        // somewhere else entirely.
                        let db = menu.name.clone();
                        entries.push(MenuEntry::action("Generate DDL", move || {
                            let _ = floem::Clipboard::set_contents(ddl.clone());
                            (oq)(ddl.clone(), Some(db.clone()));
                        }));
                    }
                    // ER diagram of the whole database (every related table).
                    let edb = menu.name.clone();
                    entries.push(MenuEntry::action("Show diagram", move || {
                        erd.set(Some(crate::ErdTarget {
                            conn_id: active_conn.get_untracked(),
                            database: edb.clone(),
                            seed: schemaic_core::erd::DiagramSeed::Database,
                        }));
                    }));
                    let rf = refresh_db.clone();
                    let dn = menu.name.clone();
                    entries.push(MenuEntry::action("Refresh", move || (rf)(dn.clone())));
                    let cd = collapse_db.clone();
                    let cn = menu.name.clone();
                    entries.push(MenuEntry::action("Collapse all", move || (cd)(cn.clone())));
                    // ── How this database *looks* in the tree ─────────────────
                    // Its own group: colour, favourite and hide change nothing
                    // about the database, only about the row standing for it —
                    // and Hide, which takes the row away, used to sit second
                    // from the top with the harmless entries.
                    entries.push(MenuEntry::Separator);
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
                    // Favorite / unfavorite: a favorited database gets a gold star
                    // and sorts to the top of the tree (oldest favorite highest).
                    let fav_now = schemaic_core::favorite::is_favorite(
                        &db_favorites.get_untracked(),
                        active_conn.get_untracked(),
                        &menu.name,
                    );
                    {
                        let dbf = db_favorites;
                        let save = save_db_favorites.clone();
                        let db = menu.name.clone();
                        let label = if fav_now { "Unfavorite" } else { "Favorite" };
                        entries.push(MenuEntry::action(label, move || {
                            let cid = active_conn.get_untracked();
                            dbf.update(|r| {
                                schemaic_core::favorite::toggle(r, cid, &db);
                            });
                            (save)();
                        }));
                    }
                    entries.push(MenuEntry::sub("Colour", swatches));
                    let th = toggle_hidden.clone();
                    let n = menu.name.clone();
                    entries.push(MenuEntry::action("Hide", move || (th)(n.clone())));
                    // ── Write ─────────────────────────────────────────────────
                    // Schema editing gets its own group — it's the one entry here
                    // that writes.
                    entries.push(MenuEntry::Separator);
                    // On PostgreSQL a database node stands for its `public`
                    // namespace (other namespaces get their own node), so a new
                    // table lands where the tree says it will.
                    {
                        let ns = crate::table_designer::default_schema(&import_ui, &menu.name);
                        entries.extend(create_submenu(
                            &import_ui,
                            &menu.name,
                            ns.as_deref(),
                            conn_read_only(&connections, active_conn),
                        ));
                    }
                }
                // One of PostgreSQL's standalone objects. Deliberately the same
                // shape as the table menu — copy, DDL, refresh, then the writing
                // group — so a type isn't a second-class node with its own idiom.
                CtxKind::Object {
                    database,
                    item,
                    ddl,
                } => {
                    let kind = item.kind();
                    // `database.schema.name`, the shape the table entry uses and
                    // for the same reason. `menu.name` is the *display* name,
                    // whose job is to drop `public` — so for the common case it
                    // was byte-identical to "Name" above and resolved through
                    // whatever `search_path` happened to be.
                    let qualified = format!(
                        "{database}.{}",
                        schemaic_core::schema::display_name(item.schema(), item.name())
                    );
                    entries.push(MenuEntry::action(
                        "Copy name",
                        copy(item.name().to_string()),
                    ));
                    entries.push(MenuEntry::action("Copy qualified name", copy(qualified)));
                    {
                        let oq = open_query.clone();
                        let db = database.clone();
                        entries.push(MenuEntry::action("Generate DDL", move || {
                            let _ = floem::Clipboard::set_contents(ddl.clone());
                            (oq)(ddl.clone(), Some(db.clone()));
                        }));
                    }
                    {
                        let rf = refresh_db.clone();
                        let db = database.clone();
                        entries.push(MenuEntry::action("Refresh", move || (rf)(db.clone())));
                    }
                    entries.push(MenuEntry::Separator);
                    {
                        let ui = import_ui.clone();
                        let (db, obj) = (database.clone(), item.clone());
                        let read_only = conn_read_only(&connections, active_conn);
                        let editable = crate::object_editor::is_editable_object(&obj);
                        // Named, not a bare "Edit": one tree holds types, domains
                        // and sequences, and the menu is the only thing that says
                        // which of them the row under the cursor is.
                        entries.push(
                            MenuEntry::action(format!("Edit {}", kind.label()), move || {
                                crate::object_editor::open_for_object(&ui, &db, &obj);
                            })
                            .disabled(read_only || !editable),
                        );
                    }
                    {
                        let ui = import_ui.clone();
                        let confirm = ui.overlay.confirm;
                        let (db, obj) = (database.clone(), item.clone());
                        let label = menu.name.clone();
                        let read_only = conn_read_only(&connections, active_conn);
                        // An identity column's counter is part of the column:
                        // PostgreSQL refuses `DROP SEQUENCE` on one and says to
                        // drop the column instead. Offering it would be an entry
                        // that can only ever fail.
                        let internal = obj.is_internal();
                        entries.push(
                            MenuEntry::action_colored("Drop", theme::error, move || {
                                let (ui, db, obj) = (ui.clone(), db.clone(), obj.clone());
                                let label = label.clone();
                                confirm.set(Some(crate::Confirm {
                                    title: format!("Drop {}", kind.label()),
                                    message: format!("Drop {label}? This can't be undone."),
                                    resolve: Rc::new(move |yes| {
                                        if !yes {
                                            return;
                                        }
                                        let ctx = crate::table_designer::edit_ctx(&ui);
                                        let cs = schemaic_core::ddl::drop_object(
                                            kind,
                                            obj.name(),
                                            obj.schema(),
                                            ctx.dialect,
                                        );
                                        crate::ddl_preview::open_preview(
                                            &ui,
                                            crate::ddl_preview::preview_of(
                                                ctx.conn_id,
                                                &db,
                                                label.clone(),
                                                &cs,
                                                ctx.read_only,
                                            ),
                                        );
                                    }),
                                }));
                            })
                            .disabled(read_only || internal),
                        );
                    }
                }
                CtxKind::Schema { database, ddl } => {
                    entries.push(MenuEntry::action("Copy name", copy(menu.name.clone())));
                    // The whole namespace as one CREATE script — the schema-level
                    // analog of a table's "Generate DDL" (same clipboard + new-tab
                    // behaviour). Empty for a namespace with no tables, in which
                    // case there's nothing to offer.
                    if !ddl.is_empty() {
                        let oq = open_query.clone();
                        let db = database.clone();
                        entries.push(MenuEntry::action("Generate DDL", move || {
                            let _ = floem::Clipboard::set_contents(ddl.clone());
                            (oq)(ddl.clone(), Some(db.clone()));
                        }));
                    }
                    // A namespace is introspected as part of its database, so
                    // refreshing targets the database.
                    let rf = refresh_db.clone();
                    let refresh_database = database.clone();
                    entries.push(MenuEntry::action("Refresh", move || {
                        (rf)(refresh_database.clone())
                    }));
                    // The writing group, set off the way every other menu here
                    // sets its own off. This was the one menu where `Create` sat
                    // between two read entries with no boundary at all.
                    entries.push(MenuEntry::Separator);
                    entries.extend(create_submenu(
                        &import_ui,
                        &database,
                        Some(&menu.name),
                        conn_read_only(&connections, active_conn),
                    ));
                }
                // A `Types`/`Domains`/`Sequences` folder. The menu is about the
                // set, not the row: a folder has no name worth copying and
                // nothing to open, so it carries the read group's script and
                // refresh, then the one thing you actually come here for —
                // making another of what it holds.
                CtxKind::ObjectGroup {
                    database,
                    schema,
                    kind,
                    ddl,
                } => {
                    if !ddl.is_empty() {
                        let oq = open_query.clone();
                        let db = database.clone();
                        entries.push(MenuEntry::action("Generate DDL", move || {
                            let _ = floem::Clipboard::set_contents(ddl.clone());
                            (oq)(ddl.clone(), Some(db.clone()));
                        }));
                    }
                    {
                        let rf = refresh_db.clone();
                        let db = database.clone();
                        entries.push(MenuEntry::action("Refresh", move || (rf)(db.clone())));
                    }
                    entries.push(MenuEntry::Separator);
                    {
                        let ui = import_ui.clone();
                        let (db, ns) = (database.clone(), schema.clone());
                        // Flat and kind-named, not a `Create` submenu: the folder
                        // has already said which kind, so a submenu would open
                        // onto one live entry and two that belong to the folders
                        // either side of it. Same lower-case spelling as the
                        // object row's "Edit type".
                        entries.push(
                            MenuEntry::action(format!("Create {}", kind.label()), move || {
                                crate::object_editor::open_for_new(&ui, &db, ns.as_deref(), kind);
                            })
                            .disabled(conn_read_only(&connections, active_conn)),
                        );
                    }
                }
                CtxKind::Table {
                    database,
                    schema,
                    table,
                    ddl,
                } => {
                    let source = TableSource::new(database.clone(), schema.clone(), table.clone());
                    // `database.schema.table` on PostgreSQL outside `public`, so
                    // "Copy qualified name" names something that actually resolves.
                    let qualified = format!("{database}.{}", source.display());
                    let refresh_database = database.clone();
                    // "Open": focus the tab already showing this table, else open one.
                    {
                        let ot = open_table.clone();
                        let src = source.clone();
                        entries.push(MenuEntry::action("Open", move || (ot)(src.clone())));
                    }
                    // "Open in new tab" is only useful (and only shown) when a tab
                    // for this table is already open — otherwise it does exactly
                    // what "Open" does. Match connection too (H13): a same-named
                    // table under another connection isn't "this table".
                    let already_open = tabs.with_untracked(|v| {
                        v.iter().any(|t| {
                            t.source.get_untracked().as_ref() == Some(&source)
                                && t.conn_id.get_untracked() == active_conn.get_untracked()
                        })
                    });
                    if already_open {
                        let otn = open_table_new.clone();
                        let src = source.clone();
                        entries.push(MenuEntry::action("Open in new tab", move || {
                            (otn)(src.clone())
                        }));
                    }
                    // The read group opens here, and its first two entries are
                    // the same two every menu in this tree opens with. Flat, not
                    // a "Copy" submenu: two children behind a hover is a hover
                    // spent on the most-used entry in the menu.
                    entries.push(MenuEntry::Separator);
                    entries.push(MenuEntry::action("Copy name", copy(menu.name.clone())));
                    entries.push(MenuEntry::action("Copy qualified name", copy(qualified)));
                    // The full `TableInfo` (columns, nullability) that the
                    // schema-editing entries below map onto, and which only
                    // exists once the schema has loaded. Read here rather than
                    // there because Properties — the first entry to want it —
                    // sits above them.
                    let info = db_nodes.with_untracked(|nodes| {
                        nodes
                            .iter()
                            .find(|n| n.database == *database)
                            .and_then(|n| match n.schema.get_untracked() {
                                schemaic_core::schema::SchemaState::Loaded(db) => db
                                    .tables
                                    .iter()
                                    .find(|t| {
                                        t.name == *table && t.schema.as_deref() == schema.as_deref()
                                    })
                                    .cloned(),
                                _ => None,
                            })
                    });
                    let is_view = info.as_ref().is_some_and(|i| i.is_view);
                    // Sizes, row estimate and index usage — the one surface that
                    // reports them. Offered for a view too: it has no storage of
                    // its own, and the panel says so rather than the menu hiding
                    // the question.
                    {
                        let ui = import_ui.clone();
                        let (db, ns, tbl) = (database.clone(), schema.clone(), table.clone());
                        entries.push(MenuEntry::action("Properties", move || {
                            crate::properties::open_for_table(
                                &ui,
                                active_conn.get_untracked(),
                                &db,
                                ns.as_deref(),
                                &tbl,
                                is_view,
                            );
                        }));
                    }
                    // Watch this table for row changes — the same action the
                    // results toolbar offers, next to Properties there and here
                    // for the same reason: both report on the table as it is,
                    // rather than on its structure. Offered for a view too; the
                    // monitor says "No row key for this table" when it can't
                    // diff one, which is a better answer than a missing entry
                    // (see [`crate::grid::GridCtx::open_monitor`]).
                    {
                        let om = open_monitor.clone();
                        let src = source.clone();
                        entries.push(MenuEntry::action("Live monitor", move || {
                            (om)(active_conn.get_untracked(), src.clone());
                        }));
                    }
                    // ER diagram seeded on this table's FK neighbourhood.
                    {
                        let db = database.clone();
                        // The seed is a diagram *node id*, which is the display
                        // name — so a table outside `public` seeds `sales.orders`
                        // and can't be confused with a same-named one elsewhere.
                        let seed_id = source.display();
                        entries.push(MenuEntry::action("Show diagram", move || {
                            erd.set(Some(crate::ErdTarget {
                                conn_id: active_conn.get_untracked(),
                                database: db.clone(),
                                seed: schemaic_core::erd::DiagramSeed::Table(seed_id.clone()),
                            }));
                        }));
                    }
                    // ── Generate ─────────────────────────────────────────────
                    // Each entry copies its statement *and* opens it in a query
                    // tab, which is what this entry has always done.
                    //
                    // A **submenu only where there is more than one thing to
                    // generate**: a table has four, and four more flat entries in
                    // a menu this long is worse than one hover. A view has one —
                    // its `CREATE` — and a lone child behind a hover is a hover
                    // spent on nothing, the same rule that keeps the two Copy
                    // entries flat above.
                    //
                    // The DML three are **drafts, not statements**:
                    // `core::skeleton` writes named placeholders, so one run by
                    // reflex fails to parse instead of writing empty rows — and
                    // its `WHERE` is `browse_key_columns`, the key the grid's
                    // write-back addresses a row with, rather than a second
                    // opinion about what identifies a row.
                    let generate = {
                        let oq = open_query.clone();
                        let db = database.clone();
                        move |sql: String| {
                            let oq = oq.clone();
                            let db = db.clone();
                            move || {
                                let _ = floem::Clipboard::set_contents(sql.clone());
                                // The tab is bound to *this* table's database,
                                // not to wherever a new tab would have started.
                                (oq)(sql.clone(), Some(db.clone()));
                            }
                        }
                    };
                    match info.as_ref().filter(|_| !is_view) {
                        Some(t) => {
                            // Built into a binding first, like the Colour swatches
                            // — a child constructor within three lines of the
                            // `entries.push(` reads as a top-level entry to
                            // `menu_order_gate`, which would then demand a place
                            // in the skeleton for a label that isn't in the menu.
                            let dialect = crate::table_designer::edit_ctx(&import_ui).dialect;
                            let db = database.as_str();
                            let items = vec![
                                MenuEntry::action("Create", generate(ddl.clone())),
                                MenuEntry::action(
                                    "Insert",
                                    generate(insert_skeleton(dialect, db, t)),
                                ),
                                MenuEntry::action(
                                    "Update",
                                    generate(update_skeleton(dialect, db, t)),
                                ),
                                MenuEntry::action(
                                    "Delete",
                                    generate(delete_skeleton(dialect, db, t)),
                                ),
                            ];
                            entries.push(MenuEntry::sub("Generate", items));
                        }
                        // A view, or a table whose schema hasn't loaded — the
                        // DDL is in hand either way, and nothing else is.
                        None => entries.push(MenuEntry::action("Generate DDL", generate(ddl))),
                    }
                    // Refresh closes the read group in every menu in this tree.
                    // It used to sit between Copy and Properties, splitting the
                    // three entries that show you something — and it doesn't act
                    // on the table at all: a table is introspected as part of
                    // its database, so this targets the database.
                    let rf = refresh_db.clone();
                    entries.push(MenuEntry::action("Refresh", move || {
                        (rf)(refresh_database.clone())
                    }));
                    // ── How this table *looks* in the tree ────────────────────
                    // The database menu's colour group, one level down: preset
                    // swatches + None, stored per (connection, database, display
                    // name) and shown as a dot on this row — and, because the ER
                    // diagram is the one surface with room for a fill, as a tint on
                    // this table's card header there.
                    entries.push(MenuEntry::Separator);
                    {
                        // The *display* name, matching what `TableColorRule` keys
                        // on and what an ERD node id is: `sales.orders` outside
                        // `public`, so two namespaces' `orders` colour separately.
                        let key = source.display();
                        let mut swatches: Vec<MenuEntry> = crate::CONN_COLOR_PRESETS
                            .iter()
                            .map(|(name, hex, cfn)| {
                                let tc = table_colors;
                                let save = save_db_colors.clone();
                                let db = database.clone();
                                let tbl = key.clone();
                                let hex = hex.to_string();
                                MenuEntry::action_icon(*name, (icons::DOT, *cfn), move || {
                                    let cid = active_conn.get_untracked();
                                    tc.update(|r| {
                                        schemaic_core::db_color::table_upsert(
                                            r,
                                            cid,
                                            &db,
                                            &tbl,
                                            Some(hex.clone()),
                                        )
                                    });
                                    (save)();
                                })
                            })
                            .collect();
                        swatches.push(MenuEntry::Separator);
                        {
                            let tc = table_colors;
                            let save = save_db_colors.clone();
                            let db = database.clone();
                            let tbl = key.clone();
                            swatches.push(MenuEntry::action("None", move || {
                                let cid = active_conn.get_untracked();
                                tc.update(|r| {
                                    schemaic_core::db_color::table_upsert(r, cid, &db, &tbl, None)
                                });
                                (save)();
                            }));
                        }
                        entries.push(MenuEntry::sub("Colour", swatches));
                    }
                    // Its own group, just above AI Explain: everything that
                    // *writes* — import and schema editing — reads as one set
                    // rather than trailing off the end of the read-only ones,
                    // with the two that can't be taken back last inside it.
                    entries.push(MenuEntry::Separator);
                    {
                        let read_only = conn_read_only(&connections, active_conn);
                        let ui = import_ui.clone();
                        let db = database.clone();
                        let ns = schema.clone();
                        let has_columns = info.as_ref().is_some_and(|i| !i.columns.is_empty());
                        let dialect = crate::table_designer::edit_ctx(&ui).dialect;
                        // A view Schemaic can edit — read before `info` is moved
                        // into the Import entry below.
                        let editable_view = crate::view_editor::is_editable_view(info.as_ref());
                        let materialized = is_view && !editable_view;
                        // Read here for the same reason `editable_view` is: the
                        // Import entry below moves `info` into its closure.
                        let triggers = info
                            .as_ref()
                            .map(|i| i.triggers.clone())
                            .unwrap_or_default();
                        // What a view cannot have at all is **absent**, not
                        // dimmed — see `object_entries`, which owns that split
                        // and is where the PostgreSQL-view-has-triggers case is
                        // stated.
                        let offers = object_entries(is_view, dialect, materialized);
                        if offers.import {
                            entries.push(
                                MenuEntry::action("Import", move || {
                                    if let Some(info) = info.clone() {
                                        crate::import_view::open_import(
                                            &ui,
                                            crate::ImportTargetInfo {
                                                conn_id: active_conn.get_untracked(),
                                                database: db.clone(),
                                                schema: ns.clone(),
                                                table: info,
                                            },
                                        );
                                    }
                                })
                                // A table whose schema hasn't loaded has no
                                // columns to map onto.
                                .disabled(read_only || !has_columns),
                            );
                        }

                        // ── Schema editing ────────────────────────────────────
                        // Everything here ends at the DDL preview; nothing runs
                        // a statement from the menu. Absent entirely where the
                        // engine can't express the edit (`object_entries`, which
                        // asks `ddl::supports_table_design` and its siblings)
                        // rather than dimmed: "not supported" and "not here" are
                        // different answers, and dimming gives the second one.
                        // All three engines can, SQLite by rebuilding the table.
                        if offers.edit {
                            let ui = import_ui.clone();
                            let (db, ns, tbl) = (database.clone(), schema.clone(), table.clone());
                            // Same entry, two editors: a view is a name and a
                            // SELECT, so it gets its own modal rather than the
                            // designer's list-plus-form. The label says which,
                            // since the two rows look alike in the tree.
                            entries.push(
                                MenuEntry::action(
                                    if is_view { "Edit view" } else { "Edit table" },
                                    move || {
                                        if is_view {
                                            crate::view_editor::open_for_view(
                                                &ui,
                                                &db,
                                                ns.as_deref(),
                                                &tbl,
                                            );
                                        } else {
                                            crate::table_designer::open_for_table(
                                                &ui,
                                                &db,
                                                ns.as_deref(),
                                                &tbl,
                                                crate::table_designer::DesignerFocus::Table,
                                            );
                                        }
                                    },
                                )
                                // An unloaded schema has nothing to edit from,
                                // and a materialized view has no
                                // `CREATE OR REPLACE` to edit it with.
                                .disabled(if is_view {
                                    !editable_view
                                } else {
                                    !has_columns
                                }),
                            );
                        }
                        // Triggers: one entry opening a modal over the table's
                        // whole set — list on the left, the selected trigger's
                        // form on the right. Its own plan rather than a designer
                        // tab, because a trigger needs its own statement and
                        // can't join the table's coalesced `ALTER TABLE`.
                        if offers.triggers {
                            let ui = import_ui.clone();
                            let (db, ns, tbl) = (database.clone(), schema.clone(), table.clone());
                            let n = triggers.len();
                            entries.push(
                                MenuEntry::action(
                                    // No ellipsis: every entry in this menu opens
                                    // something, so it says nothing, and the count
                                    // is the useful half. Bare when there are none
                                    // — "(0)" reads as a broken count.
                                    if n == 0 {
                                        "Triggers".to_string()
                                    } else {
                                        format!("Triggers ({n})")
                                    },
                                    move || {
                                        crate::trigger_editor::open_for_table(
                                            &ui,
                                            &db,
                                            ns.as_deref(),
                                            &tbl,
                                        )
                                    },
                                )
                                // An unloaded schema has no trigger list to show.
                                // Which *objects* can have triggers at all is
                                // `view_has_no_triggers` above, and decides
                                // whether this entry exists rather than dimming it.
                                .disabled(!has_columns),
                            );
                        }
                        // Truncate and drop are the two that can't be taken back
                        // and sit next to harmless entries, so they ask first and
                        // *then* show the plan. Everything else relies on the
                        // preview alone, which already names the consequence.
                        //
                        // Drop applies to a view; Truncate does not — a view owns
                        // no rows to delete — so only the second is conditional.
                        //
                        // Both name the *scale* of what goes when a row figure is
                        // already in hand: "Delete all ~4.2m rows in orders?" is
                        // a different question from "delete every row", and the
                        // difference is the one the user wants before clicking.
                        // Read from the tree's statistics cache and never
                        // fetched — this menu is built on the right-click, so a
                        // round trip here would either block it or land after the
                        // modal is already up. Whether a figure in hand is worth
                        // naming at all is `stats::truncate_prompt`'s decision.
                        let rows = crate::db_stats_slot(db_nodes, &database).and_then(|slot| {
                            slot.with_untracked(|st| match st {
                                crate::DbStatsState::Loaded(set) => set
                                    .get(schema.as_deref(), &table)
                                    .and_then(schemaic_core::stats::TableStats::row_count),
                                _ => None,
                            })
                        });
                        if offers.truncate {
                            let ui = import_ui.clone();
                            let confirm = ui.overlay.confirm;
                            let (db, ns, tbl) = (database.clone(), schema.clone(), table.clone());
                            let label = source.display();
                            entries.push(
                                MenuEntry::action_colored("Truncate", theme::error, move || {
                                    let (ui, db, ns, tbl) =
                                        (ui.clone(), db.clone(), ns.clone(), tbl.clone());
                                    confirm.set(Some(crate::Confirm {
                                        title: "Truncate table".to_string(),
                                        message: schemaic_core::stats::truncate_prompt(
                                            &label, rows,
                                        ),
                                        resolve: Rc::new(move |yes| {
                                            if yes {
                                                crate::ddl_preview::preview_change(
                                                    &ui,
                                                    &db,
                                                    &tbl,
                                                    ns.as_deref(),
                                                    schemaic_core::ddl::Change::TruncateTable,
                                                );
                                            }
                                        }),
                                    }));
                                })
                                .disabled(read_only),
                            );
                        }
                        {
                            let ui = import_ui.clone();
                            let confirm = ui.overlay.confirm;
                            let (db, ns, tbl) = (database.clone(), schema.clone(), table.clone());
                            let label = source.display();
                            entries.push(
                                MenuEntry::action_colored("Drop", theme::error, move || {
                                    let (ui, db, ns, tbl) =
                                        (ui.clone(), db.clone(), ns.clone(), tbl.clone());
                                    // A view is dropped by `DROP VIEW`; asking
                                    // about "every row in it" would be asking
                                    // about rows it doesn't own either — which is
                                    // also why `drop_prompt` never gives one a
                                    // row figure.
                                    let title = if is_view { "Drop view" } else { "Drop table" };
                                    confirm.set(Some(crate::Confirm {
                                        title: title.to_string(),
                                        message: schemaic_core::stats::drop_prompt(
                                            &label, rows, is_view,
                                        ),
                                        resolve: Rc::new(move |yes| {
                                            if yes {
                                                crate::ddl_preview::preview_change(
                                                    &ui,
                                                    &db,
                                                    &tbl,
                                                    ns.as_deref(),
                                                    if is_view {
                                                        schemaic_core::ddl::Change::DropView {
                                                            materialized,
                                                        }
                                                    } else {
                                                        schemaic_core::ddl::Change::DropTable
                                                    },
                                                );
                                            }
                                        }),
                                    }));
                                })
                                .disabled(read_only),
                            );
                        }
                    }
                }
                CtxKind::Field { source, column } => {
                    entries.push(MenuEntry::action("Copy name", copy(menu.name.clone())));
                    // `database.schema.table.column` — the one qualification a
                    // column has, and the shape you paste into a query. Built
                    // the way the table entry builds its own.
                    entries.push(MenuEntry::action(
                        "Copy qualified name",
                        copy(format!(
                            "{}.{}.{}",
                            source.database,
                            source.display(),
                            column
                        )),
                    ));
                    let read_only = conn_read_only(&connections, active_conn);
                    entries.push(MenuEntry::Separator);
                    let offers = field_entries(
                        crate::table_designer::edit_ctx(&import_ui).dialect,
                        source_is_view(db_nodes, &source),
                    );
                    if offers.edit {
                        let ui = import_ui.clone();
                        let (src, col) = (source.clone(), column.clone());
                        entries.push(MenuEntry::action("Edit column", move || {
                            crate::table_designer::open_for_table(
                                &ui,
                                &src.database,
                                src.schema.as_deref(),
                                &src.table,
                                crate::table_designer::DesignerFocus::Column(&col),
                            );
                        }));
                    }
                    if offers.drop {
                        let ui = import_ui.clone();
                        let (src, col) = (source.clone(), column.clone());
                        // Through the draft, not as a lone `DropColumn`: the
                        // index over the column and any foreign key standing on
                        // it have to come off first or the server refuses it.
                        //
                        // Red and last, as every other Drop in this tree is —
                        // this one used to be the same colour as Edit column
                        // directly above it.
                        entries.push(
                            MenuEntry::action_colored("Drop", theme::error, move || {
                                let col = col.clone();
                                crate::table_designer::preview_draft_edit(
                                    &ui,
                                    &src.database,
                                    src.schema.as_deref(),
                                    &src.table,
                                    move |d| {
                                        if let Some(i) =
                                            d.columns.iter().position(|c| c.info.name == col)
                                        {
                                            d.remove_column(i);
                                        }
                                    },
                                );
                            })
                            .disabled(read_only),
                        );
                    }
                }
                CtxKind::Key {
                    source,
                    index,
                    foreign_key,
                } => {
                    entries.push(MenuEntry::action("Copy name", copy(menu.name.clone())));
                    let read_only = conn_read_only(&connections, active_conn);
                    entries.push(MenuEntry::Separator);
                    let offers = key_entries(
                        crate::table_designer::edit_ctx(&import_ui).dialect,
                        index.constraint.as_deref(),
                        source_is_view(db_nodes, &source),
                    );
                    if offers.edit {
                        let ui = import_ui.clone();
                        let src = source.clone();
                        let ix = index.clone();
                        let fk = foreign_key.clone();
                        // Named for what the row *is*, the way the object menu's
                        // "Edit sequence" is — and it lands there: the designer
                        // opens on the Indexes or Foreign keys section with this
                        // entry selected, rather than on the table summary with
                        // the user to find the row again. A PRIMARY row names
                        // the primary key and lands on its first column, which
                        // is where the key is actually edited (the index form's
                        // own hint says so).
                        let label = if fk.is_some() {
                            "Edit foreign key"
                        } else if ix.is_primary() {
                            "Edit primary key"
                        } else {
                            "Edit index"
                        };
                        entries.push(MenuEntry::action(label, move || {
                            crate::table_designer::open_for_table(
                                &ui,
                                &src.database,
                                src.schema.as_deref(),
                                &src.table,
                                crate::table_designer::DesignerFocus::Key {
                                    index: &ix,
                                    foreign_key: fk.as_deref(),
                                },
                            );
                        }));
                    }
                    // A foreign key's backing index can't be dropped while the
                    // constraint stands, so the entry offers the constraint —
                    // which is what the row is really showing.
                    //
                    // Last in the group and red, as every other Drop in this
                    // tree is. This menu used to put it *first* under the
                    // separator with Edit table below it — the one place where
                    // the row your cursor lands on after a right-click was the
                    // irreversible one.
                    match foreign_key {
                        Some(name) if offers.drop_foreign_key => {
                            let ui = import_ui.clone();
                            let src = source.clone();
                            entries.push(
                                MenuEntry::action_colored(
                                    "Drop foreign key",
                                    theme::error,
                                    move || {
                                        crate::ddl_preview::preview_change(
                                            &ui,
                                            &src.database,
                                            &src.table,
                                            src.schema.as_deref(),
                                            schemaic_core::ddl::Change::DropForeignKey {
                                                name: name.clone(),
                                            },
                                        );
                                    },
                                )
                                .disabled(read_only),
                            );
                        }
                        // The primary key isn't dropped on its own — you replace
                        // it, which is a designer edit.
                        None if !index.is_primary() && offers.drop_index => {
                            let ui = import_ui.clone();
                            let src = source.clone();
                            let ix = index.clone();
                            entries.push(
                                MenuEntry::action_colored("Drop index", theme::error, move || {
                                    crate::ddl_preview::preview_change(
                                        &ui,
                                        &src.database,
                                        &src.table,
                                        src.schema.as_deref(),
                                        schemaic_core::ddl::Change::DropIndex {
                                            name: ix.name.clone(),
                                            constraint: ix.constraint.clone(),
                                        },
                                    );
                                })
                                .disabled(read_only),
                            );
                        }
                        _ => {}
                    }
                }
            }
            entries.push(MenuEntry::Separator);
            let ai = ai_send.clone();
            let prompt = menu.ai_prompt.clone();
            entries.push(MenuEntry::action_icon(
                "AI Explain",
                (icons::SPARKLES, theme::key_foreign),
                // Reveal, then send — as the palette's Ask AI and the grid's AI
                // Summary already did. Without it, with the right column showing
                // the Terminal or History (or closed), this sent the prompt into a
                // panel the user couldn't see and read as doing nothing at all.
                move || {
                    crate::reveal_ai_panel(right_panel);
                    (ai)(prompt.clone());
                },
            ));

            entries
        }
    });

    let render = build.clone();
    dyn_container(
        move || ctx.get(),
        move |menu| {
            let Some(menu) = menu else {
                return empty().into_any();
            };
            // Dismissal is a root-level pointer-down handler (see `workspace`); the
            // panel absorbs its own pointer-downs so it isn't closed mid-click.
            menu_panel((render)(menu), Rc::new(move || ctx.set(None)), CTX_MENU_W).into_any()
        },
    )
    // Open at the cursor, flipping to the other side of it at a window edge — the
    // same rule as the grid's menus. Without it, a right-click low in a full tree
    // ran Truncate, Drop and AI Explain off the bottom of the window, with no cue
    // that they were there.
    .style(move |s| {
        let Some(menu) = ctx.get() else {
            return s;
        };
        // `at` when the opener named a place (Shift+F10, which opens at the nav
        // cursor's row), else the pointer. Both go through the same edge-flip:
        // a row low in the tree runs its last entries off the bottom otherwise,
        // however the menu was raised.
        let from = menu.at.unwrap_or_else(|| last_mouse.get_untracked());
        let h = menu_panel_height(&(build)(menu));
        let (x, y) = cursor_menu_pos(from, (CTX_MENU_W, h), window_size().get(), CURSOR_MENU_GAP);
        s.absolute().inset_left(x).inset_top(y)
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
        // Estimated panel height, used to place an upward edge-flip so the panel's
        // bottom lands just above the cursor. Sum per entry kind — an action row is
        // ≈30.5px (14px line + 8px padding both sides − sub-pixel), a separator is
        // ≈9px (a 1px rule + 4px margins both sides) — because counting separators
        // as full rows shoved the flipped panel tens of px too high. `+14` = the
        // panel's 6px vertical padding (both sides) + 1px border (both sides). These
        // are placement estimates, not the flip *decision*, so being close matters.
        let Some(ph) = popup.with(|p| p.as_ref().map(|e| menu_panel_height(e))) else {
            return s;
        };
        let (ww, wh) = window_size().get();
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
            // Cursor menus (right-click): open at the pointer, flipping to the
            // other side of it at either edge — the shared rule, which the schema
            // tree's menu now uses too.
            None => {
                let (x, y) = cursor_menu_pos(
                    last_mouse.get_untracked(),
                    (pw, ph),
                    (ww, wh),
                    CURSOR_MENU_GAP,
                );
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
    /// A schema-style leading icon (table/column search hits); `None` for command
    /// rows, which show no icon.
    icon: Option<ResultIcon>,
    /// A trailing (right-aligned) icon glyph — the rotate-ccw-clock on search-history
    /// rows; `None` for everything else.
    right_icon: Option<&'static str>,
    /// The keyboard shortcut for this row's action, shown as a keycap at the far
    /// right — `None` when the action has no binding, which is most rows.
    ///
    /// Comes from `shortcuts::command_keys`, so it can only ever be a binding the
    /// Shortcuts modal also documents. This is where people look for a key: the
    /// palette is what they open when they can't remember one.
    ///
    /// **Set it on every row whose `primary` is the command's own label**, which
    /// means all four states an argument-command passes through as you type: the
    /// command list, the just-completed row, a valid argument, and the "enter a
    /// number" hint. Wiring only the first two made the keycap vanish the moment
    /// Tab *selected* the command — the keystroke right after the one that
    /// revealed the binding, and while the user was still reading it.
    ///
    /// The exception is an **option** row (`CmdArg::Options`), whose `primary` is
    /// the option's label — "One Dark Pro", not "Editor Theme" — and which
    /// carries none.
    ///
    /// Not because no key runs one: three of Toggle Panel's four options are
    /// byte-for-byte the Ctrl+Shift+E / Ctrl+Shift+A / Ctrl+` handlers. It is
    /// because the row's *own* label is not the command's, so a keycap on it
    /// would read as belonging to "One Dark Pro" rather than to the command it
    /// is a choice of — and there is nowhere on the row to say which. The keycap
    /// belongs where the command's name is.
    keys: Option<&'static str>,
}

/// The schema-style leading icon for a Find-Anywhere hit — mirrors the schema
/// tree: a table/view glyph in its icon colour, or a column's type-family glyph
/// tinted by its key role (PK / FK / plain) at half alpha. The row's text keeps
/// its normal colour.
#[derive(Clone, Copy)]
enum ResultIcon {
    Table,
    View,
    Column(schemaic_core::schema::ColumnTypeClass, ColKeyRole),
    /// A PostgreSQL standalone object — enum / domain / sequence. Same glyph and
    /// same muted tint the schema tree gives its row, so the thing you found in
    /// one surface is recognisable in the other.
    Object(schemaic_core::ddl::ObjectKind),
}

#[derive(Clone, Copy)]
enum ColKeyRole {
    Primary,
    Foreign,
    Plain,
}

impl ResultIcon {
    fn glyph(self) -> &'static str {
        match self {
            ResultIcon::Table => icons::TABLE,
            ResultIcon::View => icons::TABLE_CELLS_MERGE,
            ResultIcon::Column(class, _) => crate::schema_tree::column_type_icon(class),
            ResultIcon::Object(kind) => crate::schema_tree::object_icon(kind),
        }
    }
    fn color(self) -> floem::peniko::Color {
        match self {
            ResultIcon::Table => theme::table_icon(),
            ResultIcon::View => theme::view_icon(),
            // The tree's object rows, which are muted at the same alpha.
            ResultIcon::Object(_) => theme::text_muted().multiply_alpha(0.7),
            ResultIcon::Column(_, role) => {
                let base = match role {
                    ColKeyRole::Primary => theme::key_primary(),
                    ColKeyRole::Foreign => theme::key_foreign(),
                    ColKeyRole::Plain => theme::text(),
                };
                base.multiply_alpha(0.5)
            }
        }
    }
}

/// The result icon for a table/view (schema-tree parity).
fn table_result_icon(t: &schemaic_core::schema::TableInfo) -> ResultIcon {
    if t.is_view {
        ResultIcon::View
    } else {
        ResultIcon::Table
    }
}

/// The result icon for a column: its type family, tinted by key role (PK / FK /
/// plain) — mirrors the schema tree.
fn column_result_icon(
    t: &schemaic_core::schema::TableInfo,
    c: &schemaic_core::schema::ColumnInfo,
) -> ResultIcon {
    let role = if c.primary_key {
        ColKeyRole::Primary
    } else if t.fk_for_column(&c.name).is_some() {
        ColKeyRole::Foreign
    } else {
        ColKeyRole::Plain
    };
    ResultIcon::Column(
        schemaic_core::schema::classify_column_type(&c.type_name),
        role,
    )
}

/// May this object be a Find-Anywhere result at all?
///
/// The **one** gate, asked by both producers — a fresh search ([`schema_hits`])
/// and a remembered one ([`lookup_object`]) — because a palette row's whole
/// activation is `open_for_object`, and that is guarded by `is_editable_object`
/// at every schema-tree call site. Asking the editor's own predicate rather than
/// re-spelling it as `!is_internal()` is what keeps the two from drifting if
/// another kind of object ever becomes non-editable.
///
/// It has to be asked on the history path too, not just on the search that
/// created the entry: a `serial`'s sequence is an ordinary object and a fine
/// result, but migrating its column to an identity column makes that same
/// sequence internal, and the remembered row would otherwise open an editor whose
/// only irreversible action the server refuses.
fn is_palette_target(o: &schemaic_core::schema::ObjectItem) -> bool {
    crate::object_editor::is_editable_object(o)
}

/// Is the object a **view**, as far as the loaded schema knows?
///
/// `false` when the schema hasn't loaded, which is the same answer the table
/// arm's own `is_view` gives in that state: nothing is known to be a view, and
/// the entries that follow are already disabled for want of columns.
fn source_is_view(
    db_nodes: RwSignal<Vec<ConnNode>>,
    source: &schemaic_core::schema::TableSource,
) -> bool {
    db_nodes.with_untracked(|nodes| {
        nodes
            .iter()
            .find(|n| n.database == source.database)
            .and_then(|n| match n.schema.get_untracked() {
                SchemaState::Loaded(db) => db
                    .tables
                    .iter()
                    .find(|t| {
                        t.name == source.table && t.schema.as_deref() == source.schema.as_deref()
                    })
                    .map(|t| t.is_view),
                _ => None,
            })
            .unwrap_or(false)
    })
}

/// Resolve a remembered object (a search-history entry) against the live schema.
/// `None` if the database isn't loaded, the object is gone, or it is no longer
/// something the palette may offer — a history row that resolves to nothing is
/// dropped rather than shown, since its whole activation is "open the editor on
/// *this* object".
fn lookup_object(
    db_nodes: RwSignal<Vec<ConnNode>>,
    database: &str,
    schema: Option<&str>,
    kind: schemaic_core::ddl::ObjectKind,
    name: &str,
) -> Option<schemaic_core::schema::ObjectItem> {
    db_nodes.with_untracked(|nodes| {
        let node = nodes.iter().find(|n| n.database == database)?;
        let SchemaState::Loaded(s) = node.schema.get_untracked() else {
            return None;
        };
        palette_object(&s, schema, kind, name)
    })
}

/// The palette's answer for one object: it, or `None` when the database has no
/// such object **or** it isn't a destination.
///
/// Split out from [`lookup_object`] purely so the gate has a test. Deleting the
/// `is_palette_target` filter — which is what this whole rule exists to keep —
/// left `cargo test --workspace` green: the regression tests called the
/// predicate directly, and nothing reached the site that applies it.
///
/// The **search-history** path is where it matters. A `serial`'s sequence is an
/// ordinary object and a legitimate result, so it can be remembered; migrating
/// its column to an identity column makes that same sequence *internal*, and
/// only this second gate stops the remembered row from opening an editor the
/// server would refuse.
fn palette_object(
    s: &schemaic_core::schema::DbSchema,
    schema: Option<&str>,
    kind: schemaic_core::ddl::ObjectKind,
    name: &str,
) -> Option<schemaic_core::schema::ObjectItem> {
    s.find_object(schema, kind, name).filter(is_palette_target)
}

/// Build one object result row: records the activation to search history, then
/// opens the object editor — the same action the schema tree's row takes on Enter
/// or a double-click.
#[allow(clippy::too_many_arguments)]
fn object_result_item(
    database: String,
    item: schemaic_core::schema::ObjectItem,
    history: bool,
    match_term: Option<String>,
    active_conn: RwSignal<u64>,
    search_history: RwSignal<Vec<schemaic_core::search_history::SearchEntry>>,
    open_object: Rc<dyn Fn(String, schemaic_core::schema::ObjectItem)>,
    close: Rc<dyn Fn()>,
) -> PaletteItem {
    let kind = item.kind();
    // Qualified outside `public`, exactly as a table hit is, so two same-named
    // types in two namespaces are distinguishable in the list.
    let primary = schemaic_core::schema::display_name(item.schema(), item.name());
    // The kind rides in the secondary line because a type and a table may share a
    // name — without it the two rows would read identically.
    let secondary = format!("{database} · {}", kind.label());
    let name = item.name().to_string();
    let schema = item.schema().map(str::to_string);
    // Tab-completion inserts the bare name — the qualifier isn't what you're
    // typing to search for, the same call the table rows make.
    let complete = name.clone();
    PaletteItem {
        primary,
        secondary,
        activate: Rc::new(move || {
            search_history.update(|v| {
                schemaic_core::search_history::push(
                    v,
                    schemaic_core::search_history::SearchEntry {
                        conn_id: active_conn.get_untracked(),
                        database: database.clone(),
                        schema: schema.clone(),
                        table: name.clone(),
                        column: None,
                        object: Some(schemaic_core::search_history::ObjectTag::of(kind)),
                    },
                );
            });
            (open_object)(database.clone(), item.clone());
            (close)();
        }),
        complete: Some(complete),
        match_term,
        icon: Some(ResultIcon::Object(kind)),
        right_icon: history.then_some(icons::ROTATE_CCW_CLOCK),
        // A schema hit is a place, not an action — nothing binds a key to it.
        keys: None,
    }
}

/// Re-derive a history entry's icon from the live schema (its type/key info isn't
/// persisted). `None` if the schema isn't loaded or the table/column is gone.
fn lookup_result_icon(
    db_nodes: RwSignal<Vec<ConnNode>>,
    source: &TableSource,
    column: Option<&str>,
) -> Option<ResultIcon> {
    db_nodes.with_untracked(|nodes| {
        let node = nodes.iter().find(|n| n.database == source.database)?;
        let SchemaState::Loaded(schema) = node.schema.get_untracked() else {
            return None;
        };
        let t = schema.find_table(source.schema.as_deref(), &source.table)?;
        match column {
            None => Some(table_result_icon(t)),
            Some(col) => t
                .columns
                .iter()
                .find(|c| c.name == col)
                .map(|c| column_result_icon(t, c)),
        }
    })
}

/// Build one search/history result row: records the activation to search history
/// (bubbling it to the top), then opens the table — selecting + scrolling to the
/// column for a column hit (same as a schema-tree column double-click). `history`
/// adds the trailing rotate-ccw-clock marker.
#[allow(clippy::too_many_arguments)]
fn search_result_item(
    source: TableSource,
    column: Option<String>,
    left_icon: Option<ResultIcon>,
    history: bool,
    match_term: Option<String>,
    active_conn: RwSignal<u64>,
    search_history: RwSignal<Vec<schemaic_core::search_history::SearchEntry>>,
    open_table: Rc<dyn Fn(TableSource)>,
    open_table_col: Rc<dyn Fn(TableSource, String)>,
    close: Rc<dyn Fn()>,
) -> PaletteItem {
    // The table reads `sales.orders` outside `public`, so two same-named tables
    // in one database are distinguishable in the result list.
    let table = source.display();
    let primary = match &column {
        Some(c) => format!("{table}.{c}"),
        None => table.clone(),
    };
    // Tab-completion inserts the bare name — the qualifier isn't what you're
    // typing to search for.
    let complete = column.clone().unwrap_or_else(|| source.table.clone());
    PaletteItem {
        primary,
        secondary: source.database.clone(),
        activate: Rc::new(move || {
            search_history.update(|v| {
                schemaic_core::search_history::push(
                    v,
                    schemaic_core::search_history::SearchEntry {
                        conn_id: active_conn.get_untracked(),
                        database: source.database.clone(),
                        schema: source.schema.clone(),
                        table: source.table.clone(),
                        column: column.clone(),
                        object: None,
                    },
                );
            });
            match &column {
                Some(c) => (open_table_col)(source.clone(), c.clone()),
                None => (open_table)(source.clone()),
            }
            (close)();
        }),
        complete: Some(complete),
        match_term,
        icon: left_icon,
        right_icon: history.then_some(icons::ROTATE_CCW_CLOCK),
        // A schema hit is a place, not an action — nothing binds a key to it.
        keys: None,
    }
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
///
/// **`name` must be `label` lowercased**, because `name` is not an internal id —
/// Tab completion types it into the box the user is looking at. "Ask AI" that
/// completed to `>ai` read as the palette having chosen a different command;
/// [`assert_names_match_labels`] holds every entry to it.
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

/// Enforce [`Command`]'s name/label rule over a built registry.
///
/// A `debug_assert` rather than a unit test because the registry can only be
/// built from a live [`Ui`]: it fires the first time the palette opens on a dev
/// build, which is where a new command is written. Also checks the invariant
/// `schemaic_core::palette::parse` documents but cannot verify from inside — no
/// argument-command name may be a word-prefix of another, or the longer one
/// becomes unreachable.
///
/// And the half of the keycap mapping that no static test can reach:
/// `shortcuts::COMMAND_KEYS` is checked against `SHORTCUTS` in that module, but
/// only a built registry can say whether its *names* still name anything. A
/// renamed command would otherwise drop its keycap in silence.
fn assert_names_match_labels(cmds: &[Command]) {
    for (name, _, keys) in crate::shortcuts::COMMAND_KEYS {
        debug_assert!(
            cmds.iter().any(|c| c.name == *name),
            "shortcuts::COMMAND_KEYS maps {name:?} to {keys:?}, but no palette \
             command is called that — it was renamed, and the keycap silently \
             stopped showing"
        );
    }
    for c in cmds {
        debug_assert_eq!(
            c.name,
            c.label.to_lowercase(),
            "palette command name must be its label lowercased — Tab types it in front of the user"
        );
    }
    for a in cmds.iter().filter(|c| c.takes_arg()) {
        for b in cmds.iter().filter(|c| c.takes_arg()) {
            debug_assert!(
                std::ptr::eq(a, b) || !b.name.starts_with(&format!("{} ", a.name)),
                "argument-command `{}` is a word-prefix of `{}`, which the parser can never reach",
                a.name,
                b.name
            );
        }
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
    let open_sql_file = ui.tab_actions.open_sql_file.clone();
    let save_sql_file = ui.tab_actions.save_sql_file.clone();
    let save_sql_file_as = ui.tab_actions.save_sql_file_as.clone();
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
        // The file group, right after the tab lifecycle it belongs to: a tab is
        // where a `.sql` file is opened into and saved from.
        Command {
            name: "open file",
            label: "Open File",
            hint: "a .sql script",
            arg: instant(open_sql_file.clone(), &close),
        },
        Command {
            name: "save file",
            label: "Save File",
            hint: "",
            arg: instant(
                {
                    let save = save_sql_file.clone();
                    Rc::new(move || {
                        if let Some(t) = active_tab() {
                            (save)(t.id);
                        }
                    })
                },
                &close,
            ),
        },
        Command {
            name: "save file as",
            label: "Save File As",
            hint: "",
            arg: instant(
                {
                    let save_as = save_sql_file_as.clone();
                    Rc::new(move || {
                        if let Some(t) = active_tab() {
                            (save_as)(t.id);
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
            arg: instant(
                Rc::new(move || cycle_tab(tabs, active, active_conn, 1)),
                &close,
            ),
        },
        Command {
            name: "previous tab",
            label: "Previous Tab",
            hint: "",
            arg: instant(
                Rc::new(move || cycle_tab(tabs, active, active_conn, -1)),
                &close,
            ),
        },
        Command {
            name: "format code",
            label: "Format Code",
            hint: "",
            // Ask the editor pane to do it, don't reformat the text here. This
            // command used to run `sqlfmt` itself and write the result into
            // `t.query` — where nothing ever read it, because the mounted editor
            // owns its document and the next keystroke pushed the unformatted
            // text back over the signal. `format_req` reaches the same
            // `format_editor` Ctrl+Alt+L uses, which lands one undoable edit and
            // keeps the caret. (It also takes the indent settings from the
            // editor's own theme globals, so nothing here needs them.)
            arg: instant(
                Rc::new(move || {
                    if let Some(t) = active_tab() {
                        t.format_req.set(true);
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
                        let dialect = connections
                            .with_untracked(|cs| {
                                cs.iter()
                                    .find(|c| c.id == t.conn_id.get_untracked())
                                    .map(|c| {
                                        schemaic_core::intel::SqlDialect::from_db_type(&c.db_type)
                                    })
                            })
                            .unwrap_or_default();
                        let stmts: Vec<String> = schemaic_core::sql::statement_ranges(&q, dialect)
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
                                    icon: None,
                                    right_icon: None,
                                    keys: None,
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
            name: "ask ai",
            label: "Ask AI",
            hint: "<prompt>",
            arg: CmdArg::Text({
                let close = close.clone();
                Rc::new(move |arg: &str| {
                    let arg = arg.trim();
                    if arg.is_empty() {
                        return vec![hint_item("Ask AI", "Type a prompt…", None)];
                    }
                    let prompt = arg.to_string();
                    let ai_send = ai_send.clone();
                    let close = close.clone();
                    vec![PaletteItem {
                        primary: "Ask AI".to_string(),
                        secondary: prompt.clone(),
                        activate: Rc::new(move || {
                            crate::reveal_ai_panel(right_panel);
                            (ai_send)(prompt.clone());
                            (close)();
                        }),
                        complete: None,
                        match_term: None,
                        icon: None,
                        right_icon: None,
                        keys: None,
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
                        return vec![hint_item("Run in Terminal", "Type a command…", None)];
                    }
                    let cmd = arg.to_string();
                    let term_input = term_input.clone();
                    let close = close.clone();
                    vec![PaletteItem {
                        primary: "Run in Terminal".to_string(),
                        secondary: cmd.clone(),
                        activate: Rc::new(move || {
                            crate::reveal_panel(right_panel, RightPanel::Terminal);
                            (term_input)(format!("{cmd}\r").into_bytes());
                            (close)();
                        }),
                        complete: None,
                        match_term: None,
                        icon: None,
                        right_icon: None,
                        keys: None,
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
/// Next/Previous Tab, wrapping within the *active connection's* tabs — the only
/// ones the strip is showing, so cycling can't land on an invisible tab.
fn cycle_tab(
    tabs: RwSignal<Vec<crate::Tab>>,
    active: RwSignal<usize>,
    active_conn: RwSignal<u64>,
    step: isize,
) {
    let refs: Vec<(usize, u64)> = tabs.with_untracked(|v| {
        v.iter()
            .map(|t| (t.id, t.conn_id.get_untracked()))
            .collect()
    });
    if let Some(next) = schemaic_core::tabsel::cycle(
        &refs,
        active_conn.get_untracked(),
        Some(active.get_untracked()),
        step,
    ) {
        active.set(next);
    }
}

/// A non-actionable informational row (Enter does nothing, palette stays open).
///
/// Takes `keys` because a hint is one of the states an argument-command's row
/// passes through while you type — see [`PaletteItem::keys`] on why the keycap
/// has to survive all of them.
fn hint_item(primary: &str, secondary: &str, keys: Option<&'static str>) -> PaletteItem {
    PaletteItem {
        keys,
        primary: primary.to_string(),
        secondary: secondary.to_string(),
        activate: Rc::new(|| {}),
        complete: None,
        match_term: None,
        icon: None,
        right_icon: None,
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
    active_conn: RwSignal<u64>,
    search_history: RwSignal<Vec<schemaic_core::search_history::SearchEntry>>,
    open_table: &Rc<dyn Fn(TableSource)>,
    open_table_col: &Rc<dyn Fn(TableSource, String)>,
    open_object: &Rc<dyn Fn(String, schemaic_core::schema::ObjectItem)>,
    close: &Rc<dyn Fn()>,
    query: RwSignal<String>,
    caret_end: RwSignal<u64>,
) -> Vec<PaletteItem> {
    use schemaic_core::palette::Parsed;
    match parsed {
        // Default table/column search: a table hit opens the table; a column hit
        // opens the table AND selects + scrolls to that column (same as a schema-tree
        // column double-click). Each row carries a schema-style icon. With an EMPTY
        // query we instead show this connection's recent activations (search history),
        // each marked with a trailing rotate-ccw-clock — they vanish once you type.
        Parsed::Search(q) => {
            let q = q.trim().to_lowercase();
            if q.is_empty() {
                let conn = active_conn.get_untracked();
                let recent = search_history
                    .with_untracked(|v| schemaic_core::search_history::recent(v, conn));
                return recent
                    .into_iter()
                    .filter_map(|e| {
                        // An object entry resolves against the live schema; one
                        // whose type has since been dropped (or whose kind this
                        // build doesn't know) is dropped from the list rather
                        // than offered as a row that opens nothing.
                        if let Some(tag) = e.object {
                            let item = lookup_object(
                                db_nodes,
                                &e.database,
                                e.schema.as_deref(),
                                tag.kind()?,
                                &e.table,
                            )?;
                            return Some(object_result_item(
                                e.database,
                                item,
                                true, // history row → trailing clock marker
                                None, // nothing to highlight (empty query)
                                active_conn,
                                search_history,
                                open_object.clone(),
                                close.clone(),
                            ));
                        }
                        let source = TableSource::new(e.database, e.schema, e.table);
                        let left = lookup_result_icon(db_nodes, &source, e.column.as_deref());
                        Some(search_result_item(
                            source,
                            e.column,
                            left,
                            true,
                            None,
                            active_conn,
                            search_history,
                            open_table.clone(),
                            open_table_col.clone(),
                            close.clone(),
                        ))
                    })
                    .collect();
            }
            find_matches(db_nodes, hidden, &q, 80)
                .into_iter()
                .map(|hit| match hit.target {
                    FindTarget::Table { source, column } => search_result_item(
                        source,
                        column,
                        Some(hit.icon),
                        false,
                        Some(q.clone()),
                        active_conn,
                        search_history,
                        open_table.clone(),
                        open_table_col.clone(),
                        close.clone(),
                    ),
                    FindTarget::Object { database, item } => object_result_item(
                        database,
                        item,
                        false,
                        Some(q.clone()),
                        active_conn,
                        search_history,
                        open_object.clone(),
                        close.clone(),
                    ),
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
                    // Tab/ghost target: the **label**, as shown on the row, plus a
                    // trailing space for argument-commands so the caret lands ready
                    // for the argument. Completing `c.name` instead put a string in
                    // the box that the row never showed. The parser lowercases
                    // before matching, so the typed label resolves to `c.name`
                    // regardless of case, and `ghost_suffix` already compares
                    // case-insensitively against an original-cased target.
                    let complete = if c.takes_arg() {
                        format!(">{} ", c.label)
                    } else {
                        format!(">{}", c.label)
                    };
                    let activate: Rc<dyn Fn()> = match &c.arg {
                        CmdArg::Instant(run) => run.clone(),
                        // Argument-command: transition into argument entry (same as
                        // accepting the completion) and move the caret to the end.
                        _ => {
                            // Reuses `complete`, so Enter types exactly what Tab
                            // would; the two can't drift apart.
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
                        icon: None,
                        right_icon: None,
                        // The command list is the one place a keycap earns its
                        // room: this is what people open when they can't
                        // remember the key.
                        keys: crate::shortcuts::command_keys(c.name),
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
                    icon: None,
                    right_icon: None,
                    keys: crate::shortcuts::command_keys(c.name),
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
                                primary: l.clone(),
                                secondary: String::new(),
                                activate: Rc::new(move || (run)(v2.clone())),
                                // Tab fills the argument with the option as *shown*,
                                // not the value behind it — that value is a config
                                // key or a row id, so Switch Connection completed a
                                // connection's name to a bare number and Editor
                                // Theme turned "One Dark Pro" into "one-dark-pro".
                                // Running still goes through this row's own
                                // `activate`, which holds the value, so nothing
                                // depends on the typed text round-tripping.
                                complete: Some(format!(">{} {}", c.label, l)),
                                match_term: Some(a.clone()),
                                icon: None,
                                right_icon: None,
                                keys: None,
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
                    // The keycap rides every state of this row. Tab-completing
                    // `>go` to `>Go to Line ` moves the row from the command list
                    // into this branch, and dropping it here made the binding
                    // blink out of existence on the keystroke that *selected* the
                    // command — which is exactly when it was being read.
                    let keys = crate::shortcuts::command_keys(c.name);
                    let t = arg.trim();
                    if t.is_empty() {
                        return match empty {
                            Some(e) => vec![PaletteItem {
                                primary: c.label.to_string(),
                                secondary: "↵".to_string(),
                                activate: e.clone(),
                                complete: None,
                                match_term: None,
                                icon: None,
                                right_icon: None,
                                keys,
                            }],
                            None => vec![hint_item(c.label, c.hint, keys)],
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
                                icon: None,
                                right_icon: None,
                                keys,
                            }]
                        }
                        Err(_) => vec![hint_item(c.label, "Enter a number", keys)],
                    }
                }
                CmdArg::Text(f) => f(&arg),
            }
        }
    }
}

// Find Anywhere / command palette. No `>` prefix → table/column search; a `>`
// prefix enters command mode (see `schemaic_core::palette` + `palette_commands`).
/// The dim tail of `complete` beyond what the user has typed — the inline ghost
/// showing where Tab would land — or `None` when there is nothing to show.
///
/// **Every slice here is taken at a real char boundary of `complete`, and that
/// is the whole point.** The previous version tested the prefix on the two
/// strings *lowercased* and then sliced the **original-cased** `complete` at the
/// typed query's byte length. `char::to_lowercase` is not length-preserving —
/// `İ` (U+0130) is 2 bytes and lowercases to `i` + U+0307, 3 bytes — so that
/// index need not be a boundary in `complete`, and `&str` indexing panics. A
/// table named `İzmir` plus the single typed letter `i` crashed the app.
///
/// So walk `complete`'s own boundaries and take the first whose lowercased head
/// covers the query. Case-insensitive, like the hit test it has to agree with.
fn ghost_suffix(complete: &str, query: &str) -> Option<String> {
    let q = query.to_lowercase();
    if q.is_empty() {
        return (!complete.is_empty()).then(|| complete.to_string());
    }
    complete
        .char_indices()
        .map(|(i, c)| i + c.len_utf8())
        .find(|&i| complete[..i].to_lowercase().starts_with(&q))
        // A completion equal to what's typed has no tail worth painting.
        .filter(|&i| i < complete.len())
        .map(|i| complete[i..].to_string())
}

pub(crate) fn find_overlay(ui: Ui) -> impl IntoView {
    let open = ui.overlay.find_open;
    let query = ui.overlay.find_query;
    let db_nodes = ui.schema.db_nodes;
    let hidden = ui.schema.hidden_dbs;
    let open_table = ui.tab_actions.open_table.clone();
    let open_table_col = ui.tab_actions.open_table_col.clone();
    let active_conn = ui.conn.active_conn;
    let search_history = ui.overlay.search_history;
    let ui_reg = ui.clone(); // for building the command registry per open
    // Activating a type / domain / sequence opens the object editor — the same
    // action its schema-tree row takes. Built here rather than on an actions
    // bundle because the editor lives in this crate; the palette needs no help
    // from the app to reach it.
    let open_object: Rc<dyn Fn(String, schemaic_core::schema::ObjectItem)> = {
        let ui = ui.clone();
        Rc::new(move |database, item| {
            crate::object_editor::open_for_object(&ui, &database, &item);
        })
    };

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
            assert_names_match_labels(&commands);
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
                let open_table_col = open_table_col.clone();
                let open_object = open_object.clone();
                let close = close.clone();
                create_effect(move |prev: Option<String>| {
                    let raw = query.get();
                    // Everything the results are built from, tracked. `build_items`
                    // reads all of this *untracked* underneath, so the effect used
                    // to re-run on a keystroke and nothing else: searching while the
                    // per-database schemas were still landing (they arrive one
                    // signal at a time, seconds apart) said "Nothing found" and
                    // stayed there until the user typed another character. It costs
                    // nothing per keystroke — it adds exactly the missing re-run.
                    db_nodes.with(|nodes| nodes.iter().for_each(|n| n.schema.track()));
                    hidden.track();
                    active_conn.track();
                    let parsed = schemaic_core::palette::parse(&raw, &arg_names);
                    items.set(build_items(
                        parsed,
                        &commands,
                        db_nodes,
                        hidden,
                        active_conn,
                        search_history,
                        &open_table,
                        &open_table_col,
                        &open_object,
                        &close,
                        query,
                        caret_end,
                    ));
                    // Reset the cursor to the top for a new *query* only — a schema
                    // landing now re-runs this too, and it must not yank the
                    // selection out from under someone mid-arrow-key.
                    if prev.as_deref() != Some(raw.as_str()) {
                        selected.set(0);
                    }
                    raw
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
                        v.get(sel)
                            .and_then(|it| it.complete.clone())
                            .and_then(|c| ghost_suffix(&c, &q))
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

            // Suggestions: live search results while typing, or this connection's
            // recent history when the query is empty. Empty query AND no history →
            // just the box; a typed query with no matches → "Nothing found".
            let results = dyn_container(
                move || items.get(),
                move |list| {
                    if list.is_empty() {
                        if query.with_untracked(|q| q.is_empty()) {
                            return empty().into_any(); // nothing typed, no history
                        }
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
                        // Schema-style leading icon for table/column hits (commands
                        // carry none). Text keeps its normal colour.
                        let mut cells: Vec<AnyView> = Vec::new();
                        if let Some(ic) = item.icon {
                            cells.push(
                                icons::icon(ic.glyph(), 16.0)
                                    .style(move |s| s.color(ic.color()).flex_shrink(0.0_f32))
                                    .into_any(),
                            );
                        }
                        cells.push(highlighted_primary(&item.primary, &item.match_term).into_any());
                        cells.push(
                            text(item.secondary.clone())
                                .style(|s| s.color(theme::text_muted()).font_size(14.0))
                                .into_any(),
                        );
                        // Trailing keycap and history marker, pushed to the far
                        // right by a single spacer — two spacers would split the
                        // free space between them and strand the keycap mid-row.
                        if item.keys.is_some() || item.right_icon.is_some() {
                            cells.push(empty().style(|s| s.flex_grow(1.0_f32)).into_any());
                        }
                        if let Some(keys) = item.keys {
                            // The Shortcuts modal's keycap, one size down: same
                            // mono face, surface and radius, so a binding looks
                            // like itself wherever the app shows it.
                            cells.push(
                                text(keys)
                                    .style(|s| {
                                        s.color(theme::text_muted())
                                            .font_size(theme::FONT_LABEL)
                                            .font_family("IBM Plex Mono".to_string())
                                            .background(theme::bg_deepest())
                                            .padding_horiz(6.0)
                                            .padding_vert(1.0)
                                            .border_radius(4.0)
                                            .flex_shrink(0.0_f32)
                                    })
                                    .into_any(),
                            );
                        }
                        if let Some(ri) = item.right_icon {
                            cells.push(
                                icons::icon(ri, 15.0)
                                    .style(|s| s.color(theme::text_faint()).flex_shrink(0.0_f32))
                                    .into_any(),
                            );
                        }
                        let row = h_stack_from_iter(cells)
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

            // Panel: 550px input + 15px padding all around (→ 580px wide), results
            // below. Sizes to content; the results scroll caps its height. (Widened
            // from 430 so long `table.column` rows + the history clock don't clip.)
            let panel = v_stack((
                input,
                autohide(scroll(results).scroll_to(move || list_scroll.get()))
                    // 10px gap here (not inside the content) so the scrollbar clears
                    // the input too, and the first row keeps the gap when scrolled up.
                    // Only when there's something to show (search results or history) —
                    // an empty list collapses the container, so the panel's padding
                    // stays even around the bare box.
                    .style(move |s| {
                        let s = s.width_full().max_height(360.0);
                        if items.with(|l| l.is_empty()) {
                            s
                        } else {
                            s.margin_top(10.0)
                        }
                    }),
            ))
            .on_click_stop(|_| {})
            .style(|s| {
                panel_style(s)
                    .width(580.0)
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

/// "You have an open transaction" — the question raised by anything that would
/// strand one (leaving Manual mode, closing the tab, switching its database).
///
/// Deliberately *not* dismissible by clicking away or Escape-ing: those read as
/// "never mind", and there's no safe default between committing and discarding
/// someone's uncommitted writes. Cancel is spelled out as a button, and it
/// abandons the action that raised the prompt rather than the transaction.
pub(crate) fn tx_prompt_overlay(ui: Ui) -> impl IntoView {
    let prompt = ui.overlay.tx_prompt;

    dyn_container(
        move || prompt.get(),
        move |p| {
            let Some(p) = p else {
                return empty().into_any();
            };
            let stmts = match p.stmts {
                1 => "1 statement".to_string(),
                n => format!("{n} statements"),
            };
            let body = if p.can_commit {
                format!(
                    "{} has an open transaction with {stmts} in it. \
                     You can commit to keep the changes, or rollback to discard them.",
                    p.tab
                )
            } else {
                // Postgres aborted it, so committing isn't on the table.
                format!(
                    "{} has a transaction that a failed statement aborted, with {stmts} \
                     in it. It can only be rolled back, which discards the changes.",
                    p.tab
                )
            };

            // One button row: Cancel (quiet) · Rollback (danger) · Commit — all
            // three in a ring of their own, left to right. This modal asks about
            // **uncommitted writes** and answered no key at all: Escape is a
            // deliberate no-op (there is no safe "never mind" here), and its
            // three buttons were outside every ring, so the keyboard could do
            // nothing about a question the app itself had raised.
            let ring = crate::widgets::FocusRing::new();
            let btn = |label: &'static str,
                       color: fn() -> Color,
                       hover: fn() -> Color,
                       tabindex: u32,
                       act: Rc<dyn Fn()>| {
                dialog_button(label, color, hover, ring.clone(), tabindex, move || (act)())
            };
            let resolve = p.resolve.clone();
            let cancel = {
                let r = resolve.clone();
                btn(
                    "Cancel",
                    theme::text_dim,
                    theme::text,
                    ACTION_TAB,
                    Rc::new(move || (r)(TxChoice::Cancel)),
                )
            };
            let rollback = {
                let r = resolve.clone();
                // "Rollback", matching the status bar's action and the body text.
                btn(
                    "Rollback",
                    theme::tx_rollback,
                    theme::tx_rollback_hover,
                    ACTION_TAB + 10,
                    Rc::new(move || (r)(TxChoice::Rollback)),
                )
            };
            // Built only when it applies, never built-and-hidden: `hide()` is
            // `display: none`, so the view is still in the tree and still in the
            // ring, and Tab would land on a button nobody can see — offering to
            // commit a transaction PostgreSQL has already aborted.
            let commit: AnyView = if p.can_commit {
                let r = resolve.clone();
                btn(
                    "Commit",
                    theme::tx_commit,
                    theme::tx_commit_hover,
                    ACTION_TAB + 20,
                    Rc::new(move || (r)(TxChoice::Commit)),
                )
                .into_any()
            } else {
                crate::widgets::nothing()
            };

            let panel = v_stack((
                text("Open transaction").style(|s| {
                    s.font_size(15.0)
                        .font_bold()
                        .color(theme::text())
                        .margin_bottom(10.0)
                }),
                text(body).style(|s| {
                    s.width(420.0)
                        .color(theme::text())
                        .font_size(theme::FONT_BODY)
                        .line_height(1.4)
                }),
                h_stack((
                    empty().style(|s| s.flex_grow(1.0_f32)),
                    cancel,
                    rollback,
                    commit,
                ))
                .style(|s| {
                    s.width_full()
                        .flex_row()
                        .items_center()
                        .gap(6.0)
                        .margin_top(18.0)
                }),
            ))
            .on_click_stop(|_| {})
            .style(|s| {
                panel_style(s)
                    .width(470.0)
                    .padding(20.0)
                    .flex_col()
                    .border_color(theme::modal_border())
            });

            // Take focus so the editor and the global shortcuts behind the
            // backdrop stop receiving keys — Ctrl+W closing another tab while
            // this one is asking about a transaction would be a mess. Escape
            // is swallowed rather than handled: unlike every other modal here,
            // there's no safe "never mind" for uncommitted writes.
            //
            // The ring is what lets Tab reach the three answers. There is
            // deliberately **no backdrop dismiss** either — clicking away from a
            // question about uncommitted writes is not an answer.
            crate::widgets::focus_root_with_ring(container(panel), ring)
                .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| {})
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
        if prompt.get().is_some() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// The shared yes/no confirmation — "are you sure?" for anything destructive.
/// Same chrome as `tx_prompt_overlay`, but with a safe default: Escape and
/// clicking the backdrop both answer No, because declining a confirm never
/// loses anything. (The transaction prompt has no such default, which is why it
/// swallows Escape instead.)
///
/// Generic by design — raise it through [`crate::Confirm`] rather than writing
/// another one-off modal.
pub(crate) fn confirm_overlay(ui: Ui) -> impl IntoView {
    let confirm = ui.overlay.confirm;

    dyn_container(
        move || confirm.get(),
        move |c| {
            let Some(c) = c else {
                return empty().into_any();
            };
            // Answer once and close. Shared by both buttons, Escape, and the
            // backdrop, so no path can leave the modal up or resolve twice.
            let answer: Rc<dyn Fn(bool)> = {
                let resolve = c.resolve.clone();
                Rc::new(move |yes: bool| {
                    confirm.set(None);
                    (resolve)(yes);
                })
            };

            // No before Yes, left to right as they sit — so the *first* Tab
            // lands on declining, which is always the safe side of a confirm.
            let ring = crate::widgets::FocusRing::new();
            let btn = |label: &'static str,
                       color: fn() -> Color,
                       hover: fn() -> Color,
                       tabindex: u32,
                       act: Rc<dyn Fn()>| {
                dialog_button(label, color, hover, ring.clone(), tabindex, move || (act)())
            };
            let no = {
                let a = answer.clone();
                btn(
                    "No",
                    theme::text_dim,
                    theme::text,
                    ACTION_TAB,
                    Rc::new(move || (a)(false)),
                )
            };
            let yes = {
                let a = answer.clone();
                btn(
                    "Yes",
                    theme::confirm_yes,
                    theme::confirm_yes_hover,
                    ACTION_TAB + 10,
                    Rc::new(move || (a)(true)),
                )
            };

            let panel = v_stack((
                text(c.title).style(|s| {
                    s.font_size(15.0)
                        .font_bold()
                        .color(theme::text())
                        .margin_bottom(10.0)
                }),
                text(c.message).style(|s| {
                    s.width(380.0)
                        .color(theme::text())
                        .font_size(theme::FONT_BODY)
                        .line_height(1.4)
                }),
                h_stack((empty().style(|s| s.flex_grow(1.0_f32)), no, yes)).style(|s| {
                    s.width_full()
                        .flex_row()
                        .items_center()
                        .gap(6.0)
                        .margin_top(18.0)
                }),
            ))
            // Clicks inside the panel mustn't reach the backdrop's "No".
            .on_click_stop(|_| {})
            .style(|s| {
                panel_style(s)
                    .width(430.0)
                    .padding(20.0)
                    .flex_col()
                    .border_color(theme::modal_border())
            });

            // Focus so the shortcuts behind the backdrop stop firing while
            // the question is up (Ctrl+W closing a tab mid-confirm would be
            // a mess), and so Escape lands here.
            // The backdrop's "No" goes on a sibling, not on the focus root: this
            // is a *question*, and floem fires `Click` on the focused view for
            // Space — so Space answered `false` to something the user had not
            // read. See `widgets::dismiss_layer`.
            let backdrop_no = answer.clone();
            crate::widgets::focus_root_with_ring(
                stack((
                    crate::widgets::dismiss_layer(move || (backdrop_no)(false)),
                    panel,
                )),
                ring,
            )
            .on_key_down(Key::Named(NamedKey::Escape), |_| true, {
                let a = answer.clone();
                move |_| (a)(false)
            })
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
        if confirm.get().is_some() {
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
            focus_root(stack((crate::widgets::dismiss_layer(close), panel)))
                .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| close())
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

/// What one Find-Anywhere hit points at.
#[derive(Clone)]
enum FindTarget {
    /// A table (`column: None`) or a specific column within one. Activating
    /// either opens the table.
    Table {
        source: TableSource,
        column: Option<String>,
    },
    /// A PostgreSQL standalone object — enum / domain / sequence. Activating it
    /// opens the object editor, which is exactly what Enter on its schema-tree
    /// row does; the palette is a second way to reach a row, not a second set of
    /// verbs.
    ///
    /// The `ObjectItem` is captured here rather than re-resolved on activation,
    /// for the same reason the tree's own row captures it: the result list is
    /// rebuilt whenever any database's schema signal changes, so a captured item
    /// is no more stale than the row you would have clicked.
    Object {
        database: String,
        item: schemaic_core::schema::ObjectItem,
    },
}

/// One Find-Anywhere hit: what it points at, plus the schema-style icon for it.
#[derive(Clone)]
struct FindHit {
    target: FindTarget,
    icon: ResultIcon,
}

fn find_matches(
    db_nodes: RwSignal<Vec<ConnNode>>,
    hidden: RwSignal<HashSet<String>>,
    q: &str,
    limit: usize,
) -> Vec<FindHit> {
    // `with_untracked`, not `get_untracked`: this runs on **every keystroke**
    // (the palette is deliberately undebounced, unlike the schema tree's filter
    // box), and `get` is documented as "try to *clone* and return" — so the two
    // reads here allocated and freed a whole `HashSet<String>` and a whole
    // `Vec<ConnNode>`, two `String`s per node, per character typed. On the one
    // path docs/architecture.md names for both rules. `lookup_object` twenty lines below
    // already did it this way.
    let mut out = Vec::new();
    hidden.with_untracked(|hidden| {
        db_nodes.with_untracked(|nodes| {
            for node in nodes {
                if hidden.contains(&node.database) {
                    continue;
                }
                if let SchemaState::Loaded(schema) = node.schema.get_untracked()
                    && schema_hits(&node.database, &schema, q, limit, &mut out)
                {
                    return;
                }
            }
        })
    });
    out
}

/// One database's hits, appended to `out`. Returns whether `limit` was reached —
/// the caller stops there.
///
/// Split out from [`find_matches`] because that one reads signals and this one is
/// plain data: it is what the tests drive to prove the palette matches the same
/// things the schema tree's filter keeps.
fn schema_hits(
    database: &str,
    schema: &schemaic_core::schema::DbSchema,
    q: &str,
    limit: usize,
    out: &mut Vec<FindHit>,
) -> bool {
    let src = |t: &schemaic_core::schema::TableInfo| {
        TableSource::new(database.to_string(), t.schema.clone(), t.name.clone())
    };
    let room = limit.saturating_sub(out.len());
    if room == 0 {
        return true;
    }
    // Each pass collects into its own bucket, bounded by the room left, and the
    // three are then merged under `pass_shares` — see it for why ordering alone
    // was not enough.
    let mut names: Vec<FindHit> = Vec::new();
    let mut objects: Vec<FindHit> = Vec::new();
    let mut columns: Vec<FindHit> = Vec::new();
    // Pass 1 — table and view *names*.
    for t in &schema.tables {
        if !(q.is_empty() || t.name.to_lowercase().contains(q)) {
            continue;
        }
        names.push(FindHit {
            target: FindTarget::Table {
                source: src(t),
                column: None,
            },
            icon: table_result_icon(t),
        });
        if names.len() >= room {
            break;
        }
    }
    // Pass 2 — the PostgreSQL standalone objects the tree lists in its Types /
    // Domains / Sequences folders. Matched through the *same*
    // `ObjectItem::matches_search` the tree's filter uses, which is the whole
    // point: this arm did not exist, so Ctrl+P for a type you were looking at in
    // the sidebar found nothing.
    //
    // **Before the columns, deliberately.** Columns are the category that floods:
    // one `user_id` foreign key repeated across a hundred tables is a hundred
    // hits, and with the objects appended last they were pushed past `limit`
    // entirely — reproducing the same "a type can't be found" symptom under a new
    // cause. Ordering alone then opened the mirror hole: on a serial-heavy
    // PostgreSQL schema, `id` filled every slot with `*_id_seq` and *no table or
    // column was reachable at all*. Each pass now takes a share (`pass_shares`),
    // so no category can crowd the others out from either side.
    //
    // A non-editable object is skipped — an identity column's counter, whose only
    // activation would be an editor that refuses to open. The tree can list it
    // because a tree row is context: it sits under the table that owns it and says
    // so. A palette row is a destination, and one that goes nowhere is worse than
    // an absent one. Nothing is lost: such a sequence is named after its own
    // table, which the palette does find.
    //
    // `objects_matching` rather than `objects_all` because this runs on **every
    // keystroke**, over every loaded database: the whole-list form cloned every
    // object in the database — an enum's entire value list included — to answer a
    // substring test.
    for kind in [
        schemaic_core::ddl::ObjectKind::Enum,
        schemaic_core::ddl::ObjectKind::Domain,
        schemaic_core::ddl::ObjectKind::Sequence,
    ] {
        let items = if q.is_empty() {
            schema.objects_all(kind)
        } else {
            schema.objects_matching(kind, q)
        };
        for o in items {
            if !is_palette_target(&o) {
                continue;
            }
            objects.push(FindHit {
                target: FindTarget::Object {
                    database: database.to_string(),
                    item: o,
                },
                icon: ResultIcon::Object(kind),
            });
            if objects.len() >= room {
                break;
            }
        }
        if objects.len() >= room {
            break;
        }
    }
    // Pass 3 — columns, each as its own `table.column` hit, still grouped under
    // the table they belong to.
    if !q.is_empty() {
        'cols: for t in &schema.tables {
            for c in &t.columns {
                if !c.name.to_lowercase().contains(q) {
                    continue;
                }
                columns.push(FindHit {
                    target: FindTarget::Table {
                        source: src(t),
                        column: Some(c.name.clone()),
                    },
                    icon: column_result_icon(t, c),
                });
                if columns.len() >= room {
                    break 'cols;
                }
            }
        }
    }
    let take = pass_shares(room, [names.len(), objects.len(), columns.len()]);
    for (bucket, n) in [names, objects, columns].into_iter().zip(take) {
        out.extend(bucket.into_iter().take(n));
    }
    out.len() >= limit
}

/// How many hits each of [`schema_hits`]' three passes contributes, given the
/// room left in the result list.
///
/// **A share each, not first-come.** Ordering alone decided which category could
/// crowd the others out, and it did so in both directions: with the objects
/// last, one `user_id` column repeated across a hundred tables pushed every type
/// past the cap; with the objects moved ahead of the columns, a serial-heavy
/// PostgreSQL schema answered `id` with eighty `*_id_seq` rows and no table or
/// column at all. Neither is a *wrong* result — each is an absent one, for
/// something the user typed precisely.
///
/// Each pass is guaranteed `room / 3` if it can use it, and a pass that can't
/// hands its share straight on. The spare then goes in pass order — names,
/// objects, columns — so a narrow search still fills the list with the most
/// precise matches first, and only a search broad enough to overflow ever pays
/// the share.
fn pass_shares(room: usize, want: [usize; 3]) -> [usize; 3] {
    let base = room / want.len();
    let mut take = [0usize; 3];
    let mut left = room;
    for i in 0..want.len() {
        take[i] = want[i].min(base);
        left -= take[i];
    }
    for i in 0..want.len() {
        let extra = (want[i] - take[i]).min(left);
        take[i] += extra;
        left -= extra;
    }
    take
}

#[cfg(test)]
mod object_menu_tests {
    use super::object_entries;
    use schemaic_core::intel::SqlDialect::{MySql, Postgres, Sqlite};

    /// A table offers all four, on either engine with an emitter.
    #[test]
    fn a_table_offers_everything() {
        for d in [MySql, Postgres] {
            let e = object_entries(false, d, false);
            assert!(e.import && e.triggers && e.truncate && e.edit, "{d:?}");
        }
    }

    /// A view is not insertable and owns no rows to delete, on either engine —
    /// so those two entries are **absent**, not dimmed: a missing entry reads as
    /// "not supported", which is what is true.
    #[test]
    fn a_view_never_offers_import_or_truncate() {
        for d in [MySql, Postgres] {
            let e = object_entries(true, d, false);
            assert!(!e.import, "{d:?}");
            assert!(!e.truncate, "{d:?}");
        }
    }

    /// **The one that isn't uniform.** MySQL's views can't have triggers;
    /// PostgreSQL's can, and that is where `INSTEAD OF` lives — so flattening
    /// this to `!is_view` would remove a live feature from the PG menu.
    #[test]
    fn only_mysql_views_lose_the_triggers_entry() {
        assert!(!object_entries(true, MySql, false).triggers, "MySQL view");
        assert!(
            object_entries(true, Postgres, false).triggers,
            "PostgreSQL view"
        );
    }

    /// A materialized view is excluded even on PostgreSQL: the server refuses
    /// outright (`relation "mv" cannot have triggers`).
    #[test]
    fn a_materialized_view_has_no_triggers_entry() {
        assert!(!object_entries(true, Postgres, true).triggers);
    }

    /// **The whole table of answers, in one place.** Three separate tests used
    /// to assert `object_entries(…, Sqlite, …).edit`, `.triggers` and so on —
    /// each of which answers the same for every shipping engine, so none of them
    /// could fail for a dialect: delete SQLite's entire capability story and they
    /// stayed green.
    ///
    /// A matrix has content where an isolated `true` has none, because what it
    /// pins is the **shape of the disagreement**: `triggers` is the one entry
    /// where the engines differ (MySQL takes no trigger on a view; a
    /// *materialized* view takes none anywhere), and `edit`/`import`/`truncate`
    /// are asserted *and* asserted to be the same everywhere, which is the claim
    /// that would break if a fourth engine were sorted onto the wrong side.
    ///
    /// Every answer here is now computed from a capability rather than stated —
    /// `edit` from `ddl::supports_table_design` or `supports_view_editing`,
    /// `triggers` from `supports_trigger_editing`, `truncate` from
    /// `supports_change` — and each of those is pinned against the emitter in
    /// `core::ddl` (`table_design_is_offered_exactly_where_a_retype_emits` and its
    /// two siblings). This test is what pins the *menu's* half: which question
    /// each entry asks, and about which object.
    #[test]
    fn the_object_menu_matrix_is_the_same_everywhere_except_a_views_triggers() {
        for d in [MySql, Postgres, Sqlite] {
            // A base table: everything, on every engine.
            let t = object_entries(false, d, false);
            assert!(
                t.edit && t.import && t.truncate && t.triggers,
                "{d:?}: {t:?}"
            );

            // A view: no import, no truncate, and editable everywhere — SQLite
            // included, where every edit is a drop and a create because there is
            // no `CREATE OR REPLACE VIEW`.
            let v = object_entries(true, d, false);
            assert!(v.edit, "{d:?} view: {v:?}");
            assert!(!v.import && !v.truncate, "{d:?} view: {v:?}");

            // **The one real disagreement.** `INSTEAD OF` lives on PostgreSQL
            // and on SQLite, where it is the only way a view is written to at
            // all. MySQL takes no trigger on a view.
            assert_eq!(v.triggers, d != MySql, "{d:?} view triggers: {v:?}");

            // And a materialized view takes none anywhere: the server refuses
            // outright (`relation "mv" cannot have triggers`).
            assert!(!object_entries(true, d, true).triggers, "{d:?} matview");
        }
    }
}

#[cfg(test)]
mod create_menu_tests {
    use super::{CreateKind, create_children};
    use schemaic_core::ddl::ObjectKind;
    use schemaic_core::intel::SqlDialect;

    fn labels(dialect: SqlDialect) -> Vec<&'static str> {
        create_children(dialect, false)
            .into_iter()
            .map(|e| e.label)
            .collect()
    }

    /// MySQL has none of the three standalone objects, so they are **absent**
    /// rather than dimmed: a missing entry reads as "not supported", a dimmed one
    /// as "not here", and offering an entry that fails at apply is the thing this
    /// gate exists to prevent.
    #[test]
    fn mysql_offers_only_what_it_has() {
        assert_eq!(labels(SqlDialect::MySql), vec!["Table", "View"]);
    }

    #[test]
    fn postgres_offers_its_standalone_objects_too() {
        assert_eq!(
            labels(SqlDialect::Postgres),
            vec!["Table", "View", "Type", "Domain", "Sequence"]
        );
        let kinds: Vec<CreateKind> = create_children(SqlDialect::Postgres, false)
            .into_iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                CreateKind::Table,
                CreateKind::View,
                CreateKind::Object(ObjectKind::Enum),
                CreateKind::Object(ObjectKind::Domain),
                CreateKind::Object(ObjectKind::Sequence),
            ]
        );
    }

    /// The gate that matters if it drifts: every one of these opens an editor
    /// that ends at a live `run_ddl`.
    #[test]
    fn a_read_only_connection_can_create_nothing() {
        for d in [SqlDialect::MySql, SqlDialect::Postgres] {
            let entries = create_children(d, true);
            assert!(!entries.is_empty());
            assert!(entries.iter().all(|e| e.disabled), "{d:?}");
        }
        assert!(
            create_children(SqlDialect::Postgres, false)
                .iter()
                .all(|e| !e.disabled)
        );
    }
}

#[cfg(test)]
mod find_tests {
    use super::{FindHit, FindTarget, schema_hits};
    use schemaic_core::ddl::ObjectKind;
    use schemaic_core::schema::{
        DbSchema, DomainInfo, EnumInfo, SequenceInfo, SequenceOwner, TableInfo,
    };

    fn fixture() -> DbSchema {
        DbSchema {
            tables: vec![TableInfo {
                name: "orders".into(),
                schema: Some("public".into()),
                columns: vec![schemaic_core::schema::ColumnInfo {
                    name: "order_ref".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            enums: vec![
                EnumInfo {
                    name: "order_status".into(),
                    schema: Some("public".into()),
                    values: vec!["shipped".into()],
                    comment: None,
                },
                EnumInfo {
                    name: "mood".into(),
                    schema: Some("sales".into()),
                    values: vec![],
                    comment: None,
                },
            ],
            domains: vec![DomainInfo {
                name: "order_email".into(),
                schema: Some("public".into()),
                base_type: "text".into(),
                ..Default::default()
            }],
            sequences: vec![
                SequenceInfo {
                    name: "order_counter".into(),
                    schema: Some("public".into()),
                    ..Default::default()
                },
                // An identity column's counter: listed by the tree, never a
                // palette destination.
                SequenceInfo {
                    name: "orders_id_seq".into(),
                    schema: Some("public".into()),
                    owned_by: Some(SequenceOwner {
                        table: "orders".into(),
                        column: "id".into(),
                        internal: true,
                    }),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    fn hits(q: &str) -> Vec<FindHit> {
        let mut out = Vec::new();
        schema_hits("shop", &fixture(), q, 80, &mut out);
        out
    }

    fn object_names(q: &str) -> Vec<String> {
        hits(q)
            .into_iter()
            .filter_map(|h| match h.target {
                FindTarget::Object { item, .. } => Some(item.name().to_string()),
                _ => None,
            })
            .collect()
    }

    /// Every row in order, tagged by category — what the ordering tests read.
    fn labels_of(hits: Vec<FindHit>) -> Vec<String> {
        hits.into_iter()
            .map(|h| match h.target {
                FindTarget::Table {
                    source,
                    column: None,
                } => format!("table:{}", source.table),
                FindTarget::Table {
                    column: Some(c), ..
                } => format!("column:{c}"),
                FindTarget::Object { item, .. } => format!("object:{}", item.name()),
            })
            .collect()
    }

    /// The bug: the palette walked only `schema.tables`, so on a PostgreSQL
    /// connection none of the three object kinds could be found at all.
    #[test]
    fn every_standalone_object_kind_is_findable() {
        assert_eq!(object_names("order_status"), vec!["order_status"]);
        assert_eq!(object_names("order_email"), vec!["order_email"]);
        assert_eq!(object_names("order_counter"), vec!["order_counter"]);
    }

    /// One term reaching a table, all three object kinds and a column, in the
    /// order the palette commits to: **names, then objects, then columns.**
    #[test]
    fn a_database_lists_table_names_then_objects_then_columns() {
        assert_eq!(
            labels_of(hits("order")),
            vec![
                "table:orders",
                // enums, then domains, then sequences — the tree's folder order
                "object:order_status",
                "object:order_email",
                "object:order_counter",
                "column:order_ref",
            ]
        );
    }

    /// Why objects come before columns. Columns are the category that floods —
    /// one `user_id` foreign key across every table is one hit per table — and
    /// with objects appended last they were pushed past the result cap entirely,
    /// so `user_role` could not be found however precisely you typed it. That is
    /// the same "Ctrl+P for a type finds nothing" symptom this feature exists to
    /// fix, arriving by a different route.
    #[test]
    fn an_object_survives_a_flood_of_column_matches() {
        let flooded = DbSchema {
            tables: (0..20)
                .map(|i| TableInfo {
                    name: format!("t{i}"),
                    schema: Some("public".into()),
                    columns: vec![schemaic_core::schema::ColumnInfo {
                        name: "user_id".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                })
                .collect(),
            enums: vec![EnumInfo {
                name: "user_role".into(),
                schema: Some("public".into()),
                values: vec![],
                comment: None,
            }],
            ..Default::default()
        };
        // A cap far below the number of matching columns: the enum must still be
        // in the results, and ahead of them.
        let mut out = Vec::new();
        schema_hits("shop", &flooded, "user", 5, &mut out);
        let labels = labels_of(out);
        assert_eq!(
            labels.first().map(String::as_str),
            Some("object:user_role"),
            "the object must not be crowded out, got {labels:?}"
        );
        assert_eq!(labels.len(), 5, "the cap is still honoured");
    }

    /// **The mirror of the test above**, which the fix for it opened: moving the
    /// objects ahead of the columns let *them* fill the list. On a serial-heavy
    /// PostgreSQL schema, `id` answered with nothing but `*_id_seq` rows — the
    /// same "typed it precisely, found nothing" symptom, from the other side.
    #[test]
    fn a_flood_of_sequences_does_not_crowd_out_tables_and_columns() {
        let flooded = DbSchema {
            tables: vec![TableInfo {
                name: "id_map".into(),
                schema: Some("public".into()),
                columns: vec![schemaic_core::schema::ColumnInfo {
                    name: "id".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            sequences: (0..40)
                .map(|i| SequenceInfo {
                    name: format!("t{i}_id_seq"),
                    schema: Some("public".into()),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let mut out = Vec::new();
        schema_hits("shop", &flooded, "id", 9, &mut out);
        let labels = labels_of(out);
        assert_eq!(labels.len(), 9, "the cap is still honoured");
        assert!(
            labels.iter().any(|l| l.starts_with("table:")),
            "the table must survive the sequences, got {labels:?}"
        );
        assert!(
            labels.iter().any(|l| l.starts_with("column:")),
            "so must the column, got {labels:?}"
        );
    }

    /// Each pass is guaranteed its third if it can use it; a pass with nothing to
    /// show hands its share on, and the spare goes in pass order, so a narrow
    /// search still fills the list with the most precise matches first.
    #[test]
    fn each_pass_gets_a_share_and_the_spare_goes_in_order() {
        use super::pass_shares;
        // Everything fits: nothing is capped.
        assert_eq!(pass_shares(80, [2, 3, 4]), [2, 3, 4]);
        // One category floods; the others keep their share, and the spare left
        // by a pass that couldn't fill its own goes to the earlier of the two.
        assert_eq!(pass_shares(9, [1, 40, 40]), [1, 5, 3]);
        assert_eq!(pass_shares(9, [40, 40, 40]), [3, 3, 3]);
        // A pass with nothing hands its share on, in pass order.
        assert_eq!(pass_shares(9, [0, 0, 40]), [0, 0, 9]);
        assert_eq!(pass_shares(9, [40, 0, 40]), [6, 0, 3]);
        // Degenerate rooms don't over-allocate.
        assert_eq!(pass_shares(0, [5, 5, 5]), [0, 0, 0]);
        assert_eq!(pass_shares(2, [5, 5, 5]), [2, 0, 0]);
    }

    /// **The site the gate is applied at**, not just the predicate. Deleting the
    /// `is_palette_target` filter left the suite green, because both regression
    /// tests called the predicate directly and nothing reached the resolver a
    /// remembered search goes through.
    #[test]
    fn a_remembered_internal_sequence_does_not_resolve_to_a_destination() {
        use super::palette_object;
        let s = fixture();
        let ns = Some("public");
        assert!(
            palette_object(&s, ns, ObjectKind::Sequence, "order_counter").is_some(),
            "a serial's own sequence is an ordinary object and can be remembered"
        );
        assert!(
            palette_object(&s, ns, ObjectKind::Sequence, "orders_id_seq").is_none(),
            "an identity column's counter opens an editor that would refuse"
        );
        assert!(palette_object(&s, ns, ObjectKind::Sequence, "gone").is_none());
    }

    #[test]
    fn an_identity_columns_counter_is_never_a_result() {
        // It matches by name, and is still withheld: its activation would be an
        // editor that refuses to open.
        assert!(!object_names("orders_id_seq").contains(&"orders_id_seq".to_string()));
        assert!(!object_names("seq").contains(&"orders_id_seq".to_string()));
    }

    /// `is_palette_target` is asked on the **history** path as well as the search
    /// path, and this is why: a `serial`'s sequence is an ordinary object and a
    /// legitimate result, so it can be recorded in search history — but migrating
    /// its column to an identity column makes that same sequence internal. The
    /// remembered row must then stop being offered, or Enter opens an editor on a
    /// counter the server won't let anyone alter.
    #[test]
    fn a_sequence_that_becomes_internal_stops_being_a_palette_target() {
        let seq = |internal: bool| {
            schemaic_core::schema::ObjectItem::Sequence(SequenceInfo {
                name: "orders_id_seq".into(),
                schema: Some("public".into()),
                owned_by: Some(SequenceOwner {
                    table: "orders".into(),
                    column: "id".into(),
                    internal,
                }),
                ..Default::default()
            })
        };
        assert!(super::is_palette_target(&seq(false)), "a serial's sequence");
        assert!(
            !super::is_palette_target(&seq(true)),
            "the same sequence once the column is an identity column"
        );
    }

    /// The gate is the object editor's own predicate, not a second spelling of
    /// it — so a kind that becomes non-editable later drops out of the palette
    /// without anyone remembering to update it.
    #[test]
    fn the_palette_gate_is_the_editors_own_predicate() {
        for o in fixture().objects_all(ObjectKind::Sequence) {
            assert_eq!(
                super::is_palette_target(&o),
                crate::object_editor::is_editable_object(&o),
                "{}",
                o.name()
            );
        }
    }

    #[test]
    fn an_object_search_is_case_insensitive_and_matches_a_substring() {
        assert_eq!(object_names("status"), vec!["order_status"]);
        // The caller lower-cases the query; the name may be any case.
        assert_eq!(object_names("mood"), vec!["mood"]);
    }

    #[test]
    fn a_term_matching_nothing_finds_no_object() {
        assert!(object_names("zzz").is_empty());
    }

    /// The invariant this whole change exists to establish: the palette lists
    /// exactly the objects the schema tree's filter keeps — the two surfaces read
    /// the same data through the same `ObjectItem::matches_search`.
    ///
    /// The **one** deliberate divergence is the internal sequence above: a tree
    /// row is context, a palette row is a destination.
    #[test]
    fn the_palette_finds_what_the_schema_tree_filter_keeps() {
        let schema = fixture();
        for q in ["order", "status", "o", "mood", "seq", "zzz", "email"] {
            let mut tree: Vec<String> = Vec::new();
            for kind in [ObjectKind::Enum, ObjectKind::Domain, ObjectKind::Sequence] {
                let all = schema.objects_all(kind);
                tree.extend(
                    crate::schema_tree::objects_shown(&all, false, false, q)
                        .into_iter()
                        .filter(|o| super::is_palette_target(o))
                        .map(|o| o.name().to_string()),
                );
            }
            assert_eq!(object_names(q), tree, "the two surfaces disagree on {q:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ghost_suffix;

    /// The panic this function exists to prevent: a Turkish schema with a table
    /// named `İzmir`, and the user types one `i`. Reproduced in
    /// `review/palettecheck/` before the fix.
    #[test]
    fn ghost_suffix_survives_a_name_that_changes_length_when_lowercased() {
        assert_eq!(ghost_suffix("İzmir", "i").as_deref(), Some("zmir"));
        // The other two the finding names, for the same reason.
        assert_eq!(ghost_suffix("ẞtrasse", "ß").as_deref(), Some("trasse"));
        assert_eq!(ghost_suffix("Kelvin", "k").as_deref(), Some("elvin"));
    }

    #[test]
    fn ghost_suffix_is_case_insensitive_and_keeps_the_original_casing() {
        assert_eq!(ghost_suffix("Orders", "ord").as_deref(), Some("ers"));
        assert_eq!(ghost_suffix("Orders", "ORD").as_deref(), Some("ers"));
        assert_eq!(
            ghost_suffix("orderDetails", "order").as_deref(),
            Some("Details")
        );
    }

    #[test]
    fn ghost_suffix_has_nothing_to_show_when_there_is_no_tail() {
        assert_eq!(ghost_suffix("Orders", "orders"), None, "fully typed");
        assert_eq!(ghost_suffix("Orders", "xyz"), None, "not a prefix");
        assert_eq!(ghost_suffix("", ""), None, "nothing to complete");
    }

    #[test]
    fn ghost_suffix_with_an_empty_query_offers_the_whole_completion() {
        assert_eq!(ghost_suffix("Orders", "").as_deref(), Some("Orders"));
    }

    /// Multi-byte names that *don't* change length must still slice correctly.
    #[test]
    fn ghost_suffix_handles_ordinary_multibyte_names() {
        assert_eq!(ghost_suffix("café_log", "caf").as_deref(), Some("é_log"));
        assert_eq!(ghost_suffix("café_log", "café").as_deref(), Some("_log"));
        assert_eq!(
            ghost_suffix("日本語table", "日本").as_deref(),
            Some("語table")
        );
    }
}

#[cfg(test)]
mod row_menu_tests {
    use super::{field_entries, key_entries};
    use schemaic_core::intel::SqlDialect::{MySql, Postgres, Sqlite};

    /// These two rows open the designer, which every engine now has — SQLite
    /// reaches a retype or a constraint by rebuilding the table. They were once
    /// ungated for the wrong reason (nobody had gated them, so the answer was a
    /// literal `true`) and are gated now on the question they were standing in
    /// for, `ddl::supports_table_design`.
    ///
    /// **What can fail here and what can't.** All three shipping engines answer
    /// yes, so no assertion over these three can fail on a *dialect* today; the
    /// value is in the `is_view` half, which
    /// `a_view_offers_no_column_or_key_entry_on_any_engine` below covers, and in
    /// the claim underneath — that the designer can really express the edit on
    /// every engine — which is tested where it lives, against the emitter:
    /// `core::ddl`'s `table_design_is_offered_exactly_where_a_retype_emits`,
    /// `every_engine_can_express_a_column_retype` and
    /// `sqlite_reaches_a_retype_through_the_rebuild_and_the_others_alter_in_place`.
    #[test]
    fn every_engine_designs_from_a_column_or_a_key_row() {
        for d in [MySql, Postgres, Sqlite] {
            assert!(field_entries(d, false).edit, "Edit column {d:?}");
            assert!(key_entries(d, None, false).edit, "Edit index {d:?}");
        }
    }

    /// **A view's column row is not a table's.** The tree renders one under a
    /// view exactly as under a table — `is_view` only picks a different glyph —
    /// so an ungated menu offers Edit column and a red Drop for something that
    /// has neither, opens the *table* designer on the view, and lets the refusal
    /// arrive from the server instead of from the menu. Every engine refuses:
    /// MySQL `… is not BASE TABLE`, PostgreSQL in kind, and SQLite's rebuild
    /// route emits `DROP TABLE` on a view.
    #[test]
    fn a_view_offers_no_column_or_key_entry_on_any_engine() {
        for d in [MySql, Postgres, Sqlite] {
            let f = field_entries(d, true);
            assert!(!f.edit && !f.drop, "{d:?} column on a view: {f:?}");
            for constraint in [None, Some("uq_email")] {
                let k = key_entries(d, constraint, true);
                assert!(
                    !k.edit && !k.drop_foreign_key && !k.drop_index,
                    "{d:?} key {constraint:?} on a view: {k:?}"
                );
            }
        }
    }

    /// What SQLite *can* do stays. Hiding these would take away drops the
    /// engine performs — it has `ALTER TABLE … DROP COLUMN` and `DROP INDEX`.
    #[test]
    fn sqlite_still_drops_a_column_and_a_plain_index() {
        assert!(field_entries(Sqlite, false).drop);
        assert!(key_entries(Sqlite, None, false).drop_index);
    }

    /// The two that really do need the twelve-step rebuild.
    #[test]
    fn sqlite_drops_no_constraint() {
        assert!(
            !key_entries(Sqlite, None, false).drop_foreign_key,
            "no ALTER TABLE … DROP CONSTRAINT"
        );
        assert!(
            !key_entries(Sqlite, Some("uq_email"), false).drop_index,
            "a UNIQUE index is part of the table definition"
        );
    }

    #[test]
    fn the_full_engines_offer_every_row_entry() {
        for d in [MySql, Postgres] {
            let f = field_entries(d, false);
            assert!(f.edit && f.drop, "{d:?} column");
            for constraint in [None, Some("uq_email")] {
                let k = key_entries(d, constraint, false);
                assert!(
                    k.edit && k.drop_foreign_key && k.drop_index,
                    "{d:?} key {constraint:?}"
                );
            }
        }
    }
}

#[cfg(test)]
mod sqlite_create_menu_tests {
    use super::create_children;
    use schemaic_core::intel::SqlDialect::Sqlite;

    /// The submenu used to be empty on SQLite — there was no emitter to build a
    /// `CREATE TABLE` with, and then none to build a `CREATE VIEW` with either.
    /// Both are there now.
    ///
    /// **No `CREATE TRIGGER` entry, and not for the reason this comment used to
    /// give.** It said "nothing reads a SQLite trigger back", which `2cfcf4f`
    /// made false in the same range — `sqlite_trigger_info` parses one out of
    /// `sqlite_master`, and the trigger *editor* is offered on SQLite (see
    /// `object_menu_tests`). The submenu is a different list: a trigger is
    /// created from the object it hangs off, on every engine, so it has no entry
    /// here on any of them.
    #[test]
    fn sqlite_can_create_a_table_and_a_view() {
        let labels: Vec<&str> = create_children(Sqlite, false)
            .into_iter()
            .map(|e| e.label)
            .collect();
        assert_eq!(labels, vec!["Table", "View"]);
    }

    #[test]
    fn a_read_only_sqlite_connection_can_create_nothing() {
        assert!(create_children(Sqlite, true).iter().all(|e| e.disabled));
    }
}

/// **The irreversible entry is last in its group, in every context menu.**
///
/// `8a85fa1`'s whole subject is one menu order — Open · Read · Tree state ·
/// Write (irreversible last, coloured `theme::error`) · AI Explain — written out
/// above [`context_menu_overlay`]'s builder. Its commit message records that the
/// six menus had already drifted into six orderings once, and that the key row
/// *"opened straight onto Drop index"*: the row the cursor lands on after a
/// right-click was the one that destroys an index. That is a rule with a
/// production bug history and no test, because the ordering lives inside
/// `build: Rc<dyn Fn(CtxMenu) -> Vec<MenuEntry>>`, a closure over the whole `Ui`
/// bundle, which no unit test can call.
///
/// So this reads the source, like [`crate::widgets`]'s popup-anchor gate and
/// `core/tests/doc_coverage.rs`. **It checks the load-bearing half only** — the
/// half a mistake in is destructive rather than merely untidy — and it can do
/// that precisely because the destructive entries mark themselves: every one is
/// built with `MenuEntry::action_colored(…, theme::error, …)`, which is the same
/// fact the menu shows the user.
///
/// Two things it deliberately does not check, recorded here so the shorter rule
/// isn't mistaken for the whole one. The full skeleton is a *subsequence* claim
/// over five groups, and the shipped code already deviates from it twice,
/// harmlessly: the database arm pushes **Collapse all** after Refresh where the
/// skeleton closes the read group with Refresh, and the table arm's write group
/// is Import → Edit table → Triggers where the skeleton lists
/// Create/Edit/Import/Triggers. Neither misplaces a destructive entry. Catching
/// those needs the extraction the ledger asks for — a pure
/// `menu_skeleton(kind, offers) -> Vec<MenuSlot>` with the closures attached
/// afterwards — which rewrites every arm of a 700-line `match` and is a change to
/// make with the app running.
#[cfg(test)]
mod menu_order_gate {
    use std::path::{Path, PathBuf};

    fn this_file() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("overlays.rs")
    }

    /// One entry construction found in the builder.
    #[derive(Debug)]
    struct Built {
        arm: String,
        line: u32,
        label: String,
        destructive: bool,
        /// Whether this entry lands in the menu itself (`entries`) rather than in
        /// a submenu's own vector — the colour swatches are `MenuEntry`s too, and
        /// they are not rows of the menu whose order is under test.
        top_level: bool,
    }

    /// Every `MenuEntry` construction inside the context-menu builder, in source
    /// order, tagged with the `CtxKind` arm it sits in.
    ///
    /// The scan is bounded by the builder's own two landmarks: it starts at the
    /// `let build:` binding and stops at the `AI Explain` row, which is pushed
    /// *outside* the `match` and is therefore the fixed tail of every menu — the
    /// one entry that legitimately follows a Drop.
    ///
    /// A constructor's head may wrap over several lines (`rustfmt` breaks the
    /// long ones, and two labels are `if …` expressions rather than literals), so
    /// the label is the first string literal within the following few lines and
    /// `theme::error` is looked for in the same span, before the closure starts.
    /// That is approximate by design: it is an ordering gate, not a parser.
    fn built_entries(src: &str) -> Vec<Built> {
        let lines: Vec<&str> = src.lines().collect();
        // Both landmarks are searched *forward*, and the second from the first:
        // the module comment above the builder quotes "AI Explain" too, and
        // taking that one made the range empty and the whole gate vacuous — it
        // passed by finding nothing, which is why the counts below are asserted.
        let find_from = |at: usize, needle: &str| {
            lines
                .iter()
                .skip(at)
                .position(|l| l.contains(needle))
                .map(|i| i + at)
                .unwrap_or_else(|| panic!("the builder's landmark is gone: {needle}"))
        };
        let start = find_from(0, "let build: Rc<dyn Fn(CtxMenu)");
        let end = find_from(start, "\"AI Explain\"");

        let mut out = Vec::new();
        let mut arm = "(before the match)".to_string();
        for i in start..end {
            let t = lines[i].trim_start();
            if let Some(rest) = t.strip_prefix("CtxKind::") {
                arm = rest
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .find(|s| !s.is_empty())
                    .unwrap_or("?")
                    .to_string();
            }
            // The two arms whose write group is a helper's output rather than a
            // constructor here (`create_submenu` builds the Create ▸ entry). The
            // gate has to see it or those arms look as though they stop at the
            // read group.
            if t.starts_with("entries.extend(create_submenu(") {
                out.push(Built {
                    arm: arm.clone(),
                    line: i as u32 + 1,
                    label: CREATE_SUBMENU.to_string(),
                    destructive: false,
                    top_level: true,
                });
                continue;
            }
            let is_ctor = ["action(", "action_icon(", "action_colored(", "sub("]
                .iter()
                .any(|c| t.contains(&format!("MenuEntry::{c}")));
            if !is_ctor {
                continue;
            }
            // Comments are cut out of the span first: one entry explains its own
            // label with `"(0)" reads as a broken count` two lines above the label
            // itself, and the scan read the comment's string as the entry's name.
            let span = |from: usize, len: usize| -> String {
                lines[from..(from + len).min(lines.len())]
                    .iter()
                    .map(|l| l.split_once("//").map_or(*l, |(code, _)| code))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            // Twelve lines, and the label is read only from the part *before* the
            // action closure: one entry's label is an `if` whose arms are four
            // comment lines below the constructor, and a window narrow enough to
            // miss it read the entry as nameless. Cutting at `move ||` is what
            // makes the wider window safe — a literal in a closure body is not a
            // label, and mistaking one for a label is how this would pass.
            let head: String = span(i, 12);
            // The AI Explain row is pushed *outside* the `match` and is the fixed
            // tail of every menu — the one entry that legitimately follows a
            // Drop. Its constructor sits one line above its label, so bounding
            // the scan by the label alone still catches it.
            if head.contains("\"AI Explain\"") {
                continue;
            }
            let first_literal = |span: &str| {
                span.split_once('"')
                    .and_then(|(_, r)| r.split_once('"'))
                    .map(|(l, _)| l.to_string())
            };
            let closure_at = head.find("move ||").unwrap_or(head.len());
            let mut label = head[..closure_at]
                .split_once("MenuEntry::")
                .and_then(|(_, r)| first_literal(r))
                .unwrap_or_default();
            // Two entries name themselves through a `let label = if … { "A" }
            // else { "B" }` above the constructor (Favorite/Unfavorite, and the
            // key row's Edit, which is named for what the row *is*). The binding
            // holds every alternative and they all belong to the same group, so
            // the first is enough to place the entry — and leaving the label empty
            // would have silently excused both from the group check below.
            if label.is_empty() {
                label = (i.saturating_sub(12)..i)
                    .rev()
                    .find(|&j| lines[j].contains("let label = "))
                    .and_then(|j| first_literal(&span(j, 6)))
                    .unwrap_or_default();
            }
            // In `entries` itself, or in a submenu's own vector? The statement the
            // constructor belongs to opens at most a line or two above it.
            let top_level = (i.saturating_sub(3)..=i)
                .rev()
                .find_map(|j| {
                    let s = lines[j].trim_start();
                    s.split_once(".push(")
                        .or_else(|| s.split_once(".extend("))
                        .map(|(recv, _)| recv)
                })
                .is_some_and(|recv| recv == "entries");
            out.push(Built {
                arm: arm.clone(),
                line: i as u32 + 1,
                label,
                destructive: head[..closure_at].contains("theme::error"),
                top_level,
            });
        }
        out
    }

    /// The Create ▸ entry, which two arms contribute through `create_submenu`
    /// rather than by constructing it here.
    const CREATE_SUBMENU: &str = "Create ▸";

    /// **The skeleton as data.** Which group of the ordering comment above `build`
    /// each entry belongs to — 1 Open, 2 Read, 3 Tree state, 4 Write — so "every
    /// menu is a subsequence of one skeleton" becomes an assertion instead of a
    /// paragraph. `AI Explain` is group 5 and is pushed outside the `match`, so it
    /// never reaches this table.
    ///
    /// An unknown label **fails** the gate rather than being skipped, and that is
    /// the load-bearing half: a thirteenth entry cannot be added to any menu
    /// without placing it in the skeleton first, which is the drift the comment was
    /// written to stop and could not.
    fn group(label: &str) -> Option<u8> {
        Some(match label {
            // 1. Open — what a double-click would have done.
            "Open in CLI" | "Open" | "Open in new tab" => 1,
            // 2. Read — the clipboard, then what the node can show you, closing
            //    with Refresh. `Collapse all` only rearranges the tree under the
            //    row and sits *after* Refresh in the database arm; the skeleton's
            //    wording says the group closes with Refresh, so that is a real
            //    deviation — one this gate tolerates because it stays inside the
            //    group and misplaces nothing irreversible. Recorded, not hidden.
            "Copy name"
            | "Copy qualified name"
            | "Properties"
            | "Live monitor"
            | "Show diagram"
            // One node, two spellings, same slot: a table has four things to
            // generate and gets the submenu, a view has one and keeps the flat
            // entry rather than spending a hover on a lone child.
            | "Generate"
            | "Generate DDL"
            | "Refresh"
            | "Collapse all" => 2,
            // 3. Tree state — the row, not the object.
            "Favorite" | "Unfavorite" | "Colour" | "Hide" => 3,
            // 4. Write, with the irreversible entries last inside it. The order
            //    *within* the group is not checked: the table arm ships
            //    Import → Edit table → Triggers where the skeleton lists
            //    Create/Edit/Import/Triggers, the second recorded deviation.
            //    `drop_is_the_last_entry_before_ai_explain` is what pins the part
            //    of the group order that matters.
            CREATE_SUBMENU | "Create {}" | "Import" | "Edit table" | "Edit view"
            | "Edit column" | "Edit index" | "Edit primary key" | "Edit foreign key"
            | "Edit {}" | "Triggers" | "Truncate" | "Drop" | "Drop index" | "Drop foreign key" => 4,
            _ => return None,
        })
    }

    /// `8a85fa1`'s whole subject: one skeleton, and each menu a subsequence of it,
    /// so the same action is in the same place whatever the row is. The commit
    /// records that the six menus had already drifted into six orderings once —
    /// with the key row opening straight onto `Drop index` — and it added no test.
    ///
    /// The scan is over the source rather than over a built menu because the order
    /// is a property of the *source*: a dialect or a read-only connection can only
    /// omit an entry, never move one, so one pass covers every engine and every
    /// permission state at once. That is also why the ordering was untestable
    /// through `build` — it closes over a `Ui`.
    #[test]
    fn every_menu_is_a_subsequence_of_the_skeleton() {
        let src = std::fs::read_to_string(this_file()).expect("this file");
        let built: Vec<Built> = built_entries(&src)
            .into_iter()
            .filter(|b| b.top_level)
            .collect();

        let mut unplaced: Vec<String> = Vec::new();
        let mut out_of_order: Vec<String> = Vec::new();
        let mut arms: Vec<String> = Vec::new();
        let mut highest: (u8, String) = (0, String::new());
        for b in &built {
            if arms.last() != Some(&b.arm) {
                arms.push(b.arm.clone());
                highest = (0, String::new());
            }
            let Some(g) = group(&b.label) else {
                unplaced.push(format!("{}: `{}` at line {}", b.arm, b.label, b.line));
                continue;
            };
            if g < highest.0 {
                out_of_order.push(format!(
                    "{}: `{}` (group {g}) at line {} comes after `{}` (group {}) — \
                     the menu is no longer a subsequence of the skeleton",
                    b.arm, b.label, b.line, highest.1, highest.0
                ));
            } else {
                highest = (g, b.label.clone());
            }
        }
        assert!(
            unplaced.is_empty(),
            "entries with no place in the skeleton — add them to `group`, in the \
             group they belong to:\n{}",
            unplaced.join("\n")
        );
        assert!(out_of_order.is_empty(), "{}", out_of_order.join("\n"));

        // And the scan is still finding the menus, or it passes by seeing nothing.
        assert_eq!(
            arms.len(),
            7,
            "seven `CtxKind` arms are expected, found {arms:?}"
        );
        assert!(
            built.len() >= 35,
            "only {} rows found across the seven menus — has the builder moved?",
            built.len()
        );
    }

    /// The one position in the skeleton that is not a matter of taste: the row the
    /// cursor lands on after a right-click must never be the irreversible one, so
    /// every Drop is the **last** entry its own menu contributes, and the only
    /// thing that follows it is the `AI Explain` row pushed outside the `match`.
    #[test]
    fn drop_is_the_last_entry_before_ai_explain() {
        let src = std::fs::read_to_string(this_file()).expect("this file");
        let built: Vec<Built> = built_entries(&src)
            .into_iter()
            .filter(|b| b.top_level)
            .collect();

        let mut arms: Vec<String> = Vec::new();
        for b in &built {
            if !arms.contains(&b.arm) {
                arms.push(b.arm.clone());
            }
        }
        let mut dropless: Vec<&str> = Vec::new();
        for arm in &arms {
            let rows: Vec<&Built> = built.iter().filter(|b| &b.arm == arm).collect();
            let drops: Vec<&&Built> = rows
                .iter()
                .filter(|b| b.label.starts_with("Drop"))
                .collect();
            if drops.is_empty() {
                dropless.push(arm);
                continue;
            }
            let last = rows.last().expect("a non-empty arm");
            assert!(
                last.label.starts_with("Drop"),
                "{arm}: the menu ends on `{}` at line {}, after the irreversible \
                 `{}` at line {}",
                last.label,
                last.line,
                drops.last().expect("a drop").label,
                drops.last().expect("a drop").line
            );
        }
        // The three read-only menus have nothing to drop; every menu that writes
        // ends on its Drop.
        assert_eq!(
            dropless,
            vec!["Database", "Schema", "ObjectGroup"],
            "which menus carry a Drop has changed"
        );

        // Nothing but `AI Explain` follows the `match` — the tail every menu
        // shares, and the reason a Drop being last in its arm is a Drop being last
        // in the menu.
        let lines: Vec<&str> = src.lines().collect();
        let build_at = lines
            .iter()
            .position(|l| l.contains("let build: Rc<dyn Fn(CtxMenu)"))
            .expect("the builder");
        let ai_at = (build_at..lines.len())
            .find(|&i| lines[i].contains("\"AI Explain\""))
            .expect("the AI Explain row");
        let close_at = (ai_at..lines.len())
            .find(|&i| lines[i].trim() == "});")
            .expect("the builder's close");
        let after: Vec<String> = (ai_at + 1..close_at)
            .filter(|&i| lines[i].contains("MenuEntry::"))
            .map(|i| format!("line {}: {}", i + 1, lines[i].trim()))
            .collect();
        assert!(
            after.is_empty(),
            "an entry was added after `AI Explain`, which every menu ends on:\n{}",
            after.join("\n")
        );
    }

    #[test]
    fn nothing_follows_an_irreversible_entry_in_its_own_menu() {
        let src = std::fs::read_to_string(this_file()).expect("this file");
        let built = built_entries(&src);

        let mut offenders: Vec<String> = Vec::new();
        let mut seen: Option<(String, u32, String)> = None;
        let mut current_arm = String::new();
        for b in &built {
            if b.arm != current_arm {
                current_arm.clone_from(&b.arm);
                seen = None;
            }
            if b.destructive {
                // A second irreversible entry beside the first is the ordinary
                // shape — Truncate then Drop.
                seen = Some((b.arm.clone(), b.line, b.label.clone()));
            } else if let Some((arm, at, first)) = &seen {
                offenders.push(format!(
                    "{arm}: `{}` at line {} comes after the irreversible `{first}` \
                     at line {at} — a right-click would land the cursor on the \
                     entry that can't be taken back",
                    b.label, b.line
                ));
            }
        }
        assert!(offenders.is_empty(), "{}", offenders.join("\n"));

        // The scan has to still be finding the menus, or it passes by seeing
        // nothing: six irreversible entries across the seven arms.
        let marked: Vec<(&str, &str)> = built
            .iter()
            .filter(|b| b.destructive)
            .map(|b| (b.arm.as_str(), b.label.as_str()))
            .collect();
        assert_eq!(
            marked.len(),
            6,
            "has the builder or the `theme::error` marking changed? {marked:?}"
        );
        assert!(
            built.len() > 40,
            "only {} entries found — has the builder moved?",
            built.len()
        );
    }

    /// The entries that mark themselves destructive are the ones that really
    /// are: using that colour for anything else here would make the gate above
    /// silently weaker, since it keys on exactly that.
    #[test]
    fn the_error_colour_marks_the_drops_and_truncate_and_nothing_else() {
        let src = std::fs::read_to_string(this_file()).expect("this file");
        let mut labels: Vec<String> = built_entries(&src)
            .into_iter()
            .filter(|b| b.destructive)
            .map(|b| b.label)
            .collect();
        labels.sort();
        labels.dedup();
        assert_eq!(
            labels,
            vec!["Drop", "Drop foreign key", "Drop index", "Truncate"],
            "an entry gained or lost the irreversible marking"
        );
    }
}
