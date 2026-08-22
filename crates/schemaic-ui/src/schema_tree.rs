//! The SCHEMA sidebar: the database → table → columns/keys tree, its keyboard
//! navigation (`Nav`/`NavRow` + `visible_nav_rows`), the per-node row builders
//! (`db_node`/`table_node`/`column_row`/`key_row`), the disclosure `chevron`,
//! shared `tree_row` styling, and the table-name filter box (`schema_search`).
//! `schema_panel` is the entry point wired into `body`; everything else is
//! internal. Right-clicking any node stages a `CtxMenu` (rendered by
//! `overlays::context_menu_overlay`).

use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use floem::AnyView;
use floem::action::exec_after;
use floem::event::{Event, EventListener, EventPropagation};
use floem::keyboard::{Key, NamedKey};
use floem::kurbo::Point;
use floem::prelude::*;
use floem::reactive::{Memo, create_effect};

use schemaic_core::db_color::DbColorRule;
use schemaic_core::ddl::ObjectKind;
use schemaic_core::favorite::FavoriteRule;
use schemaic_core::intel::SqlDialect;
use schemaic_core::schema::{
    ColumnInfo, ColumnTypeClass, DbSchema, IndexInfo, ObjectItem, SchemaState, TableInfo,
    TableSource, classify_column_type, db_visible,
};
use schemaic_core::text::plural;

use crate::consts::*;
use crate::widgets::{
    autohide, debounced, highlight_text, loading_dots, section_title, shift_hscroll,
};
use crate::{
    ConnNode, CtxKind, CtxMenu, FieldCfg, Ui, db_color_dot, edit_field, favorite_star, icons, theme,
};

// ===== moved from lib.rs (schema tree) =====
// Keyboard-navigation state for the schema tree. `focused` = the panel has nav
// focus (drives the highlight + arm the arrow keys); `selected` = the current
// row's key (same scheme as `expanded`, extended to leaves). Copy so it threads
// cheaply through the row builders.
#[derive(Clone, Copy)]
struct Nav {
    focused: RwSignal<bool>,
    selected: RwSignal<Option<String>>,
    /// One-shot "scroll this row into view" request (its key), consumed by
    /// `with_nav_scroll`. Set only by keyboard navigation — NOT by focus-gain — so
    /// clicking a row to focus the tree never yanks the viewport to another row.
    reveal: RwSignal<Option<String>>,
    /// Where the cursor row's **content** starts along the bottom edge, in window
    /// coords — where a menu raised from the keyboard should drop from. Its
    /// content and not its box, because a row spans the whole panel and the panel
    /// is flush left, so every row's own x is 0.
    ///
    /// Focus lives on the tree container, not on a row, so a key pressed in the
    /// tree has no idea where the cursor is on screen, and a context menu it
    /// raises has to appear at the row it is about rather than at whatever the
    /// pointer last touched.
    ///
    /// From `on_move`/`on_resize` on the row, which is how every other anchored
    /// menu in the app finds its widget: floem fires `on_move` **during layout**
    /// with the view's *window* origin, so it stays true across scrolling, and
    /// `on_resize` supplies the height (its own rect is view-local, so only the
    /// size is taken from it).
    cursor_at: RwSignal<Option<(f64, f64)>>,
    /// How to raise the cursor row's context menu — see [`CtxOpener`]. Published
    /// beside `cursor_view` by the same effect, so the two always describe the
    /// same row.
    cursor_menu: RwSignal<Option<CtxOpener>>,
    /// The key of the row whose **context menu is open**, painted by
    /// [`with_nav_scroll`] as a rule above and below it.
    ///
    /// A menu is a panel floating away from the row it acts on, and once the
    /// pointer has moved onto it there was nothing left on screen saying which
    /// database, table or column *this* Drop is about. Not the nav cursor: a
    /// right-click deliberately doesn't move that (see [`resume_cursor`] — a cursor
    /// that exists is the user's), so this is a second, shorter-lived mark that
    /// lives exactly as long as the menu.
    ///
    /// Set by [`marking_opener`], cleared by watching the menu itself go away.
    menu_row: RwSignal<Option<String>>,
}

/// A row's context menu, as a function of **where** to open it: `None` at the
/// pointer (a right-click), `Some(window coords)` at a named place (Shift+F10,
/// which opens at the row itself).
///
/// One per row that has a menu, called by that row's `on_secondary_click_stop`
/// and published to [`Nav::cursor_menu`] while the row is the cursor — so the two
/// routes cannot drift into offering different menus for the same row.
type CtxOpener = Rc<dyn Fn(Option<(f64, f64)>)>;

/// Move the nav cursor to `key` AND request it scrolled into view (keyboard nav).
fn nav_select(nav: Nav, key: String) {
    nav.selected.set(Some(key.clone()));
    nav.reveal.set(Some(key));
}

// ── Tree identity: one place that builds every node's key ────────────────────
//
// Keys are shared by the expand/collapse set, the nav cursor, and the persisted
// expansion state, so the render and `visible_nav_rows` must agree exactly. A
// table's key uses its *display* name (`schema.table` outside PostgreSQL's
// `public`), which keeps every MySQL and single-schema key byte-identical to
// what earlier versions wrote — a saved expansion set still applies.

/// The expansion-set key for a database row.
///
/// Public because the app needs it too: the size column is fetched only for
/// *expanded* databases, and that check is a lookup in the same `HashSet` the
/// tree writes. A second `format!("db:{…}")` over there is exactly the drift
/// the comment above warns about.
pub fn db_key(database: &str) -> String {
    format!("db:{database}")
}

/// A PostgreSQL namespace group. Only rendered when a database has more than one
/// (see [`schema_groups`]), so this key never appears for MySQL.
fn schema_key(database: &str, schema: &str) -> String {
    format!("sch:{database}:{schema}")
}

fn table_key(database: &str, t: &TableInfo) -> String {
    table_key_named(
        database,
        &schemaic_core::schema::display_name(t.schema.as_deref(), &t.name),
    )
}

/// [`table_key`] for a caller that already has the displayed name — a
/// `TableSource::display()`, or a database being collapsed wholesale.
///
/// Three sites used to format `tbl:{db}:{name}` inline, under a section headed
/// "one place that builds every node's key". Each was correct, and each was a
/// place the format could drift from the tests, which only ever exercised the
/// builders.
pub fn table_key_named(database: &str, table: &str) -> String {
    format!("tbl:{database}:{table}")
}

/// The prefix every one of `database`'s table keys starts with — what
/// "collapse this database" matches on.
pub fn table_key_prefix(database: &str) -> String {
    format!("tbl:{database}:")
}

fn column_key(database: &str, t: &TableInfo, column: &str) -> String {
    column_key_named(
        database,
        &schemaic_core::schema::display_name(t.schema.as_deref(), &t.name),
        column,
    )
}

/// [`column_key`] for a caller that already has the displayed table name.
pub fn column_key_named(database: &str, table: &str, column: &str) -> String {
    format!("col:{database}:{table}:{column}")
}

/// The `Types`/`Domains`/`Sequences` folder under a database or namespace.
///
/// Scoped by the *tree* level rather than by the objects' own namespace, for the
/// same reason [`TableScope`] exists: flat means "this database has no schema
/// level", and its one folder covers every namespace's objects.
fn object_group_key(database: &str, scope: TableScope, kind: ObjectKind) -> String {
    format!(
        "objgrp:{database}:{}:{}",
        scope.name().unwrap_or_default(),
        kind.label()
    )
}

/// A key/index leaf row, **for the context-menu mark only**.
///
/// Its own prefix, and deliberately outside every other key's: a key row is not in
/// the nav sequence (no cursor, no expansion, nothing persisted — see
/// [`key_row`]), so this string is never compared against `expanded` or
/// `Nav::selected` and cannot collide with one that is. It exists because the row
/// still *has* a context menu, and a marker that skipped it would be a marker the
/// user learns not to trust.
fn key_row_menu_key(database: &str, table: &str, index: &str) -> String {
    format!("keyrow:{database}:{table}:{index}")
}

fn object_key(database: &str, scope: TableScope, kind: ObjectKind, name: &str) -> String {
    format!(
        "obj:{database}:{}:{}:{name}",
        scope.name().unwrap_or_default(),
        kind.label()
    )
}

/// The object folders one tree level shows, in a fixed order, skipping the empty
/// ones — an empty folder is a click that leads nowhere.
///
/// Fixed order rather than "biggest first" so the tree doesn't rearrange itself
/// when a type is added.
fn object_groups(schema: &DbSchema, scope: TableScope) -> Vec<(ObjectKind, Vec<ObjectItem>)> {
    ObjectKind::ALL
        .into_iter()
        .filter_map(|k| {
            let items = match scope {
                TableScope::Flat => schema.objects_all(k),
                TableScope::Namespace(ns) => schema.objects_in(Some(ns), k),
            };
            (!items.is_empty()).then_some((k, items))
        })
        .collect()
}

/// The folder's label. Plural, because it names a group.
fn object_group_label(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Enum => "Types",
        ObjectKind::Domain => "Domains",
        ObjectKind::Sequence => "Sequences",
        ObjectKind::Function => "Functions",
        ObjectKind::Procedure => "Procedures",
    }
}

/// The glyph for a standalone object's kind — shared with the Find-Anywhere
/// palette, so a type looks the same wherever it is listed.
pub(crate) fn object_icon(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Enum => icons::TAG,
        ObjectKind::Domain => icons::SCAN_SQUARE,
        ObjectKind::Sequence => icons::FILE_DIGIT,
        // Two glyphs, not one: a function returns something and a procedure is
        // called for its effect, and the folders they sit in are separate for
        // exactly that reason.
        ObjectKind::Function => icons::SQUARE_FUNCTION,
        ObjectKind::Procedure => icons::SQUARE_PLAY,
    }
}

/// Does anything in this database match by object name?
///
/// The database and namespace filters both need this, because a search for a
/// type would otherwise hide the very database that defines it — the level above
/// is dropped before the folder holding the match is ever reached.
fn has_object_match(schema: &DbSchema, filt: &str) -> bool {
    ObjectKind::ALL
        .into_iter()
        .any(|k| schema.objects_all(k).iter().any(|o| o.matches_search(filt)))
}

fn namespace_has_object_match(schema: &DbSchema, ns: &str, filt: &str) -> bool {
    ObjectKind::ALL.into_iter().any(|k| {
        schema
            .objects_in(Some(ns), k)
            .iter()
            .any(|o| o.matches_search(filt))
    })
}

/// Does this database's row survive the filter?
///
/// **Shared by the render and `nav_rows`**, which is the whole point: the two
/// had drifted in both directions at once — the render dropped a database whose
/// only match was a type's name, so the object the user searched for was
/// unreachable, while `nav_rows` kept it, so the arrow keys walked rows that
/// weren't on screen. The `TableScope` doc comment records the previous instance
/// of exactly this divergence.
///
/// A database whose schema is still loading survives: nothing is known about its
/// contents yet, and hiding it would be a guess.
fn db_survives(schema: Option<&DbSchema>, db_name: &str, filt: &str) -> bool {
    if filt.is_empty() || db_name.to_lowercase().contains(filt) {
        return true;
    }
    match schema {
        Some(s) => s.tables.iter().any(|t| t.matches_search(filt)) || has_object_match(s, filt),
        None => true,
    }
}

/// Does this namespace's row survive the filter? Same rule one level down, and
/// shared for the same reason.
fn namespace_survives(schema: &DbSchema, ns: &str, db_hit: bool, filt: &str) -> bool {
    if filt.is_empty() || db_hit || ns.to_lowercase().contains(filt) {
        return true;
    }
    schema
        .tables
        .iter()
        .any(|t| t.schema.as_deref() == Some(ns) && t.matches_search(filt))
        || namespace_has_object_match(schema, ns, filt)
}

/// Which of a folder's objects the filter leaves. Empty means the folder itself
/// renders nothing — a header with a count, no children and a chevron that can
/// never open was the converse half of the same divergence.
pub(crate) fn objects_shown<'a>(
    items: &'a [ObjectItem],
    parent_hit: bool,
    ns_hit: bool,
    filt: &str,
) -> Vec<&'a ObjectItem> {
    items
        .iter()
        .filter(|o| filt.is_empty() || parent_hit || ns_hit || o.matches_search(filt))
        .collect()
}

/// The namespaces to render as their own tree level, or empty when the tables
/// should be listed flat directly under the database.
///
/// A schema level only earns its extra click when there's a choice to make: MySQL
/// has no namespaces at all, and a PostgreSQL database with only `public` looks
/// exactly as it did before multi-schema browsing existed.
fn schema_groups(schema: &schemaic_core::schema::DbSchema) -> Vec<String> {
    let names = schema.schemas();
    if names.len() > 1 { names } else { Vec::new() }
}

/// The source identity of a schema-tree table row.
fn table_source(database: &str, t: &TableInfo) -> schemaic_core::schema::TableSource {
    schemaic_core::schema::TableSource::new(database, t.schema.clone(), t.name.clone())
}

// One row in the flattened, currently-visible tree (respecting expand state,
// hidden DBs, and the search filter) — the sequence the arrow keys walk.
struct NavRow {
    key: String,
    parent: Option<String>,
    expandable: bool,
    expanded: bool,
}

/// Where the nav cursor goes when the tree **gains focus**: it stays where it
/// was, and only an unset cursor is seeded from the open table's row.
///
/// **A cursor that exists is the user's.** Seeding unconditionally was right while
/// the only way the tree could gain focus was a click from outside it — but the
/// context menu now hands focus back (`widgets::set_menu_return`), which re-fires
/// `FocusGained` on a tree that is already focused and already has a cursor. So
/// arrowing to `customers`, pressing Shift+F10 and pressing Escape moved the
/// cursor to whatever table happened to be open, with no keypress asking for it —
/// and the next Shift+F10 or Enter acted on that row instead. Its own comment
/// described this rule ("otherwise resume wherever the cursor last was"); the code
/// only had the first half.
///
/// `visible` is the key list the arrows walk: a cursor is only worth seeding to a
/// row that is actually on screen.
fn resume_cursor(
    selected: Option<&str>,
    active_key: Option<&str>,
    visible: impl Fn(&str) -> bool,
) -> Option<String> {
    if let Some(cur) = selected {
        return Some(cur.to_string());
    }
    active_key.filter(|k| visible(k)).map(str::to_string)
}

// Reorder databases so favorited ones come first (oldest favorite highest); the
// rest keep their natural order. Stable, so within each group the original order
// is preserved. Shared by the tree render and keyboard-nav row list so the two
// agree.
fn sort_favorites_first(nodes: &mut [ConnNode], favorites: &[FavoriteRule], conn_id: u64) {
    nodes.sort_by_key(|c| {
        schemaic_core::favorite::rank(favorites, conn_id, &c.database)
            .map(|r| (0usize, r))
            .unwrap_or((1, 0))
    });
}

/// Which of a database's tables one `push_tables` call covers.
///
/// This exists because the two cases were once both spelled `Option<&str>`, and
/// the flat one passed `None` — which then filtered on `table.schema == None`.
/// But **flat means "this database has no schema level", not "these tables have
/// no namespace"**: on PostgreSQL every table carries `Some("public")`, so the
/// filter matched nothing and keyboard navigation could not reach a single table
/// or column, while the render — which has no such filter — listed them all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TableScope<'a> {
    /// No schema level: every table hangs off the database row, whatever
    /// namespace it happens to carry.
    Flat,
    /// One PostgreSQL namespace's tables.
    Namespace(&'a str),
}

impl<'a> TableScope<'a> {
    /// Does a table with this namespace belong to the scope?
    fn covers(self, table_schema: Option<&str>) -> bool {
        match self {
            TableScope::Flat => true,
            TableScope::Namespace(ns) => table_schema == Some(ns),
        }
    }

    /// The namespace name, for the search-term check (`Flat` has none to match).
    fn name(self) -> Option<&'a str> {
        match self {
            TableScope::Flat => None,
            TableScope::Namespace(ns) => Some(ns),
        }
    }
}

/// One database as the nav walk sees it — the signals already read, in display
/// order. Exists so [`nav_rows`] is a pure function of what the tree renders and
/// can be tested against it; `visible_nav_rows` is the thin signal-reading shell.
struct NavDb {
    database: String,
    name: String,
    /// `None` while introspection is still in flight or has failed — the walk
    /// keeps such a database (its tables aren't knowable yet), like the tree.
    schema: Option<std::sync::Arc<DbSchema>>,
}

// Build the visible-row list in display order. Reads the signals the walk depends
// on, then hands plain data to `nav_rows`.
fn visible_nav_rows(
    db_nodes: RwSignal<Vec<ConnNode>>,
    expanded: RwSignal<HashSet<String>>,
    hidden_dbs: Memo<HashSet<String>>,
    filter: RwSignal<String>,
    db_favorites: RwSignal<Vec<FavoriteRule>>,
    active_conn: RwSignal<u64>,
) -> Vec<NavRow> {
    let mut nodes = db_nodes.get_untracked();
    sort_favorites_first(
        &mut nodes,
        &db_favorites.get_untracked(),
        active_conn.get_untracked(),
    );
    let dbs: Vec<NavDb> = nodes
        .into_iter()
        .map(|n| NavDb {
            database: n.database,
            name: n.name,
            schema: match n.schema.get_untracked() {
                SchemaState::Loaded(s) => Some(s),
                _ => None,
            },
        })
        .collect();
    nav_rows(
        &dbs,
        &expanded.get_untracked(),
        &hidden_dbs.get_untracked(),
        &filter.get_untracked(),
    )
}

/// The nav walk. Mirrors the tree's own render rules: hidden DBs dropped; a
/// non-empty filter force-expands DBs and narrows their tables to name matches;
/// only expanded tables contribute columns. Pure — this is the function that must
/// stay bug-for-bug identical to the render, so it is the one worth testing.
fn nav_rows(
    dbs: &[NavDb],
    exp: &HashSet<String>,
    hidden: &HashSet<String>,
    filter: &str,
) -> Vec<NavRow> {
    let filt = filter.trim().to_lowercase();
    let filtering = !filt.is_empty();
    let mut rows = Vec::new();
    for n in dbs {
        if !db_visible(hidden, &n.database) {
            continue;
        }
        // Mirror the tree's search filtering (so arrow-key nav walks exactly what's
        // shown): a DB matches by its own name or by containing a matching table; a
        // DB whose schema is still loading is kept (we can't know its tables yet).
        let db_hit = filtering && n.name.to_lowercase().contains(&filt);
        let schema = n.schema.as_ref();
        if !db_survives(schema.map(|s| s.as_ref()), &n.name, &filt) {
            continue;
        }
        let db_key = db_key(&n.database);
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
        let Some(schema) = schema else {
            continue;
        };
        // Mirror the tree: with >1 PostgreSQL namespace, tables hang off a schema
        // row; otherwise they hang off the database directly.
        let groups = schema_groups(schema);
        let push_tables = |rows: &mut Vec<NavRow>, parent: &String, scope: TableScope| {
            // A schema whose own name matches shows all its tables, like `db_hit`.
            let ns_hit = filtering
                && scope
                    .name()
                    .is_some_and(|s| s.to_lowercase().contains(&filt));
            for t in schema
                .tables
                .iter()
                .filter(|t| scope.covers(t.schema.as_deref()))
            {
                if filtering && !db_hit && !ns_hit && !t.matches_search(&filt) {
                    continue;
                }
                let tbl_key = table_key(&n.database, t);
                // A column match force-reveals the table's columns/keys (like the tree).
                let force_cols = filtering && t.any_column_matches(&filt);
                let tbl_open = exp.contains(&tbl_key) || force_cols;
                rows.push(NavRow {
                    key: tbl_key.clone(),
                    parent: Some(parent.clone()),
                    expandable: true,
                    expanded: tbl_open,
                });
                if !tbl_open {
                    continue;
                }
                for c in &t.columns {
                    rows.push(NavRow {
                        key: column_key(&n.database, t, &c.name),
                        parent: Some(tbl_key.clone()),
                        expandable: false,
                        expanded: false,
                    });
                }
                // Key/index rows are intentionally *not* navigable: they open
                // nowhere, so keyboard-selecting them would be a dead end (columns
                // are the only leaf that acts on Enter/double-click).
            }
        };
        // The standalone-object folders, which sit after a level's tables. Unlike
        // key rows these *are* navigable: a leaf opens its editor, so selecting
        // one goes somewhere.
        let push_objects = |rows: &mut Vec<NavRow>, parent: &String, scope: TableScope| {
            let ns_hit = filtering
                && scope
                    .name()
                    .is_some_and(|s| s.to_lowercase().contains(&filt));
            for (kind, items) in object_groups(schema, scope) {
                let shown = objects_shown(&items, db_hit, ns_hit, &filt);
                if shown.is_empty() {
                    continue;
                }
                let gk = object_group_key(&n.database, scope, kind);
                // A filter that matched inside this folder opens it, the same way
                // a column match force-reveals its table.
                let open = exp.contains(&gk) || (filtering && !db_hit && !ns_hit);
                rows.push(NavRow {
                    key: gk.clone(),
                    parent: Some(parent.clone()),
                    expandable: true,
                    expanded: open,
                });
                if !open {
                    continue;
                }
                for o in shown {
                    rows.push(NavRow {
                        key: object_key(&n.database, scope, kind, o.name()),
                        parent: Some(gk.clone()),
                        expandable: false,
                        expanded: false,
                    });
                }
            }
        };
        if groups.is_empty() {
            push_tables(&mut rows, &db_key, TableScope::Flat);
            push_objects(&mut rows, &db_key, TableScope::Flat);
            continue;
        }
        for ns in &groups {
            if !namespace_survives(schema, ns, db_hit, &filt) {
                continue; // no match in this namespace → the group is hidden
            }
            let key = schema_key(&n.database, ns);
            let open = exp.contains(&key) || filtering;
            rows.push(NavRow {
                key: key.clone(),
                parent: Some(db_key.clone()),
                expandable: true,
                expanded: open,
            });
            if open {
                push_tables(&mut rows, &key, TableScope::Namespace(ns));
                push_objects(&mut rows, &key, TableScope::Namespace(ns));
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

/// The rule a row wears **while its context menu is open** — 1px above and below,
/// in [`theme::row_menu_edge`]. The visible half of [`Nav::menu_row`]; called from
/// a row's `.style()`, like [`is_nav_selected`].
///
/// Borders and not an `outline`: floem strokes a per-side border *inside* the
/// view's own rect (`paint_border` puts the top line at y = 0.5 and the bottom at
/// height − 0.5), so nothing bleeds into the rows on either side and no `z_index`
/// is needed to keep their hover backgrounds off it — an outline, which floem
/// inflates *outward*, would have needed exactly that. And since taffy sizes the
/// **border box**, a row's `height(TREE_ROW_H)` is unchanged: the rule costs 2px of
/// content box on a vertically centred row, and no layout shift.
fn menu_mark(s: floem::style::Style, nav: Nav, key: &str) -> floem::style::Style {
    if nav.menu_row.with(|k| k.as_deref() == Some(key)) {
        s.border_top(1.0)
            .border_bottom(1.0)
            .border_color(theme::row_menu_edge())
    } else {
        s
    }
}

/// Wrap a row's [`CtxOpener`] so that raising the menu also **marks the row it
/// belongs to** ([`Nav::menu_row`], painted by [`with_nav_scroll`]).
///
/// Wrapped where the opener is *built*, not at the click, because a row has two
/// ways to raise its menu — the pointer, and Shift+F10 through
/// [`Nav::cursor_menu`] — and only one of them is a click. Both go through the
/// opener, so both mark, and the keyboard route cannot quietly lose the
/// affordance the pointer route has.
///
/// Give it the same `key` the row passes to [`with_nav_scroll`]: that is the key
/// the mark is compared against, and it is the tree's one row identity (the
/// expansion set, the nav cursor and the persisted state all use it).
fn marking_opener(nav: Nav, key: &str, open: CtxOpener) -> CtxOpener {
    let key = key.to_string();
    Rc::new(move |at| {
        nav.menu_row.set(Some(key.clone()));
        open(at);
    })
}

// Attach a self-scroll-into-view effect to a row's view: whenever it becomes the
// focused nav cursor, scroll it into the tree viewport. Returns the same view.
fn with_nav_scroll(view: AnyView, nav: Nav, key: String, menu: Option<CtxOpener>) -> AnyView {
    let id = view.id();
    // Where this row is, kept live by layout. `on_move` is the window origin and
    // `on_resize`'s rect is view-local, so only its *height* is taken.
    let origin = RwSignal::new(Point::ZERO);
    let height = RwSignal::new(0.0_f64);
    // **The row whose context menu is open wears a rule top and bottom**
    // ([`menu_mark`]). Chained here rather than added to each row builder's own
    // `.style()` for the reason the scroll effect is here: every navigable row in
    // the tree comes through this one function, menu or no menu. (The key/index
    // leaf is the one row that doesn't — it is out of the nav sequence entirely —
    // and `key_row` therefore calls `menu_mark` itself.)
    let marked = key.clone();
    let view = view
        .style(move |s| menu_mark(s, nav, &marked))
        .on_move(move |p| origin.set(p))
        .on_resize(move |r| height.set(r.height()))
        .into_any();
    // The cursor row publishes where it is and what its menu is, so Shift+F10 —
    // which arrives at the tree *container*, focus never being on a row — can
    // open the right menu in the right place. Both geometry signals are read
    // **inside** the effect, so a scroll that moves the row refreshes this
    // rather than leaving a stale point behind.
    let cursor_key = key.clone();
    create_effect(move |_| {
        if is_nav_selected(nav, &cursor_key) {
            let o = origin.get();
            // **The row's content, not the row.** A tree row spans the whole
            // panel and the panel is flush against the window's left edge, so a
            // row's own x is 0 at every depth and the menu hugged the edge. The
            // indent that makes the tree a tree is the row's `padding_left`
            // (`tree_row`), which `get_content_rect` reports — read from the same
            // layout that positioned the row rather than passed down from six
            // call sites that each compute it their own way.
            let indent = id.get_content_rect().x0;
            nav.cursor_at.set(Some((o.x + indent, o.y + height.get())));
            nav.cursor_menu.set(menu.clone());
        }
    });
    create_effect(move |_| {
        if nav.reveal.with(|r| r.as_deref() == Some(key.as_str())) {
            // Defer to the next tick so the scroll target is computed against
            // settled layout — an immediate `scroll_to` runs before the viewport
            // reflects the prior move and under-scrolls, so the bar visibly lags
            // several rows behind the cursor. Clearing `reveal` here (after the
            // scroll) keeps it a one-shot, so a later focus toggle can't re-scroll.
            exec_after(Duration::ZERO, move |_| {
                id.scroll_to(None);
                nav.reveal.set(None);
            });
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
    let open_table_col = ui.tab_actions.open_table_col.clone();
    let active_table = ui.schema.active_table;
    let active_db = ui.tabs_ui.active_db;
    let active_conn = ui.conn.active_conn;
    let connections = ui.conn.connections;
    let db_colors = ui.db_colors;
    let db_favorites = ui.db_favorites;
    let hidden_dbs = ui.schema.hidden_dbs;
    let db_menu_open = ui.schema.db_menu_open;
    let schema_menu_open = ui.schema.schema_menu_open;
    let db_menu_anchor = ui.schema.db_menu_anchor;
    let schema_menu_anchor = ui.schema.schema_menu_anchor;
    let context_menu = ui.overlay.context_menu;
    // `schema_panel_w()` — the width the shell renders this panel at — is
    // published by `body`, which is where the clamp lives. The panel and its rows
    // read it; nothing in here may size itself from `layout.schema_w`, the
    // unclamped intent.
    // Close every *other* dropdown when the eye/settings menus open, so all the
    // app's menus are mutually exclusive (the eye/gear absorb their own pointer-down,
    // so the root dismissal handler never runs for them).
    // One list, in `widgets::MenuFlags` — three copies of it in three files is
    // how the activity clock's dropdown came to be missing from this one.
    let menus = crate::widgets::MenuFlags::of(&ui);
    // Search filter (local to the panel). `filter_input` is bound to the search box
    // (updates per keystroke); `filter` is its debounced mirror — the tree filters,
    // highlights, and re-expands off `filter`, so a burst of typing churns the
    // (potentially large) schema once, not on every character.
    let filter_input = RwSignal::new(String::new());
    let filter = debounced(filter_input, Duration::from_millis(SEARCH_DEBOUNCE_MS));
    // Keyboard-navigation cursor + focus (local to the panel).
    let nav = Nav {
        focused: RwSignal::new(false),
        selected: RwSignal::new(None),
        reveal: RwSignal::new(None),
        cursor_at: RwSignal::new(None),
        cursor_menu: RwSignal::new(None),
        menu_row: RwSignal::new(None),
    };
    // **The mark goes away with the menu**, however it went: Escape, a click
    // outside, an action taken from it, or a second right-click somewhere the tree
    // doesn't own. Derived from the menu's own state rather than cleared at each of
    // those sites, because the last of them isn't the tree's code at all. The write
    // is guarded so a menu-less tree doesn't re-notify every row on every close
    // (`RwSignal::set` doesn't dedup).
    create_effect(move |_| {
        if context_menu.with(|m| m.is_none()) && nav.menu_row.with_untracked(|k| k.is_some()) {
            nav.menu_row.set(None);
        }
    });

    let nav_tree_id: RwSignal<Option<floem::ViewId>> = RwSignal::new(None);

    // Cloned up front: `on_toggle`/`open_table` are moved into the tree's
    // dyn_stack closure below, but the keyboard-nav handler needs them too
    // (Right/Left toggle; Enter opens the selected table).
    let nav_toggle = on_toggle.clone();
    let nav_open_table = open_table.clone();
    let nav_open_table_col = open_table_col.clone();
    // Enter on an object leaf opens its editor, which needs the whole `Ui`; the
    // row builders need it for the matching double-click.
    let nav_ui = ui.clone();
    let tree_ui = ui.clone();

    // Not `width_full`: the tree sizes to its widest row so the horizontal
    // scrollbar appears when a deep/long row overflows the panel. Hidden
    // databases are filtered out entirely (also drops them from local search).
    let tree = dyn_stack(
        move || {
            let filt = filter.get();
            let filt = filt.trim().to_lowercase();
            let mut list = db_nodes
                .get()
                .into_iter()
                // `with`, not `get` — this runs per database, and `get` would clone
                // the whole hidden set each time.
                .filter(|c| hidden_dbs.with(|h| db_visible(h, &c.database)))
                // While filtering, drop a database with no match — through the
                // same predicate `nav_rows` uses, so the keyboard walks exactly
                // what is on screen.
                .filter(|c| match c.schema.get() {
                    SchemaState::Loaded(s) => db_survives(Some(&s), &c.name, &filt),
                    _ => db_survives(None, &c.name, &filt),
                })
                .collect::<Vec<_>>();
            // Favorited databases sort to the top (oldest favorite first); re-runs
            // when a favorite is toggled (`db_favorites`) or the connection changes.
            sort_favorites_first(&mut list, &db_favorites.get(), active_conn.get());
            list
        },
        |c: &ConnNode| c.id,
        move |c| {
            // The active connection's dialect (drives engine-correct DDL). Rebuilt
            // with the tree on a connection switch (db_nodes changes).
            let dialect = connections
                .with_untracked(|cs| {
                    cs.iter()
                        .find(|k| k.id == active_conn.get_untracked())
                        .map(|k| SqlDialect::from_db_type(&k.db_type))
                })
                .unwrap_or_default();
            db_node(
                c,
                SchemaTreeCtx {
                    ui: tree_ui.clone(),
                    expanded,
                    filter,
                    on_toggle: on_toggle.clone(),
                    open_table: open_table.clone(),
                    open_table_col: open_table_col.clone(),
                    active_table,
                    active_db,
                    context_menu,
                    active_conn,
                    db_colors,
                    db_favorites,
                    dialect,
                    nav,
                    indent: 0.0,
                },
            )
        },
    )
    // Bottom padding so the last row can scroll clear of the (overlay) horizontal
    // scrollbar instead of sitting under it.
    .style(|s| s.flex_col().padding_bottom(10.0));

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
            // Start from the active table when the cursor is nowhere; otherwise
            // resume where it was. The decision is `resume_cursor`, which states
            // why the second half matters now that the context menu hands focus
            // back to a tree that already has a cursor.
            let seeded = nav.selected.with_untracked(|cur| {
                let active = active_table
                    .get_untracked()
                    .map(|src| table_key_named(&src.database, &src.display()));
                resume_cursor(cur.as_deref(), active.as_deref(), |k| {
                    visible_nav_rows(
                        db_nodes,
                        expanded,
                        hidden_dbs,
                        filter,
                        db_favorites,
                        active_conn,
                    )
                    .iter()
                    .any(|r| r.key == k)
                })
            });
            if let Some(k) = seeded {
                nav.selected.set(Some(k));
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
            // **Shift+F10 / the Menu key** raise the cursor row's context menu —
            // the same menu its right-click builds, from the same closure
            // (`CtxOpener`), so the two can't drift apart.
            //
            // It opens **at the row**, not at the pointer, which may be anywhere
            // or never have been in the tree; `cursor_view` is the row's own view
            // and `layout_rect` is already in window coordinates. And it publishes
            // a focus return, because the menu panel is a `focus_root` with none
            // above it out here: without one, closing the menu would drop focus
            // and the tree would answer no further keys — the same fault the grid
            // toolbar hit (`widgets::set_menu_return`).
            let menu_key = match &ke.key.logical_key {
                // The dedicated Menu key, where a keyboard has one.
                Key::Named(NamedKey::ContextMenu) => true,
                // …and the shifted-F10 spelling every platform accepts, for the
                // keyboards that don't.
                Key::Named(NamedKey::F10) => ke.modifiers.shift(),
                _ => false,
            };
            if menu_key {
                // **Re-resolve the cursor first, exactly as the arrows and Enter
                // do.** `cursor_menu`/`cursor_at` are published by the cursor row's
                // own effect and are never cleared, so a row that has gone —
                // collapse its parent, or refresh the database out from under it —
                // leaves a callable opener and a stale window point behind: the menu
                // for an invisible object, positioned over an unrelated row. The
                // opener captures owned data rather than row signals, which is why
                // this misbehaves instead of panicking.
                let cursor_visible = nav.selected.with_untracked(|cur| {
                    cur.as_deref().is_some_and(|k| {
                        visible_nav_rows(
                            db_nodes,
                            expanded,
                            hidden_dbs,
                            filter,
                            db_favorites,
                            active_conn,
                        )
                        .iter()
                        .any(|r| r.key == k)
                    })
                });
                if let Some(open) = nav.cursor_menu.get_untracked().filter(|_| cursor_visible) {
                    let at = nav.cursor_at.get_untracked();
                    let tree_id = nav_tree_id.get_untracked();
                    crate::widgets::set_menu_return(Rc::new(move || {
                        if let Some(id) = tree_id {
                            exec_after(Duration::ZERO, move |_| id.request_focus());
                        }
                    }));
                    (open)(at);
                }
                return EventPropagation::Stop;
            }
            // Enter opens the selected row: a table row → open it (new tab, or the
            // existing one — see `open_table`); a column row → open its table and
            // highlight the column (mirrors the column double-click). BOTH are
            // matched by rebuilding their exact nav key from the schema rather than
            // parsing it — identifiers may contain ':' and, now, a '.' that a
            // schema-qualified key also uses.
            if matches!(ke.key.logical_key, Key::Named(NamedKey::Enter)) {
                if let Some(sel) = nav.selected.get_untracked() {
                    'find: for node in db_nodes.get_untracked() {
                        let SchemaState::Loaded(schema) = node.schema.get_untracked() else {
                            continue;
                        };
                        for t in &schema.tables {
                            if table_key(&node.database, t) == sel {
                                (nav_open_table)(table_source(&node.database, t));
                                break 'find;
                            }
                            for c in &t.columns {
                                if column_key(&node.database, t, &c.name) == sel {
                                    (nav_open_table_col)(
                                        table_source(&node.database, t),
                                        c.name.clone(),
                                    );
                                    break 'find;
                                }
                            }
                        }
                        // The standalone objects, whose leaves `nav_rows` makes
                        // navigable *because* Enter opens their editor. Without
                        // this arm the cursor could park on a row with no
                        // keyboard action — the dead end the key rows are kept
                        // out of `nav_rows` to avoid. The key is rebuilt exactly
                        // as `push_objects` builds it, scopes included.
                        let names = schema_groups(&schema);
                        let scopes: Vec<TableScope> = if names.is_empty() {
                            vec![TableScope::Flat]
                        } else {
                            names.iter().map(|n| TableScope::Namespace(n)).collect()
                        };
                        for scope in scopes {
                            for (kind, items) in object_groups(&schema, scope) {
                                for o in &items {
                                    if object_key(&node.database, scope, kind, o.name()) == sel {
                                        if crate::object_editor::is_editable_object(o) {
                                            crate::object_editor::open_for_object(
                                                &nav_ui,
                                                &node.database,
                                                o,
                                            );
                                        }
                                        break 'find;
                                    }
                                }
                            }
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
            let rows = visible_nav_rows(
                db_nodes,
                expanded,
                hidden_dbs,
                filter,
                db_favorites,
                active_conn,
            );
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
                    nav_select(nav, rows[ni].key.clone());
                }
                -1 => {
                    let ni = match pos {
                        Some(i) => i.saturating_sub(1),
                        None => rows.len() - 1,
                    };
                    nav_select(nav, rows[ni].key.clone());
                }
                2 => match pos {
                    Some(i) if rows[i].expandable && !rows[i].expanded => {
                        (nav_toggle)(rows[i].key.clone());
                    }
                    Some(i) if rows[i].expandable => {
                        if let Some(child) = rows.get(i + 1)
                            && child.parent.as_deref() == Some(rows[i].key.as_str())
                        {
                            nav_select(nav, child.key.clone());
                        }
                    }
                    Some(_) => {}
                    None => nav_select(nav, rows[0].key.clone()),
                },
                -2 => match pos {
                    Some(i) if rows[i].expandable && rows[i].expanded => {
                        (nav_toggle)(rows[i].key.clone());
                    }
                    Some(i) => {
                        if let Some(parent) = rows[i].parent.clone() {
                            nav_select(nav, parent);
                        }
                    }
                    None => nav_select(nav, rows[0].key.clone()),
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
    // The tree's own id, so a context menu it raised can give the keyboard back
    // when it closes. Filled after the view exists; the handler above reads it
    // through the signal for that reason.
    nav_tree_id.set(Some(tree.id()));

    // Title row: "SCHEMA" left; the visibility (eye) and settings (gear) menus
    // right. The gear is rightmost; the eye sits 10px to its left. Each icon
    // closes the OTHER menu and toggles its own — so clicking one while the
    // other is open switches in a single click. The icons absorb their own
    // PointerDown so the root-level dismiss handler (see `workspace`) doesn't
    // pre-close the menu and cause a toggle to immediately reopen it.
    // Each icon publishes its own box (window coords) so its dropdown can hang off
    // it. The panel's width is a live, persisted, window-clamped signal, so
    // anything derived from the `SCHEMA_W` default detaches the moment it differs —
    // which includes a narrow window with no resize at all.
    let (eye_origin, eye_size) = (RwSignal::new(Point::ZERO), RwSignal::new((0.0, 0.0)));
    let (gear_origin, gear_size) = (RwSignal::new(Point::ZERO), RwSignal::new((0.0, 0.0)));
    create_effect(move |_| {
        let (o, (_, h)) = (eye_origin.get(), eye_size.get());
        db_menu_anchor.set(Point::new(o.x, o.y + h));
    });
    create_effect(move |_| {
        let (o, (_, h)) = (gear_origin.get(), gear_size.get());
        schema_menu_anchor.set(Point::new(o.x, o.y + h));
    });
    let eye_hov = RwSignal::new(false);
    let eye = container(icons::icon(icons::EYE, 16.0).style(move |s| {
        s.flex_shrink(0.0_f32)
            .color(crate::widgets::menu_icon_color(
                db_menu_open.get(),
                eye_hov.get(),
            ))
    }))
    .on_move(move |p| eye_origin.set(p))
    .on_resize(move |r| eye_size.set((r.width(), r.height())))
    .on_click_stop(move |_| {
        menus.close_except(Some(crate::widgets::MenuId::SchemaEye));
        db_menu_open.update(|o| *o = !*o);
    })
    .on_event_stop(
        EventListener::PointerDown,
        crate::widgets::menu_trigger_press,
    )
    .on_event_cont(EventListener::PointerEnter, move |_| eye_hov.set(true))
    .on_event_cont(EventListener::PointerLeave, move |_| eye_hov.set(false))
    .style(|s| {
        s.items_center()
            .margin_top(4.0)
            .margin_right(2.0)
            .padding_horiz(5.0)
            .padding_vert(3.0)
    })
    // Tooltips, on the wrapper — `.tooltip()` wraps the view, so it goes *after*
    // the style, leaving `on_move`/`on_resize` on the padded container inside.
    // That box is what the dropdown hangs off; anchoring to a bare glyph would
    // put the menu 3px under the icon's ink rather than under its hitbox.
    .tooltip(|| text("Show or hide databases").style(crate::widgets::tooltip_style));
    let gear_hov = RwSignal::new(false);
    let gear = container(icons::icon(icons::SLIDERS_VERTICAL, 16.0).style(move |s| {
        s.flex_shrink(0.0_f32)
            .color(crate::widgets::menu_icon_color(
                schema_menu_open.get(),
                gear_hov.get(),
            ))
    }))
    .on_move(move |p| gear_origin.set(p))
    .on_resize(move |r| gear_size.set((r.width(), r.height())))
    .on_click_stop(move |_| {
        menus.close_except(Some(crate::widgets::MenuId::SchemaGear));
        schema_menu_open.update(|o| *o = !*o);
    })
    .on_event_stop(
        EventListener::PointerDown,
        crate::widgets::menu_trigger_press,
    )
    .on_event_cont(EventListener::PointerEnter, move |_| gear_hov.set(true))
    .on_event_cont(EventListener::PointerLeave, move |_| gear_hov.set(false))
    .style(|s| {
        s.items_center()
            .margin_top(4.0)
            .margin_right(9.0)
            .padding_horiz(5.0)
            .padding_vert(3.0)
    })
    .tooltip(|| text("Schema options").style(crate::widgets::tooltip_style));
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
        schema_search(filter_input),
        empty().style(|s| s.height(10.0).flex_shrink(0.0_f32)),
        tree,
    ))
    .style(move |s| {
        // The width the shell *renders* the wrapper at, not the user's intent —
        // the wrapper clips, so laying out to the intent under the clamp put the
        // search box's clear button past the cut.
        s.width(schema_panel_w().get())
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
    /// The whole `Ui`, for the row actions that open a modal rather than a tab —
    /// an object leaf's editor, which Enter and double-click both reach.
    ui: Ui,
    expanded: RwSignal<HashSet<String>>,
    filter: RwSignal<String>,
    on_toggle: Rc<dyn Fn(String)>,
    open_table: Rc<dyn Fn(TableSource)>,
    /// Open the column's table and highlight that column in the grid (column-row
    /// double-click).
    open_table_col: Rc<dyn Fn(TableSource, String)>,
    active_table: RwSignal<Option<TableSource>>,
    /// Extra left inset (px) for rows below the database level, so a table nested
    /// under a PostgreSQL schema group sits one level deeper than a flat one. `0.0`
    /// whenever tables hang off the database directly.
    indent: f64,
    active_db: Memo<Option<String>>,
    context_menu: RwSignal<Option<CtxMenu>>,
    /// The active connection id + the app-wide DB-colour store, for the identity
    /// dot on database rows. (Schema-tree nodes all belong to the active connection.)
    active_conn: RwSignal<u64>,
    db_colors: RwSignal<Vec<DbColorRule>>,
    db_favorites: RwSignal<Vec<FavoriteRule>>,
    /// The active connection's SQL dialect — for engine-correct `create_ddl`
    /// (Copy/Generate DDL). All tree nodes belong to the active connection.
    dialect: SqlDialect,
    nav: Nav,
}

fn db_node(conn: ConnNode, ctx: SchemaTreeCtx) -> impl IntoView {
    let node_ui = ctx.ui.clone();
    let SchemaTreeCtx {
        expanded,
        filter,
        on_toggle,
        open_table,
        open_table_col,
        active_table,
        active_db,
        context_menu,
        active_conn,
        db_colors,
        db_favorites,
        dialect,
        nav,
        ..
    } = ctx;
    let key = db_key(&conn.database);
    let schema_sig = conn.schema;

    let toggle_row = on_toggle.clone();
    let key_row = key.clone();
    let ctx_db = conn.database.clone();
    // The row's context menu as a *function of where to open it*, so the pointer
    // and Shift+F10 raise the same menu — the second being what
    // `with_nav_scroll` publishes for whichever row the nav cursor is on.
    let open_menu: CtxOpener = Rc::new(move |at| {
        let ai_prompt = format!(
            "Give me a concise overview of the `{ctx_db}` database — the domain it models, \
             its key tables, and how they relate."
        );
        // Built here rather than per render, for the reason the namespace script
        // is: a whole database is a lot of `CREATE`s, and it is only ever needed
        // once the menu opens. Empty until the schema has loaded — the node
        // expands lazily, so a database nobody has opened has nothing to write.
        let ddl = schema_sig.with_untracked(|s| match s {
            SchemaState::Loaded(db) => db.create_ddl_script_all(dialect),
            _ => String::new(),
        });
        context_menu.set(Some(CtxMenu {
            kind: CtxKind::Database { ddl },
            name: ctx_db.clone(),
            ai_prompt,
            at,
        }));
    });
    // Shadowed, so the pointer's `on_secondary_click_stop` clone below and the
    // Shift+F10 route through `with_nav_scroll` both get the marking wrapper.
    let open_menu = marking_opener(nav, &key, open_menu);
    let dot_db = conn.database.clone();
    let star_db = conn.database.clone();
    let icon_fav_db = conn.database.clone();
    let name_fav_db = conn.database.clone();
    // The database name, highlighting the search term. Wrapped in a `dyn_container`
    // on `filter` so the highlight tracks the search without rebuilding the node
    // (`dyn_stack` keys DB nodes by id and won't rebuild a surviving one).
    let name_disp = conn.name.clone();
    let db_name = dyn_container(
        move || filter.get(),
        move |f| {
            let f = f.trim();
            let term = (!f.is_empty()).then(|| f.to_string());
            // Favorited → the name is gold. Read reactively inside `rich_text`, so a
            // favorite toggle recolours it without rebuilding the node.
            let fav_db = name_fav_db.clone();
            let base = move || {
                if db_favorites
                    .with(|r| schemaic_core::favorite::is_favorite(r, active_conn.get(), &fav_db))
                {
                    theme::favorite_star()
                } else {
                    theme::text()
                }
            };
            highlight_text(name_disp.clone(), term, theme::FONT_BODY, base, true, 1.0).into_any()
        },
    );
    let header = h_stack((
        chevron(expanded, key.clone(), on_toggle.clone()),
        // Gold star before the DB icon (only when this database is favorited).
        favorite_star(
            db_favorites,
            move || Some((active_conn.get(), star_db.clone())),
            13.0,
            CHEVRON_GAP,
            0.0,
        ),
        icons::icon(icons::DATABASE, SCHEMA_ICON as f32).style(move |s| {
            // Favorited → gold icon (matching the gold name), else the default.
            let fav = db_favorites
                .with(|r| schemaic_core::favorite::is_favorite(r, active_conn.get(), &icon_fav_db));
            s.color(if fav {
                theme::favorite_star()
            } else {
                theme::db_icon()
            })
            .margin_left(CHEVRON_GAP)
            .margin_right(ICON_GAP)
            .flex_shrink(0.0_f32)
        }),
        db_name,
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
    .on_secondary_click_stop({
        let open = open_menu.clone();
        move |_| (open)(None)
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
    let header = with_nav_scroll(header.into_any(), nav, key.clone(), Some(open_menu));

    // Children rebuild on expand/schema/filter change. A non-empty filter
    // force-expands the node and narrows its tables to name matches.
    let key_children = key.clone();
    let database = conn.database.clone();
    // Lower-cased display name, for the "the DB itself matched" check below.
    let db_name_lc = conn.name.to_lowercase();
    let ot_tables = open_table;
    let otc_tables = open_table_col;
    let toggle_tables = on_toggle;
    let children = dyn_container(
        move || {
            (
                // `with`, not `get`: every row in the tree reads this set, and
                // `get` clones all of it to answer one `contains`.
                expanded.with(|e| e.contains(&key_children)),
                schema_sig.get(),
                filter.get(),
            )
        },
        move |(open, state, filt)| {
            let filt = filt.trim().to_lowercase();
            let filtering = !filt.is_empty();
            // The DB itself matched → show all its tables (not just matching ones).
            let db_hit = filtering && db_name_lc.contains(&filt);
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
                SchemaState::Failed(e) => info_row(e, theme::error).into_any(),
                SchemaState::Loaded(schema) => {
                    let db = database.clone();
                    let child_ctx = |indent: f64| SchemaTreeCtx {
                        ui: node_ui.clone(),
                        expanded,
                        filter,
                        on_toggle: toggle_tables.clone(),
                        open_table: ot_tables.clone(),
                        open_table_col: otc_tables.clone(),
                        active_table,
                        active_db,
                        context_menu,
                        active_conn,
                        db_colors,
                        db_favorites,
                        dialect,
                        nav,
                        indent,
                    };
                    // More than one PostgreSQL namespace → group the tables under a
                    // schema row each. A single-schema (or MySQL) database keeps the
                    // flat list it has always had.
                    let groups = schema_groups(&schema);
                    if !groups.is_empty() {
                        let visible: Vec<String> = groups
                            .into_iter()
                            .filter(|ns| namespace_survives(&schema, ns, db_hit, &filt))
                            .collect();
                        if visible.is_empty() {
                            return empty().into_any();
                        }
                        // `schema` is already the `Arc` out of `SchemaState`.
                        return v_stack_from_iter(visible.into_iter().map(move |ns| {
                            schema_node(
                                db.clone(),
                                ns,
                                schema.clone(),
                                db_hit,
                                child_ctx(LEVEL_INDENT),
                            )
                        }))
                        .style(|s| s.flex_col())
                        .into_any();
                    }
                    let tables: Vec<TableInfo> = schema
                        .tables
                        .iter()
                        .filter(|t| !filtering || db_hit || t.matches_search(&filt))
                        .cloned()
                        .collect();
                    let objects = object_group_nodes(
                        node_ui.clone(),
                        db.clone(),
                        None,
                        schema.clone(),
                        db_hit,
                        filt.clone(),
                        child_ctx(0.0),
                    );
                    if tables.is_empty() {
                        // Hide the node's body entirely while filtering with no
                        // match; otherwise show the empty-schema hint — but a
                        // database can hold types and no tables, and saying "No
                        // tables" above a list of them would be a flat lie.
                        let none = object_groups(&schema, TableScope::Flat).is_empty();
                        return if filtering {
                            objects.into_any()
                        } else if none {
                            info_row("No tables", theme::text_muted).into_any()
                        } else {
                            objects.into_any()
                        };
                    }
                    v_stack((
                        v_stack_from_iter(
                            tables
                                .into_iter()
                                .map(move |t| table_node(db.clone(), t, child_ctx(0.0))),
                        )
                        .style(|s| s.flex_col()),
                        objects,
                    ))
                    .style(|s| s.flex_col())
                    .into_any()
                }
            }
        },
    );

    v_stack((header, children)).style(|s| s.flex_col())
}

// A PostgreSQL namespace node: a header row over the tables in that schema.
// Rendered only when the database has more than one (see `schema_groups`), so a
// MySQL or `public`-only tree is untouched. Purely structural — a schema row
// opens nothing, so (like key/index rows) it isn't a keyboard-Enter target,
// though it is navigable and expandable.
fn schema_node(
    database: String,
    ns: String,
    schema: std::sync::Arc<schemaic_core::schema::DbSchema>,
    db_hit: bool,
    ctx: SchemaTreeCtx,
) -> impl IntoView {
    let node_ui = ctx.ui.clone();
    let SchemaTreeCtx {
        expanded,
        filter,
        on_toggle,
        nav,
        context_menu,
        dialect,
        ..
    } = ctx.clone();
    let key = schema_key(&database, &ns);
    let table_count = schema
        .tables
        .iter()
        .filter(|t| t.schema.as_deref() == Some(ns.as_str()))
        .count();

    let toggle_row = on_toggle.clone();
    let key_row = key.clone();
    let open_menu: CtxOpener = {
        let ctx_db = database.clone();
        let ctx_ns = ns.clone();
        let ddl_schema = schema.clone();
        Rc::new(move |at| {
            let ai_prompt = format!(
                "Give me a concise overview of the `{ctx_ns}` schema in the `{ctx_db}` \
                 database — what it models, its key tables, and how they relate."
            );
            // Built here rather than per render: a namespace can hold a lot of
            // tables, and the script is only ever needed once the menu opens.
            let ddl = ddl_schema.create_ddl_script(Some(&ctx_ns), dialect);
            context_menu.set(Some(CtxMenu {
                kind: CtxKind::Schema {
                    database: ctx_db.clone(),
                    ddl,
                },
                name: ctx_ns.clone(),
                ai_prompt,
                at,
            }));
        })
    };
    let open_menu = marking_opener(nav, &key, open_menu);
    let name_term = {
        let f = filter.get_untracked();
        let f = f.trim();
        (!f.is_empty()).then(|| f.to_string())
    };
    let header = h_stack((
        chevron(expanded, key.clone(), on_toggle),
        // Muted: a schema row is structural, not something you open — it should
        // read quieter than the database above and the tables below it.
        icons::icon(icons::FOLDER, SCHEMA_ICON as f32).style(move |s| {
            s.color(theme::text_muted())
                .margin_left(CHEVRON_GAP)
                .margin_right(ICON_GAP)
                .flex_shrink(0.0_f32)
        }),
        highlight_text(
            ns.clone(),
            name_term,
            theme::FONT_BODY,
            theme::text,
            false,
            1.0,
        ),
        capsule(format!(
            "{table_count} {}",
            plural(table_count, "table", "tables")
        )),
    ))
    .on_double_click_stop(move |_| (toggle_row)(key_row.clone()))
    .on_secondary_click_stop({
        let open = open_menu.clone();
        move |_| (open)(None)
    })
    .style({
        let hl = key.clone();
        move |s| {
            let s = tree_row(s, ROW_PAD + LEVEL_INDENT).gap(6.0);
            if is_nav_selected(nav, &hl) {
                s.background(theme::row_selected())
            } else {
                s
            }
        }
    });
    let header = with_nav_scroll(header.into_any(), nav, key.clone(), Some(open_menu));

    let key_children = key.clone();
    let ns_children = ns.clone();
    let children = dyn_container(
        move || (expanded.with(|e| e.contains(&key_children)), filter.get()),
        move |(open, filt)| {
            let filt = filt.trim().to_lowercase();
            let filtering = !filt.is_empty();
            if !open && !filtering {
                return empty().into_any();
            }
            // The schema's own name matching reveals all its tables, mirroring the
            // database-level `db_hit` rule.
            let ns_hit = filtering && ns_children.to_lowercase().contains(&filt);
            let tables: Vec<TableInfo> = schema
                .tables
                .iter()
                .filter(|t| t.schema.as_deref() == Some(ns_children.as_str()))
                .filter(|t| !filtering || db_hit || ns_hit || t.matches_search(&filt))
                .cloned()
                .collect();
            let db = database.clone();
            let ctx = ctx.clone();
            let objects = object_group_nodes(
                node_ui.clone(),
                db.clone(),
                Some(ns_children.clone()),
                schema.clone(),
                db_hit || ns_hit,
                filt.clone(),
                ctx.clone(),
            );
            if tables.is_empty() {
                let none = object_groups(&schema, TableScope::Namespace(&ns_children)).is_empty();
                return if filtering {
                    objects.into_any()
                } else if none {
                    info_row("No tables", theme::text_muted).into_any()
                } else {
                    objects.into_any()
                };
            }
            v_stack((
                v_stack_from_iter(
                    tables
                        .into_iter()
                        .map(move |t| table_node(db.clone(), t, ctx.clone())),
                )
                .style(|s| s.flex_col()),
                objects,
            ))
            .style(|s| s.flex_col())
            .into_any()
        },
    );

    v_stack((header, children)).style(|s| s.flex_col())
}

/// The `Types`/`Domains`/`Sequences` folders for one tree level, after its
/// tables. Empty when the database has none — which is every MySQL connection,
/// so nothing about that tree changes.
fn object_group_nodes(
    ui: Ui,
    database: String,
    scope_ns: Option<String>,
    schema: std::sync::Arc<DbSchema>,
    parent_hit: bool,
    filt: String,
    ctx: SchemaTreeCtx,
) -> impl IntoView {
    let scope = || match &scope_ns {
        Some(ns) => TableScope::Namespace(ns.as_str()),
        None => TableScope::Flat,
    };
    let ns_hit = !filt.is_empty()
        && scope_ns
            .as_deref()
            .is_some_and(|s| s.to_lowercase().contains(&filt));
    // A folder with nothing to show renders nothing at all — header included.
    // `nav_rows` skips it, so leaving the header on screen made a row with a
    // count that the keyboard could not reach and that expanded to nothing.
    let groups: Vec<_> = object_groups(&schema, scope())
        .into_iter()
        .filter(|(_, items)| !objects_shown(items, parent_hit, ns_hit, &filt).is_empty())
        .collect();
    v_stack_from_iter(groups.into_iter().map(move |(kind, items)| {
        object_group_node(
            ui.clone(),
            database.clone(),
            scope_ns.clone(),
            kind,
            items,
            parent_hit,
            ctx.clone(),
        )
    }))
    .style(|s| s.flex_col())
}

/// One folder: a header row over the objects of that kind.
fn object_group_node(
    ui: Ui,
    database: String,
    scope_ns: Option<String>,
    kind: ObjectKind,
    items: Vec<ObjectItem>,
    parent_hit: bool,
    ctx: SchemaTreeCtx,
) -> impl IntoView {
    let SchemaTreeCtx {
        expanded,
        filter,
        on_toggle,
        nav,
        context_menu,
        dialect,
        indent,
        ..
    } = ctx;
    let scope = match &scope_ns {
        Some(ns) => TableScope::Namespace(ns.as_str()),
        None => TableScope::Flat,
    };
    let key = object_group_key(&database, scope, kind);
    let count = items.len();

    // A folder's menu is about the *set* it holds, which is why it exists at all
    // now: `Create sequence` used to live only in the database node's `Create`
    // submenu, two levels away from the folder named after it.
    let open_menu: CtxOpener = {
        let (db, ns, objects) = (database.clone(), scope_ns.clone(), items.clone());
        Rc::new(move |at| {
            let label = object_group_label(kind);
            let ai_prompt = format!(
                "In the `{db}` database, explain the {} it defines — what each one is \
                 for and where it is used.",
                label.to_lowercase()
            );
            // Every object in the folder, in the order the rows are in. Built on
            // open, as the namespace and database scripts are.
            let ddl = objects
                .iter()
                .map(|o| o.create_sql(dialect))
                .collect::<Vec<_>>()
                .join("\n\n");
            context_menu.set(Some(CtxMenu {
                kind: CtxKind::ObjectGroup {
                    database: db.clone(),
                    schema: ns.clone(),
                    kind,
                    ddl,
                },
                name: label.to_string(),
                ai_prompt,
                at,
            }));
        })
    };
    let open_menu = marking_opener(nav, &key, open_menu);

    let toggle_row = on_toggle.clone();
    let key_row = key.clone();
    let header = h_stack((
        chevron(expanded, key.clone(), on_toggle),
        // Muted like a namespace row: a folder is structural, not something you
        // open.
        icons::icon(icons::FOLDER, SCHEMA_ICON as f32).style(move |s| {
            s.color(theme::text_muted())
                .margin_left(CHEVRON_GAP)
                .margin_right(ICON_GAP)
                .flex_shrink(0.0_f32)
        }),
        text(object_group_label(kind))
            .style(|s| s.font_size(theme::FONT_BODY).color(theme::text())),
        capsule(count.to_string()),
    ))
    .on_double_click_stop(move |_| (toggle_row)(key_row.clone()))
    .on_secondary_click_stop({
        let open = open_menu.clone();
        move |_| (open)(None)
    })
    .style({
        let hl = key.clone();
        move |s| {
            // Exactly `table_node`'s indent, because a folder sits at the same
            // level a table does: under the database when the tree is flat, and
            // one step under the namespace row when it isn't. `ROW_PAD + indent`
            // put it *level with* the namespace it belongs to.
            let s = tree_row(s, ROW_PAD + LEVEL_INDENT + indent).gap(6.0);
            if is_nav_selected(nav, &hl) {
                s.background(theme::row_selected())
            } else {
                s
            }
        }
    });
    let header = with_nav_scroll(header.into_any(), nav, key.clone(), Some(open_menu));

    let key_children = key.clone();
    let ns_hit_base = scope_ns.clone();
    let children = dyn_container(
        move || (expanded.with(|e| e.contains(&key_children)), filter.get()),
        move |(open, filt)| {
            let filt = filt.trim().to_lowercase();
            let filtering = !filt.is_empty();
            let ns_hit = filtering
                && ns_hit_base
                    .as_deref()
                    .is_some_and(|s| s.to_lowercase().contains(&filt));
            // A filter that matched inside this folder opens it, the same way a
            // column match force-reveals its table. `nav_rows` mirrors this.
            if !open && !(filtering && !parent_hit && !ns_hit) {
                return empty().into_any();
            }
            let term = (!filt.is_empty()).then(|| filt.clone());
            let shown: Vec<ObjectItem> = objects_shown(&items, parent_hit, ns_hit, &filt)
                .into_iter()
                .cloned()
                .collect();
            if shown.is_empty() {
                return empty().into_any();
            }
            let db = database.clone();
            let ns = scope_ns.clone();
            let ui = ui.clone();
            v_stack_from_iter(shown.into_iter().map(move |o| {
                object_row(
                    ui.clone(),
                    db.clone(),
                    ns.clone(),
                    o,
                    context_menu,
                    dialect,
                    term.clone(),
                    nav,
                    indent,
                )
            }))
            .style(|s| s.flex_col())
            .into_any()
        },
    );

    v_stack((header, children)).style(|s| s.flex_col())
}

/// One object (leaf): its icon, name, and a dim summary — an enum's values, a
/// domain's base type, a sequence's owner.
#[allow(clippy::too_many_arguments)]
fn object_row(
    ui: Ui,
    database: String,
    scope_ns: Option<String>,
    o: ObjectItem,
    context_menu: RwSignal<Option<CtxMenu>>,
    dialect: SqlDialect,
    term: Option<String>,
    nav: Nav,
    indent: f64,
) -> impl IntoView {
    let scope = match &scope_ns {
        Some(ns) => TableScope::Namespace(ns.as_str()),
        None => TableScope::Flat,
    };
    let kind = o.kind();
    let nav_key = object_key(&database, scope, kind, o.name());
    let name = o.name().to_string();
    let detail = o.detail();
    // An identity column's counter isn't an object anyone can act on alone, so
    // it reads quieter — the same signal a lossy index gets.
    let dim = o.is_internal();
    let open_menu: CtxOpener = {
        let (db, obj) = (database.clone(), o.clone());
        let display = schemaic_core::schema::display_name(obj.schema(), obj.name());
        Rc::new(move |at| {
            let ai_prompt = format!(
                "In the `{db}` database, explain the `{display}` {} — what it is for \
                 and where it is used.",
                kind.label()
            );
            context_menu.set(Some(CtxMenu {
                kind: CtxKind::Object {
                    database: db.clone(),
                    item: Box::new(obj.clone()),
                    // Built here rather than per render: it is only ever needed
                    // once the menu opens, as a namespace's script is.
                    ddl: obj.create_sql(dialect),
                },
                name: display.clone(),
                ai_prompt,
                at,
            }));
        })
    };
    let open_menu = marking_opener(nav, &nav_key, open_menu);
    let row = h_stack((
        icons::icon(object_icon(kind), SCHEMA_ICON as f32).style(move |s| {
            s.color(theme::text_muted().multiply_alpha(if dim { 0.4 } else { 0.7 }))
                .margin_right(ICON_GAP)
                .flex_shrink(0.0_f32)
        }),
        highlight_text(
            name.clone(),
            term,
            theme::FONT_BODY,
            move || {
                if dim {
                    theme::text_muted()
                } else {
                    theme::text()
                }
            },
            false,
            1.0,
        ),
        text(detail).style(|s| {
            s.color(theme::text_muted())
                .font_size(theme::FONT_LABEL)
                .margin_left(12.0)
        }),
    ))
    .style(|s| s.items_center())
    // Double-click opens the editor, the way a table row does — and the same
    // action Enter takes, which is what makes these leaves worth navigating to.
    .on_double_click_stop({
        let (ui, db, obj) = (ui.clone(), database.clone(), o.clone());
        move |_| {
            if crate::object_editor::is_editable_object(&obj) {
                crate::object_editor::open_for_object(&ui, &db, &obj);
            }
        }
    })
    .on_secondary_click_stop({
        let open = open_menu.clone();
        move |_| (open)(None)
    })
    .style({
        let hl = nav_key.clone();
        move |s| {
            let s = tree_row(s, COL_PAD + indent);
            if is_nav_selected(nav, &hl) {
                s.background(theme::row_selected())
            } else {
                s
            }
        }
    });
    with_nav_scroll(row.into_any(), nav, nav_key, Some(open_menu))
}

// A table node: a header row (double-click opens & runs `SELECT *`) over its
// columns then indexes, shown when expanded. Highlighted while it is the
// active tab's source table.
/// The table's on-disk size at the right edge of its row — the size *column*.
///
/// This is the half of the properties work that answers a question a modal
/// cannot: not "how big is this table" but "which of these is the big one",
/// which needs them all on screen at once.
///
/// Renders nothing at all unless the column is on **and** this table has a size,
/// so a view, a partitioned parent, or an engine that publishes nothing leaves
/// the row exactly as it was rather than showing a placeholder dash down the
/// whole tree.
///
/// **Absolutely positioned, and anchored to the panel rather than to the row.**
/// A `flex_grow` spacer used to push the size to the row's right edge, which is
/// only the panel's right edge while the tree fits: rows stretch to the widest one,
/// so expanding a table — whose column rows are indented and carry a type —
/// widened every row and carried the whole size column off past the viewport,
/// reachable only by scrolling sideways. Taking it out of flow and pinning it
/// `tree_row_min_w()` from the row's left edge puts it back where the panel ends.
/// `inset_left`, not `inset_right`, for exactly that reason — the right edge is
/// the one that moves. Taffy measures the inset from the row's *border* box, so
/// the anchor ignores the per-level `padding_left` and lands identically on
/// every row whatever its depth.
///
/// The cost of leaving the flow is that a table name long enough to reach the
/// panel edge now runs under the size instead of pushing it along. Sizes sit on
/// table rows only, which is what makes that trade a good one: there is no
/// column row underneath to collide with, and the alternative was a column that
/// disappeared whenever anything was open.
fn size_badge(
    stats: Option<RwSignal<crate::DbStatsState>>,
    enabled: RwSignal<bool>,
    schema: Option<String>,
    table: String,
) -> impl IntoView {
    dyn_container(
        move || {
            if !enabled.get() {
                return None;
            }
            let stats = stats?;
            stats.with(|st| match st {
                crate::DbStatsState::Loaded(set) => set
                    .get(schema.as_deref(), &table)
                    .and_then(|t| t.total_bytes())
                    .map(schemaic_core::stats::format_bytes),
                _ => None,
            })
        },
        move |size| match size {
            None => empty().into_any(),
            Some(s) => text(s)
                .style(|s| {
                    s.font_size(theme::FONT_STATUS)
                        .color(theme::text_faint())
                        .flex_shrink(0.0_f32)
                })
                .into_any(),
        },
    )
    // Invisible to the mouse, and **not optional**. This box spans the row from
    // its left edge, so it lies over the chevron and the name — and Floem walks a
    // row's children back-to-front looking for a pointer target and stops at the
    // first one whose bounds contain the point, whether or not it handles the
    // event (`context.rs`, `unconditional_view_event`: `if event.is_pointer()
    // { break }`). As the last child, the badge won that race for the whole row
    // and clicking a table stopped expanding it — with the column *off* too,
    // since the empty state is still a full-width box. Nothing here is clickable,
    // so it opts out and the row underneath goes back to receiving everything.
    .pointer_events(|| false)
    // `justify_end` inside a panel-wide box does what the spacer used to. The
    // padding is the row's own 8px plus 10 more: sitting hard against the panel
    // edge read as too tight next to the tree's scrollbar, so the column is
    // pulled in by eye rather than to match a constant.
    .style(|s| {
        s.absolute()
            .inset_left(0.0)
            .inset_top(0.0)
            .width(tree_row_min_w())
            .height_full()
            .justify_end()
            .items_center()
            .padding_right(18.0)
    })
}

fn table_node(database: String, table: TableInfo, ctx: SchemaTreeCtx) -> impl IntoView {
    // The size column's two inputs, read before `ctx` is destructured. The stats
    // signal is looked up by database name rather than threaded through
    // `SchemaTreeCtx`: `db_nodes` is replaced wholesale on a connection switch,
    // which rebuilds this node anyway, so the handle captured here is always the
    // live one for the row on screen.
    let table_sizes = ctx.ui.schema.table_sizes;
    // Read before the destructure below moves `ctx`.
    let table_colors = ctx.ui.table_colors;
    let row_conn = ctx.ui.conn.active_conn;
    let db_stats = ctx.ui.schema.db_nodes.with_untracked(|nodes| {
        nodes
            .iter()
            .find(|n| n.database == database)
            .map(|n| n.stats)
    });
    let SchemaTreeCtx {
        expanded,
        filter,
        on_toggle,
        open_table,
        open_table_col,
        active_table,
        context_menu,
        dialect,
        nav,
        indent,
        ..
    } = ctx;
    // The active filter term to highlight in the table + column names (the tree
    // rebuilds this node on every `filter` change, so an untracked read here is
    // always current). `force_cols` reveals a table's columns when the match is on a
    // column (not the table name), so the highlighted column is actually visible.
    let name_term = {
        let f = filter.get_untracked();
        let f = f.trim();
        (!f.is_empty()).then(|| f.to_string())
    };
    let force_cols = match &name_term {
        Some(t) => {
            let tl = t.to_lowercase();
            table
                .columns
                .iter()
                .any(|c| c.name.to_lowercase().contains(&tl))
        }
        None => false,
    };
    let key = table_key(&database, &table);
    let source = table_source(&database, &table);
    let col_count = table.columns.len();
    let key_count = table.indexes.len();
    let ddl = table.create_ddl(dialect);
    // Views get a distinct glyph + tint; base tables keep the green table icon.
    // The colour is the **function**, not its value: this node is rebuilt only
    // when the expansion, schema or filter changes, none of which a theme switch
    // touches, so a `Color` resolved here would keep the old theme's tint until
    // something else forced a rebuild.
    let (glyph, glyph_color): (&'static str, fn() -> Color) = if table.is_view {
        (icons::TABLE_CELLS_MERGE, theme::view_icon)
    } else {
        (icons::TABLE, theme::table_icon)
    };

    let dbl_source = source.clone();
    let hl_source = source.clone();
    let ctx_db = database.clone();
    let ctx_table = table.name.clone();
    // Qualified in the AI prompt + the context menu's title, so a question about
    // `sales.orders` isn't answered about `public.orders`.
    let ctx_display = source.display();
    let open_menu: CtxOpener = {
        let ctx_schema = source.schema.clone();
        Rc::new(move |at| {
            let ai_prompt = format!(
                "Explain the `{ctx_display}` table in the `{ctx_db}` database: what each column \
                 represents, the primary key, and any foreign-key relationships. Keep it concise."
            );
            context_menu.set(Some(CtxMenu {
                kind: CtxKind::Table {
                    database: ctx_db.clone(),
                    schema: ctx_schema.clone(),
                    table: ctx_table.clone(),
                    ddl: ddl.clone(),
                },
                name: ctx_display.clone(),
                ai_prompt,
                at,
            }));
        })
    };
    let open_menu = marking_opener(nav, &key, open_menu);
    let col_source = source.clone();
    let header = h_stack((
        chevron(expanded, key.clone(), on_toggle),
        icons::icon(glyph, SCHEMA_ICON as f32).style(move |s| {
            s.color(glyph_color())
                .margin_left(CHEVRON_GAP)
                .margin_right(ICON_GAP)
                .flex_shrink(0.0_f32)
        }),
        highlight_text(
            table.name.clone(),
            name_term.clone(),
            theme::FONT_BODY,
            theme::text,
            false,
            1.0,
        ),
        // Identity dot after the name (only when this table has a colour), placed
        // and spaced exactly like the database row's — the same colour tints this
        // table's card header in the ER diagram.
        crate::table_color_dot(
            table_colors,
            {
                let db = database.clone();
                let tbl = source.display();
                move || Some((row_conn.get(), db.clone(), tbl.clone()))
            },
            7.0,
            0.0,
            1.0,
        ),
        size_badge(
            db_stats,
            table_sizes,
            source.schema.clone(),
            table.name.clone(),
        ),
    ))
    .on_double_click_stop(move |_| (open_table)(dbl_source.clone()))
    .on_secondary_click_stop({
        let open = open_menu.clone();
        move |_| (open)(None)
    })
    .style({
        let hl = key.clone();
        move |s| {
            let s = tree_row(s, ROW_PAD + LEVEL_INDENT + indent);
            if is_nav_selected(nav, &hl) {
                s.background(theme::row_selected())
            } else if active_table.get().as_ref() == Some(&hl_source) {
                s.background(theme::row_active())
            } else {
                s
            }
        }
    });
    let header = with_nav_scroll(header.into_any(), nav, key.clone(), Some(open_menu));

    let key_children = key.clone();
    let cols = table.columns;
    let idxs = table.indexes;
    let fkeys = table.foreign_keys;
    let col_term = name_term;
    let children = dyn_container(
        // Show columns/keys when the table is expanded OR when a column matched the
        // filter (`force_cols`) — so the highlighted column is actually revealed.
        move || expanded.with(|e| e.contains(&key_children)) || force_cols,
        move |open| {
            if !open {
                return empty().into_any();
            }
            let counts = count_row(col_count, key_count, indent);
            let csrc = col_source.clone();
            let key_source = col_source.clone();
            let otc = open_table_col.clone();
            let cterm = col_term.clone();
            // Foreign-key referencing columns — tinted purple like their key.
            // From the table's FKs directly (authoritative), not the backing
            // index's name, which needn't match the constraint (classicmodels).
            let fk_cols: HashSet<String> = fkeys
                .iter()
                .flat_map(|fk| fk.columns.iter().cloned())
                .collect();
            let cols_block = v_stack_from_iter(cols.iter().cloned().map(move |c| {
                let ckind = if c.primary_key {
                    ColKey::Primary
                } else if fk_cols.contains(&c.name) {
                    ColKey::Foreign
                } else {
                    ColKey::None
                };
                column_row(
                    c,
                    ckind,
                    context_menu,
                    csrc.clone(),
                    otc.clone(),
                    cterm.clone(),
                    nav,
                    indent,
                )
            }))
            .style(|s| s.flex_col());
            // PRIMARY first, then the rest in their original order (stable sort).
            let mut sorted_idxs: Vec<IndexInfo> = idxs.to_vec();
            sorted_idxs.sort_by_key(|ix| !ix.is_primary());
            // Which constraint each foreign-key-backing index belongs to, matched
            // by *columns* — the same rule the introspection uses to set
            // `IndexInfo::foreign`, and for the same reason: the index is often
            // named after its column, not after the constraint.
            let key_src = key_source.clone();
            let key_fks = fkeys.clone();
            let keys_block = v_stack_from_iter(sorted_idxs.into_iter().map(move |ix| {
                let names: Vec<&str> = ix.column_names().collect();
                let fk = key_fks
                    .iter()
                    .find(|fk| {
                        fk.columns.len() == names.len() && fk.columns.iter().eq(names.iter())
                    })
                    .map(|fk| fk.name.clone());
                key_row(ix, context_menu, key_src.clone(), fk, nav, indent)
            }))
            .style(|s| s.flex_col());
            v_stack((counts, cols_block, keys_block))
                .style(|s| s.flex_col())
                .into_any()
        },
    );

    v_stack((header, children)).style(|s| s.flex_col())
}

// The "N cols · M keys" capsule row shown directly under a table's header.
fn count_row(cols: usize, keys: usize, indent: f64) -> impl IntoView {
    h_stack((
        capsule(format!("{cols} {}", plural(cols, "col", "cols"))),
        capsule(format!("{keys} {}", plural(keys, "key", "keys"))),
    ))
    .style(move |s| {
        s.flex_row()
            .gap(5.0)
            .padding_left(LEAF_PAD + indent)
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

/// The schema-tree glyph for a column type family. Reused by the Find-Anywhere
/// search results so they mirror the schema tree.
pub(crate) fn column_type_icon(class: ColumnTypeClass) -> &'static str {
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

#[allow(clippy::too_many_arguments)]
fn column_row(
    c: ColumnInfo,
    kind: ColKey,
    context_menu: RwSignal<Option<CtxMenu>>,
    source: TableSource,
    open_table_col: Rc<dyn Fn(TableSource, String)>,
    term: Option<String>,
    nav: Nav,
    indent: f64,
) -> impl IntoView {
    let name = c.name;
    let ty = c.type_name;
    let ctx_name = name.clone();
    let ctx_ty = ty.clone();
    // Double-click opens the column's table (reusing a tab) + highlights it.
    let dbl_source = source.clone();
    let ctx_source = source.clone();
    let dbl_col = name.clone();
    let (database, table) = (source.database.clone(), source.display());
    let nav_key = column_key_named(&database, &table, &name);
    let open_menu: CtxOpener = Rc::new(move |at| {
        let ai_prompt = format!(
            "In `{database}`.`{table}`, explain the `{ctx_name}` column (type `{ctx_ty}`) — \
             what it stores and how it's typically used."
        );
        context_menu.set(Some(CtxMenu {
            kind: CtxKind::Field {
                source: ctx_source.clone(),
                column: ctx_name.clone(),
            },
            name: ctx_name.clone(),
            ai_prompt,
            at,
        }));
    });
    let open_menu = marking_opener(nav, &nav_key, open_menu);
    // The glyph always reflects the column's *type* family — the key glyph is for
    // the key/index rows, not the columns they cover (so `id` is a numeric column,
    // and `PRIMARY(id)` is the key). Key membership only tints the row: a PK column
    // stays gold, an FK column purple. The icon is a 50%-alpha version of that
    // colour so it reads as a quieter marker beside the full-strength name.
    let glyph = column_type_icon(classify_column_type(&ty));
    let row = h_stack((
        icons::icon(glyph, SCHEMA_ICON as f32).style(move |s| {
            s.color(kind.color().multiply_alpha(0.5))
                .margin_right(ICON_GAP)
                .flex_shrink(0.0_f32)
        }),
        highlight_text(
            name,
            term,
            theme::FONT_BODY,
            move || kind.color(),
            false,
            1.0,
        ),
        text(ty).style(|s| {
            s.color(theme::text_muted())
                .font_size(theme::FONT_LABEL)
                .margin_left(12.0)
        }),
    ))
    // The name inherits `kind.color()`; the icon overrides to 50% of it above and
    // the type text to muted.
    .style(move |s| s.color(kind.color()).items_center())
    .on_double_click_stop(move |_| (open_table_col)(dbl_source.clone(), dbl_col.clone()))
    .on_secondary_click_stop({
        let open = open_menu.clone();
        move |_| (open)(None)
    })
    .style({
        let hl = nav_key.clone();
        move |s| {
            let s = tree_row(s, COL_PAD + indent);
            if is_nav_selected(nav, &hl) {
                s.background(theme::row_selected())
            } else {
                s
            }
        }
    });
    with_nav_scroll(row.into_any(), nav, nav_key, Some(open_menu))
}

// A single key (leaf): name + its columns, colored by kind (PRIMARY gold,
// FOREIGN purple, other indexes blue), with a trailing UNIQUE/INDEX/FOREIGN tag.
fn key_row(
    ix: IndexInfo,
    context_menu: RwSignal<Option<CtxMenu>>,
    source: TableSource,
    // `fk_name`: the foreign key this index backs, when it backs one — resolved
    // by the caller from the table's own constraints, since the names needn't
    // match.
    fk_name: Option<String>,
    // `nav` only for the context-menu mark — a key row takes no part in keyboard
    // navigation (see the style at the end).
    nav: Nav,
    indent: f64,
) -> impl IntoView {
    let (database, table) = (source.database.clone(), source.display());
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
    let cols = ix.column_names().collect::<Vec<_>>().join(", ");
    // Built before `database`/`table` are moved into the click closure below.
    let menu_key = key_row_menu_key(&database, &table, &ix.name);
    let mark_key = menu_key.clone();
    let ctx_name = ix.name.clone();
    let label = format!("{} ({cols})", ix.name);
    let ctx_index = ix.clone();
    h_stack((
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
    // Label + key glyph both at 50% alpha (a quiet, non-actionable leaf); the
    // trailing type tag stays full-strength (its own muted colour, above).
    .style(move |s| s.color(color.multiply_alpha(0.5)).items_center())
    .on_secondary_click_stop(move |_| {
        // This row has no `CtxOpener` to wrap (`marking_opener`) because it has no
        // keyboard route to share one with, so it marks itself.
        nav.menu_row.set(Some(menu_key.clone()));
        let ai_prompt = format!(
            "In `{database}`.`{table}`, explain the `{ctx_name}` {kind} on ({cols}) — its \
             purpose (uniqueness, faster lookups, or a foreign-key relationship)."
        );
        context_menu.set(Some(CtxMenu {
            kind: CtxKind::Key {
                source: source.clone(),
                index: Box::new(ctx_index.clone()),
                foreign_key: fk_name.clone(),
            },
            name: ctx_name.clone(),
            ai_prompt,
            // Always at the pointer: a key row is deliberately out of the nav
            // sequence (see below), so Shift+F10 can never be about one.
            at: None,
        }));
    })
    // Non-interactive: static layout (no hover), and not in the nav sequence, so
    // it never shows a selection highlight either — it opens nowhere. The
    // context-menu mark is not that highlight: the row has a menu like any other,
    // and while it is open it says so.
    .style(move |s| menu_mark(tree_row_static(s, COL_PAD + indent), nav, &mark_key))
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
        move || expanded.with(|e| e.contains(&key_read)),
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

/// Live schema-panel width — the width the shell *renders* the panel at, which
/// on a narrow window is narrower than the user's stored intent. Published by
/// `body` (where the clamp lives) and read by the panel and its row styles, so
/// there is one answer to "how wide is the schema panel". Detached scope → lives
/// for the whole process (like `window_size`).
pub(crate) fn schema_panel_w() -> RwSignal<f64> {
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
    tree_row_static(s, pad_left).hover(|s| s.background(theme::row_hover()))
}

// Row layout without the hover highlight — for non-interactive rows (keys/indexes,
// which can't be opened, so a hover/selection affordance would mislead).
fn tree_row_static(s: floem::style::Style, pad_left: f64) -> floem::style::Style {
    s.min_width(tree_row_min_w())
        .height(TREE_ROW_H)
        .min_height(TREE_ROW_H)
        .items_center()
        .flex_row()
        .padding_left(pad_left)
        .padding_right(8.0)
        .font_size(theme::FONT_BODY)
}

// A non-interactive status line inside the tree (Loading / error / empty).
// `color` is a fn for the same reason `widgets::centered_msg`'s is: a captured
// `Color` freezes at build and stops following a live theme switch.
fn info_row(
    msg: impl Into<String>,
    color: impl Fn() -> floem::peniko::Color + 'static,
) -> impl IntoView {
    let msg = msg.into();
    container(text(msg).style(move |s| s.color(color()).font_size(theme::FONT_LABEL))).style(
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tbl(ns: Option<&str>, name: &str) -> TableInfo {
        TableInfo {
            name: name.to_string(),
            schema: ns.map(str::to_string),
            ..Default::default()
        }
    }

    fn tbl_cols(ns: Option<&str>, name: &str, cols: &[&str]) -> TableInfo {
        TableInfo {
            columns: cols
                .iter()
                .map(|c| ColumnInfo {
                    name: c.to_string(),
                    ..Default::default()
                })
                .collect(),
            ..tbl(ns, name)
        }
    }

    fn db(database: &str, tables: Vec<TableInfo>) -> NavDb {
        NavDb {
            database: database.to_string(),
            name: database.to_string(),
            schema: Some(std::sync::Arc::new(DbSchema {
                tables,
                ..Default::default()
            })),
        }
    }

    fn set(keys: &[&str]) -> HashSet<String> {
        keys.iter().map(|k| k.to_string()).collect()
    }

    /// The walk's output as `(key, parent)` pairs — the two fields navigation
    /// actually moves on.
    fn walk(dbs: &[NavDb], exp: &[&str], hidden: &[&str], filter: &str) -> Vec<(String, String)> {
        nav_rows(dbs, &set(exp), &set(hidden), filter)
            .into_iter()
            .map(|r| (r.key, r.parent.unwrap_or_default()))
            .collect()
    }

    // ── the nav walk (`nav_rows`) ─────────────────────────────────────────
    //
    // This is the one function that must stay bug-for-bug identical to the
    // render: arrow-key navigation walks *its* output, so anything it omits is a
    // row the keyboard can't reach while the tree shows it normally. That is
    // exactly how the PostgreSQL flat-database bug reached a user.

    #[test]
    fn nav_walk_reaches_tables_and_columns_on_a_flat_database() {
        // The regression: a `public`-only PostgreSQL database is rendered flat,
        // and its tables still carry `Some("public")`. Selecting them by
        // `schema == None` reached no table or column at all.
        for ns in [None, Some("public")] {
            let dbs = vec![db("chinook", vec![tbl_cols(ns, "album", &["id", "title"])])];
            assert_eq!(
                walk(&dbs, &["db:chinook", "tbl:chinook:album"], &[], ""),
                vec![
                    ("db:chinook".to_string(), String::new()),
                    ("tbl:chinook:album".to_string(), "db:chinook".to_string()),
                    (
                        "col:chinook:album:id".to_string(),
                        "tbl:chinook:album".to_string()
                    ),
                    (
                        "col:chinook:album:title".to_string(),
                        "tbl:chinook:album".to_string()
                    ),
                ],
                "namespace {ns:?}"
            );
        }
    }

    #[test]
    fn nav_walk_hangs_tables_off_the_schema_row_when_there_are_several() {
        let dbs = vec![db(
            "warehouse",
            vec![tbl(Some("public"), "staging"), tbl(Some("sales"), "orders")],
        )];
        // Only the `sales` group is expanded, so `public`'s table stays hidden.
        assert_eq!(
            walk(&dbs, &["db:warehouse", "sch:warehouse:sales"], &[], ""),
            vec![
                ("db:warehouse".to_string(), String::new()),
                ("sch:warehouse:public".to_string(), "db:warehouse".into()),
                ("sch:warehouse:sales".to_string(), "db:warehouse".into()),
                (
                    "tbl:warehouse:sales.orders".to_string(),
                    "sch:warehouse:sales".into()
                ),
            ]
        );
    }

    #[test]
    fn nav_walk_stops_at_a_collapsed_row() {
        let dbs = vec![db("shop", vec![tbl_cols(None, "users", &["id"])])];
        // Collapsed database → its own row only.
        assert_eq!(
            walk(&dbs, &[], &[], ""),
            vec![("db:shop".to_string(), String::new())]
        );
        // Expanded database, collapsed table → no columns.
        assert_eq!(
            walk(&dbs, &["db:shop"], &[], ""),
            vec![
                ("db:shop".to_string(), String::new()),
                ("tbl:shop:users".to_string(), "db:shop".to_string()),
            ]
        );
    }

    #[test]
    fn nav_walk_drops_a_hidden_database_entirely() {
        let dbs = vec![
            db("shop", vec![tbl(None, "users")]),
            db("blog", vec![tbl(None, "posts")]),
        ];
        assert_eq!(
            walk(&dbs, &["db:shop", "db:blog"], &["shop"], ""),
            vec![
                ("db:blog".to_string(), String::new()),
                ("tbl:blog:posts".to_string(), "db:blog".to_string()),
            ]
        );
    }

    #[test]
    fn nav_walk_keeps_a_database_whose_schema_has_not_loaded() {
        // Its tables aren't knowable yet, so the database stays reachable rather
        // than disappearing from navigation until introspection lands.
        let dbs = vec![NavDb {
            database: "shop".to_string(),
            name: "shop".to_string(),
            schema: None,
        }];
        assert_eq!(
            walk(&dbs, &["db:shop"], &[], ""),
            vec![("db:shop".to_string(), String::new())]
        );
        assert_eq!(walk(&dbs, &[], &[], "any"), walk(&dbs, &[], &[], "other"));
    }

    #[test]
    fn nav_walk_under_a_filter_force_expands_and_narrows_to_matches() {
        let dbs = vec![db(
            "shop",
            vec![
                tbl_cols(None, "users", &["id"]),
                tbl_cols(None, "orders", &["id"]),
            ],
        )];
        // Nothing expanded, but a filter opens the database and keeps only the
        // matching table — and a *column* match force-reveals that table's columns.
        assert_eq!(
            walk(&dbs, &[], &[], "order"),
            vec![
                ("db:shop".to_string(), String::new()),
                ("tbl:shop:orders".to_string(), "db:shop".to_string()),
            ]
        );
        assert_eq!(
            walk(&dbs, &[], &[], "id"),
            vec![
                ("db:shop".to_string(), String::new()),
                ("tbl:shop:users".to_string(), "db:shop".to_string()),
                (
                    "col:shop:users:id".to_string(),
                    "tbl:shop:users".to_string()
                ),
                ("tbl:shop:orders".to_string(), "db:shop".to_string()),
                (
                    "col:shop:orders:id".to_string(),
                    "tbl:shop:orders".to_string()
                ),
            ]
        );
        // A database matching by its own name keeps all of its tables.
        assert_eq!(walk(&dbs, &[], &[], "sho").len(), 3);
        // No match anywhere → the database is gone from the walk.
        assert!(walk(&dbs, &[], &[], "zzz").is_empty());
    }

    #[test]
    fn nav_walk_marks_expandability_the_way_the_tree_does() {
        let dbs = vec![db("shop", vec![tbl_cols(None, "users", &["id"])])];
        let rows = nav_rows(&dbs, &set(&["db:shop", "tbl:shop:users"]), &set(&[]), "");
        assert_eq!(
            rows.iter()
                .map(|r| (r.expandable, r.expanded))
                .collect::<Vec<_>>(),
            // database (open), table (open), column (a leaf)
            vec![(true, true), (true, true), (false, false)]
        );
    }

    // ── when the tree grows a schema level ────────────────────────────────

    #[test]
    fn mysql_never_gets_a_schema_level() {
        // No namespaces at all → the flat list every MySQL user already has.
        let s = DbSchema {
            tables: vec![tbl(None, "users"), tbl(None, "orders")],
            ..Default::default()
        };
        assert!(schema_groups(&s).is_empty());
    }

    /// The flat branch is taken whenever a database has no *schema level* — which
    /// on PostgreSQL includes every ordinary `public`-only database, whose tables
    /// still carry `Some("public")`. Selecting them by `schema == None` there
    /// matched nothing, so arrow-key navigation reached no table or column at all
    /// while the tree rendered them normally. Observed on the user's instance.
    #[test]
    fn flat_scope_covers_tables_whatever_namespace_they_carry() {
        // PostgreSQL: a namespace is present and must not exclude the table.
        assert!(TableScope::Flat.covers(Some("public")));
        assert!(TableScope::Flat.covers(Some("sales")));
        // MySQL: no namespace at all — the case that always worked.
        assert!(TableScope::Flat.covers(None));
    }

    /// The grouped branch must stay exact, or a table would appear under every
    /// namespace of a multi-schema database.
    #[test]
    fn namespace_scope_selects_only_that_namespace() {
        let sales = TableScope::Namespace("sales");
        assert!(sales.covers(Some("sales")));
        assert!(!sales.covers(Some("public")));
        assert!(!sales.covers(None));
        assert_eq!(sales.name(), Some("sales"));
        assert_eq!(TableScope::Flat.name(), None, "no namespace to search on");
    }

    #[test]
    fn a_public_only_database_stays_flat() {
        // One namespace is no choice at all — grouping would just cost a click.
        let s = DbSchema {
            tables: vec![tbl(Some("public"), "album"), tbl(Some("public"), "artist")],
            ..Default::default()
        };
        assert!(schema_groups(&s).is_empty());
    }

    #[test]
    fn more_than_one_namespace_groups_public_first() {
        let s = DbSchema {
            tables: vec![
                tbl(Some("sales"), "orders"),
                tbl(Some("public"), "staging"),
                tbl(Some("analytics"), "daily"),
            ],
            ..Default::default()
        };
        assert_eq!(schema_groups(&s), vec!["public", "analytics", "sales"]);
    }

    #[test]
    fn a_single_non_public_namespace_also_stays_flat() {
        // Everything lives in one schema that just isn't `public`: still no choice
        // to present, so no level — the table rows carry the qualifier themselves.
        let s = DbSchema {
            tables: vec![tbl(Some("sales"), "orders")],
            ..Default::default()
        };
        assert!(schema_groups(&s).is_empty());
    }

    // ── node keys ─────────────────────────────────────────────────────────

    #[test]
    fn table_keys_are_unchanged_for_mysql_and_public() {
        // The expand/collapse set is persisted, so these keys must stay exactly
        // what earlier versions wrote or every saved expansion silently resets.
        assert_eq!(table_key("shop", &tbl(None, "users")), "tbl:shop:users");
        assert_eq!(
            table_key("chinook", &tbl(Some("public"), "album")),
            "tbl:chinook:album"
        );
        assert_eq!(
            column_key("shop", &tbl(None, "users"), "id"),
            "col:shop:users:id"
        );
    }

    /// **Every row-key family owns its prefix, and no family's prefix is a prefix
    /// of another's.** The keys share one string space — the expansion set, the nav
    /// cursor, and now the context-menu mark all compare against it — so two
    /// families that could produce the same string would mark, select or expand a
    /// row nobody named. `keyrow:` is the newest and the only one that never
    /// reaches the persisted set, which makes it the easiest to get wrong; note
    /// `obj:` and `objgrp:` are the pair that already share three letters.
    #[test]
    fn every_row_key_family_owns_its_prefix() {
        let keys = [
            ("db:", db_key("shop")),
            ("sch:", schema_key("shop", "public")),
            ("tbl:", table_key_named("shop", "users")),
            ("col:", column_key_named("shop", "users", "id")),
            (
                "objgrp:",
                object_group_key("shop", TableScope::Flat, ObjectKind::Sequence),
            ),
            (
                "obj:",
                object_key(
                    "shop",
                    TableScope::Flat,
                    ObjectKind::Sequence,
                    "users_id_seq",
                ),
            ),
            ("keyrow:", key_row_menu_key("shop", "users", "PRIMARY")),
        ];
        for (prefix, key) in &keys {
            assert!(key.starts_with(prefix), "{key} is not a {prefix} key");
            for (other, _) in &keys {
                if other == prefix {
                    continue;
                }
                assert!(
                    !key.starts_with(other),
                    "{key} answers to {other} as well as {prefix}"
                );
            }
        }
    }

    #[test]
    fn the_by_name_builders_agree_with_the_table_ones() {
        // Three call sites formatted these keys inline — a focus handler with a
        // `TableSource`, a column nav key, and the app's collapse-this-database
        // prefix. Each was right, and each could drift, because the tests only
        // ever ran the `TableInfo` builders. Now they share an implementation
        // and this is what says so.
        for t in [tbl(None, "users"), tbl(Some("sales"), "orders")] {
            let display = schemaic_core::schema::display_name(t.schema.as_deref(), &t.name);
            assert_eq!(table_key("shop", &t), table_key_named("shop", &display));
            assert_eq!(
                column_key("shop", &t, "id"),
                column_key_named("shop", &display, "id")
            );
            // The collapse prefix must match the keys it is meant to drop.
            assert!(table_key("shop", &t).starts_with(&table_key_prefix("shop")));
        }
        // …and must not match a database whose name merely starts the same way.
        assert!(!table_key("shop", &tbl(None, "users")).starts_with(&table_key_prefix("sho")));
    }

    #[test]
    fn table_keys_separate_same_named_tables_in_two_schemas() {
        let a = table_key("warehouse", &tbl(Some("public"), "orders"));
        let b = table_key("warehouse", &tbl(Some("sales"), "orders"));
        assert_eq!(a, "tbl:warehouse:orders");
        assert_eq!(b, "tbl:warehouse:sales.orders");
        assert_ne!(a, b, "one expand state must not toggle both rows");

        let ca = column_key("warehouse", &tbl(Some("public"), "orders"), "id");
        let cb = column_key("warehouse", &tbl(Some("sales"), "orders"), "id");
        assert_ne!(ca, cb);
    }

    #[test]
    fn schema_key_is_namespaced_by_database() {
        assert_eq!(schema_key("warehouse", "sales"), "sch:warehouse:sales");
        assert_ne!(
            schema_key("warehouse", "sales"),
            schema_key("other", "sales")
        );
    }

    #[test]
    fn table_source_carries_the_namespace() {
        let s = table_source("warehouse", &tbl(Some("sales"), "orders"));
        assert_eq!(s.database, "warehouse");
        assert_eq!(s.schema.as_deref(), Some("sales"));
        assert_eq!(s.table, "orders");
        // MySQL rows carry none, so they compare equal to a restored source.
        assert_eq!(table_source("shop", &tbl(None, "users")).schema, None);
    }

    // ── Standalone objects in the tree ──────────────────────────────────────

    use schemaic_core::schema::{DomainInfo, EnumInfo, SequenceInfo};

    fn objects_db(database: &str, tables: Vec<TableInfo>, ns: &str) -> NavDb {
        NavDb {
            database: database.to_string(),
            name: database.to_string(),
            schema: Some(std::sync::Arc::new(DbSchema {
                tables,
                enums: vec![EnumInfo {
                    name: "mood".into(),
                    schema: Some(ns.into()),
                    values: vec!["ok".into()],
                    comment: None,
                }],
                domains: vec![DomainInfo {
                    name: "email".into(),
                    schema: Some(ns.into()),
                    base_type: "text".into(),
                    ..Default::default()
                }],
                sequences: vec![SequenceInfo {
                    name: "counter".into(),
                    schema: Some(ns.into()),
                    ..Default::default()
                }],
                ..Default::default()
            })),
        }
    }

    #[test]
    fn object_folders_come_after_the_tables_of_their_level() {
        let dbs = vec![objects_db(
            "shop",
            vec![tbl(Some("public"), "orders")],
            "public",
        )];
        let keys: Vec<String> = walk(&dbs, &["db:shop"], &[], "")
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(
            keys,
            vec![
                "db:shop",
                "tbl:shop:orders",
                "objgrp:shop::type",
                "objgrp:shop::domain",
                "objgrp:shop::sequence",
            ]
        );
    }

    #[test]
    fn an_expanded_folder_lists_its_objects() {
        let dbs = vec![objects_db("shop", vec![], "public")];
        let rows = walk(&dbs, &["db:shop", "objgrp:shop::type"], &[], "");
        assert!(
            rows.contains(&(
                "obj:shop::type:mood".to_string(),
                "objgrp:shop::type".to_string()
            )),
            "{rows:?}"
        );
        // The other folders stay closed, so their objects aren't in the walk.
        assert!(!rows.iter().any(|(k, _)| k.starts_with("obj:shop::domain")));
    }

    #[test]
    fn empty_folders_are_not_rendered_at_all() {
        // A folder that leads nowhere is a click that leads nowhere.
        let dbs = vec![db("shop", vec![tbl(None, "orders")])];
        let keys: Vec<String> = walk(&dbs, &["db:shop"], &[], "")
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, vec!["db:shop", "tbl:shop:orders"]);
    }

    /// A search for a type must not be hidden by the level above it: the
    /// database and the namespace are both dropped before the folder holding the
    /// match is ever reached.
    #[test]
    fn filtering_by_an_object_name_keeps_every_level_above_it() {
        let dbs = vec![objects_db(
            "shop",
            vec![tbl(Some("public"), "orders")],
            "public",
        )];
        let keys: Vec<String> = walk(&dbs, &[], &[], "mood")
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        // The database survives, its folder force-opens, and the match is in it.
        assert!(keys.contains(&"db:shop".to_string()), "{keys:?}");
        assert!(
            keys.contains(&"obj:shop::type:mood".to_string()),
            "{keys:?}"
        );
        // The unrelated table and the other folders are filtered away.
        assert!(!keys.contains(&"tbl:shop:orders".to_string()), "{keys:?}");
        assert!(
            !keys.iter().any(|k| k.starts_with("objgrp:shop::domain")),
            "{keys:?}"
        );
    }

    /// The render and `nav_rows` now decide through **one** predicate each, so
    /// the two can't drift again. The tests above exercise `nav_rows`; these
    /// pin the shared functions themselves, which is the half that was missing
    /// — a `nav_rows`-only test passes while the render hides the row.
    #[test]
    fn the_shared_filter_predicates_keep_a_level_its_object_matches() {
        let s = objects_db("shop", vec![tbl(Some("public"), "orders")], "public")
            .schema
            .expect("loaded");
        // No table matches `mood`, but the enum does — so the database survives.
        assert!(db_survives(Some(&s), "shop", "mood"));
        assert!(namespace_survives(&s, "public", false, "mood"));
        // Nothing matches at all: both levels go.
        assert!(!db_survives(Some(&s), "shop", "zzz"));
        assert!(!namespace_survives(&s, "public", false, "zzz"));
        // A database whose schema hasn't loaded can't be judged, so it stays.
        assert!(db_survives(None, "shop", "zzz"));
        // The level's own name still wins, and so does an empty filter.
        assert!(db_survives(Some(&s), "shop", "sho"));
        assert!(db_survives(Some(&s), "shop", ""));
        assert!(namespace_survives(&s, "public", true, "zzz"));

        // The converse: a folder with nothing to show renders nothing, so a
        // header with a count and no reachable children can't happen.
        let enums = s.objects_all(ObjectKind::Enum);
        assert!(objects_shown(&enums, false, false, "zzz").is_empty());
        assert_eq!(objects_shown(&enums, false, false, "mood").len(), 1);
        // A match on the level above shows the whole folder.
        assert_eq!(objects_shown(&enums, true, false, "zzz").len(), enums.len());
        assert_eq!(objects_shown(&enums, false, true, "zzz").len(), enums.len());
    }

    /// Enter rebuilds an object leaf's key from the schema, exactly as
    /// `push_objects` builds it — the two are compared as strings, so they have
    /// to agree byte for byte or the handler silently does nothing.
    #[test]
    fn an_object_leafs_nav_key_is_the_one_the_enter_handler_rebuilds() {
        let s = objects_db("shop", vec![], "public").schema.expect("loaded");
        for (k, items) in object_groups(&s, TableScope::Flat) {
            let group = object_group_key("shop", TableScope::Flat, k);
            let rows = walk(
                &[objects_db("shop", vec![], "public")],
                &["db:shop", &group],
                &[],
                "",
            );
            for o in &items {
                let from_render = object_key("shop", TableScope::Flat, k, o.name());
                assert!(
                    rows.iter().any(|(key, _)| key == &from_render),
                    "{from_render} is not a nav row"
                );
            }
        }
    }

    #[test]
    fn a_namespace_survives_a_filter_that_only_its_objects_match() {
        let dbs = vec![objects_db(
            "warehouse",
            vec![tbl(Some("public"), "staging"), tbl(Some("sales"), "orders")],
            "sales",
        )];
        let keys: Vec<String> = walk(&dbs, &[], &[], "counter")
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert!(
            keys.contains(&"sch:warehouse:sales".to_string()),
            "{keys:?}"
        );
        assert!(
            keys.contains(&"obj:warehouse:sales:sequence:counter".to_string()),
            "{keys:?}"
        );
        // `public` holds nothing matching, so its group is gone.
        assert!(
            !keys.contains(&"sch:warehouse:public".to_string()),
            "{keys:?}"
        );
    }

    /// Flat means "this database has no schema level", not "these objects have no
    /// namespace" — the same distinction that once made keyboard navigation reach
    /// no table at all on a `public`-only PostgreSQL database.
    #[test]
    fn a_flat_database_shows_objects_whatever_namespace_they_carry() {
        let dbs = vec![objects_db("chinook", vec![], "public")];
        let rows = walk(&dbs, &["db:chinook", "objgrp:chinook::sequence"], &[], "");
        assert!(
            rows.iter()
                .any(|(k, _)| k == "obj:chinook::sequence:counter"),
            "{rows:?}"
        );
    }

    #[test]
    fn an_enums_detail_lists_its_values_and_says_when_there_are_more() {
        let e = |vals: &[&str]| {
            ObjectItem::Enum(EnumInfo {
                name: "mood".into(),
                schema: None,
                values: vals.iter().map(|v| v.to_string()).collect(),
                comment: None,
            })
        };
        assert_eq!(e(&["sad", "ok"]).detail(), "sad, ok");
        // Clipped, because a tree row is one line and past a few values the
        // useful fact is that there are more.
        assert_eq!(
            e(&["a", "b", "c", "d", "e", "f"]).detail(),
            "a, b, c, d, +2"
        );
        // A label is arbitrary text and may hold a newline or a tab — the same
        // fact `pg_types` reads its labels one row at a time for. This row has a
        // fixed height, so whitespace runs collapse before the join.
        assert_eq!(e(&["a\nb", "c\t\td"]).detail(), "a b, c d");
        // Surrounding whitespace is *data* in a label: keeping one space is what
        // stops `"ok "` and `"ok"` rendering identically.
        assert_eq!(e(&["  ok  "]).detail(), " ok ");
    }

    #[test]
    fn a_sequence_shows_its_owner_and_an_identity_one_is_undroppable() {
        let owned = |internal: bool| {
            ObjectItem::Sequence(SequenceInfo {
                name: "orders_id_seq".into(),
                owned_by: Some(schemaic_core::schema::SequenceOwner {
                    table: "orders".into(),
                    column: "id".into(),
                    internal,
                }),
                ..Default::default()
            })
        };
        assert_eq!(owned(false).detail(), "orders.id");
        assert!(!owned(false).is_internal());
        // An identity column's counter is part of the column: PostgreSQL refuses
        // to drop it separately, so the menu must not offer to.
        assert!(owned(true).is_internal());
        // A free-standing sequence falls back to its storage type.
        assert_eq!(
            ObjectItem::Sequence(SequenceInfo::default()).detail(),
            "bigint"
        );
    }

    // ── The nav cursor on refocus ─────────────────────────────────────────

    /// **A cursor that exists is the user's.** The context menu hands focus back
    /// to a tree that is already focused and already has a cursor, so re-seeding
    /// unconditionally moved the cursor off the row the menu was about — and the
    /// next Shift+F10 or Enter acted on a different row, one whose menu carries
    /// Drop and Truncate.
    #[test]
    fn a_live_cursor_survives_the_tree_regaining_focus() {
        let visible = |_: &str| true;
        assert_eq!(
            resume_cursor(Some("tbl:shop:customers"), Some("tbl:shop:orders"), visible).as_deref(),
            Some("tbl:shop:customers")
        );
    }

    /// With no cursor, the open table's row is where the walk starts — the
    /// click-in-from-outside case this was written for.
    #[test]
    fn an_absent_cursor_is_seeded_from_the_open_table() {
        assert_eq!(
            resume_cursor(None, Some("tbl:shop:orders"), |_| true).as_deref(),
            Some("tbl:shop:orders")
        );
        // …but only to a row that is on screen: a collapsed database's table is
        // not somewhere the arrows could have reached.
        assert_eq!(
            resume_cursor(None, Some("tbl:shop:orders"), |_| false),
            None
        );
        // Nothing open, nothing to seed from.
        assert_eq!(resume_cursor(None, None, |_| true), None);
    }
}
