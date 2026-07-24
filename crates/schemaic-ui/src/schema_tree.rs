//! The SCHEMA sidebar: the database → table → columns/keys tree, its keyboard
//! navigation (`Nav`/`NavRow` + `visible_nav_rows`), the per-node row builders
//! (`db_node`/`table_node`/`column_row`/`key_row`), the disclosure `chevron`,
//! shared `tree_row` styling, and the table-name filter box (`schema_search`).
//! `schema_panel` is the entry point wired into `body`; everything else is
//! internal. Right-clicking any node stages a `CtxMenu` (rendered by
//! `overlays::context_menu_overlay`).

use std::collections::HashSet;
use std::rc::Rc;

use floem::AnyView;
use floem::event::{Event, EventListener, EventPropagation};
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::{Memo, create_effect};

use schemaic_core::db_color::DbColorRule;
use schemaic_core::schema::{
    ColumnInfo, ColumnTypeClass, IndexInfo, SchemaState, TableInfo, classify_column_type,
};

use crate::consts::*;
use crate::widgets::{autohide, loading_dots, section_title, shift_hscroll};
use crate::{ConnNode, CtxKind, CtxMenu, FieldCfg, Ui, db_color_dot, edit_field, icons, theme};

// ===== moved from lib.rs (schema tree) =====
// Keyboard-navigation state for the schema tree. `focused` = the panel has nav
// focus (drives the highlight + arm the arrow keys); `selected` = the current
// row's key (same scheme as `expanded`, extended to leaves). Copy so it threads
// cheaply through the row builders.
#[derive(Clone, Copy)]
struct Nav {
    focused: RwSignal<bool>,
    selected: RwSignal<Option<String>>,
}

// One row in the flattened, currently-visible tree (respecting expand state,
// hidden DBs, and the search filter) — the sequence the arrow keys walk.
struct NavRow {
    key: String,
    parent: Option<String>,
    expandable: bool,
    expanded: bool,
}

// Build the visible-row list in display order. Mirrors the tree's own render
// rules: hidden DBs dropped; a non-empty filter force-expands DBs and narrows
// their tables to name matches; only expanded tables contribute columns+keys.
fn visible_nav_rows(
    db_nodes: RwSignal<Vec<ConnNode>>,
    expanded: RwSignal<HashSet<String>>,
    hidden_dbs: RwSignal<HashSet<String>>,
    filter: RwSignal<String>,
) -> Vec<NavRow> {
    let filt = filter.get_untracked().trim().to_lowercase();
    let filtering = !filt.is_empty();
    let exp = expanded.get_untracked();
    let hidden = hidden_dbs.get_untracked();
    let mut rows = Vec::new();
    for n in db_nodes.get_untracked() {
        if hidden.contains(&n.database) {
            continue;
        }
        let db_key = format!("db:{}", n.database);
        let db_open = exp.contains(&db_key) || filtering;
        rows.push(NavRow {
            key: db_key.clone(),
            parent: None,
            expandable: true,
            expanded: db_open,
        });
        if !db_open {
            continue;
        }
        let SchemaState::Loaded(schema) = n.schema.get_untracked() else {
            continue;
        };
        for t in &schema.tables {
            if filtering && !t.name.to_lowercase().contains(&filt) {
                continue;
            }
            let tbl_key = format!("tbl:{}:{}", n.database, t.name);
            let tbl_open = exp.contains(&tbl_key);
            rows.push(NavRow {
                key: tbl_key.clone(),
                parent: Some(db_key.clone()),
                expandable: true,
                expanded: tbl_open,
            });
            if !tbl_open {
                continue;
            }
            for c in &t.columns {
                rows.push(NavRow {
                    key: format!("col:{}:{}:{}", n.database, t.name, c.name),
                    parent: Some(tbl_key.clone()),
                    expandable: false,
                    expanded: false,
                });
            }
            for ix in &t.indexes {
                rows.push(NavRow {
                    key: format!("idx:{}:{}:{}", n.database, t.name, ix.name),
                    parent: Some(tbl_key.clone()),
                    expandable: false,
                    expanded: false,
                });
            }
        }
    }
    rows
}

// True when this row is the nav cursor (panel focused + key matches). Row
// builders call it in their `.style()` to paint the selection background.
fn is_nav_selected(nav: Nav, key: &str) -> bool {
    nav.focused.get() && nav.selected.with(|s| s.as_deref() == Some(key))
}

// Attach a self-scroll-into-view effect to a row's view: whenever it becomes the
// focused nav cursor, scroll it into the tree viewport. Returns the same view.
fn with_nav_scroll(view: AnyView, nav: Nav, key: String) -> AnyView {
    let id = view.id();
    create_effect(move |_| {
        if nav.focused.get() && nav.selected.with(|s| s.as_deref() == Some(key.as_str())) {
            id.scroll_to(None);
        }
    });
    view
}

// ── Schema sidebar: databases → tables → columns/indexes ─────────────────────
pub(crate) fn schema_panel(ui: Ui) -> impl IntoView {
    let db_nodes = ui.schema.db_nodes;
    let expanded = ui.schema.expanded;
    let on_toggle = ui.schema_actions.on_toggle.clone();
    let open_table = ui.tab_actions.open_table.clone();
    let active_table = ui.schema.active_table;
    let active_db = ui.tabs_ui.active_db;
    let active_conn = ui.conn.active_conn;
    let db_colors = ui.db_colors;
    let hidden_dbs = ui.schema.hidden_dbs;
    let db_menu_open = ui.schema.db_menu_open;
    let schema_menu_open = ui.schema.schema_menu_open;
    let context_menu = ui.overlay.context_menu;
    let schema_w = ui.layout.schema_w;
    // Publish the live panel width so the row styles (`tree_row_min_w`) widen the
    // rows — and their hover/selection highlight — as the panel is resized.
    create_effect(move |_| schema_panel_w().set(schema_w.get()));
    // Close every *other* dropdown when the eye/settings menus open, so all the
    // app's menus are mutually exclusive (the eye/gear absorb their own pointer-down,
    // so the root dismissal handler never runs for them).
    let popup_menu = ui.overlay.popup_menu;
    let conn_menu_open = ui.conn.conn_menu_open;
    let active_db_menu_open = ui.tabs_ui.active_db_menu_open;
    let close_other_menus = move || {
        if popup_menu.get_untracked().is_some() {
            popup_menu.set(None);
        }
        if context_menu.get_untracked().is_some() {
            context_menu.set(None);
        }
        if conn_menu_open.get_untracked() {
            conn_menu_open.set(false);
        }
        if active_db_menu_open.get_untracked() {
            active_db_menu_open.set(false);
        }
    };
    // Live table-name filter (local to the panel). Non-empty ⇒ every database is
    // force-expanded and its tables are narrowed to matches.
    let filter = RwSignal::new(String::new());
    // Keyboard-navigation cursor + focus (local to the panel).
    let nav = Nav {
        focused: RwSignal::new(false),
        selected: RwSignal::new(None),
    };

    // Cloned up front: `on_toggle`/`open_table` are moved into the tree's
    // dyn_stack closure below, but the keyboard-nav handler needs them too
    // (Right/Left toggle; Enter opens the selected table).
    let nav_toggle = on_toggle.clone();
    let nav_open_table = open_table.clone();

    // Not `width_full`: the tree sizes to its widest row so the horizontal
    // scrollbar appears when a deep/long row overflows the panel. Hidden
    // databases are filtered out entirely (also drops them from local search).
    let tree = dyn_stack(
        move || {
            db_nodes
                .get()
                .into_iter()
                .filter(|c| !hidden_dbs.get().contains(&c.database))
                .collect::<Vec<_>>()
        },
        |c: &ConnNode| c.id,
        move |c| {
            db_node(
                c,
                SchemaTreeCtx {
                    expanded,
                    filter,
                    on_toggle: on_toggle.clone(),
                    open_table: open_table.clone(),
                    active_table,
                    active_db,
                    context_menu,
                    active_conn,
                    db_colors,
                    nav,
                },
            )
        },
    )
    .style(|s| s.flex_col());

    // Keyboard navigation: arrow keys walk `visible_nav_rows` when the tree has
    // focus. Down/Up move the cursor (never expand); Right expands a collapsed
    // node (or steps into the first child if already open); Left collapses an
    // open node (or steps to the parent). The tree is `keyboard_navigable`, so
    // clicking anywhere in it focuses it (and clicking away blurs it) — and while
    // the search box holds focus, key events go there instead, so its own arrow
    // handling (caret movement) wins and tree nav is naturally disabled.
    let tree = autohide(shift_hscroll(tree))
        .keyboard_navigable()
        .on_event(EventListener::FocusGained, move |_| {
            nav.focused.set(true);
            // Start from the active table whenever one is open and visible;
            // otherwise resume wherever the cursor last was.
            if let Some((db, tbl)) = active_table.get_untracked() {
                let k = format!("tbl:{db}:{tbl}");
                let rows = visible_nav_rows(db_nodes, expanded, hidden_dbs, filter);
                if rows.iter().any(|r| r.key == k) {
                    nav.selected.set(Some(k));
                }
            }
            EventPropagation::Continue
        })
        .on_event(EventListener::FocusLost, move |_| {
            nav.focused.set(false);
            EventPropagation::Continue
        })
        .on_event(EventListener::KeyDown, move |e| {
            let Event::KeyDown(ke) = e else {
                return EventPropagation::Continue;
            };
            // Enter on a selected table row opens it (a new query tab, or the
            // existing one if it's already open — see `open_table` in the app).
            if matches!(ke.key.logical_key, Key::Named(NamedKey::Enter)) {
                if let Some(sel) = nav.selected.get_untracked() {
                    for node in db_nodes.get_untracked() {
                        let prefix = format!("tbl:{}:", node.database);
                        if let Some(table) = sel.strip_prefix(&prefix) {
                            (nav_open_table)(node.database.clone(), table.to_string());
                            break;
                        }
                    }
                }
                return EventPropagation::Stop;
            }
            let dir = match ke.key.logical_key {
                Key::Named(NamedKey::ArrowDown) => 1i32,
                Key::Named(NamedKey::ArrowUp) => -1,
                Key::Named(NamedKey::ArrowRight) => 2,
                Key::Named(NamedKey::ArrowLeft) => -2,
                _ => return EventPropagation::Continue,
            };
            let rows = visible_nav_rows(db_nodes, expanded, hidden_dbs, filter);
            if rows.is_empty() {
                return EventPropagation::Stop;
            }
            let cur = nav.selected.get_untracked();
            let pos = cur
                .as_ref()
                .and_then(|k| rows.iter().position(|r| &r.key == k));
            match dir {
                1 => {
                    let ni = match pos {
                        Some(i) => (i + 1).min(rows.len() - 1),
                        None => 0,
                    };
                    nav.selected.set(Some(rows[ni].key.clone()));
                }
                -1 => {
                    let ni = match pos {
                        Some(i) => i.saturating_sub(1),
                        None => rows.len() - 1,
                    };
                    nav.selected.set(Some(rows[ni].key.clone()));
                }
                2 => match pos {
                    Some(i) if rows[i].expandable && !rows[i].expanded => {
                        (nav_toggle)(rows[i].key.clone());
                    }
                    Some(i) if rows[i].expandable => {
                        if let Some(child) = rows.get(i + 1)
                            && child.parent.as_deref() == Some(rows[i].key.as_str())
                        {
                            nav.selected.set(Some(child.key.clone()));
                        }
                    }
                    Some(_) => {}
                    None => nav.selected.set(Some(rows[0].key.clone())),
                },
                -2 => match pos {
                    Some(i) if rows[i].expandable && rows[i].expanded => {
                        (nav_toggle)(rows[i].key.clone());
                    }
                    Some(i) => {
                        if let Some(parent) = rows[i].parent.clone() {
                            nav.selected.set(Some(parent));
                        }
                    }
                    None => nav.selected.set(Some(rows[0].key.clone())),
                },
                _ => {}
            }
            EventPropagation::Stop
        })
        .style(|s| {
            s.flex_grow(1.0_f32)
                .width_full()
                .min_height(0.0)
                .min_width(0.0)
        });

    // Title row: "SCHEMA" left; the visibility (eye) and settings (gear) menus
    // right. The gear is rightmost; the eye sits 10px to its left. Each icon
    // closes the OTHER menu and toggles its own — so clicking one while the
    // other is open switches in a single click. The icons absorb their own
    // PointerDown so the root-level dismiss handler (see `workspace`) doesn't
    // pre-close the menu and cause a toggle to immediately reopen it.
    let eye_hov = RwSignal::new(false);
    let eye = container(icons::icon(icons::EYE, 16.0).style(move |s| {
        s.flex_shrink(0.0_f32).color(if eye_hov.get() {
            theme::text()
        } else {
            theme::text_muted()
        })
    }))
    .on_click_stop(move |_| {
        close_other_menus();
        schema_menu_open.set(false);
        db_menu_open.update(|o| *o = !*o);
    })
    .on_event_stop(EventListener::PointerDown, |_| {})
    .on_event_cont(EventListener::PointerEnter, move |_| eye_hov.set(true))
    .on_event_cont(EventListener::PointerLeave, move |_| eye_hov.set(false))
    .style(|s| {
        s.items_center()
            .margin_top(4.0)
            .margin_right(2.0)
            .padding_horiz(5.0)
            .padding_vert(3.0)
    });
    let gear_hov = RwSignal::new(false);
    let gear = container(icons::icon(icons::SLIDERS_VERTICAL, 16.0).style(move |s| {
        s.flex_shrink(0.0_f32).color(if gear_hov.get() {
            theme::text()
        } else {
            theme::text_muted()
        })
    }))
    .on_click_stop(move |_| {
        close_other_menus();
        db_menu_open.set(false);
        schema_menu_open.update(|o| *o = !*o);
    })
    .on_event_stop(EventListener::PointerDown, |_| {})
    .on_event_cont(EventListener::PointerEnter, move |_| gear_hov.set(true))
    .on_event_cont(EventListener::PointerLeave, move |_| gear_hov.set(false))
    .style(|s| {
        s.items_center()
            .margin_top(4.0)
            .margin_right(9.0)
            .padding_horiz(5.0)
            .padding_vert(3.0)
    });
    // Title left, icon group right. `justify_between` pins the group's right edge
    // to the panel edge (a lone flex-grow spacer under-fills here — its default
    // `flex_basis: auto` leaves ~18px unclaimed — so we don't rely on it). The
    // gear's `margin_right(14)` sets its 14px inset from the panel edge.
    let icons_group =
        h_stack((eye, gear)).style(|s| s.flex_row().items_start().flex_shrink(0.0_f32));
    let title_row = h_stack((section_title("SCHEMA"), icons_group))
        .style(|s| s.width_full().flex_row().items_start().justify_between());

    v_stack((
        title_row,
        // Spacers (not search margins): 5px above / 10px below the box, which land
        // at ~15px each visually (the box + title row add ~10px top / ~5px bottom of
        // their own). Spacers not margins because any vertical margin on a flex
        // sibling isn't subtracted from the flex-grow scroll's height, so it
        // overflows and clips short of the footer.
        empty().style(|s| s.height(5.0).flex_shrink(0.0_f32)),
        schema_search(filter),
        empty().style(|s| s.height(10.0).flex_shrink(0.0_f32)),
        tree,
    ))
    .style(move |s| {
        s.width(schema_w.get())
            .flex_shrink(0.0_f32)
            .height_full()
            // Let the flex-grow tree scroll consume all remaining height down to
            // the footer (without this the panel bottoms out ~25px short).
            .min_height(0.0)
            .flex_col()
            .background(theme::bg_panel())
            .border_right(1.0)
            .border_color(theme::border())
    })
}

// A database node: a header row over its lazily-loaded tables. Double-clicking
// the row — or clicking its chevron — expands/collapses.
/// Shared context threaded through the schema-tree row builders (`db_node`,
/// `table_node`). Bundled to keep each builder's argument count in check; cheap to
/// clone (signals are `Copy`, the two callbacks are `Rc`).
#[derive(Clone)]
struct SchemaTreeCtx {
    expanded: RwSignal<HashSet<String>>,
    filter: RwSignal<String>,
    on_toggle: Rc<dyn Fn(String)>,
    open_table: Rc<dyn Fn(String, String)>,
    active_table: RwSignal<Option<(String, String)>>,
    active_db: Memo<Option<String>>,
    context_menu: RwSignal<Option<CtxMenu>>,
    /// The active connection id + the app-wide DB-colour store, for the identity
    /// dot on database rows. (Schema-tree nodes all belong to the active connection.)
    active_conn: RwSignal<u64>,
    db_colors: RwSignal<Vec<DbColorRule>>,
    nav: Nav,
}

fn db_node(conn: ConnNode, ctx: SchemaTreeCtx) -> impl IntoView {
    let SchemaTreeCtx {
        expanded,
        filter,
        on_toggle,
        open_table,
        active_table,
        active_db,
        context_menu,
        active_conn,
        db_colors,
        nav,
    } = ctx;
    let key = format!("db:{}", conn.database);
    let schema_sig = conn.schema;

    let toggle_row = on_toggle.clone();
    let key_row = key.clone();
    let ctx_db = conn.database.clone();
    let dot_db = conn.database.clone();
    let header = h_stack((
        chevron(expanded, key.clone(), on_toggle.clone()),
        icons::icon(icons::DATABASE, SCHEMA_ICON as f32).style(|s| {
            s.color(theme::db_icon())
                .margin_left(CHEVRON_GAP)
                .margin_right(ICON_GAP)
                .flex_shrink(0.0_f32)
        }),
        text(conn.name.clone()).style(|s| s.color(theme::text()).font_bold()),
        // Identity dot after the name (only when this database has a colour).
        db_color_dot(
            db_colors,
            move || Some((active_conn.get(), dot_db.clone())),
            7.0,
            0.0,
            1.0,
        ),
    ))
    .on_double_click_stop(move |_| (toggle_row)(key_row.clone()))
    .on_secondary_click_stop(move |_| {
        let ai_prompt = format!(
            "Give me a concise overview of the `{ctx_db}` database — the domain it models, \
             its key tables, and how they relate."
        );
        context_menu.set(Some(CtxMenu {
            kind: CtxKind::Database,
            name: ctx_db.clone(),
            ai_prompt,
        }));
    })
    .style({
        let hl = key.clone();
        let db_name = conn.database.clone();
        move |s| {
            let s = tree_row(s, ROW_PAD);
            if is_nav_selected(nav, &hl) {
                s.background(theme::row_selected())
            } else if active_db.get().as_deref() == Some(db_name.as_str()) {
                // The active "use database" context — a resting highlight like an
                // open table's row.
                s.background(theme::row_active())
            } else {
                s
            }
        }
    });
    let header = with_nav_scroll(header.into_any(), nav, key.clone());

    // Children rebuild on expand/schema/filter change. A non-empty filter
    // force-expands the node and narrows its tables to name matches.
    let key_children = key.clone();
    let database = conn.database.clone();
    let ot_tables = open_table;
    let toggle_tables = on_toggle;
    let children = dyn_container(
        move || {
            (
                expanded.get().contains(&key_children),
                schema_sig.get(),
                filter.get(),
            )
        },
        move |(open, state, filt)| {
            let filt = filt.trim().to_lowercase();
            let filtering = !filt.is_empty();
            if !open && !filtering {
                return empty().into_any();
            }
            match state {
                // Animated dots (matches info_row's layout) while the schema loads.
                SchemaState::Loading => container(loading_dots(
                    "Loading",
                    theme::text_muted,
                    theme::FONT_LABEL,
                ))
                .style(|s| {
                    s.min_width(tree_row_min_w())
                        .padding_left(LEAF_PAD)
                        .padding_vert(3.0)
                })
                .into_any(),
                SchemaState::Failed(e) => info_row(e, theme::error()).into_any(),
                SchemaState::Loaded(schema) => {
                    let tables: Vec<TableInfo> = schema
                        .tables
                        .into_iter()
                        .filter(|t| !filtering || t.name.to_lowercase().contains(&filt))
                        .collect();
                    if tables.is_empty() {
                        // Hide the node's body entirely while filtering with no
                        // match; otherwise show the empty-schema hint.
                        return if filtering {
                            empty().into_any()
                        } else {
                            info_row("No tables", theme::text_muted()).into_any()
                        };
                    }
                    let db = database.clone();
                    let ot = ot_tables.clone();
                    let toggle = toggle_tables.clone();
                    v_stack_from_iter(tables.into_iter().map(move |t| {
                        table_node(
                            db.clone(),
                            t,
                            SchemaTreeCtx {
                                expanded,
                                filter,
                                on_toggle: toggle.clone(),
                                open_table: ot.clone(),
                                active_table,
                                active_db,
                                context_menu,
                                active_conn,
                                db_colors,
                                nav,
                            },
                        )
                    }))
                    .style(|s| s.flex_col())
                    .into_any()
                }
            }
        },
    );

    v_stack((header, children)).style(|s| s.flex_col())
}

// A table node: a header row (double-click opens & runs `SELECT *`) over its
// columns then indexes, shown when expanded. Highlighted while it is the
// active tab's source table.
fn table_node(database: String, table: TableInfo, ctx: SchemaTreeCtx) -> impl IntoView {
    let SchemaTreeCtx {
        expanded,
        on_toggle,
        open_table,
        active_table,
        context_menu,
        nav,
        ..
    } = ctx;
    let key = format!("tbl:{}:{}", database, table.name);
    let col_count = table.columns.len();
    let key_count = table.indexes.len();
    let ddl = table.create_ddl();
    // Views get a distinct glyph + tint; base tables keep the green table icon.
    let (glyph, glyph_color) = if table.is_view {
        (icons::TABLE_CELLS_MERGE, theme::view_icon())
    } else {
        (icons::TABLE, theme::table_icon())
    };

    let dbl_db = database.clone();
    let dbl_table = table.name.clone();
    let hl_db = database.clone();
    let hl_table = table.name.clone();
    let ctx_db = database.clone();
    let ctx_table = table.name.clone();
    let header = h_stack((
        chevron(expanded, key.clone(), on_toggle),
        icons::icon(glyph, SCHEMA_ICON as f32).style(move |s| {
            s.color(glyph_color)
                .margin_left(CHEVRON_GAP)
                .margin_right(ICON_GAP)
                .flex_shrink(0.0_f32)
        }),
        text(table.name.clone()).style(|s| s.color(theme::text())),
    ))
    .on_double_click_stop(move |_| (open_table)(dbl_db.clone(), dbl_table.clone()))
    .on_secondary_click_stop(move |_| {
        let ai_prompt = format!(
            "Explain the `{ctx_table}` table in the `{ctx_db}` database: what each column \
             represents, the primary key, and any foreign-key relationships. Keep it concise."
        );
        context_menu.set(Some(CtxMenu {
            kind: CtxKind::Table {
                database: ctx_db.clone(),
                table: ctx_table.clone(),
                ddl: ddl.clone(),
            },
            name: ctx_table.clone(),
            ai_prompt,
        }));
    })
    .style({
        let hl = key.clone();
        move |s| {
            let s = tree_row(s, ROW_PAD + LEVEL_INDENT);
            if is_nav_selected(nav, &hl) {
                s.background(theme::row_selected())
            } else if active_table.get().as_ref() == Some(&(hl_db.clone(), hl_table.clone())) {
                s.background(theme::row_active())
            } else {
                s
            }
        }
    });
    let header = with_nav_scroll(header.into_any(), nav, key.clone());

    let key_children = key.clone();
    let cols = table.columns;
    let idxs = table.indexes;
    let cols_db = database.clone();
    let cols_table = table.name.clone();
    let children = dyn_container(
        move || expanded.get().contains(&key_children),
        move |open| {
            if !open {
                return empty().into_any();
            }
            let counts = count_row(col_count, key_count);
            let (cdb, ctbl) = (cols_db.clone(), cols_table.clone());
            // Columns backing a FOREIGN KEY index — tinted purple like their key.
            let fk_cols: HashSet<String> = idxs
                .iter()
                .filter(|ix| ix.foreign)
                .flat_map(|ix| ix.columns.iter().cloned())
                .collect();
            let cols_block = v_stack_from_iter(cols.iter().cloned().map(move |c| {
                let ckind = if c.primary_key {
                    ColKey::Primary
                } else if fk_cols.contains(&c.name) {
                    ColKey::Foreign
                } else {
                    ColKey::None
                };
                column_row(c, ckind, context_menu, cdb.clone(), ctbl.clone(), nav)
            }))
            .style(|s| s.flex_col());
            let (kdb, ktbl) = (cols_db.clone(), cols_table.clone());
            let keys_block = v_stack_from_iter(
                idxs.iter()
                    .cloned()
                    .map(move |ix| key_row(ix, context_menu, kdb.clone(), ktbl.clone(), nav)),
            )
            .style(|s| s.flex_col());
            v_stack((counts, cols_block, keys_block))
                .style(|s| s.flex_col())
                .into_any()
        },
    );

    v_stack((header, children)).style(|s| s.flex_col())
}

// The "N cols · M keys" capsule row shown directly under a table's header.
fn count_row(cols: usize, keys: usize) -> impl IntoView {
    h_stack((
        capsule(format!("{cols} cols")),
        capsule(format!("{keys} keys")),
    ))
    .style(|s| {
        s.flex_row()
            .gap(5.0)
            .padding_left(LEAF_PAD)
            .margin_top(6.0)
            .margin_bottom(6.0)
    })
}

fn capsule(label: String) -> impl IntoView {
    container(text(label).style(|s| s.font_size(theme::FONT_LABEL).color(theme::text_muted())))
        .style(|s| {
            s.height(18.0)
                .items_center()
                .justify_center()
                .padding_horiz(7.0)
                .background(theme::capsule_bg())
                .border_radius(4.0)
        })
}

// A single column (leaf): name then, 12px to its right, the SQL type. Primary
// keys take the gold accent. No right-alignment — the type trails the name.
/// Whether a column participates in a key, for tinting its row (the glyph still
/// reflects the column's *type*; only the colour signals key membership).
#[derive(Clone, Copy, PartialEq)]
enum ColKey {
    Primary,
    Foreign,
    None,
}

impl ColKey {
    /// Row colour: gold PK / purple FK / normal text.
    fn color(self) -> floem::peniko::Color {
        match self {
            ColKey::Primary => theme::key_primary(),
            ColKey::Foreign => theme::key_foreign(),
            ColKey::None => theme::text(),
        }
    }
}

/// The schema-tree glyph for a column type family.
fn column_type_icon(class: ColumnTypeClass) -> &'static str {
    match class {
        ColumnTypeClass::Text => icons::TYPE,
        ColumnTypeClass::Numeric => icons::HASH,
        ColumnTypeClass::Boolean => icons::CIRCLE_DOT,
        ColumnTypeClass::DateTime => icons::CALENDAR,
        ColumnTypeClass::Json => icons::BRACES,
        ColumnTypeClass::Binary => icons::FILE_DIGIT,
        ColumnTypeClass::Other => icons::PANEL_LEFT_DASHED,
    }
}

fn column_row(
    c: ColumnInfo,
    kind: ColKey,
    context_menu: RwSignal<Option<CtxMenu>>,
    database: String,
    table: String,
    nav: Nav,
) -> impl IntoView {
    let name = c.name;
    let ty = c.type_name;
    let ctx_name = name.clone();
    let ctx_ty = ty.clone();
    let nav_key = format!("col:{database}:{table}:{name}");
    // The glyph always reflects the column's *type* family — the key glyph is for
    // the key/index rows, not the columns they cover (so `id` is a numeric column,
    // and `PRIMARY(id)` is the key). Key membership only tints the row: a PK column
    // stays gold, an FK column purple. The icon is a 50%-alpha version of that
    // colour so it reads as a quieter marker beside the full-strength name.
    let glyph = column_type_icon(classify_column_type(&ty));
    let row = h_stack((
        icons::icon(glyph, SCHEMA_ICON as f32)
            .style(move |s| s.color(kind.color().multiply_alpha(0.5)).margin_right(ICON_GAP).flex_shrink(0.0_f32)),
        text(name),
        text(ty).style(|s| {
            s.color(theme::text_muted())
                .font_size(theme::FONT_LABEL)
                .margin_left(12.0)
        }),
    ))
    // The name inherits `kind.color()`; the icon overrides to 50% of it above and
    // the type text to muted.
    .style(move |s| s.color(kind.color()).items_center())
    .on_secondary_click_stop(move |_| {
        let ai_prompt = format!(
            "In `{database}`.`{table}`, explain the `{ctx_name}` column (type `{ctx_ty}`) — \
             what it stores and how it's typically used."
        );
        context_menu.set(Some(CtxMenu {
            kind: CtxKind::Field,
            name: ctx_name.clone(),
            ai_prompt,
        }));
    })
    .style({
        let hl = nav_key.clone();
        move |s| {
            let s = tree_row(s, COL_PAD);
            if is_nav_selected(nav, &hl) {
                s.background(theme::row_selected())
            } else {
                s
            }
        }
    });
    with_nav_scroll(row.into_any(), nav, nav_key)
}

// A single key (leaf): name + its columns, colored by kind (PRIMARY gold,
// FOREIGN purple, other indexes blue), with a trailing UNIQUE/INDEX/FOREIGN tag.
fn key_row(
    ix: IndexInfo,
    context_menu: RwSignal<Option<CtxMenu>>,
    database: String,
    table: String,
    nav: Nav,
) -> impl IntoView {
    let (color, tag) = if ix.is_primary() {
        (theme::key_primary(), "UNIQUE")
    } else if ix.foreign {
        (theme::key_foreign(), "FOREIGN")
    } else if ix.unique {
        (theme::key_index(), "UNIQUE")
    } else {
        (theme::key_index(), "INDEX")
    };
    let kind = if ix.foreign { "foreign key" } else { "index" };
    let cols = ix.columns.join(", ");
    let ctx_name = ix.name.clone();
    let label = format!("{} ({cols})", ix.name);
    let nav_key = format!("idx:{database}:{table}:{}", ix.name);
    let row = h_stack((
        icons::icon(icons::KEY_ROUND, SCHEMA_ICON as f32).style(move |s| {
            // 50%-alpha key colour, matching the column icons' quieter marker.
            s.color(color.multiply_alpha(0.5))
                .margin_right(ICON_GAP)
                .flex_shrink(0.0_f32)
        }),
        text(label),
        text(tag).style(|s| {
            s.color(theme::text_muted())
                .font_size(theme::FONT_LABEL)
                .margin_left(12.0)
        }),
    ))
    // Label + key glyph share the index's colour (gold PK / purple FK / blue);
    // the trailing tag overrides to muted above.
    .style(move |s| s.color(color).items_center())
    .on_secondary_click_stop(move |_| {
        let ai_prompt = format!(
            "In `{database}`.`{table}`, explain the `{ctx_name}` {kind} on ({cols}) — its \
             purpose (uniqueness, faster lookups, or a foreign-key relationship)."
        );
        context_menu.set(Some(CtxMenu {
            kind: CtxKind::Field,
            name: ctx_name.clone(),
            ai_prompt,
        }));
    })
    .style({
        let hl = nav_key.clone();
        move |s| {
            let s = tree_row(s, COL_PAD);
            if is_nav_selected(nav, &hl) {
                s.background(theme::row_selected())
            } else {
                s
            }
        }
    });
    with_nav_scroll(row.into_any(), nav, nav_key)
}

// A clickable disclosure chevron: chevron-down when expanded, chevron-right
// when collapsed. The SVG inherits the container's text color (muted, brighter
// on hover). Clicking toggles the node (propagation stopped).
fn chevron(
    expanded: RwSignal<HashSet<String>>,
    key: String,
    on_toggle: Rc<dyn Fn(String)>,
) -> impl IntoView {
    let key_read = key.clone();
    let glyph = dyn_container(
        move || expanded.get().contains(&key_read),
        move |open| {
            let svg = if open {
                icons::CHEVRON_DOWN
            } else {
                icons::CHEVRON_RIGHT
            };
            icons::icon(svg, SCHEMA_ICON as f32).into_any()
        },
    );
    container(glyph)
        .on_click_stop(move |_| (on_toggle)(key.clone()))
        .style(|s| {
            s.width(SCHEMA_ICON)
                .height(TREE_ROW_H)
                .flex_shrink(0.0_f32)
                .items_center()
                .justify_center()
                .color(theme::text_muted())
                .hover(|s| s.color(theme::text()))
        })
}

// Shared height/hover styling for every tree row; `pad_left` sets the indent.
thread_local! {
    static SCHEMA_PANEL_W: std::cell::RefCell<Option<(RwSignal<f64>, floem::reactive::Scope)>> =
        const { std::cell::RefCell::new(None) };
}

/// Live schema-panel width, published by [`schema_panel`] and read by the row
/// styles. Detached scope → lives for the whole process (like `window_size`).
fn schema_panel_w() -> RwSignal<f64> {
    SCHEMA_PANEL_W.with(|cell| {
        if cell.borrow().is_none() {
            let scope = floem::reactive::Scope::new();
            let sig = scope.create_rw_signal(theme::SCHEMA_W);
            *cell.borrow_mut() = Some((sig, scope));
        }
        cell.borrow().as_ref().unwrap().0
    })
}

/// The width a tree row should fill so its hover/selection highlight spans the
/// panel (tracking a live resize), while still overflowing for long content.
/// Reading `schema_panel_w()` inside a reactive `.style(…)` closure re-runs it on
/// resize. `TREE_ROW_MIN_W` is the floor (before the panel width is published).
fn tree_row_min_w() -> f64 {
    // −2 (not −3): the row's highlight then sits flush inside the panel's 1px
    // `border_right` instead of stopping 1px short of it.
    (schema_panel_w().get() - 2.0).max(TREE_ROW_MIN_W)
}

// Rows fill the panel width (so hover/selection highlight spans it) via a live
// `min_width`; long content still overflows and the sidebar gains a horizontal
// scrollbar.
fn tree_row(s: floem::style::Style, pad_left: f64) -> floem::style::Style {
    s.min_width(tree_row_min_w())
        .height(TREE_ROW_H)
        .min_height(TREE_ROW_H)
        .items_center()
        .flex_row()
        .padding_left(pad_left)
        .padding_right(8.0)
        .font_size(theme::FONT_BODY)
        .hover(|s| s.background(theme::row_hover()))
}

// A non-interactive status line inside the tree (Loading / error / empty).
fn info_row(msg: impl Into<String>, color: floem::peniko::Color) -> impl IntoView {
    let msg = msg.into();
    container(text(msg).style(move |s| s.color(color).font_size(theme::FONT_LABEL))).style(
        move |s| {
            s.min_width(tree_row_min_w())
                .padding_left(LEAF_PAD)
                .padding_vert(3.0)
        },
    )
}

// The schema-tree table-name filter. Non-empty ⇒ databases force-expand and
// their tables narrow to matches (see `db_node`).
fn schema_search(filter: RwSignal<String>) -> impl IntoView {
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
