//! SQL autocomplete: the context-aware suggestion engine and its popup view.
//! `recompute_completions` classifies the caret context (statement start / column
//! / table / `qualifier.` / mixed) from a lightweight token scan built on the
//! shared `skip_noncode` lexer, ranks candidates (schema tables/columns via
//! `SchemaIndex`, plus keyword/function tables) by a fuzzy score within
//! context tiers, and drives the `Completion` state that `completion_popup`
//! renders below the caret. `accept_completion` writes the picked word back into
//! the editor. Scope/context resolution now comes from the shared
//! `schemaic_core::intel` engine (AST-backed, with a lexer fallback); this module
//! is the ranking + popup layer over it. Only `Completion`/`recompute_completions`/
//! `accept_completion`/`completion_popup` are `pub(crate)`; the rest is internal.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use floem::keyboard::{Key, NamedKey};
use floem::kurbo::{Point, Rect};
use floem::prelude::*;
use floem::reactive::Memo;
use floem::views::editor::Editor;
use floem::views::editor::core::cursor::CursorAffinity;
use floem::views::editor::core::editor::EditType;
use floem::views::editor::core::selection::Selection;

use schemaic_core::intel::{self, ClauseCtx, SqlDialect};
use schemaic_core::schema::{DbSchema, SchemaState, db_contributes};
use schemaic_core::snippet;
use schemaic_core::sql::StatementIndex;

// The keyword/function sets live in `schemaic_core::intel` (so the core analysis
// + diagnostics share one authoritative copy), and the ranking that seeds itself
// from them lives in `schemaic_core::rank`.
use schemaic_core::rank;

use floem::AnyView;

use crate::consts::*;
use crate::widgets::{autohide, measure_text_px_at};
use crate::{ConnNode, icons, theme};

// ===== moved from lib.rs (autocomplete) =====
// ── Autocomplete ────────────────────────────────────────────────────────────

/// Autocomplete popup state, shared between the editor key handler, the
/// per-edit recompute, and the popup view.
#[derive(Clone, Copy)]
pub(crate) struct Completion {
    pub(crate) items: RwSignal<Vec<Suggestion>>,
    /// Width the current `items` want, measured when they are set (`set_items`) —
    /// the popup's style closures re-run on every scroll and resize, and a font-system
    /// measurement per row is not a thing to do there.
    pub(crate) width: RwSignal<f64>,
    pub(crate) sel: RwSignal<usize>,
    pub(crate) open: RwSignal<bool>,
    /// Bottom of the caret's line, in editor-*content* coordinates — the anchor the
    /// popup hangs under. Content, not editor-area: the view subtracts the live
    /// viewport origin itself, so the popup keeps up with scrolling.
    pub(crate) point: RwSignal<Point>,
    /// Top of the caret's line, same coordinate space as `point`. A popup that
    /// doesn't fit below the caret flips *above* it, and needs the line's top edge
    /// to hang its bottom from.
    pub(crate) line_top: RwSignal<f64>,
    /// Set right after accepting, so the edit that follows doesn't re-open the
    /// popup on the just-inserted word.
    pub(crate) suppress: RwSignal<bool>,
    /// Did the last key the editor saw *type a character*? Written by the key
    /// handler for every keypress, read by the recompute the resulting edit
    /// schedules — see [`types_a_character`] for why the question cannot be
    /// answered later, and [`popup_may_open`] for what it decides.
    ///
    /// **A one-shot, like [`Completion::suppress`]**: `recompute_completions`
    /// clears it as it reads it. An edit that arrives without a keypress at all
    /// (context-menu paste, IME commit, dropped text) would otherwise be judged
    /// by whatever the last key happened to be.
    pub(crate) typed: RwSignal<bool>,
    /// Signature help for the function call enclosing the caret (independent of the
    /// suggestion list — shown whenever the caret is inside a builtin's parens).
    pub(crate) sig: RwSignal<Option<intel::SignatureHelp>>,
    /// Caret-anchored point for the signature-help popup (its bottom-left; the popup
    /// sits just *above* the caret so it doesn't collide with the suggestion list).
    /// Editor-*content* coordinates, like `point`.
    pub(crate) sig_point: RwSignal<Point>,
    /// The inline prediction at the caret (`intel::predict`), drawn by
    /// `InlineDiffDoc` and taken by Tab. Set with [`inline_diff::set_ghost`] —
    /// never a bare `set`, for that function's `cache_rev` reason.
    ///
    /// [`inline_diff::set_ghost`]: crate::inline_diff::set_ghost
    pub(crate) ghost: crate::inline_diff::GhostSignal,
    /// The word at the caret already spells a candidate in full
    /// (`rank::Ranked::completes_a_word`), so Enter is a newline, not an
    /// accept of whatever longer word heads the list.
    pub(crate) word_complete: RwSignal<bool>,
    /// **The widened list** (`rank::RankInput::wide`): set by a Ctrl+Space
    /// while the list is already open, and kept while it stays open — the
    /// next list that opens by typing starts narrow again.
    pub(crate) wide: RwSignal<bool>,
    /// What each connection's user has accepted before (`accepted.json`):
    /// recorded by [`accept_completion`], read as a ranking bonus by
    /// [`recompute_completions`].
    pub(crate) accepted: crate::stored::Stored<schemaic_core::accepted::AcceptedFile>,
    /// The tab's connection, which an accepted word is recorded against.
    pub(crate) conn: RwSignal<u64>,
    /// The buffer's statement boundaries ([`stmt_index`]), which signature
    /// help and the run outline read on every caret move.
    pub(crate) stmts: StmtIndex,
    /// The tab's text, as the editor's last edit left it — borrowed by what
    /// runs per keystroke and caret move rather than copied out of the
    /// document each time.
    pub(crate) query: RwSignal<String>,
    /// The editor's edits, by revision: recorded before `query` is set, so
    /// what keeps an answer about the text ([`stmt_index`], the styling's
    /// caches) takes it forward without keeping a copy of the text.
    pub(crate) edits: RwSignal<schemaic_core::text::EditLog>,
}

/// `f` over the editor's text, **borrowed** from the pane's `query` — set from
/// the document on every edit, before anything that reads it here runs — rather
/// than copied out of the document per keystroke and caret move. A `query`
/// whose length is not the document's is not trusted, and the document is
/// copied instead.
pub(crate) fn with_text<R>(ed: &Editor, comp: Completion, f: impl FnOnce(&str) -> R) -> R {
    let len = ed.doc().text().len();
    if comp.query.with_untracked(String::len) == len {
        comp.query.with_untracked(|q| f(q))
    } else {
        f(&ed.doc().text().to_string())
    }
}

/// A pane's [`StatementIndex`], with the text revision
/// ([`Completion::edits`]) and dialect it was built for.
pub(crate) type StmtIndex = RwSignal<Option<(u64, SqlDialect, Rc<StatementIndex>)>>;

/// The statement index of `sql`, the pane's text — the one held at this
/// revision, else the held one taken forward over the edits the log recorded
/// since ([`StatementIndex::after_edit`]), else a new one. So a caret move
/// searches and a keystroke rescans about its own statement, where each used
/// to walk the document to the caret — and no copy of the text is kept, nor
/// compared, to find the edit.
pub(crate) fn stmt_index(comp: Completion, sql: &str, dialect: SqlDialect) -> Rc<StatementIndex> {
    use schemaic_core::text::{EditLog, Since};
    let rev = comp.edits.with_untracked(EditLog::rev);
    let held = comp.stmts.with_untracked(Clone::clone);
    if let Some((r, d, idx)) = &held
        && *r == rev
        && *d == dialect
        && idx.text_len() == sql.len()
    {
        return idx.clone();
    }
    let next = Rc::new(match held {
        Some((r, d, idx)) if d == dialect => match comp.edits.with_untracked(|l| l.since(r)) {
            Since::Edit { pre, suf } if pre + suf <= sql.len().min(idx.text_len()) => {
                idx.after_edit(sql, pre, suf)
            }
            _ => StatementIndex::new(sql, dialect),
        },
        _ => StatementIndex::new(sql, dialect),
    });
    comp.stmts.set(Some((rev, dialect, next.clone())));
    next
}

/// The ranking vocabulary, which lives in `core::rank` — see that module for why
/// the policy is not in this file.
pub(crate) use schemaic_core::rank::{
    ColMeta, KeyKind, SchemaIndex, SuggestGlyph, SuggestKind, Suggestion,
};

/// The SVG a ranked row's [`SuggestGlyph`] asks for.
///
/// **The mapping is here and the reason is there.** `core::rank` decides *why* a
/// row gets the glyph it gets — a column's type family, an FK target, an in-scope
/// alias — and this turns that into a picture, because an icon body is a
/// `&'static str` of SVG and core has no business holding one. It is the same
/// split `schema_tree::column_type_icon` already is on the other side of.
fn glyph_icon(glyph: SuggestGlyph, kind: SuggestKind) -> &'static str {
    match glyph {
        SuggestGlyph::Kind => kind_icon(kind),
        SuggestGlyph::ColumnType(class) => crate::schema_tree::column_type_icon(class),
        SuggestGlyph::ForeignKey => icons::KEY_SQUARE,
        SuggestGlyph::Alias => icons::TAG,
        SuggestGlyph::Table => icons::TABLE,
        // The schema tree's view glyph, so the two read alike.
        SuggestGlyph::View => icons::TABLE_CELLS_MERGE,
    }
}

use schemaic_core::sql::is_word_byte;

/// Byte offset where the identifier ending at `offset` begins.
fn word_start(text: &str, offset: usize) -> usize {
    let bytes = text.as_bytes();
    let mut start = offset.min(text.len());
    while start > 0 && is_word_byte(bytes[start - 1]) {
        start -= 1;
    }
    start
}

/// Byte offset where the identifier the caret is in ends — [`word_start`]'s
/// mirror. An accept replaces the word from its start to here, not to the
/// caret: accepting `customer_id` at `cust|_id` wrote `customer_id_id`.
fn word_end(text: &str, offset: usize) -> usize {
    let bytes = text.as_bytes();
    let mut end = offset.min(text.len());
    while end < bytes.len() && is_word_byte(bytes[end]) {
        end += 1;
    }
    end
}

thread_local! {
    /// One memoised catalog per UI thread — see [`intel::CatalogCache`]. A single
    /// keystroke reaches `build_catalog` up to four times (column completion, JOIN
    /// targets, signature help, diagnostics) and each build is milliseconds at
    /// customer scale, so they share one. The cache keys on the schemas'
    /// `Arc` identity, which a re-introspection changes, so nothing here has to be
    /// invalidated by hand.
    static CATALOG: RefCell<intel::CatalogCache> = RefCell::new(intel::CatalogCache::default());

    /// One memoised [`SchemaIndex`] per UI thread, for the reason the catalog
    /// beside it gives — and it was the half that never got it.
    ///
    /// `SchemaIndex::build` walks every loaded database, every table and every
    /// column, allocating a fresh `ColMeta` per column plus three `HashMap`s.
    /// `recompute_completions` called it directly, and that function is
    /// deliberately undebounced (`editor_pane` schedules it at
    /// `Duration::ZERO`, one tick, only so the caret has settled) — so it ran
    /// on effectively every keystroke of the ordinary case, since typing an
    /// identifier leaves a non-empty prefix. This exact cost was found and
    /// fixed once, for `Catalog`, with a cache keyed on `Arc` identity; the
    /// index kept paying it.
    static SCHEMA_INDEX: RefCell<Option<CachedIndex>> = const { RefCell::new(None) };
}

/// One database node as the index reads it: its name and the schema `Arc` it
/// currently holds, if any.
///
/// Taken out of the signals once, so the cache key and the build see the same
/// snapshot — reading `node.schema` twice could otherwise key an index on a
/// schema it was not built from.
struct LoadedNode {
    database: String,
    schema: Option<std::sync::Arc<schemaic_core::schema::DbSchema>>,
}

/// One built index and the inputs it was built from.
struct CachedIndex {
    nodes: Vec<LoadedNode>,
    hidden: HashSet<String>,
    active_db: Option<String>,
    index: Rc<SchemaIndex>,
}

/// The [`SchemaIndex`] for these nodes, rebuilding only when the node list,
/// their schemas, the hidden set or the active database has moved.
///
/// Identity, not equality, for the schemas: `Arc::ptr_eq` is what a
/// re-introspection changes, so nothing here has to be invalidated by hand —
/// the same key [`intel::CatalogCache`] uses, for the same reason.
fn schema_index(
    db_nodes: RwSignal<Vec<ConnNode>>,
    hidden: &HashSet<String>,
    active_db: Option<&str>,
) -> Rc<SchemaIndex> {
    let nodes: Vec<LoadedNode> = db_nodes
        .get_untracked()
        .into_iter()
        .map(|n| LoadedNode {
            schema: match n.schema.get_untracked() {
                SchemaState::Loaded(s) => Some(s),
                _ => None,
            },
            database: n.database,
        })
        .collect();
    SCHEMA_INDEX.with(|cell| {
        let mut cell = cell.borrow_mut();
        if let Some(hit) = cell.as_ref()
            && hit.active_db.as_deref() == active_db
            && &hit.hidden == hidden
            && hit.nodes.len() == nodes.len()
            && hit.nodes.iter().zip(&nodes).all(|(a, b)| {
                a.database == b.database
                    && match (&a.schema, &b.schema) {
                        (None, None) => true,
                        (Some(x), Some(y)) => std::sync::Arc::ptr_eq(x, y),
                        _ => false,
                    }
            })
        {
            return Rc::clone(&hit.index);
        }
        let index = Rc::new(index::build(&nodes, hidden, active_db));
        *cell = Some(CachedIndex {
            nodes,
            hidden: hidden.clone(),
            active_db: active_db.map(str::to_string),
            index: Rc::clone(&index),
        });
        index
    })
}

/// The `schemaic_core::intel::Catalog` for **validation** — every loaded
/// database, hidden or not.
///
/// `schema::db_contributes`' own doc draws the line this pair of functions
/// exists to keep: *"Hiding governs what is offered; never what is true."* The
/// editor's diagnostics are the "true" side — a column that exists must not be
/// squiggled because its database is hidden behind the SCHEMA eye — so this one
/// asks nothing about hiding. Everything that makes an **offer** takes
/// [`build_offer_catalog`] instead.
///
/// That split is the fix for one popup answering "does this database exist" two
/// ways: `SchemaIndex::build` filters with `db_contributes` and this did not, so
/// with `archive` hidden and no database selected the plain table list skipped
/// it entirely while the FK JOIN-target rows immediately above offered
/// `archive`'s tables in the top tier — and accepting one spliced
/// `customers ON o.customer_id = customers.id` for a database the tree was
/// hiding.
pub(crate) fn build_catalog(
    db_nodes: RwSignal<Vec<ConnNode>>,
    active_db: Option<&str>,
) -> Arc<intel::Catalog> {
    catalog_of(db_nodes, None, active_db)
}

/// The catalog for **offers** — FK auto-join, FK JOIN targets, `SELECT *`
/// expansion. A database the SCHEMA eye has hidden contributes nothing to it,
/// which is the rule `SchemaIndex::build` has always followed and these three
/// surfaces did not.
///
/// Shares [`CATALOG`]'s cache, which keeps **two** slots for exactly this: the
/// filtered `loaded` list is a different key from the unfiltered one
/// diagnostics pass, and the two callers alternate. A different key is what
/// makes two views *coexist* only in a cache with room for both — this line
/// claimed it of a single-entry one, which serves an alternation at a 0% hit
/// rate, so with any database hidden every keystroke paid a full
/// `Catalog::build` and the debounced diagnostics paid a second. Invisible
/// until a database is hidden, since an empty hidden set makes the two lists
/// byte-identical. See [`intel::CatalogCache`].
pub(crate) fn build_offer_catalog(
    db_nodes: RwSignal<Vec<ConnNode>>,
    hidden: &HashSet<String>,
    active_db: Option<&str>,
) -> Arc<intel::Catalog> {
    catalog_of(db_nodes, Some(hidden), active_db)
}

/// Every database of the connection whose schema has loaded, by name — the
/// `Arc`s out of `SchemaState`, so refcount bumps rather than copies.
pub(crate) fn loaded_schemas(db_nodes: RwSignal<Vec<ConnNode>>) -> Vec<(String, Arc<DbSchema>)> {
    db_nodes.with_untracked(|nodes| {
        nodes
            .iter()
            .filter_map(|node| match node.schema.get_untracked() {
                SchemaState::Loaded(schema) => Some((node.database.clone(), schema)),
                _ => None,
            })
            .collect()
    })
}

fn catalog_of(
    db_nodes: RwSignal<Vec<ConnNode>>,
    hidden: Option<&HashSet<String>>,
    active_db: Option<&str>,
) -> Arc<intel::Catalog> {
    // Each `schema` is the `Arc` out of `SchemaState`, so this walk is refcount
    // bumps rather than a deep copy of every table and column of every loaded
    // database — which is what it was, several times per keystroke.
    let loaded: Vec<(String, Arc<DbSchema>)> = db_nodes
        .get_untracked()
        .into_iter()
        .filter(|node| hidden.is_none_or(|h| db_contributes(h, &node.database, active_db)))
        .filter_map(|node| match node.schema.get_untracked() {
            SchemaState::Loaded(schema) => Some((node.database, schema)),
            _ => None,
        })
        .collect();
    CATALOG.with(|c| c.borrow_mut().get(&loaded, active_db))
}

/// Building the index is this crate's half — its input is the schema tree's
/// signals. The *shape* is [`SchemaIndex`], in `core::rank`, because ranking is
/// what reads it.
mod index {
    use super::*;

    /// A table's columns as completion offers them — a synonym's built from
    /// the table it resolves to, by the same rule.
    fn col_metas(t: &schemaic_core::schema::TableInfo) -> Rc<Vec<ColMeta>> {
        // Is this column covered by a foreign key (→ the FK tint)? A linear
        // scan of the table's FK columns, which number a handful: building a
        // lowercased `HashSet` cost an allocation per FK column *and* one per
        // column looked up.
        let is_fk = |name: &str| {
            t.foreign_keys
                .iter()
                .flat_map(|fk| fk.columns.iter())
                .any(|c| c.eq_ignore_ascii_case(name))
        };
        Rc::new(
            t.columns
                .iter()
                .map(|c| ColMeta {
                    name: c.name.clone(),
                    type_name: c.type_name.clone(),
                    nullable: c.nullable,
                    primary_key: c.primary_key,
                    foreign_key: is_fk(&c.name),
                })
                .collect(),
        )
    }

    /// Build the completion index. When `active_db` is `Some`, the *unqualified*
    /// suggestion pool (`tables`/`columns`) is scoped to that database, so a tab with
    /// a selected database isn't polluted by every other database's tables.
    /// `databases`/`tables_by_db` stay complete so an explicit `otherdb.table`
    /// qualifier still completes — and `database_suggestion_visible` keeps the other
    /// database *names* out of the table list until a prefix is typed.
    ///
    /// A database the SCHEMA eye has hidden contributes nothing at all — not its
    /// name, not its tables, not its columns — unless it is the active one; see
    /// [`schemaic_core::schema::db_contributes`].
    pub(super) fn build(
        nodes: &[LoadedNode],
        hidden: &HashSet<String>,
        active_db: Option<&str>,
    ) -> SchemaIndex {
        let mut databases = Vec::new();
        let mut tables = Vec::new();
        // Each in-scope table name's namespaces, so `rank` can qualify one no
        // bare name reaches.
        let mut namespaces: HashMap<String, Vec<Option<String>>> = HashMap::new();
        let mut columns: HashMap<String, Vec<ColMeta>> = HashMap::new();
        let mut columns_by_db: HashMap<(String, String), Rc<Vec<ColMeta>>> = HashMap::new();
        let mut tables_by_db: HashMap<String, Vec<String>> = HashMap::new();
        // `schema.table` on PostgreSQL and SQL Server: the in-scope database's
        // tables by their namespace, as `intel::Catalog::build` keeps them.
        let mut columns_by_schema: HashMap<(String, String), Rc<Vec<ColMeta>>> = HashMap::new();
        let mut tables_by_schema: HashMap<String, Vec<String>> = HashMap::new();
        // Every contributing database's tables, for the widened list.
        let mut all_tables: Vec<rank::IndexedTable> = Vec::new();
        let mut views: HashSet<(String, String)> = HashSet::new();
        let mut enum_types: HashMap<String, Vec<String>> = HashMap::new();
        let mut routines: Vec<rank::IndexedRoutine> = Vec::new();
        for node in nodes {
            if !db_contributes(hidden, &node.database, active_db) {
                continue;
            }
            if !databases
                .iter()
                .any(|d: &String| d.eq_ignore_ascii_case(&node.database))
            {
                databases.push(node.database.clone());
            }
            if let Some(schema) = &node.schema {
                let db_lower = node.database.to_ascii_lowercase();
                let by_db = tables_by_db.entry(db_lower.clone()).or_default();
                // Unqualified pool: only the selected database (or all, if none).
                let in_scope = active_db.is_none_or(|db| db.eq_ignore_ascii_case(&node.database));
                // A PostgreSQL enum's labels, by the name a column's type
                // carries — bare or `schema.name`.
                if in_scope {
                    for e in &schema.enums {
                        let name = e.name.to_ascii_lowercase();
                        if let Some(ns) = &e.schema {
                            enum_types.insert(
                                format!("{}.{name}", ns.to_ascii_lowercase()),
                                e.values.clone(),
                            );
                        }
                        enum_types.entry(name).or_insert_with(|| e.values.clone());
                    }
                    // Its stored functions and procedures, offered where each
                    // can be called.
                    routines.extend(schema.routines.iter().map(|r| rank::IndexedRoutine {
                        name: r.name.clone(),
                        kind: r.kind,
                        arguments: r.arguments.clone(),
                        returns: r.returns.clone(),
                    }));
                }
                for t in &schema.tables {
                    by_db.push(t.name.clone());
                    all_tables.push(rank::IndexedTable {
                        database: node.database.clone(),
                        schema: t.schema.clone(),
                        name: t.name.clone(),
                    });
                    if t.is_view {
                        views.insert((db_lower.clone(), t.name.to_ascii_lowercase()));
                    }
                    let metas = col_metas(t);
                    if in_scope && let Some(ns) = &t.schema {
                        let ns = ns.to_ascii_lowercase();
                        tables_by_schema
                            .entry(ns.clone())
                            .or_default()
                            .push(t.name.clone());
                        columns_by_schema.insert((ns, t.name.to_ascii_lowercase()), metas.clone());
                    }
                    // Every database's columns are keyed by (db, table) for qualified
                    // (incl. cross-database) completion.
                    columns_by_db.insert(
                        (db_lower.clone(), t.name.to_ascii_lowercase()),
                        metas.clone(),
                    );
                    if in_scope {
                        tables.push((t.name.clone(), node.database.clone()));
                        namespaces
                            .entry(t.name.to_ascii_lowercase())
                            .or_default()
                            .push(t.schema.clone());
                        let entry = columns.entry(t.name.to_ascii_lowercase()).or_default();
                        if entry.is_empty() {
                            // The ordinary case — nothing to dedup against, so skip
                            // the per-column scan that makes the merge quadratic.
                            entry.extend(metas.iter().cloned());
                        } else {
                            // A same-named table in another database: merge by name.
                            for m in metas.iter() {
                                if !entry.iter().any(|e| e.name.eq_ignore_ascii_case(&m.name)) {
                                    entry.push(m.clone());
                                }
                            }
                        }
                    }
                }
                // A SQL Server synonym is offered as a table, with its target's
                // columns where the target is this database's own table
                // (`DbSchema::synonyms`), none otherwise.
                //
                // **The resolved table's own columns** (S4.2-L1-05), not the
                // `(db, name)` entry's, which holds whichever same-named table
                // was indexed last — `sales.docs` for a synonym over
                // `dbo.docs`, offering columns diagnostics then squiggled. And
                // merged into a same-named table's entries, never over them, as
                // the diagnostics catalogue does.
                let merge = |entry: &mut Vec<ColMeta>, metas: &[ColMeta]| {
                    for m in metas {
                        if !entry.iter().any(|e| e.name.eq_ignore_ascii_case(&m.name)) {
                            entry.push(m.clone());
                        }
                    }
                };
                for (syn, target) in schema.synonyms(&node.database) {
                    by_db.push(syn.name.clone());
                    let name_lower = syn.name.to_ascii_lowercase();
                    let metas = target.map(col_metas);
                    if let Some(m) = &metas {
                        let key = (db_lower.clone(), name_lower.clone());
                        match columns_by_db.get(&key) {
                            Some(have) => {
                                let mut all: Vec<ColMeta> = have.iter().cloned().collect();
                                merge(&mut all, m);
                                columns_by_db.insert(key, Rc::new(all));
                            }
                            None => {
                                columns_by_db.insert(key, m.clone());
                            }
                        }
                    }
                    if in_scope {
                        tables.push((syn.name.clone(), node.database.clone()));
                        if let Some(m) = metas {
                            merge(columns.entry(name_lower).or_default(), &m);
                        }
                    }
                }
            }
        }
        SchemaIndex {
            databases,
            tables,
            namespaces,
            columns,
            columns_by_db,
            tables_by_db,
            columns_by_schema,
            tables_by_schema,
            all_tables,
            views,
            enum_types,
            routines,
        }
    }
}

/// Pin the suggestion popup to the caret's line: the line's top and bottom edges,
/// each plus the editor's top padding (which `points_of_offset` doesn't count).
///
/// These stay in editor-**content** coordinates. `points_of_offset` answers an
/// absolute document y, so an overlay pinned in the (unscrolling) `editor_area` has
/// to subtract the viewport origin — and the popup's style closure is the only place
/// that can, because it re-runs when the editor scrolls and this doesn't. Baking the
/// subtraction in here would freeze the popup at the scroll position it opened at.
fn set_anchor(ed: &Editor, comp: Completion, offset: usize) {
    let (top, bot) = ed.points_of_offset(offset, CursorAffinity::Backward);
    comp.point.set(Point::new(bot.x, bot.y + EDITOR_PAD_TOP));
    comp.line_top.set(top.y + EDITOR_PAD_TOP);
}

/// Where the suggestion list goes: its top y in `editor_area` coordinates, and the
/// height cap its scroll area takes.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Placement {
    top: f64,
    max_h: f64,
}

/// Fit `rows` suggestions against the caret line `[line_top, line_bottom]` (already
/// viewport-relative) inside an `area_h`-tall editor pane.
///
/// Below the caret when the list fits there, else flipped above it, else on the
/// roomier side with the list shortened. The popup used to do none of that — it hung
/// unconditionally below the caret, so completing on one of the last lines of the
/// editor drew the list down across the results grid.
fn popup_placement(line_top: f64, line_bottom: f64, rows: usize, area_h: f64) -> Placement {
    let want = (rows as f64 * completion_row_h()).min(completion_max_h()) + COMPLETION_BORDER;
    let below_top = line_bottom + COMPLETION_LINE_H;
    let room_below = (area_h - completion_edge_pad() - below_top).max(0.0);
    let room_above = (line_top - COMPLETION_LINE_H - completion_edge_pad()).max(0.0);
    // A flipped popup hangs from its *bottom*, so its height has to be pinned rather
    // than left to the content: `completion_row_h()` is a measurement, and if the real
    // rows come out a hair taller the box would grow back down over the caret line.
    // Capping it means an under-estimate costs a few scrolled pixels in a list that
    // already scrolls, which is the cheaper way to be wrong.
    let above = |h: f64| Placement {
        top: (line_top - COMPLETION_LINE_H - h).max(0.0),
        max_h: h - COMPLETION_BORDER,
    };
    // An unmeasured pane (height 0 until the first layout) keeps the plain
    // below-the-caret placement rather than flipping on a height that isn't real.
    if area_h <= 0.0 || want <= room_below {
        return Placement {
            top: below_top,
            max_h: completion_max_h(),
        };
    }
    if want <= room_above {
        return above(want);
    }
    // Neither side fits the whole list: take the roomier one and shorten to it.
    if room_below >= room_above {
        Placement {
            top: below_top,
            max_h: (room_below - COMPLETION_BORDER).max(completion_min_h()),
        }
    } else {
        above(room_above.max(completion_min_h() + COMPLETION_BORDER))
    }
}

/// Width one suggestion row wants, from its already-measured text widths, rounded
/// up and with `completion_slack_w()` of air. The annotation columns are optional; the
/// chrome around them isn't (see the `COMPLETION_*_W` consts for what each term is).
fn row_width(name_w: f64, table_w: f64, detail_w: f64) -> f64 {
    (completion_icon_w()
        + name_w
        + completion_gap_w()
        + table_w
        + completion_detail_gap()
        + detail_w
        + 2.0 * completion_row_pad()
        + completion_slack_w())
    .ceil()
}

/// The list's natural width: its widest row, measured through the same font system
/// the rows paint with (`measure_text_px_at`), so the box is sized to what is
/// actually in it. Before this the popup was a flat `min_width(320)`, which left a
/// list of one-letter column names three-quarters empty.
fn natural_width(items: &[Suggestion]) -> f64 {
    // Each measurement builds a `TextLayout`, and this runs over the whole list on
    // every keystroke — most keyword rows carry no table and no detail at all, so
    // short-circuit the empty ones rather than laying out an empty string 80 times.
    fn text_w(s: &str, size: f32) -> f64 {
        if s.is_empty() {
            0.0
        } else {
            measure_text_px_at(s, size)
        }
    }
    items
        .iter()
        .map(|it| {
            row_width(
                text_w(&it.text, completion_name_size()),
                text_w(
                    &annotation_label(&it.table, &it.alias),
                    completion_annot_size(),
                ),
                text_w(&it.detail, completion_annot_size()),
            )
        })
        .fold(0.0_f64, f64::max)
        + COMPLETION_BORDER
}

/// Set the suggestion list and the width it wants in one step, so the two can't get
/// out of step. Every path that fills `comp.items` goes through here.
fn set_items(comp: Completion, items: Vec<Suggestion>) {
    comp.width.set(natural_width(&items));
    comp.items.set(items);
    // Said of the list this replaces; the one caller that knows sets it after.
    comp.word_complete.set(false);
}

/// A column row's mid annotation: its owning table, plus the in-scope alias when it
/// has one. Shared with the row builder so the measurement can't drift from what is
/// drawn.
fn annotation_label(table: &str, alias: &str) -> String {
    // No table means no annotation — the row builder omits the node entirely, and an
    // orphan alias would measure width the row never draws.
    if table.is_empty() {
        String::new()
    } else if alias.is_empty() {
        table.to_string()
    } else {
        format!("{table} {alias}")
    }
}

/// The width the popup is drawn at: its `natural` width, floored so a short list
/// isn't a sliver, capped so one long function signature ellipsizes instead of
/// dragging every row out with it, then capped again by the pane it has to fit in.
fn popup_w(natural: f64, area_w: f64) -> f64 {
    let want = natural.clamp(completion_min_w(), completion_max_w());
    // Width 0 until the pane is first laid out — no cap to apply yet.
    if area_w <= 0.0 {
        return want;
    }
    // A pane narrower than the floor wins over the floor: better a cramped list
    // than one that starts outside the editor.
    want.min((area_w - 2.0 * completion_edge_pad()).max(0.0))
}

/// Left edge of a popup `w` wide against a caret at `caret_x` (viewport-relative)
/// in an `area_w`-wide pane.
///
/// Under the caret, slid left as far as it takes to keep the right edge inside the
/// pane. It used to be `COMPLETION_GUTTER + caret_x` flat, so completing near the
/// right edge ran the list off the pane and `.clip()` cut every row's annotations
/// off mid-word — worse still with the AI panel hidden, where the editor's right
/// edge is the window's.
fn popup_x(caret_x: f64, w: f64, area_w: f64) -> f64 {
    let want = COMPLETION_GUTTER + caret_x;
    if area_w <= 0.0 {
        return want.max(0.0);
    }
    // Right edge first, then left: a popup wider than the pane starts flush at 0
    // rather than at a negative x, which would hide the names rather than the
    // details and defeat the whole point.
    want.min(area_w - completion_edge_pad() - w).max(0.0)
}

/// Update the signature-help state for the caret. Independent of the suggestion
/// list — runs on every edit so it appears the moment the caret enters a builtin's
/// parentheses (including right after accepting `func()`), and clears when it leaves.
pub(crate) fn update_signature_help(
    ed: &Editor,
    comp: Completion,
    dialect: SqlDialect,
    db_nodes: RwSignal<Vec<ConnNode>>,
    hidden_dbs: floem::reactive::Memo<HashSet<String>>,
    active_db: Option<&str>,
) {
    let offset = ed.cursor.get_untracked().offset();
    // The database's own routines too — the index is memoised, so this is a
    // lookup on a caret move, not a rebuild.
    let schema = hidden_dbs.with_untracked(|h| schema_index(db_nodes, h, active_db));
    let help = with_text(ed, comp, |text| {
        let (lo, hi) = stmt_index(comp, text, dialect).statement_range(text, offset);
        intel::signature_help(text, lo, hi, offset, dialect, &schema.routines)
    });
    if help.is_some() {
        // `.0` is the point at the *top* of the caret's line; the popup sits above it.
        let mut p = ed.points_of_offset(offset, CursorAffinity::Backward).0;
        p.y += EDITOR_PAD_TOP;
        comp.sig_point.set(p);
    }
    comp.sig.set(help);
}

/// Did this keypress **type a character** into the document?
///
/// Asked by the editor's key handler of every key, and recorded on
/// [`Completion::typed`] for the recompute that the resulting edit schedules.
///
/// Floem inserts a plain character *after* the key handler returns, and the
/// recompute runs a tick after that — by which time the only thing left to say
/// what happened is the document, and a document cannot tell a typed `x` from
/// Ctrl+X. So the question is answered here, while the key is still in hand.
///
/// `Ctrl`/`Alt` mean *command*, never text: Ctrl+X deletes a line and Ctrl+Z
/// undoes, and both arrive as `Character` — which is precisely how they used to
/// pop a suggestion list wherever the caret landed. This matches the auto-pair
/// handler's own test one screen down, including its cost: AltGr (`Ctrl+Alt` on
/// Windows) is excluded, so a character typed that way opens nothing on its own
/// and needs Ctrl+Space. Space is deliberately *in* — it arrives named on some
/// platforms and as `" "` on others, and the empty-prefix list after `WHERE `
/// (`clause_continuation`'s `auto_show`) is typed input like any other.
pub(crate) fn types_a_character(key: &Key, ctrl: bool, alt: bool) -> bool {
    if ctrl || alt {
        return false;
    }
    matches!(key, Key::Character(_) | Key::Named(NamedKey::Space))
}

/// May a recompute that finds the popup **closed** open it?
///
/// Every document change re-runs the recompute, and the recompute is what
/// decides to show a list — so before this rule existed, *any* edit could summon
/// one: Ctrl+X landed the caret mid-word on the following line and the list
/// appeared for a word nobody was typing, undo did the same, and Enter after a
/// clause keyword opened the `auto_show` list on the new blank line. None of
/// those asked for a suggestion.
///
/// Typing is the only thing that opens the popup by itself; Ctrl+Space is the
/// explicit request. A list that is *already* open keeps recomputing whatever
/// the edit was, so Backspace still refines it and closes it when the prefix is
/// gone — this rule is about what may **start** showing one, not about what may
/// change one.
///
/// [`Completion::suppress`] is the other half and is not this: it is a one-shot
/// that closes an open list after an edit the app itself made.
pub(crate) fn popup_may_open(force: bool, already_open: bool, typed: bool) -> bool {
    force || already_open || typed
}

/// Recompute context-aware suggestions for the word at the caret. Ranks the most
/// relevant kind first (columns of the in-scope tables after SELECT/WHERE, tables
/// after FROM, a qualifier's columns after `x.`, statement keywords at the
/// start), then functions/keywords; within a tier, best fuzzy match wins. Empty
/// prefix closes the popup unless `force` (Ctrl+Space) or the caret is right
/// after a `.`.
/// Everything the suggestion engine knows about *where* the caret is — the
/// catalogue it can name things from, the dialect it parses in, and the snippet
/// library with the connection its scopes are judged against.
///
/// One argument rather than six because they are only meaningful together: a
/// call that passed the schema of one connection and the snippets of another
/// would be a bug no signature could catch, and the list had grown past what a
/// reader can check at a call site.
#[derive(Clone, Copy)]
pub(crate) struct CompletionCtx<'a> {
    pub(crate) db_nodes: RwSignal<Vec<ConnNode>>,
    pub(crate) hidden_dbs: Memo<HashSet<String>>,
    pub(crate) active_db: Option<&'a str>,
    pub(crate) dialect: SqlDialect,
    pub(crate) snippets: Memo<Vec<snippet::Snippet>>,
    pub(crate) conn_id: u64,
    /// The editor's inline predictions are on (Settings → Editor).
    pub(crate) predictions: bool,
    /// And may be remembered, not only reasoned (Settings → Editor).
    pub(crate) remembered: bool,
    /// The run history, every connection's; [`prediction_at`] reads this
    /// connection's.
    pub(crate) history: RwSignal<Vec<schemaic_core::history::HistoryEntry>>,
    /// Every open tab; [`prediction_at`] reads this connection's.
    pub(crate) tabs: RwSignal<Vec<crate::Tab>>,
}

/// The ghost to show at `offset`, or `None` — [`intel::predict`] over the
/// *offer* catalog, the one the popup's foreign-key join reads, so a hidden
/// database is no more predicted from than it is offered; failing that, the
/// one [`recall`] remembers from past runs, snippets and open tabs. Which of
/// the two, and whether it can be shown at all, is [`pick_prediction`]'s.
fn prediction_at(
    ed: &Editor,
    text: &str,
    offset: usize,
    ctx: &CompletionCtx<'_>,
    own: Option<OwnTab<'_>>,
) -> Option<crate::inline_diff::EditorGhost> {
    if !ctx.predictions {
        return None;
    }
    // The catalog only where a ghost could go: building it is the costly half
    // of a keystroke's prediction, and most carets are mid-word.
    let reasoned = intel::may_predict_at(text, offset, ctx.dialect)
        .then(|| {
            let catalog = ctx
                .hidden_dbs
                .with_untracked(|h| build_offer_catalog(ctx.db_nodes, h, ctx.active_db));
            intel::predict(text, offset, &catalog, ctx.dialect)
        })
        .flatten();
    let (guess, remembered) = pick_prediction(reasoned, || recall(text, offset, ctx, own))?;
    Some(ghost_at(ed, offset, guess, false, remembered))
}

/// The tab the caret is in, for [`recall`]: its text's signal, and the
/// statement index the pane already holds of that text.
type OwnTab<'a> = (RwSignal<String>, &'a StatementIndex);

/// The remembered prediction at `offset` — [`intel::Recall`], its gate run
/// once, asked of each text in the order it is trusted: this connection's
/// runs, newest first ([`recallable`]); then the user's own snippets that
/// apply here ([`snippet_recallable`]); then the open tabs in the same place
/// ([`same_place`]) — statements written but not run, the tab being typed in
/// among them. `None` with remembered predictions switched off.
///
/// Each tab's text is read inside its own signal rather than cloned out: this
/// runs on a keystroke, and a tab disposed under it answers nothing. **The
/// caret's own tab is not lexed again**: `own` is its text's signal and the
/// pane's index of it, whose statements are read straight off
/// ([`intel::Recall::from_ranges`]).
fn recall(
    text: &str,
    offset: usize,
    ctx: &CompletionCtx<'_>,
    own: Option<OwnTab<'_>>,
) -> Option<String> {
    if !ctx.remembered {
        return None;
    }
    let r = intel::Recall::at(text, offset, ctx.dialect)?;
    ctx.history
        .with_untracked(|entries| {
            entries
                .iter()
                .filter(|e| recallable(e, ctx.dialect, ctx.conn_id, ctx.active_db))
                .find_map(|e| r.from(&e.sql))
        })
        .or_else(|| {
            ctx.snippets.with_untracked(|all| {
                all.iter()
                    .filter(|s| snippet_recallable(s, ctx.dialect, ctx.conn_id))
                    .find_map(|s| r.from(&s.body))
            })
        })
        .or_else(|| {
            ctx.tabs.with_untracked(|tabs| {
                tabs.iter().find_map(|t| {
                    let conn = t.conn_id.try_get_untracked()?;
                    let db = t.database.try_get_untracked()?;
                    if !same_place(ctx.dialect, conn, db.as_deref(), ctx.conn_id, ctx.active_db) {
                        return None;
                    }
                    t.query.try_with_untracked(|q| {
                        let q = q?;
                        match own {
                            Some((sig, idx)) if sig == t.query && idx.text_len() == q.len() => {
                                r.from_ranges(q, &idx.statement_ranges(q))
                            }
                            _ => r.from(q),
                        }
                    })
                })
            })
        })
}

/// Which prediction to show: the one [`intel::predict`] reasons, else the one
/// history remembers (answered `true`) — and neither when it holds a line
/// break. `InlineDiffDoc::ghost_on` cannot draw one, but Tab still inserted it
/// and Escape spent a press on it: an offer nobody could see.
fn pick_prediction(
    reasoned: Option<String>,
    remembered: impl FnOnce() -> Option<String>,
) -> Option<(String, bool)> {
    let (guess, from_history) = match reasoned {
        Some(g) => (g, false),
        None => (remembered()?, true),
    };
    (!guess.contains('\n')).then_some((guess, from_history))
}

/// Is `e` a run to remember in this tab? Its connection's; not one that
/// failed — a statement the server refused is not one to offer again — and
/// run against the database the tab is in, where both say which.
fn recallable(
    e: &schemaic_core::history::HistoryEntry,
    dialect: SqlDialect,
    conn_id: u64,
    active_db: Option<&str>,
) -> bool {
    e.outcome != schemaic_core::history::Outcome::Failed
        && same_place(
            dialect,
            e.conn_id,
            e.database.as_deref(),
            conn_id,
            active_db,
        )
}

/// Was a text written in this tab's place — its connection, and its database
/// where both say which, compared by the engine's case rule
/// ([`schemaic_core::history::same_database`])?
fn same_place(
    dialect: SqlDialect,
    conn_id: u64,
    database: Option<&str>,
    here_conn: u64,
    here_db: Option<&str>,
) -> bool {
    let same_db = match (database, here_db) {
        (Some(there), Some(here)) => schemaic_core::history::same_database(dialect, there, here),
        _ => true,
    };
    conn_id == here_conn && same_db
}

/// Is `s` a snippet to remember from here? The user's own — a built-in is a
/// template, not a statement anyone wrote — and one that applies on this
/// connection ([`snippet::applies`]).
fn snippet_recallable(s: &snippet::Snippet, dialect: SqlDialect, conn_id: u64) -> bool {
    s.source == snippet::Source::User && snippet::applies(s, dialect, conn_id)
}

/// Does `ghost` take the place of the list at the word being typed (`prefix`)?
///
/// A reasoned one does at an empty word: the list opens there only on its own
/// guesses — after `WHERE`/`ON`/`BY`, the FK join row — and the ghost is the
/// same kind of guess, made with more certainty. A remembered one does not: it
/// is one past statement, and taking the empty word's place made the table
/// list after `FROM ` and the columns after `WHERE ` stop opening once any
/// such statement had run. That list opens, and the remembered ghost gives way
/// to it. A kept ghost does at any word, remembered or not: its first part was
/// just taken or typed, so it is the offer the user is following, and a list
/// opening on the letters typed into it would take Tab from it.
fn ghost_displaces_the_list(ghost: Option<&crate::inline_diff::EditorGhost>, prefix: &str) -> bool {
    ghost.is_some_and(|g| g.kept || (prefix.is_empty() && !g.remembered))
}

/// The rest of `g` once the user has typed its first characters: `typed` is
/// the document's text from the ghost's offset to the `caret`, now `doc_len`
/// bytes long. `None` unless that edit was exactly those characters typed at
/// the ghost — the caret ahead of it by as many bytes as the document grew,
/// and the text there the ghost's own — or the ghost is used up.
///
/// The fish shell's and the code editors' rule: a suggestion stays while what
/// is typed is what it says, rather than vanishing at the first letter and
/// coming back after the next space.
pub(crate) fn typed_through<'g>(
    g: &'g crate::inline_diff::EditorGhost,
    caret: usize,
    doc_len: usize,
    typed: &str,
) -> Option<&'g str> {
    let k = caret.checked_sub(g.offset).filter(|&k| k > 0)?;
    if doc_len != g.doc_len + k || g.text.get(..k)? != typed {
        return None;
    }
    let rest = &g.text[k..];
    (!rest.is_empty()).then_some(rest)
}

/// The caret a ghost may sit at: the cursor's offset when it is one caret with
/// nothing selected. Not with several carets — the ghost was drawn at one,
/// and taking it there collapsed the rest — nor with a selection, which Tab
/// indents and the ghost would have replaced.
pub(crate) fn lone_caret(cursor: &floem::views::editor::core::cursor::Cursor) -> Option<usize> {
    use floem::views::editor::core::cursor::CursorMode;
    match &cursor.mode {
        CursorMode::Normal(offset) => Some(*offset),
        CursorMode::Insert(sel) => match sel.regions() {
            [only] if only.is_caret() => Some(only.end),
            _ => None,
        },
        CursorMode::Visual { .. } => None,
    }
}

/// Is `g` the rest of a prediction to keep as it stands, at `caret` in a
/// document of `doc_len` bytes?
///
/// Only one a Ctrl+→ or a typed letter left ([`EditorGhost::kept`]), at the
/// caret it left, while the document is the length it left it: an edit that
/// moves no caret — a forward Delete there — changes what follows the caret,
/// and the rest is asked for afresh.
///
/// [`EditorGhost::kept`]: crate::inline_diff::EditorGhost::kept
fn keeps_ghost(g: &crate::inline_diff::EditorGhost, caret: usize, doc_len: usize) -> bool {
    g.kept && g.doc_len == doc_len && g.offset == caret
}

/// A ghost of `text` at `offset`, with the line and column [`EditorGhost`]
/// carries and the document's length now.
///
/// [`EditorGhost`]: crate::inline_diff::EditorGhost
pub(crate) fn ghost_at(
    ed: &Editor,
    offset: usize,
    text: String,
    kept: bool,
    remembered: bool,
) -> crate::inline_diff::EditorGhost {
    let rope = ed.doc().text();
    let line = rope.line_of_offset(offset);
    crate::inline_diff::EditorGhost {
        offset,
        line,
        col: offset - rope.offset_of_line(line),
        text,
        doc_len: rope.len(),
        kept,
        remembered,
    }
}

pub(crate) fn recompute_completions(
    ed: &Editor,
    ctx: CompletionCtx<'_>,
    comp: Completion,
    force: bool,
) {
    // Per keystroke: the text borrowed, not copied out of the document.
    with_text(ed, comp, |text| {
        recompute_completions_in(ed, ctx, comp, force, text)
    });
}

/// [`recompute_completions`]' body, over the editor's text.
fn recompute_completions_in(
    ed: &Editor,
    ctx: CompletionCtx<'_>,
    comp: Completion,
    force: bool,
    text: &str,
) {
    let CompletionCtx {
        db_nodes,
        hidden_dbs,
        active_db,
        dialect,
        snippets,
        conn_id,
        predictions: _,
        remembered: _,
        history: _,
        tabs: _,
    } = ctx;
    let offset = ed.cursor.get_untracked().offset();
    // A `.` just typed is a qualifier trigger — reveal the qualifier's members even
    // right after accepting it (otherwise `suppress`, set by the accept, would swallow
    // the very next keystroke and the popup wouldn't reopen until another char).
    let after_dot = offset > 0 && text.as_bytes().get(offset - 1) == Some(&b'.');
    // **A one-shot, consumed here and now** — read before anything can return, so
    // every path through this function spends it exactly once.
    //
    // `typed` is written by the key handler, but not every edit arrives through
    // one: a paste from the OS context menu, an IME commit and dropped text all
    // change the document with no keypress in between. Left standing, the verdict
    // from the *previous* keystroke answered for them — type `sel`, dismiss the
    // list, then right-click → Paste, and the popup opened on the pasted text
    // because `typed` was still true from the `l`. Clearing it makes the absence
    // of a keypress mean what it should: not typing.
    let typed = comp.typed.get_untracked();
    comp.typed.set(false);
    // **The ghost is asked before either one-shot can return**, because it is
    // not the popup and neither rule is about it: `suppress` is set by an
    // accept, and the ghost is wanted straight after one — Tab on `BY ` leaves
    // `GROUP BY ` and the grouping columns are the next thing to offer — and
    // an edit no key typed still ends somewhere a ghost can be right. Ctrl+Space
    // asks for the list, so a forced recompute shows no ghost beside it. The
    // rest of a prediction taken word by word is kept while the caret is where
    // the last word left it: asked afresh there, it would often be nothing.
    // And only at a lone caret ([`lone_caret`]).
    let ghost = if force || lone_caret(&ed.cursor.get_untracked()) != Some(offset) {
        None
    } else {
        comp.ghost
            .with_untracked(|g| g.clone().filter(|g| keeps_ghost(g, offset, text.len())))
            .or_else(|| {
                let index = stmt_index(comp, text, dialect);
                prediction_at(ed, text, offset, &ctx, Some((comp.query, &index)))
            })
    };
    let remembered = ghost.as_ref().is_some_and(|g| g.remembered);
    let displaces = |prefix: &str| ghost_displaces_the_list(ghost.as_ref(), prefix);
    // A remembered ghost gives way to a list that opens at its caret.
    let give_way = |opening: bool| {
        if opening && remembered {
            crate::inline_diff::set_ghost(comp.ghost, ed, None);
        }
    };
    crate::inline_diff::set_ghost(comp.ghost, ed, ghost.clone());
    if comp.suppress.get_untracked() {
        comp.suppress.set(false);
        if !force && !after_dot {
            comp.open.set(false);
            set_items(comp, Vec::new());
            return;
        }
    }
    // Nothing below this line may *start* showing a list, so a closed popup that
    // isn't wanted stops here — after the `suppress` one-shot above, which has to
    // be consumed by the next recompute either way or it would swallow a later
    // keystroke instead of the edit it was set for.
    if !popup_may_open(force, comp.open.get_untracked(), typed) {
        return;
    }
    // A list opening by typing is the narrow one; only Ctrl+Space over an
    // open list widens it (`Completion::wide`), and it stays wide while open.
    if !force && !comp.open.get_untracked() {
        comp.wide.set(false);
    }
    let word_lo = word_start(text, offset);
    let prefix = text.get(word_lo..offset).unwrap_or("").to_string();
    // **A reasoned prediction takes the empty word's place** — see
    // `ghost_displaces_the_list`. Two offers at one caret would leave Tab meaning
    // whichever happened to be open; Ctrl+Space (`force`) still brings the list,
    // and a remembered ghost gives way to one instead (`give_way`).
    if displaces(&prefix) {
        comp.open.set(false);
        set_items(comp, Vec::new());
        return;
    }

    // The statement being *written*: past a `;` the caret starts a new one, and
    // Ctrl+Space on the blank line below `DELETE FROM t;` offered the `DELETE`'s
    // `WHERE` and columns. From the pane's index: this runs per keystroke.
    let (lo, hi) = stmt_index(comp, text, dialect).writing_range(text, offset);
    // **Not inside a value or a remark**, not even on Ctrl+Space: `name = 'jo`
    // offered columns, and Tab spliced one into the literal. A quoted name
    // is still a name, and completes.
    if intel::caret_in_literal_or_comment(text, lo, offset, dialect) {
        // …except the one value completion knows: a string compared with an
        // enum or set column offers that column's members.
        if let Some(slot) = intel::enum_value_slot(text, lo, offset, dialect) {
            let schema = hidden_dbs.with_untracked(|h| schema_index(db_nodes, h, active_db));
            let scope = intel::statement_scope(text, lo, hi, offset, dialect).tables;
            let items = rank::enum_value_suggestions(&schema, &scope, &slot, text, offset, dialect);
            if !items.is_empty() {
                set_anchor(ed, comp, offset);
                set_items(comp, items);
                comp.sel.set(0);
                give_way(true);
                comp.open.set(true);
                return;
            }
        }
        comp.open.set(false);
        set_items(comp, Vec::new());
        return;
    }
    // Context is lexer-based (correct mid-edit); scope prefers the real AST
    // (robust CTE/alias/derived-table resolution), falling back to the lexer.
    let ctx = intel::clause_context(text, lo, word_lo, dialect);
    let qualified = matches!(ctx, ClauseCtx::Qualified(_));
    // Expected next keyword/phrase continuations from SQL clause grammar (the
    // `WHERE` after a complete table ref, `FROM` after the projection, `GROUP BY`
    // as one item). These seed the top suggestion tier; `auto_show` opens the popup
    // on an empty prefix right after an operand-taking clause keyword.
    let cont = intel::clause_continuation(text, lo, word_lo, dialect);

    // FK-aware auto-join: right after a fresh `JOIN … ON `, offer the foreign-key
    // join predicate as a single, ready-to-insert suggestion (DataGrip-style). Only
    // on an empty ON expression (`prefix` empty, in a column/ON context), so it
    // never fights manual typing.
    if prefix.is_empty() && matches!(ctx, ClauseCtx::Column) {
        // The *offer* catalog: a hidden database must not be joined to.
        let catalog = hidden_dbs.with_untracked(|h| build_offer_catalog(db_nodes, h, active_db));
        if let Some(pred) = intel::join_condition(text, lo, hi, offset, &catalog, dialect) {
            set_anchor(ed, comp, offset);
            set_items(
                comp,
                vec![Suggestion {
                    text: pred,
                    kind: SuggestKind::Column,
                    detail: "foreign key".to_string(),
                    table: String::new(),
                    alias: String::new(),
                    // A purple key-square marks the ready-to-insert FK join predicate.
                    glyph: SuggestGlyph::ForeignKey,
                    key: KeyKind::Foreign,
                    insert: None,
                    replace: None,
                }],
            );
            comp.sel.set(0);
            give_way(true);
            comp.open.set(true);
            return;
        }
    }

    // Don't pop the list on every space: an empty prefix only shows suggestions
    // right after a `.`, right after an operand-taking clause keyword (`cont.
    // auto_show` — columns after WHERE/ON/BY/SET, tables after FROM), after a
    // `CALL`/`EXEC` (whose name is the one thing the slot can hold), or when
    // explicitly requested (Ctrl+Space).
    let routine = matches!(ctx, ClauseCtx::Routine);
    if prefix.is_empty() && !qualified && !routine && !force && !cont.auto_show {
        comp.open.set(false);
        set_items(comp, Vec::new());
        return;
    }

    // ── Everything below the caret reads is `core::rank`'s ──────────────────
    //
    // The schema view, the statement's scope, the snippet rows, the FK join
    // targets and the star expansion are this crate's to gather — each reads a
    // signal or the schema tree — and the *policy* over them is not. See
    // `core::rank`'s module doc for why: all of it used to live here, inside a
    // function taking `&Editor`, where nothing could call it.
    let schema = hidden_dbs.with_untracked(|h| schema_index(db_nodes, h, active_db));
    let mut scope = intel::statement_scope(text, lo, hi, offset, dialect).tables;
    // The statement's own CTEs and derived tables, where a column goes: their
    // columns are in no catalogue. A derived table's alias is in no scope
    // either (the scope holds the tables inside only for a caret inside it),
    // so it joins.
    let mut relations = if matches!(ctx, ClauseCtx::Column | ClauseCtx::Qualified(_)) {
        intel::local_relations(text, lo, hi, dialect)
    } else {
        Vec::new()
    };
    // …and the document's temporary tables, read only when the statement
    // names one, since that reading parses every statement that makes one.
    if matches!(ctx, ClauseCtx::Column | ClauseCtx::Qualified(_))
        && scope.iter().any(|t| t.name.starts_with('#'))
    {
        relations.extend(intel::temp_table_relations(text, offset, dialect));
    }
    for r in relations.iter().filter(|r| r.derived) {
        let named = |t: &intel::TableRef| {
            t.alias
                .as_deref()
                .unwrap_or(&t.name)
                .eq_ignore_ascii_case(&r.name)
        };
        if !scope.iter().any(named) {
            scope.push(intel::TableRef {
                name: r.name.clone(),
                alias: None,
                db: None,
            });
        }
    }

    // Gathered on exactly the paths that used to gather them, which is what keeps
    // the cost where it was: the FK catalog is a walk of the whole schema tree,
    // and only a table slot ever wanted it.
    let join_targets = if matches!(ctx, ClauseCtx::Table) {
        let catalog = hidden_dbs.with_untracked(|h| build_offer_catalog(db_nodes, h, active_db));
        let mut targets = intel::join_targets(text, lo, hi, offset, &catalog, dialect);
        if !theme::completion_table_aliases() {
            targets.iter_mut().for_each(|t| t.aliased = None);
        }
        targets
    } else {
        Vec::new()
    };
    let star = hidden_dbs
        .with_untracked(|h| star_expansion(text, lo, hi, offset, db_nodes, h, active_db, dialect));
    let snippet_rows = rank::snippet_abbrev_rows(
        &snippets.get_untracked(),
        &prefix,
        matches!(ctx, ClauseCtx::Qualified(_)),
        dialect,
        conn_id,
    );
    // Identifiers already written in this statement rank a little higher (recency):
    // you tend to reference the same columns/tables again.
    let used = rank::statement_identifiers(text, lo, hi, (word_lo, offset));
    // The statement's own names, read only where they are offered: its CTEs
    // at a table slot, its select list's aliases where a column goes (the
    // reader answers nothing outside an `ORDER BY` or `HAVING`).
    let local = rank::LocalNames {
        ctes: if matches!(ctx, ClauseCtx::Table) {
            intel::ctes_at(text, lo, word_lo, dialect)
        } else {
            Vec::new()
        },
        output_aliases: if matches!(ctx, ClauseCtx::Column) {
            intel::output_aliases_at(text, lo, word_lo, dialect)
        } else {
            Vec::new()
        },
        // The whole document's: a variable or temporary table outlives the
        // statement that made it.
        variables: if matches!(ctx, ClauseCtx::Variable) {
            intel::variables_at(text, offset, dialect)
        } else {
            Vec::new()
        },
        temp_tables: if matches!(ctx, ClauseCtx::TempTable) {
            intel::temp_tables_at(text, offset, dialect)
        } else {
            Vec::new()
        },
        bare_columns: matches!(ctx, ClauseCtx::Column)
            && intel::in_using_list(text, lo, offset, dialect),
        relations: relations
            .into_iter()
            .map(|r| {
                let cols = r
                    .columns
                    .into_iter()
                    .map(|name| rank::ColMeta {
                        name,
                        type_name: String::new(),
                        nullable: true,
                        primary_key: false,
                        foreign_key: false,
                    })
                    .collect();
                (r.name, cols)
            })
            .collect(),
    };

    let ranked = rank::rank_full(
        &schema,
        &rank::RankInput {
            ctx: &ctx,
            cont: &cont,
            scope: &scope,
            prefix: &prefix,
            snippets: &snippet_rows,
            join_targets: &join_targets,
            star: star.as_ref(),
            used: &used,
            active_db,
            dialect,
            // **Which MySQL-family server, read where `SELECT VERSION()` was
            // actually asked** — the loaded `DbSchema` for the tab's active
            // database. `SqlDialect` cannot say, and for one thing only it
            // has to: a MySQL 8 tab was offered forty-nine names from
            // MariaDB's manual that it cannot call. The same lookup the
            // table designer makes for `ddl::Target`, so there is one of it.
            //
            // No active database means no schema to read it off, and
            // `Unknown` offers everything — see `RankInput::flavour`.
            flavour: active_db
                .map(|db| crate::table_designer::db_flavour(db_nodes, db))
                .unwrap_or_default(),
            local: &local,
            wide: comp.wide.get_untracked(),
            accepted: &comp
                .accepted
                .read_only()
                .with_untracked(|f| f.bonuses(conn_id, crate::now_secs())),
        },
    );

    set_anchor(ed, comp, offset);
    let items = ranked.items;
    let open = !items.is_empty();
    set_items(comp, items);
    comp.word_complete.set(ranked.completes_a_word);
    comp.sel.set(0);
    give_way(open);
    comp.open.set(open);
}

/// Replace the word at the caret with the selected suggestion — a name quoted
/// where bare it would be something else, unless the caret is inside a quote
/// already (`rank::accepted_insert`).
pub(crate) fn accept_completion(ed: &Editor, comp: Completion, dialect: SqlDialect) {
    let offset = ed.cursor.get_untracked().offset();
    let doc = ed.doc();
    let text = doc.text().to_string();
    let start = word_start(&text, offset);
    let end = word_end(&text, offset);
    let idx = comp.sel.get_untracked();
    let (lo, hi) = schemaic_core::sql::writing_range(&text, offset, dialect);
    let in_quoted_name = intel::caret_in_quoted_name(&text, lo, start, dialect);
    if let Some((word, kind, over, replace, alias)) = comp.items.with_untracked(|v| {
        v.get(idx).map(|s| {
            let over = rank::accepted_insert(s, dialect, in_quoted_name);
            let alias = (theme::completion_table_aliases() && !in_quoted_name)
                .then(|| accepted_table_alias(s, &text, lo, hi, offset, dialect))
                .flatten();
            (s.text.clone(), s.kind, over, s.replace, alias)
        })
    }) {
        comp.suppress.set(true);
        let (insert, caret) = if let Some(over) = over {
            // An explicit splice override (e.g. an FK JOIN target: `orders ON …`, or a
            // `SELECT *` expansion); caret lands at its end.
            let len = over.len();
            (over, len)
        } else {
            // A function inserts `name()` with the caret between the parens — unless
            // the call parens are already there just ahead (re-accepting over a call).
            completion_insertion(
                &word,
                kind == SuggestKind::Function,
                call_parens_follow(&text[end..]),
            )
        };
        let (insert, caret) = match alias {
            Some(a) => (format!("{insert} {a}"), caret + 1 + a.len()),
            None => (insert, caret),
        };
        // Most completions replace the word being typed — the whole of it, the
        // part after the caret too; a `replace` override (star expansion) swaps
        // a specific range instead (the `*` / `t.*`).
        let (from, to) = replace.unwrap_or((start, end));
        doc.edit_single(Selection::region(from, to), &insert, EditType::Completion);
        // `edit_single` doesn't move the caret, so place it explicitly.
        ed.cursor
            .update(|c| c.set_offset(from + caret, false, false));
        // Taken once more on this connection: it ranks higher next time.
        let conn = comp.conn.get_untracked();
        comp.accepted
            .update(|f| f.record(conn, &word, crate::now_secs()));
    }
    comp.open.set(false);
    set_items(comp, Vec::new());
}

/// The alias to write after an accepted row, or `None`: only a plain table row
/// takes one (`intel::table_alias_at` decides where). An FK join target carries
/// its own in its insert, and a star expansion replaces a range of its own.
fn accepted_table_alias(
    s: &Suggestion,
    text: &str,
    lo: usize,
    hi: usize,
    offset: usize,
    dialect: SqlDialect,
) -> Option<String> {
    if s.kind != SuggestKind::Table || s.glyph == SuggestGlyph::ForeignKey || s.replace.is_some() {
        return None;
    }
    intel::table_alias_at(text, lo, hi, offset, &s.text, dialect)
}

/// `SELECT *` expansion for the candidate at the caret, or `None`. Cheap-guards on
/// the caret sitting right after a `*` before building the catalog + delegating to
/// `intel::expand_star`, so the common keystroke path stays allocation-free.
#[allow(clippy::too_many_arguments)] // the caret's whole context; a struct adds no clarity
fn star_expansion(
    text: &str,
    lo: usize,
    hi: usize,
    offset: usize,
    db_nodes: RwSignal<Vec<ConnNode>>,
    hidden: &HashSet<String>,
    active_db: Option<&str>,
    dialect: SqlDialect,
) -> Option<intel::StarExpansion> {
    let b = text.as_bytes();
    let mut p = offset.min(hi);
    while p > lo && matches!(b.get(p - 1), Some(b' ') | Some(b'\t')) {
        p -= 1;
    }
    if p <= lo || b.get(p - 1) != Some(&b'*') {
        return None;
    }
    // An offer, so a hidden database's columns are not part of it.
    let catalog = build_offer_catalog(db_nodes, hidden, active_db);
    intel::expand_star(text, lo, hi, offset, &catalog, dialect)
}

/// Are the call parens already present **just ahead** of the caret — i.e. on this
/// line, past nothing but spaces and tabs?
///
/// Only intra-line whitespace is skipped: `trim_start` also crosses newlines, so a
/// `(` opening an unrelated statement on the *next* line read as this call's
/// parens and `COUNT` was accepted without them.
fn call_parens_follow(after_caret: &str) -> bool {
    after_caret.trim_start_matches([' ', '\t']).starts_with('(')
}

/// The text to splice for an accepted completion and the caret offset *within* that
/// text afterwards. A function becomes `name()` with the caret between the parens,
/// unless the call parens are already present just ahead; everything else is the
/// word verbatim with the caret at its end.
fn completion_insertion(word: &str, is_function: bool, followed_by_paren: bool) -> (String, usize) {
    if is_function && !followed_by_paren {
        (format!("{word}()"), word.len() + 1)
    } else {
        (word.to_string(), word.len())
    }
}

/// Row text color for a suggestion kind (columns stay neutral; the rest are
/// tinted so the kind reads at a glance).
fn suggest_color(kind: SuggestKind) -> floem::peniko::Color {
    match kind {
        SuggestKind::Keyword => theme::suggest_keyword(),
        SuggestKind::Function => theme::suggest_function(),
        SuggestKind::Table => theme::suggest_table(),
        SuggestKind::Database => theme::suggest_database(),
        SuggestKind::Snippet => theme::suggest_table(),
        SuggestKind::Column => theme::text(),
    }
}

/// The default leading glyph for a non-column suggestion kind (columns pick a
/// type-family glyph in `add_col`). Keywords/functions get Lucide `square-function`.
fn kind_icon(kind: SuggestKind) -> &'static str {
    match kind {
        SuggestKind::Keyword | SuggestKind::Function => icons::SQUARE_FUNCTION,
        SuggestKind::Table => icons::TABLE,
        SuggestKind::Database => icons::DATABASE,
        SuggestKind::Snippet => icons::BOOKMARK,
        SuggestKind::Column => icons::TYPE,
    }
}

/// Leading-icon color, matching the schema tree: db/table icons keep their schema
/// tint; a column's icon is a quiet 50%-alpha version of its key colour (gold PK /
/// purple FK / neutral); keywords are muted, functions keep the function tint.
fn suggest_icon_color(kind: SuggestKind, key: KeyKind) -> floem::peniko::Color {
    match kind {
        SuggestKind::Column => {
            let base = match key {
                KeyKind::Primary => theme::key_primary(),
                KeyKind::Foreign => theme::key_foreign(),
                KeyKind::None => theme::text(),
            };
            base.multiply_alpha(0.5)
        }
        SuggestKind::Table => theme::table_icon(),
        SuggestKind::Database => theme::db_icon(),
        SuggestKind::Keyword => theme::text_muted(),
        SuggestKind::Function => theme::suggest_function(),
        SuggestKind::Snippet => theme::suggest_table(),
    }
}

/// Floating suggestion list, anchored to the caret and kept inside the editor pane.
///
/// `area_h`/`area_w` are `editor_area`'s measured size and `viewport` the editor's
/// live scroll rect. Between them the popup follows the caret while it scrolls,
/// flips above the line rather than spilling over the results grid, and slides left
/// rather than off the pane's right edge.
/// `editor` is here so a **click** can accept a suggestion. It could not before:
/// the rows carried `.hover(|s| s.background(theme::completion_active()))` and
/// no click handler, so clicking one highlighted it, did nothing, and did not
/// dismiss the list either — while the popup, having no handler, swallowed that
/// click from the editor underneath.
///
/// The list is **not** `pointer_events(false)`, unlike `signature_popup`: it
/// wraps a `scroll` and wants the wheel. The fix for an overlay that has
/// something to interact with is to make the interaction real, not to opt the
/// overlay out — see `editor_pane`'s squiggles for what opting out costs when
/// there *is* something to hover.
pub(crate) fn completion_popup(
    comp: Completion,
    editor: floem::views::editor::Editor,
    area_h: RwSignal<f64>,
    area_w: RwSignal<f64>,
    viewport: RwSignal<Rect>,
    dialect: Memo<SqlDialect>,
) -> impl IntoView {
    // The anchor is in content coords, so the viewport origin comes off here — in a
    // reactive read, which is what keeps the popup pinned to the caret while the
    // editor scrolls under it. Returns the caret line's (top, bottom) and x.
    let anchor = move || {
        let vp = viewport.get();
        let p = comp.point.get();
        (comp.line_top.get() - vp.y0, p.y - vp.y0, p.x - vp.x0)
    };
    dyn_container(
        // Keyed on open/items only — NOT `sel`. The selection highlight reads
        // `comp.sel` reactively per row (below), so moving the selection repaints in
        // place instead of rebuilding the list (which would reset the scroll offset).
        move || (comp.open.get(), comp.items.get()),
        move |(open, items)| {
            if !open || items.is_empty() {
                return empty().into_any();
            }
            // The builder is an `Fn` — it runs on every rebuild — so the handle
            // is cloned per rebuild and again per row below. `Editor` is a
            // handle over signals; `editor_pane` already keeps a dozen clones of
            // it for the same reason.
            let editor = editor.clone();
            let rows_n = items.len();
            let rows: Vec<AnyView> = items
                .into_iter()
                .enumerate()
                .map(move |(i, item)| {
                    let Suggestion {
                        text: name,
                        kind,
                        detail,
                        table,
                        alias,
                        glyph,
                        key,
                        insert: _,
                        replace: _,
                    } = item;
                    // Schema-style leading glyph, coloured by kind/key (see
                    // `suggest_icon_color`): a column's type family tinted gold (PK) /
                    // purple (FK), a table/db icon, or the muted `square-function`
                    // mark for keywords/functions. The *reason* for the glyph comes
                    // ranked (`SuggestGlyph`); `glyph_icon` is the picture.
                    let lead: AnyView = icons::icon(glyph_icon(glyph, kind), COMPLETION_ICON_BASE)
                        .style(move |s| {
                            s.color(suggest_icon_color(kind, key))
                                .margin_right(completion_icon_w() - completion_icon_size())
                                .flex_shrink(0.0_f32)
                        })
                        .into_any();
                    // Right-side annotation. For a column: its owning table (+
                    // in-scope alias) in a muted colour, then the type — so a row
                    // reads `id      orders o      int`, making the column's origin
                    // obvious. For everything else: the single dim detail (a table's
                    // database, etc.).
                    let table_ref = if table.is_empty() {
                        empty().into_any()
                    } else {
                        text(annotation_label(&table, &alias))
                            .style(|s| {
                                s.font_size(completion_annot_size())
                                    .color(theme::text_dim())
                                    .min_width(0.0)
                                    .text_ellipsis()
                            })
                            .into_any()
                    };
                    // Name (kind-tinted) on the left; annotations right-aligned. The
                    // selected/hovered background spans the full row width.
                    //
                    // Give way in annotation-first order when the box is narrower than
                    // the row wants (a caret near the pane's right edge, or a pane too
                    // narrow for `completion_max_w()`): the name never shrinks — it's the
                    // thing being picked — and the two dim columns ellipsize.
                    h_stack((
                        lead,
                        // `suggest_color(kind)` **inside** the closure, like its
                        // sibling `suggest_icon_color(kind, key)` above and the
                        // two `theme::` calls below — themable colours reach a
                        // reactive style as a call, never as a captured
                        // `Color`. This closure re-runs on every selection move
                        // (it reads `comp.sel`), so a colour frozen at build
                        // time kept the old tint after a theme switch until the
                        // next keystroke rebuilt the list.
                        text(name).style(move |s| {
                            s.font_size(completion_name_size())
                                .color(suggest_color(kind))
                                .flex_shrink(0.0_f32)
                        }),
                        empty().style(|s| s.flex_grow(1.0_f32).min_width(completion_gap_w())),
                        table_ref,
                        text(detail).style(|s| {
                            s.font_size(completion_annot_size())
                                .color(theme::text_muted())
                                .margin_left(completion_detail_gap())
                                .min_width(0.0)
                                .text_ellipsis()
                        }),
                    ))
                    // **Clicking a row picks it**, which is what the hover
                    // highlight has always promised. `on_click_stop` so the
                    // press does not also travel to the editor and move the
                    // caret out from under the insertion; `accept_completion`
                    // closes the list itself.
                    //
                    // **And hands the keyboard back**, which the key route never
                    // had to. floem takes focus on *every* `PointerDown`, before
                    // dispatch, and returns it only to a view under the pointer
                    // that is `keyboard_navigable`. These rows are a plain
                    // `h_stack`, and the editor is a **sibling** of the popup in
                    // `editor_area` rather than an ancestor of the click target,
                    // so nothing claimed it back: the word spliced, the list
                    // closed, and the next character typed went nowhere until
                    // the user clicked into the editor again. `accept_completion`
                    // never needed this because until the rows grew a click
                    // handler the only way to reach it was a key handler, which
                    // by definition runs on the focused view.
                    //
                    // Deferred a frame, like the five sibling hand-backs in
                    // `editor_pane` (the run menu, Ctrl+K twice, find close, goto
                    // close/submit) — the focus floem takes is cleared during
                    // this frame's dispatch, so requesting it back inside the
                    // handler is undone by the same frame that stole it.
                    .on_click_stop({
                        let editor = editor.clone();
                        move |_| {
                            comp.sel.set(i);
                            accept_completion(&editor, comp, dialect.get_untracked());
                            let ed = editor.clone();
                            floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                                if let Some(Some(vid)) = ed.editor_view_id.try_get_untracked() {
                                    // **Claimed, like the five it is modelled
                                    // on.** The comment above says this is one
                                    // of the deferred hand-backs; the three
                                    // movers the `claim_keyboard` gotcha names
                                    // all claim, and this one did not. Without
                                    // it a `refocus_grid` scheduled in the same
                                    // pass has nothing to stand down for, and
                                    // the focus this asks for is taken away by
                                    // the deferred action queued behind it.
                                    crate::widgets::claim_keyboard();
                                    vid.request_focus();
                                }
                            });
                        }
                    })
                    .style(move |s| {
                        let s = s
                            .flex_row()
                            .items_center()
                            .width_full()
                            .padding_horiz(completion_row_pad())
                            .padding_vert(theme::scaled(5.0))
                            .hover(|s| s.background(theme::completion_active()));
                        // Selection highlight, read reactively so keyboard nav
                        // repaints without rebuilding (and resetting the scroll).
                        if comp.sel.get() == i {
                            s.background(theme::completion_active())
                        } else {
                            s
                        }
                    })
                    .into_any()
                })
                .collect();
            // Each row's id, so the scroll can follow the keyboard selection.
            let row_ids: Vec<floem::ViewId> = rows.iter().map(|v| v.id()).collect();
            // An explicit width, NOT `width_full()`. A percentage resolves against the
            // parent's *definite* width, and a `scroll` lays its child out against
            // max-content available space instead — so `width_full()` here silently
            // became "as wide as the widest row", the rows never stretched to the box,
            // and that widest row then sat exactly on its own ellipsis boundary (its
            // `main` truncated to `m…` while every shorter row rendered clean). The
            // rows have to span the popup, so the popup's width is what they get: the
            // outer box minus its border, from the same `popup_w` the box uses.
            let list = v_stack_from_iter(rows).style(move |s| {
                let w = popup_w(comp.width.get(), area_w.get());
                s.flex_col().width(w - COMPLETION_BORDER)
            });
            // The scroll makes an overflowing list navigable by wheel; `scroll_to_view`
            // keeps the keyboard-selected row visible. `autohide` gives it the shared
            // thin, auto-hiding scrollbar (same as the schema tree / history / etc.).
            // The surface (bg #14151A, #373942 outline, rounded) + `.clip()` live on
            // the wrapping container so the full-width row highlights round to the
            // corners.
            // The height cap comes from the same placement the outer style uses, so a
            // list squeezed against the top or bottom of the pane shortens instead of
            // overhanging it.
            // **The selected function's summary, under the list** — one line,
            // ellipsized, for the row the keyboard is on rather than beside
            // every row, which would make the popup as wide as the longest
            // description. It counts as a row in the placement, and its height
            // comes off the scroll's, so a flipped popup still clears the caret.
            let summary = move || selected_summary(comp, dialect.get());
            let footer = label(move || summary().unwrap_or_default().to_string()).style(move |s| {
                s.width_full()
                    .min_width(0.0)
                    .text_ellipsis()
                    .font_size(completion_annot_size())
                    .color(theme::text_muted())
                    .padding_horiz(completion_row_pad())
                    .padding_vert(theme::scaled(5.0))
                    .border_top(1.0)
                    .border_color(theme::completion_border())
                    .apply_if(summary().is_none(), |s| s.hide())
            });
            container(
                v_stack((
                    autohide(
                        scroll(list).scroll_to_view(move || row_ids.get(comp.sel.get()).copied()),
                    )
                    .style(move |s| {
                        let (line_top, line_bot, _) = anchor();
                        let shown = summary().is_some();
                        let place = popup_placement(
                            line_top,
                            line_bot,
                            rows_n + usize::from(shown),
                            area_h.get(),
                        );
                        let footer_h = if shown { completion_row_h() } else { 0.0 };
                        s.width_full().max_height((place.max_h - footer_h).max(0.0))
                    }),
                    footer,
                ))
                .style(|s| s.flex_col().width_full()),
            )
            .style(|s| {
                s.width_full()
                    .background(theme::bg_deepest())
                    .border(1.0)
                    .border_color(theme::completion_border())
                    .border_radius(6.0)
            })
            .clip()
            // **Say that this press landed on the popup, before the blur asks.**
            // The editor's `FocusLost` effect closes `comp.open`, which is what
            // this `dyn_container` is keyed on — so without this the rows were
            // torn out of the tree on the `PointerDown` and the row's own
            // `on_click_stop` below, which fires on the `PointerUp`, never ran.
            // Nothing was inserted and the editor was left without the keyboard.
            //
            // `on_event_cont`, never `on_event_stop`, for the same reason
            // `note_pointer_focus` gives: the press must go on reaching the
            // workspace root, which is what closes the menus.
            .on_event_cont(floem::event::EventListener::PointerDown, |_| {
                crate::widgets::note_pointer_on_completion();
            })
            .into_any()
        },
    )
    .style(move |s| {
        // A high z-index lifts the popup above the results pane below it: a list that
        // can't fit inside the editor pane still overhangs the (unclipped) pane, and
        // without this the later-painted results grid draws over it (paint order =
        // tree order). z-index gives the vger renderer a global ordering so the popup
        // composites last. Set unconditionally so it applies whenever it's shown.
        let s = s.z_index(1000);
        if comp.open.get() {
            let (line_top, line_bot, caret_x) = anchor();
            // The summary footer is one row more — see the list's own style.
            let footer = usize::from(selected_summary(comp, dialect.get()).is_some());
            let rows = comp.items.with(Vec::len) + footer;
            let place = popup_placement(line_top, line_bot, rows, area_h.get());
            // An explicit width, not `min_width`/`max_width`: the left edge has to be
            // computed against the width to slide the box back inside the pane, and a
            // flex-resolved width isn't knowable here.
            let w = popup_w(comp.width.get(), area_w.get());
            s.absolute()
                .inset_left(popup_x(caret_x, w, area_w.get()))
                .inset_top(place.top)
                .width(w)
        } else {
            s
        }
    })
}

/// The selected row's one-line summary, when it is a function the engine's
/// catalogue describes ([`intel::builtin_summary`]) — the popup's footer.
/// Tracked: it follows `comp.sel` as the keyboard walks the list.
fn selected_summary(comp: Completion, dialect: SqlDialect) -> Option<&'static str> {
    let sel = comp.sel.get();
    comp.items.with(|v| {
        v.get(sel)
            .filter(|s| s.kind == rank::SuggestKind::Function)
            .and_then(|s| intel::builtin_summary(dialect, &s.text))
    })
}

/// Signature-help popup: the enclosing function's signature (active parameter
/// emphasised in the function tint) over its dim summary, anchored just above and
/// right of the caret. Hidden while the suggestion list is open so the two never
/// stack — the hint returns the moment the list closes (empty arg slot, a literal,
/// or nothing left to complete). The suggestion list stays useful for column args.
pub(crate) fn signature_popup(comp: Completion, viewport: RwSignal<Rect>) -> impl IntoView {
    // **How tall the hint is, so it can be lifted clear of the caret's line** —
    // and it is as tall as its own two lines of text plus its padding, every one
    // of which scales. Frozen at 48 it was correct at Normal only: from 130% the
    // popup's bottom fell below the caret's line top and covered the statement
    // being typed, which is the one thing a hint about that statement must not do.
    let sig_help_h = || theme::scaled(48.0);
    // Nudged right of the caret so it doesn't sit on top of the cursor. Air, so
    // it grows with the caret it is dodging.
    let sig_help_dx = || theme::scaled(30.0);
    dyn_container(
        move || (comp.sig.get(), comp.open.get()),
        move |(sig, open)| {
            let Some(sig) = sig.filter(|_| !open) else {
                return empty().into_any();
            };
            let sig_line: AnyView = match sig.active_range {
                Some((s, e)) => h_stack((
                    text(sig.signature[..s].to_string()).style(|s| s.color(theme::text())),
                    text(sig.signature[s..e].to_string())
                        .style(|s| s.color(theme::suggest_function()).font_bold()),
                    text(sig.signature[e..].to_string()).style(|s| s.color(theme::text())),
                ))
                .style(|s| s.font_size(theme::scaled_font(13.0)))
                .into_any(),
                None => text(sig.signature.to_string())
                    .style(|s| s.font_size(theme::scaled_font(13.0)).color(theme::text()))
                    .into_any(),
            };
            // Same size as the signature — the dim colour alone distinguishes it.
            let summary = text(sig.summary.to_string()).style(|s| {
                s.font_size(theme::scaled_font(13.0))
                    .margin_top(theme::scaled(2.0))
                    .color(theme::text_dim())
            });
            container(v_stack((sig_line, summary)))
                .style(|s| {
                    // Padding matches the autocomplete rows.
                    s.flex_col()
                        .padding_horiz(theme::scaled(10.0))
                        .padding_vert(theme::scaled(5.0))
                        .background(theme::bg_deepest())
                        .border(1.0)
                        .border_color(theme::completion_border())
                        .border_radius(6.0)
                })
                .into_any()
        },
    )
    .style(move |s| {
        let s = s.z_index(1001);
        if comp.sig.get().is_some() && !comp.open.get() {
            // `sig_point` is in content coords; the viewport origin comes off here so
            // the hint tracks the caret as the editor scrolls (see `set_anchor`).
            let vp = viewport.get();
            let p = comp.sig_point.get();
            let (px, py) = (p.x - vp.x0, p.y - vp.y0);
            // Above the caret when there's room; otherwise below the line (near line 1).
            let top = if py >= sig_help_h() {
                py - sig_help_h()
            } else {
                py + COMPLETION_LINE_H
            };
            s.absolute()
                .inset_left(COMPLETION_GUTTER + px + sig_help_dx())
                .inset_top(top)
                .max_width(theme::scaled(560.0))
        } else {
            s
        }
    })
    // **Click-through.** This is a hint: it has no handler of its own, so every
    // click and wheel landing in its rect was not handled but *swallowed* —
    // floem stops a pointer event at the first eligible view under it, and this
    // is a later sibling of the editor in `editor_area`. The hint is up to 48px
    // tall and 560px wide and sits directly above the caret's line, so the code
    // it covered was the code being typed next to: clicking a word there did not
    // move the caret, and the wheel did not scroll.
    //
    // Safe precisely because it is paint-only — the trap `pointer_events(false)`
    // sets is an overlay that has something to hover, and this has nothing. See
    // `editor_pane`'s squiggles for the other side of that.
    .pointer_events(|| false)
}

#[cfg(test)]
mod recall_order_tests {
    use super::{CompletionCtx, recall};
    use floem::reactive::{RwSignal, Scope, SignalUpdate};
    use schemaic_core::history::{HistoryEntry, Outcome};
    use schemaic_core::intel::SqlDialect;
    use schemaic_core::snippet::{self, Snippet, Source};
    use std::collections::HashSet;

    const TYPED: &str = "SELECT * FROM orders ";

    fn run(conn_id: u64, sql: &str) -> HistoryEntry {
        HistoryEntry {
            conn_id,
            database: None,
            sql: sql.to_string(),
            ts: 0,
            tab_name: None,
            run_id: 1,
            duration_ms: None,
            rows: None,
            rows_capped: false,
            outcome: Outcome::Ok,
        }
    }

    fn user_snippet(body: &str) -> Snippet {
        Snippet {
            id: 1,
            name: "s".to_string(),
            abbrev: None,
            body: body.to_string(),
            scope: snippet::Scope::Global,
            source: Source::User,
            last_used: None,
        }
    }

    /// `recall` on connection 1 over these runs, snippets and open tabs (each
    /// tab a `(conn_id, text)`), the typed statement's own tab among them.
    fn recalled(
        runs: Vec<HistoryEntry>,
        snippets: &[&str],
        tabs: &[(u64, &str)],
    ) -> Option<String> {
        let cx = Scope::new();
        let snippets: Vec<Snippet> = snippets.iter().map(|b| user_snippet(b)).collect();
        let mut open = vec![crate::Tab::new(cx, 1, TYPED, 1, None)];
        for (i, (conn, text)) in tabs.iter().enumerate() {
            open.push(crate::Tab::new(cx, i + 2, text, *conn, None));
        }
        let ctx = CompletionCtx {
            db_nodes: RwSignal::new(Vec::new()),
            hidden_dbs: cx.create_memo(|_| HashSet::new()),
            active_db: None,
            dialect: SqlDialect::MySql,
            snippets: cx.create_memo(move |_| snippets.clone()),
            conn_id: 1,
            predictions: true,
            remembered: true,
            history: RwSignal::new(runs),
            tabs: RwSignal::new(open),
        };
        let got = recall(TYPED, TYPED.len(), &ctx, None);
        ctx.tabs.update(|t| t.clear());
        cx.dispose();
        got
    }

    /// **This connection's runs, then the user's snippets, then the open tabs
    /// on the same connection** — the first to continue the typed statement
    /// wins, and a source that does not apply here is passed over rather than
    /// ending the search.
    #[test]
    fn runs_come_before_snippets_and_snippets_before_tabs() {
        let snippets = ["SELECT * FROM orders WHERE id = 2"];
        let tabs = [(1, "SELECT * FROM orders WHERE id = 3")];
        assert_eq!(
            recalled(
                vec![run(1, "SELECT * FROM orders WHERE id = 1")],
                &snippets,
                &tabs
            )
            .as_deref(),
            Some("WHERE id = 1")
        );
        assert_eq!(
            recalled(
                vec![run(2, "SELECT * FROM orders WHERE id = 1")],
                &snippets,
                &tabs
            )
            .as_deref(),
            Some("WHERE id = 2"),
            "another connection's run is no run here"
        );
        assert_eq!(
            recalled(Vec::new(), &[], &tabs).as_deref(),
            Some("WHERE id = 3")
        );
        assert_eq!(
            recalled(Vec::new(), &[], &[(2, "SELECT * FROM orders WHERE id = 3")]),
            None,
            "another connection's tab"
        );
    }
}

#[cfg(test)]
mod kept_ghost_tests {
    use super::{keeps_ghost, typed_through};
    use crate::inline_diff::EditorGhost;

    /// A ghost at `offset`, kept at document length `kept` when that is given.
    fn ghost(offset: usize, kept: Option<usize>) -> EditorGhost {
        EditorGhost {
            offset,
            line: 0,
            col: offset,
            text: "ON o.id = c.id".to_string(),
            doc_len: kept.unwrap_or(40),
            kept: kept.is_some(),
            remembered: false,
        }
    }

    /// A fresh ghost of `text` at offset 20 in a 40-byte document.
    fn fresh(text: &str) -> EditorGhost {
        EditorGhost {
            text: text.to_string(),
            kept: false,
            ..ghost(20, None)
        }
    }

    /// Typing the ghost's next letters leaves the rest of it — however many
    /// were typed at once, and a character outside ASCII whole.
    #[test]
    fn typing_into_a_ghost_leaves_the_rest_of_it() {
        assert_eq!(typed_through(&fresh("BY "), 21, 41, "B"), Some("Y "));
        assert_eq!(typed_through(&fresh("BY "), 22, 42, "BY"), Some(" "));
        assert_eq!(
            typed_through(&fresh("名前, x"), 23, 43, "名"),
            Some("前, x")
        );
    }

    /// A letter the ghost does not say (case counts: Tab would write the
    /// ghost's own), a caret that moved without typing, a ghost typed to its
    /// end, or a caret behind it leave nothing to keep.
    #[test]
    fn anything_but_the_ghosts_own_letters_drops_it() {
        assert_eq!(typed_through(&fresh("BY "), 21, 41, "b"), None);
        assert_eq!(typed_through(&fresh("BY "), 21, 41, "X"), None);
        assert_eq!(typed_through(&fresh("BY "), 21, 40, "B"), None);
        assert_eq!(typed_through(&fresh("BY "), 23, 43, "BY "), None);
        assert_eq!(typed_through(&fresh("BY "), 20, 40, ""), None);
        assert_eq!(typed_through(&fresh("BY "), 19, 39, ""), None);
        assert_eq!(typed_through(&fresh("名前"), 21, 41, "\u{FFFD}"), None);
    }

    /// The recompute a Ctrl+→ schedules keeps the rest it left.
    #[test]
    fn the_rest_of_a_word_taken_is_kept_where_it_was_left() {
        assert!(keeps_ghost(&ghost(20, Some(40)), 20, 40));
    }

    /// A forward Delete at the caret moves nothing but changes what follows,
    /// and a caret moved elsewhere, or a prediction no Ctrl+→ left, is asked
    /// for afresh.
    #[test]
    fn a_kept_rest_is_dropped_once_the_text_or_the_caret_changes() {
        assert!(!keeps_ghost(&ghost(20, Some(40)), 20, 39));
        assert!(!keeps_ghost(&ghost(20, Some(40)), 21, 40));
        assert!(!keeps_ghost(&ghost(20, None), 20, 40));
    }
}

#[cfg(test)]
mod ghost_choice_tests {
    use super::{
        ghost_displaces_the_list, lone_caret, pick_prediction, recallable, same_place,
        snippet_recallable,
    };
    use schemaic_core::intel::SqlDialect;
    use schemaic_core::snippet::{Scope, Snippet, Source};

    /// A text belongs here when it was written on this connection, in this
    /// database where both say which.
    #[test]
    fn an_open_tab_is_remembered_from_only_in_the_same_place() {
        let ms = SqlDialect::MsSql;
        assert!(same_place(ms, 1, Some("shop"), 1, Some("Shop")));
        assert!(same_place(ms, 1, None, 1, Some("shop")));
        assert!(same_place(ms, 1, Some("shop"), 1, None));
        assert!(!same_place(ms, 2, Some("shop"), 1, Some("shop")));
        assert!(!same_place(ms, 1, Some("archive"), 1, Some("shop")));
        // PostgreSQL keeps `Shop` and `shop` as two databases.
        let pg = SqlDialect::Postgres;
        assert!(!same_place(pg, 1, Some("shop"), 1, Some("Shop")));
        assert!(same_place(pg, 1, Some("shop"), 1, Some("shop")));
    }

    fn snippet(source: Source, scope: Scope) -> Snippet {
        Snippet {
            id: 1,
            name: "recent orders".to_string(),
            abbrev: None,
            body: "SELECT * FROM orders WHERE id = :id".to_string(),
            scope,
            source,
            last_used: None,
        }
    }

    /// The user's own snippets that apply here are remembered from; a
    /// built-in is a template nobody wrote, and a snippet scoped to another
    /// connection or engine is not offered here at all.
    #[test]
    fn a_snippet_is_remembered_from_only_if_it_is_the_users_and_applies_here() {
        let pg = SqlDialect::Postgres;
        assert!(snippet_recallable(
            &snippet(Source::User, Scope::Global),
            pg,
            1
        ));
        assert!(snippet_recallable(
            &snippet(Source::User, Scope::Conn(1)),
            pg,
            1
        ));
        assert!(snippet_recallable(
            &snippet(Source::User, Scope::Dialect(pg)),
            pg,
            1
        ));
        assert!(!snippet_recallable(
            &snippet(Source::Builtin, Scope::Global),
            pg,
            1
        ));
        assert!(!snippet_recallable(
            &snippet(Source::User, Scope::Conn(2)),
            pg,
            1
        ));
        assert!(!snippet_recallable(
            &snippet(Source::User, Scope::Dialect(SqlDialect::MySql)),
            pg,
            1
        ));
    }
    use crate::inline_diff::EditorGhost;
    use floem::views::editor::core::cursor::{Cursor, CursorMode};
    use floem::views::editor::core::selection::{SelRegion, Selection};
    use schemaic_core::history::{HistoryEntry, Outcome};

    fn cursor(sel: Selection) -> Cursor {
        Cursor::new(CursorMode::Insert(sel), None, None)
    }

    /// One caret with nothing selected is where a ghost may sit; several
    /// carets, or a selection, have no one caret for it.
    #[test]
    fn a_ghost_sits_only_at_a_lone_caret() {
        assert_eq!(lone_caret(&cursor(Selection::caret(5))), Some(5));
        let mut two = Selection::caret(2);
        two.add_region(SelRegion::caret(9));
        assert_eq!(lone_caret(&cursor(two)), None);
        assert_eq!(lone_caret(&cursor(Selection::region(2, 5))), None);
    }

    /// The reasoned prediction first, else the remembered one, flagged — and
    /// neither when it holds a line break, which the ghost cannot draw.
    #[test]
    fn a_prediction_is_shown_only_if_it_can_be_drawn() {
        assert_eq!(
            pick_prediction(Some("BY ".into()), || Some("x".into())),
            Some(("BY ".to_string(), false))
        );
        assert_eq!(
            pick_prediction(None, || Some("WHERE id = 1".into())),
            Some(("WHERE id = 1".to_string(), true))
        );
        assert_eq!(pick_prediction(Some("a, '\n'".into()), || None), None);
        assert_eq!(pick_prediction(None, || Some("a\nb".into())), None);
    }

    fn run(conn_id: u64, database: Option<&str>, outcome: Outcome) -> HistoryEntry {
        HistoryEntry {
            conn_id,
            database: database.map(str::to_string),
            sql: "SELECT 1".to_string(),
            ts: 0,
            tab_name: None,
            run_id: 1,
            duration_ms: None,
            rows: None,
            rows_capped: false,
            outcome,
        }
    }

    /// A run is remembered on its own connection, when it did not fail, and
    /// in the database it ran against — where both the run and the tab say.
    #[test]
    fn a_run_is_remembered_only_where_it_ran() {
        let ms = SqlDialect::MsSql;
        let here = |e: &HistoryEntry, conn_id, db| recallable(e, ms, conn_id, db);
        assert!(here(&run(1, Some("shop"), Outcome::Ok), 1, Some("shop")));
        assert!(here(
            &run(1, Some("Shop"), Outcome::Unknown),
            1,
            Some("shop")
        ));
        assert!(here(&run(1, None, Outcome::Ok), 1, Some("shop")));
        assert!(here(&run(1, Some("shop"), Outcome::Ok), 1, None));
        assert!(!here(
            &run(1, Some("archive"), Outcome::Ok),
            1,
            Some("shop")
        ));
        assert!(!here(&run(2, Some("shop"), Outcome::Ok), 1, Some("shop")));
        assert!(!here(
            &run(1, Some("shop"), Outcome::Failed),
            1,
            Some("shop")
        ));
        // By the engine's case rule: another database on PostgreSQL.
        assert!(!recallable(
            &run(1, Some("Shop"), Outcome::Ok),
            SqlDialect::Postgres,
            1,
            Some("shop")
        ));
    }

    fn ghost(remembered: bool) -> EditorGhost {
        EditorGhost {
            offset: 14,
            line: 0,
            col: 14,
            text: "orders WHERE id = 1".to_string(),
            doc_len: 14,
            kept: false,
            remembered,
        }
    }

    /// A kept ghost — one being typed into or taken word by word — holds the
    /// caret against the list at any word, remembered or not: a list opening
    /// on the letters typed into it would take Tab from it.
    #[test]
    fn a_kept_ghost_displaces_the_list_at_any_word() {
        for remembered in [false, true] {
            let kept = EditorGhost {
                kept: true,
                ..ghost(remembered)
            };
            assert!(ghost_displaces_the_list(Some(&kept), "o"), "{remembered}");
            assert!(ghost_displaces_the_list(Some(&kept), ""), "{remembered}");
        }
    }

    /// A reasoned ghost takes an empty word's place; a remembered one leaves
    /// the list that opens there to open; nothing displaces a typed word.
    #[test]
    fn only_a_reasoned_ghost_displaces_the_list() {
        assert!(ghost_displaces_the_list(Some(&ghost(false)), ""));
        assert!(!ghost_displaces_the_list(Some(&ghost(true)), ""));
        assert!(!ghost_displaces_the_list(Some(&ghost(false)), "or"));
        assert!(!ghost_displaces_the_list(None, ""));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        KeyKind, SuggestGlyph, SuggestKind, Suggestion, accepted_table_alias, call_parens_follow,
        completion_insertion, natural_width, popup_may_open, popup_placement, popup_w, popup_x,
        row_width, types_a_character, word_end, word_start,
    };
    // These moved to `core::rank` with the policy that reads them; the tests stay
    // here because what they pin is this pane's behaviour, and the popup is here.
    use crate::consts::{
        COMPLETION_BORDER, COMPLETION_GUTTER, COMPLETION_LINE_H, completion_detail_gap,
        completion_edge_pad, completion_gap_w, completion_icon_w, completion_max_h,
        completion_max_w, completion_min_h, completion_min_w, completion_row_h, completion_row_pad,
        completion_slack_w,
    };
    use floem::keyboard::{Key, NamedKey};
    use schemaic_core::intel::SqlDialect;
    use schemaic_core::rank::{
        database_suggestion_visible, fuzzy_score, recency_bonus, snippet_abbrev_rows,
        statement_identifiers,
    };
    use schemaic_core::snippet::{Scope, Snippet, Source};
    use std::collections::HashSet;

    // ── What is allowed to summon the popup ───────────────────────────────

    #[test]
    fn a_typed_letter_is_typing() {
        assert!(types_a_character(&Key::Character("a".into()), false, false));
        // Shift is still typing — it is how a capital arrives.
        assert!(types_a_character(&Key::Character("A".into()), false, false));
    }

    #[test]
    fn space_is_typing_however_it_arrives() {
        // Reported as a named key on some platforms and as `" "` on others, and
        // the `auto_show` list after `WHERE ` hangs off it either way.
        assert!(types_a_character(
            &Key::Named(NamedKey::Space),
            false,
            false
        ));
        assert!(types_a_character(&Key::Character(" ".into()), false, false));
    }

    #[test]
    fn enter_and_tab_are_not_typing() {
        // The reported case: adding a line should not summon a suggestion list.
        assert!(!types_a_character(
            &Key::Named(NamedKey::Enter),
            false,
            false
        ));
        assert!(!types_a_character(&Key::Named(NamedKey::Tab), false, false));
        assert!(!types_a_character(
            &Key::Named(NamedKey::Backspace),
            false,
            false
        ));
    }

    #[test]
    fn a_command_is_not_typing_even_though_it_carries_a_letter() {
        // Ctrl+X (delete line) and Ctrl+Z (undo) arrive as `Character`, which is
        // why they used to reopen the popup wherever the caret landed.
        assert!(!types_a_character(&Key::Character("x".into()), true, false));
        assert!(!types_a_character(&Key::Character("z".into()), true, false));
        assert!(!types_a_character(&Key::Character("v".into()), true, false));
        // Alt combos likewise (`Alt+↑` moves a line; `Ctrl+Alt+L` reformats).
        assert!(!types_a_character(&Key::Character("l".into()), true, true));
    }

    #[test]
    fn a_closed_popup_opens_only_for_typing_or_a_request() {
        // Typing opens it; a document change that wasn't typed does not.
        assert!(popup_may_open(false, false, true));
        assert!(!popup_may_open(false, false, false));
        // Ctrl+Space asks for it explicitly, whatever the last key was.
        assert!(popup_may_open(true, false, false));
    }

    #[test]
    fn an_open_popup_keeps_recomputing_whatever_the_edit_was() {
        // Backspace isn't typing, but a list already on screen must still refine
        // (and close itself once the prefix is gone) rather than freeze.
        assert!(popup_may_open(false, true, false));
    }

    fn snip(id: u64, name: &str, abbrev: &str, scope: Scope) -> Snippet {
        Snippet {
            id,
            name: name.to_string(),
            abbrev: Some(abbrev.to_string()),
            body: format!("-- {name}"),
            scope,
            source: Source::User,
            last_used: None,
        }
    }

    #[test]
    fn an_abbrev_is_offered_only_once_something_has_been_typed() {
        // On an empty prefix the abbrev is not a weak row, it is the *selected*
        // one — every candidate scores 0 and the sort falls to shortest text — so
        // Enter after `SELECT * FROM ` spliced a whole statement into the one
        // being typed. Nobody had started typing the abbrev.
        let all = [snip(
            1,
            "Processes",
            "ps",
            Scope::Dialect(SqlDialect::MySql),
        )];
        assert!(snippet_abbrev_rows(&all, "", false, SqlDialect::MySql, 7).is_empty());
        assert_eq!(
            snippet_abbrev_rows(&all, "p", false, SqlDialect::MySql, 7).len(),
            1
        );
        // Never after a `qualifier.`, prefix or no prefix.
        assert!(snippet_abbrev_rows(&all, "p", true, SqlDialect::MySql, 7).is_empty());
        // Nor on a connection the snippet does not apply to.
        assert!(snippet_abbrev_rows(&all, "p", false, SqlDialect::Postgres, 7).is_empty());
    }

    #[test]
    fn an_abbrev_row_does_not_carry_the_matching_filter_itself() {
        // The caller ranks; this only decides what is in scope. A prefix that
        // matches nothing still yields the in-scope abbrevs, and `fuzzy_score`
        // drops them — the same path every other candidate takes.
        let all = [snip(1, "Processes", "ps", Scope::Global)];
        assert_eq!(
            snippet_abbrev_rows(&all, "zzz", false, SqlDialect::Sqlite, 3).len(),
            1
        );
    }

    #[test]
    fn one_row_per_spelling_resolved_by_scope() {
        // Two snippets share an abbrev: one row, and `by_abbrev` picks which.
        let all = [
            snip(1, "Shipped", "ps", Scope::Global),
            snip(2, "This connection's", "PS", Scope::Conn(7)),
        ];
        let rows = snippet_abbrev_rows(&all, "p", false, SqlDialect::MySql, 7);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "This connection's");
    }

    #[test]
    fn databases_hidden_until_prefix_and_never_the_active_one() {
        // Empty prefix → no databases (keeps the FROM/JOIN list tables-only).
        assert!(!database_suggestion_visible(
            "sakila",
            "",
            Some("classicmodels")
        ));
        // Typed prefix → other databases surface for cross-db `otherdb.table`.
        assert!(database_suggestion_visible(
            "sakila",
            "sak",
            Some("classicmodels")
        ));
        // The active database is never suggested (qualifying with it is redundant),
        // case-insensitively.
        assert!(!database_suggestion_visible(
            "classicmodels",
            "clas",
            Some("ClassicModels")
        ));
        // No active database → any database shows once a prefix is typed.
        assert!(database_suggestion_visible("world", "wo", None));
    }

    #[test]
    fn statement_identifiers_collects_words_excluding_the_prefix() {
        let sql = "SELECT customer_id, total FROM orders WHERE cust";
        // The word being typed (`cust`, the last 4 bytes) is excluded.
        let skip = (sql.len() - 4, sql.len());
        let ids = statement_identifiers(sql, 0, sql.len(), skip);
        assert!(ids.contains("customer_id"));
        assert!(ids.contains("total"));
        assert!(ids.contains("orders"));
        assert!(!ids.contains("cust")); // the prefix is skipped
    }

    #[test]
    fn recency_bonus_only_boosts_used_identifiers() {
        let used: HashSet<String> = ["customer_id".to_string()].into_iter().collect();
        assert_eq!(recency_bonus("customer_id", SuggestKind::Column, &used), 18);
        assert_eq!(recency_bonus("CUSTOMER_ID", SuggestKind::Column, &used), 18); // case-insensitive
        assert_eq!(recency_bonus("name", SuggestKind::Column, &used), 0); // not used
        // Keywords/functions never get the boost, even if present.
        let kw: HashSet<String> = ["select".to_string()].into_iter().collect();
        assert_eq!(recency_bonus("SELECT", SuggestKind::Keyword, &kw), 0);
    }

    /// An accept replaces the whole word the caret is in, the part after it
    /// too — `cust|_id` is all of `cust_id` — and a caret at a word's end or
    /// on a blank replaces only what is before it.
    #[test]
    fn an_accept_replaces_the_whole_word_the_caret_is_in() {
        let text = "SELECT cust_id FROM t";
        let at = 11; // cust|_id
        assert_eq!(&text[..at], "SELECT cust");
        assert_eq!(
            (word_start(text, at), word_end(text, at)),
            (7, 14),
            "the whole of `cust_id`"
        );
        assert_eq!(word_end(text, 14), 14, "at the word's end");
        assert_eq!(
            word_end(text, 6),
            6,
            "on the space before a word, nothing after"
        );
        assert_eq!(word_end("SELECT cou", 10), 10, "at the end of the text");
    }

    #[test]
    fn function_completion_adds_parens_and_places_caret_inside() {
        let (s, c) = completion_insertion("COUNT", true, false);
        assert_eq!(s, "COUNT()");
        assert_eq!(c, 6); // between `(` and `)`
        assert_eq!(&s[..c], "COUNT(");
    }

    #[test]
    fn function_completion_skips_parens_when_already_present() {
        let (s, c) = completion_insertion("COUNT", true, true);
        assert_eq!(s, "COUNT");
        assert_eq!(c, 5);
    }

    #[test]
    fn call_parens_must_be_on_the_same_line() {
        // "just ahead" means this line. A `(` opening an unrelated statement on the
        // next one is not this call's parens.
        assert!(!call_parens_follow("\n(SELECT 1)"));
        assert!(!call_parens_follow("\r\n  (SELECT 1)"));
        assert!(!call_parens_follow(""));
        assert!(!call_parens_follow(" FROM t"));
        // Spaces and tabs before the parens still count as present.
        assert!(call_parens_follow("("));
        assert!(call_parens_follow("  (a, b)"));
        assert!(call_parens_follow("\t()"));
    }

    #[test]
    fn non_function_completion_is_verbatim() {
        let (s, c) = completion_insertion("orders", false, false);
        assert_eq!(s, "orders");
        assert_eq!(c, 6);
    }

    // ── Popup placement ─────────────────────────────────────────────────────
    // The caret line is 24px tall in these; `area_h` is the editor pane's height.

    /// Box height for `rows` suggestions, i.e. what `popup_placement` has to fit.
    fn want(rows: usize) -> f64 {
        (rows as f64 * completion_row_h()).min(completion_max_h()) + COMPLETION_BORDER
    }

    #[test]
    fn popup_hangs_below_the_caret_when_the_list_fits_there() {
        // Line 2 of a 248px pane: 5 rows (122px) fit under it with room to spare.
        let p = popup_placement(29.0, 53.0, 5, 248.0);
        assert_eq!(p.top, 53.0 + COMPLETION_LINE_H);
        assert_eq!(p.max_h, completion_max_h());
    }

    #[test]
    fn popup_flips_above_the_caret_when_the_list_would_overhang() {
        // The reported bug: completing on the last line drew the list down over the
        // results grid. The same list now hangs above the caret line instead.
        let (line_top, line_bot, area_h) = (197.0, 221.0, 248.0);
        let p = popup_placement(line_top, line_bot, 5, area_h);
        assert_eq!(p.top, line_top - COMPLETION_LINE_H - want(5));
        // Pinned to the predicted height, not the cap — a flipped popup grows
        // downwards over the caret line otherwise.
        assert_eq!(p.max_h, want(5) - COMPLETION_BORDER);
        // …and the whole box now lands inside the pane, which is the point.
        assert!(p.top >= 0.0);
        assert!(p.top + want(5) <= area_h);
    }

    #[test]
    fn a_full_length_list_is_capped_and_still_fits_above() {
        // 40 rows clamp to completion_max_h(), so a tall pane can still flip it whole.
        let p = popup_placement(400.0, 424.0, 40, 500.0);
        assert_eq!(want(40), completion_max_h() + COMPLETION_BORDER);
        assert_eq!(p.top, 400.0 - COMPLETION_LINE_H - want(40));
        assert_eq!(p.max_h, completion_max_h());
    }

    #[test]
    fn a_list_too_tall_for_either_side_shortens_to_the_roomier_one() {
        // Caret past the middle of a short pane: below has 130 − 4 − 93 = 33px,
        // above has 86 − 3 − 4 = 79px, so it goes above, shortened to 79.
        let p = popup_placement(86.0, 90.0, 20, 130.0);
        assert_eq!(p.max_h, 79.0 - COMPLETION_BORDER);
        assert_eq!(p.top, 86.0 - COMPLETION_LINE_H - 79.0);
        assert!(p.top >= 0.0);
        // Mirrored: a caret near the top leaves more room below, so it stays below.
        let q = popup_placement(10.0, 34.0, 20, 100.0);
        assert_eq!(q.top, 34.0 + COMPLETION_LINE_H);
        assert_eq!(
            q.max_h,
            100.0 - completion_edge_pad() - (34.0 + COMPLETION_LINE_H) - COMPLETION_BORDER
        );
    }

    #[test]
    fn a_squeezed_list_stops_shrinking_at_the_minimum() {
        // Both sides are hopeless (a 40px pane). It keeps two readable rows and
        // overhangs rather than collapsing to a sliver — but never above y=0.
        let p = popup_placement(30.0, 34.0, 20, 40.0);
        assert!(p.max_h >= completion_min_h());
        assert!(p.top >= 0.0);
    }

    // ── Popup width and horizontal placement ────────────────────────────────

    /// A suggestion with the given name/table/detail; the rest doesn't affect width.
    fn sugg(name: &str, table: &str, detail: &str) -> Suggestion {
        Suggestion {
            text: name.to_string(),
            kind: SuggestKind::Column,
            detail: detail.to_string(),
            table: table.to_string(),
            alias: String::new(),
            glyph: SuggestGlyph::Kind,
            key: KeyKind::None,
            insert: None,
            replace: None,
        }
    }

    #[test]
    fn only_a_plain_table_row_is_given_an_alias_on_accept() {
        let sql = "SELECT * FROM ord";
        let at = |s: &Suggestion| {
            accepted_table_alias(s, sql, 0, sql.len(), sql.len(), SqlDialect::MySql)
        };
        let table = Suggestion {
            kind: SuggestKind::Table,
            ..sugg("orders", "", "")
        };
        assert_eq!(at(&table).as_deref(), Some("o"));
        // A column never; an FK target's insert already carries its own.
        assert_eq!(at(&sugg("orders", "", "")), None);
        let fk = Suggestion {
            glyph: SuggestGlyph::ForeignKey,
            ..table.clone()
        };
        assert_eq!(at(&fk), None);
    }

    #[test]
    fn the_natural_width_is_the_widest_row_not_the_last_one() {
        let narrow = natural_width(&[sugg("id", "", "")]);
        let wide = natural_width(&[
            sugg("id", "", ""),
            sugg("customer_reference_number", "orders", "varchar(255)"),
            sugg("n", "", ""),
        ]);
        assert!(wide > narrow, "{wide} should exceed {narrow}");
        // Chrome alone is the floor for an empty-ish row: nothing measures negative.
        assert!(narrow >= row_width(0.0, 0.0, 0.0) + COMPLETION_BORDER);
        // An empty list has no rows to measure, so it wants nothing.
        assert_eq!(natural_width(&[]), COMPLETION_BORDER);
    }

    #[test]
    fn a_row_is_never_sized_to_exactly_its_own_content() {
        // The `main` → `m…` regression: a box sized to the widest row's exact content
        // puts that row on its ellipsis boundary. Every prediction carries slack.
        let bare = completion_icon_w()
            + 40.0
            + completion_gap_w()
            + 0.0
            + completion_detail_gap()
            + 26.0
            + 2.0 * completion_row_pad();
        assert!(
            row_width(40.0, 0.0, 26.0) >= bare + completion_slack_w(),
            "a row must ask for more than it strictly needs"
        );
        // And the list inherits it, so the widest row has room in the box.
        let items = [sugg("parent", "", "main"), sugg("v", "", "main")];
        assert!(natural_width(&items) > row_width(40.21, 0.0, 26.14) - completion_slack_w());
    }

    #[test]
    fn a_narrow_list_gets_a_narrow_box_and_a_wide_one_is_capped() {
        // The floor stops a list of one-letter column names coming up as a sliver.
        assert_eq!(popup_w(80.0, 1000.0), completion_min_w());
        // Between the two it's sized to its content — this is what replaced the flat
        // `min_width(320)` that left short rows three-quarters empty.
        assert_eq!(popup_w(300.0, 1000.0), 300.0);
        // And a long function signature ellipsizes rather than dragging the box out.
        assert_eq!(popup_w(2000.0, 5000.0), completion_max_w());
    }

    #[test]
    fn the_pane_caps_the_width_even_below_the_floor() {
        // A 200px pane beats the 230px floor: cramped beats starting off the edge.
        assert_eq!(popup_w(400.0, 200.0), 200.0 - 2.0 * completion_edge_pad());
        // Unmeasured pane — nothing to cap against yet.
        assert_eq!(popup_w(300.0, 0.0), 300.0);
    }

    #[test]
    fn the_popup_slides_left_to_keep_its_right_edge_in_the_pane() {
        // Caret comfortably inside: straight under it, as before.
        assert_eq!(popup_x(100.0, 300.0, 900.0), COMPLETION_GUTTER + 100.0);
        // The reported bug — caret near the right edge, so the box shifts left far
        // enough to land inside instead of being clipped mid-row.
        let x = popup_x(500.0, 300.0, 600.0);
        assert_eq!(x, 600.0 - completion_edge_pad() - 300.0);
        assert!(x + 300.0 <= 600.0);
        assert!(x < COMPLETION_GUTTER + 500.0, "it must have moved left");
    }

    #[test]
    fn a_popup_wider_than_the_pane_starts_flush_rather_than_off_the_left() {
        // Names matter more than details, so the left edge wins the tie.
        assert_eq!(popup_x(200.0, 700.0, 400.0), 0.0);
        // Unmeasured pane: the plain under-the-caret x, never negative.
        assert_eq!(popup_x(50.0, 300.0, 0.0), COMPLETION_GUTTER + 50.0);
        assert_eq!(popup_x(-500.0, 300.0, 0.0), 0.0);
    }

    #[test]
    fn an_unmeasured_pane_keeps_the_plain_below_the_caret_placement() {
        // Height is 0 until the first layout; flipping on that would put the popup
        // above the editor entirely.
        let p = popup_placement(197.0, 221.0, 5, 0.0);
        assert_eq!(p.top, 221.0 + COMPLETION_LINE_H);
        assert_eq!(p.max_h, completion_max_h());
    }
    // ── fuzzy_score ───────────────────────────────────────────────────────────
    //
    // **What Enter and Tab splice.** `recompute_completions` sorts by
    // `(tier, score, text.len())` and sets `sel` to 0, so whatever this ranks
    // first is what goes into the user's buffer — and it had no test at all.
    // The scores themselves are free to change, so these are written as
    // comparisons and as answers (`Some`/`None`), never as absolute numbers.

    /// An **empty query scores every candidate the same**, which is the
    /// property `snippet_abbrev_rows`' own doc identifies as the mechanism of a
    /// shipped bug: `SELECT * FROM ` auto-opened the popup with the
    /// two-character built-in `ps` preselected, and Enter spliced a whole
    /// `;`-terminated statement into the one being typed. The guard that
    /// shipped lives in `snippet_abbrev_rows` — which is tested — and this is
    /// the behaviour that made it necessary.
    #[test]
    fn an_empty_query_ranks_nothing_above_anything() {
        for cand in ["orders", "ps", "customer_id", ""] {
            assert_eq!(fuzzy_score(cand, ""), Some(0), "{cand}");
        }
    }

    /// A candidate that is not a subsequence of the query answers `None`, which
    /// is the only thing keeping non-matches out of the list.
    #[test]
    fn a_non_subsequence_does_not_match_at_all() {
        assert_eq!(fuzzy_score("orders", "zx"), None);
        assert_eq!(fuzzy_score("orders", "sr"), None, "order matters");
        // The length guard, which is also the one place the function could
        // index out of bounds.
        assert_eq!(fuzzy_score("a", "ab"), None);
        assert_eq!(fuzzy_score("", "a"), None);
        // A subsequence that is not contiguous still matches — that is the
        // whole point of a fuzzy score.
        assert!(fuzzy_score("customer_id", "cid").is_some());
    }

    /// **A prefix outranks a match buried inside a longer name.** Typing `cus`
    /// has to preselect `customer_id`, not `account_customs`.
    #[test]
    fn a_prefix_match_outranks_an_interior_one() {
        let better = |a: &str, b: &str, q: &str| {
            let (sa, sb) = (fuzzy_score(a, q), fuzzy_score(b, q));
            assert!(
                sa > sb,
                "{q:?}: {a} scored {sa:?}, {b} scored {sb:?} — the prefix must win"
            );
        };
        better("customer_id", "account_customs", "cus");
        better("orders", "line_orders", "ord");
        // A word-boundary match outranks one mid-word, at equal position.
        better("order_total", "reordertotal", "ot");
        // And a shorter candidate outranks a longer one that matches as well.
        better("id", "identifier_column", "id");
    }

    /// Case is folded, and only over ASCII — the scan compares bytes, so a
    /// candidate with a non-ASCII letter matches byte-for-byte and its case is
    /// not folded. Recorded rather than desired: `is_word_byte` admits
    /// `>= 0x80`, so such candidates do reach here.
    #[test]
    fn matching_folds_ascii_case_only() {
        assert!(fuzzy_score("Orders", "ord").is_some());
        assert!(fuzzy_score("orders", "ORD").is_some());
        assert!(fuzzy_score("café", "café").is_some());
        assert_eq!(fuzzy_score("café", "CAFÉ"), None);
        // The ASCII head of the same name still folds.
        assert!(fuzzy_score("café", "CAF").is_some());
    }

    // **The composition lives in `rank.rs`, with the comparator.** A test here
    // called `the_callers_comparator_preselects_the_prefix_match`, and its own
    // doc said "the composition, not the function" — but it built its rows from
    // `fuzzy_score` and sorted them with a *copy* of the comparator written
    // inline, missing the tier and the `recency_bonus` term. Flipping `rank`'s
    // sort to score-ascending, so the popup preselects the worst match and Enter
    // splices it, left the whole workspace green. `rank` is callable now, so the
    // seam is asserted by calling it:
    // `the_comparator_preselects_the_prefix_match_and_buries_the_interior_one`
    // and `a_recently_used_name_is_lifted_past_a_better_scoring_one`, both of
    // which do fail against that flip.
}

#[cfg(test)]
mod catalog_tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use floem::prelude::SignalUpdate;
    use floem::reactive::{RwSignal, Scope};
    use schemaic_core::intel::SqlDialect;
    use schemaic_core::schema::{ColumnInfo, DbSchema, ForeignKeyInfo, SchemaState, TableInfo};

    use super::{build_catalog, build_offer_catalog};

    fn tbl(name: &str, cols: &[&str], fk: Option<(&str, &str, &str)>) -> TableInfo {
        TableInfo {
            name: name.to_string(),
            columns: cols
                .iter()
                .map(|c| ColumnInfo {
                    name: c.to_string(),
                    type_name: "int".into(),
                    nullable: true,
                    ..Default::default()
                })
                .collect(),
            foreign_keys: fk
                .map(|(col, ref_table, ref_col)| {
                    vec![ForeignKeyInfo {
                        name: "fk".into(),
                        columns: vec![col.to_string()],
                        ref_table: ref_table.to_string(),
                        ref_columns: vec![ref_col.to_string()],
                        ..Default::default()
                    }]
                })
                .unwrap_or_default(),
            ..Default::default()
        }
    }

    /// **One popup answered "does this database exist" two ways.**
    ///
    /// `SchemaIndex::build` filters with `schema::db_contributes` — a hidden
    /// database "contributes nothing at all, not its name, not its tables, not
    /// its columns" — and `build_catalog` never asked. So with `archive` hidden
    /// and no database selected, the plain table list skipped it entirely while
    /// the FK JOIN-target rows immediately above offered `archive`'s tables in
    /// the **top** tier, and accepting one spliced a join to a database the tree
    /// was hiding.
    ///
    /// The two catalogs are the fix, and the rule that separates them is
    /// `db_contributes`' own: hiding governs what is *offered*, never what is
    /// true — so the diagnostics keep the unfiltered view, or a column that
    /// exists would be squiggled for living in a hidden database.
    #[test]
    fn a_hidden_database_is_offered_no_join_but_still_validates() {
        let cx = Scope::new();
        let shop = DbSchema {
            tables: vec![tbl("orders", &["id", "customer_id"], None)],
            ..Default::default()
        };
        let archive = DbSchema {
            tables: vec![tbl(
                "customers",
                &["id", "order_id"],
                Some(("order_id", "orders", "id")),
            )],
            ..Default::default()
        };
        let nodes = vec![
            crate::ConnNode::new(cx, 0, "conn", "shop"),
            crate::ConnNode::new(cx, 1, "conn", "archive"),
        ];
        nodes[0].schema.set(SchemaState::Loaded(Arc::new(shop)));
        nodes[1].schema.set(SchemaState::Loaded(Arc::new(archive)));
        let db_nodes = RwSignal::new(nodes);

        let sql = "SELECT * FROM orders o JOIN ";
        let offered = |cat: &schemaic_core::intel::Catalog| -> Vec<String> {
            schemaic_core::intel::join_targets(sql, 0, sql.len(), sql.len(), cat, SqlDialect::MySql)
                .into_iter()
                .map(|t| t.table)
                .collect()
        };

        // Nothing hidden: the cross-database FK target is offered.
        let none = HashSet::new();
        let cat = build_offer_catalog(db_nodes, &none, None);
        assert!(
            offered(&cat).iter().any(|t| t == "customers"),
            "{:?}",
            offered(&cat)
        );

        // `archive` hidden, no database selected: it contributes nothing to an
        // offer.
        let hidden: HashSet<String> = ["archive".to_string()].into_iter().collect();
        let cat = build_offer_catalog(db_nodes, &hidden, None);
        assert!(
            !offered(&cat).iter().any(|t| t == "customers"),
            "a hidden database was offered as a JOIN target: {:?}",
            offered(&cat)
        );

        // The validation catalog is unfiltered, so a real column in a hidden
        // database is still known — "hiding governs what is offered, never what
        // is true".
        let cat = build_catalog(db_nodes, None);
        assert!(
            offered(&cat).iter().any(|t| t == "customers"),
            "the validation catalog must not be filtered: {:?}",
            offered(&cat)
        );

        // And the active-database exception holds: hiding the database you are
        // working in does not take it out of your own completion.
        let cat = build_offer_catalog(db_nodes, &hidden, Some("archive"));
        assert!(
            offered(&cat).iter().any(|t| t == "customers"),
            "the active database's own tables must survive hiding: {:?}",
            offered(&cat)
        );
        cx.dispose();
    }
}

#[cfg(test)]
mod schema_index_cache_tests {
    use super::*;
    use floem::reactive::Scope;
    use schemaic_core::schema::DbSchema;
    use std::sync::Arc;

    fn node(scope: Scope, db: &str, schema: Option<Arc<DbSchema>>) -> ConnNode {
        let n = ConnNode::new(scope, 1, "conn", db);
        if let Some(s) = schema {
            n.schema.set(SchemaState::Loaded(s));
        }
        n
    }

    /// **The index is rebuilt only when something it is built from moves.**
    ///
    /// `SchemaIndex::build` walks every loaded database, every table and every
    /// column, allocating a `ColMeta` per column plus three `HashMap`s — and
    /// `recompute_completions` called it directly, on a path that is
    /// deliberately undebounced, so it ran on effectively every keystroke. The
    /// catalog fourteen lines above it was memoised for this exact cost, on
    /// this exact key.
    ///
    /// Asserted by identity: the same `Rc` back means nothing was rebuilt.
    #[test]
    fn the_index_is_reused_until_a_schema_or_the_filters_move() {
        let scope = Scope::new();
        let s1 = Arc::new(DbSchema::default());
        let nodes = scope.create_rw_signal(vec![node(scope, "shop", Some(Arc::clone(&s1)))]);
        let none: HashSet<String> = HashSet::new();

        let a = schema_index(nodes, &none, Some("shop"));
        let b = schema_index(nodes, &none, Some("shop"));
        assert!(Rc::ptr_eq(&a, &b), "an unchanged input rebuilt the index");

        // A different active database is a different index.
        let c = schema_index(nodes, &none, None);
        assert!(!Rc::ptr_eq(&a, &c));

        // …and so is a different hidden set.
        let hidden: HashSet<String> = ["archive".to_string()].into_iter().collect();
        let d = schema_index(nodes, &hidden, None);
        assert!(!Rc::ptr_eq(&c, &d));

        // **A re-introspection replaces the `Arc`**, which is what the key is
        // for: the contents may be identical and the index must still be
        // rebuilt, because nothing else tells it the schema was re-read.
        let s2 = Arc::new(DbSchema::default());
        nodes.update(|v| v[0].schema.set(SchemaState::Loaded(Arc::clone(&s2))));
        let e = schema_index(nodes, &hidden, None);
        assert!(!Rc::ptr_eq(&d, &e), "a re-introspection was not noticed");

        // A node appearing or disappearing is a change too.
        nodes.update(|v| v.push(node(scope, "other", None)));
        let f = schema_index(nodes, &hidden, None);
        assert!(!Rc::ptr_eq(&e, &f));

        scope.dispose();
    }
}

/// [`index::build`] is a pure function over a slice — no signal, no `Editor`,
/// nothing that needs a mounted view — and had no test at all. The hidden-database
/// rule it enforces was pinned only for the *catalog* half (`catalog_tests`),
/// which is a different function over a different structure: the index is what
/// produces the plain table list, the column rows and the database rows.
#[cfg(test)]
mod index_tests {
    use super::*;
    use schemaic_core::schema::{ColumnInfo, DbSchema, ForeignKeyInfo, TableInfo};
    use std::sync::Arc;

    fn tbl(name: &str, cols: &[&str], fk_col: Option<&str>) -> TableInfo {
        TableInfo {
            name: name.to_string(),
            columns: cols
                .iter()
                .map(|c| ColumnInfo {
                    name: c.to_string(),
                    type_name: "int".into(),
                    ..Default::default()
                })
                .collect(),
            foreign_keys: fk_col
                .map(|c| {
                    vec![ForeignKeyInfo {
                        name: "fk".into(),
                        columns: vec![c.to_string()],
                        ref_table: "other".into(),
                        ref_columns: vec!["id".into()],
                        ..Default::default()
                    }]
                })
                .unwrap_or_default(),
            ..Default::default()
        }
    }

    fn node(db: &str, tables: Vec<TableInfo>) -> LoadedNode {
        LoadedNode {
            database: db.to_string(),
            schema: Some(Arc::new(DbSchema {
                tables,
                ..Default::default()
            })),
        }
    }

    fn hidden(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// **A hidden database contributes nothing at all** — not its name, not its
    /// tables, not its columns. The function's own doc says so; nothing asserted
    /// it for the index.
    /// The tab's own database's tables are indexed by their schema too —
    /// `sales.orders` on PostgreSQL or SQL Server — and another database's
    /// are not, since a `schema.table` never crosses a database.
    #[test]
    fn the_tabs_database_is_indexed_by_schema_too() {
        let in_ns = |ns: &str, name: &str, cols: &[&str]| TableInfo {
            schema: Some(ns.to_string()),
            ..tbl(name, cols, None)
        };
        let nodes = [
            node(
                "shop",
                vec![
                    in_ns("sales", "orders", &["region"]),
                    in_ns("dbo", "orders", &["id"]),
                ],
            ),
            node("archive", vec![in_ns("old", "orders", &["x"])]),
        ];
        let ix = index::build(&nodes, &HashSet::new(), Some("shop"));
        let cols = |ns: &str| {
            ix.columns_by_schema
                .get(&(ns.to_string(), "orders".to_string()))
                .map(|c| c.iter().map(|m| m.name.clone()).collect::<Vec<_>>())
        };
        assert_eq!(cols("sales"), Some(vec!["region".to_string()]));
        assert_eq!(cols("dbo"), Some(vec!["id".to_string()]));
        assert_eq!(cols("old"), None, "another database's schema");
        assert_eq!(
            ix.tables_by_schema.get("sales"),
            Some(&vec!["orders".to_string()])
        );
        // The widened list's pool holds every database's, with its namespace.
        assert!(ix.all_tables.contains(&rank::IndexedTable {
            database: "archive".to_string(),
            schema: Some("old".to_string()),
            name: "orders".to_string(),
        }));
        assert_eq!(ix.all_tables.len(), 3);
    }

    #[test]
    fn a_hidden_database_contributes_no_name_no_table_and_no_column() {
        let nodes = [
            node("shop", vec![tbl("orders", &["id"], None)]),
            node("archive", vec![tbl("old_orders", &["id"], None)]),
        ];
        let ix = index::build(&nodes, &hidden(&["archive"]), None);
        assert_eq!(ix.databases, vec!["shop".to_string()]);
        assert!(
            !ix.tables.iter().any(|(t, _)| t == "old_orders"),
            "{:?}",
            ix.tables
        );
        assert!(!ix.columns_by_db.keys().any(|(d, _)| d == "archive"));
        assert!(!ix.tables_by_db.contains_key("archive"));
    }

    /// …**unless it is the active one**, which is the exception that keeps a tab
    /// usable after its own database is hidden.
    #[test]
    fn the_active_database_contributes_even_when_it_is_hidden() {
        let nodes = [node("archive", vec![tbl("old_orders", &["id"], None)])];
        let ix = index::build(&nodes, &hidden(&["archive"]), Some("archive"));
        assert_eq!(ix.databases, vec!["archive".to_string()]);
        assert!(ix.tables.iter().any(|(t, _)| t == "old_orders"));
        assert!(ix.columns_by_db.keys().any(|(d, _)| d == "archive"));
    }

    /// **The unqualified pool is scoped to the active database; the qualified one
    /// is not.** A tab with a database selected must not be polluted by every
    /// other database's tables, while `otherdb.table` still completes.
    #[test]
    fn only_the_active_database_reaches_the_unqualified_pool() {
        let nodes = [
            node("shop", vec![tbl("orders", &["id"], None)]),
            node("archive", vec![tbl("old_orders", &["id"], None)]),
        ];
        let ix = index::build(&nodes, &HashSet::new(), Some("shop"));
        assert!(
            !ix.tables.iter().any(|(t, _)| t == "old_orders"),
            "{:?}",
            ix.tables
        );
        assert!(!ix.columns.contains_key("old_orders"));
        // But the qualified maps hold both, which is what `archive.old_orders`
        // completes from.
        assert!(
            ix.columns_by_db
                .contains_key(&("archive".to_string(), "old_orders".to_string()))
        );
        assert!(ix.tables_by_db.contains_key("archive"));
    }

    /// The FK tint's source: a column covered by a foreign key, and only that
    /// column.
    #[test]
    fn a_foreign_key_column_is_marked_and_its_siblings_are_not() {
        let nodes = [node(
            "shop",
            vec![tbl("orders", &["id", "customer_id"], Some("customer_id"))],
        )];
        let ix = index::build(&nodes, &HashSet::new(), Some("shop"));
        let cols = ix.columns.get("orders").expect("orders");
        let fk = |n: &str| cols.iter().find(|c| c.name == n).expect(n).foreign_key;
        assert!(fk("customer_id"));
        assert!(!fk("id"));
    }

    /// **A same-named table in two databases merges by column name.** The fast
    /// path (`entry.is_empty()`) exists to keep the ordinary case out of the
    /// quadratic merge, so both halves have to be exercised: the first table
    /// takes the fast path, the second the merge.
    #[test]
    fn two_databases_sharing_a_table_name_merge_their_columns_once_each() {
        let nodes = [
            node("shop", vec![tbl("orders", &["id", "total"], None)]),
            node("eu", vec![tbl("orders", &["id", "vat"], None)]),
        ];
        // No active database, so both are in the unqualified pool.
        let ix = index::build(&nodes, &HashSet::new(), None);
        let mut names: Vec<&str> = ix
            .columns
            .get("orders")
            .expect("orders")
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            ["id", "total", "vat"],
            "the shared `id` is not doubled"
        );
    }

    /// **A SQL Server synonym is offered as a table**, with its target's
    /// columns where the target is this database's own table
    /// (`DbSchema::synonyms`) and none otherwise — it was not offered at
    /// all, being held apart from `tables`.
    #[test]
    fn a_synonym_is_offered_with_its_targets_columns() {
        use schemaic_core::schema::{TsqlObject, TsqlObjectKind};
        let syn = |name: &str, target: &[&str]| TsqlObject {
            schema: Some("dbo".into()),
            name: name.into(),
            kind: TsqlObjectKind::Synonym {
                target: target.iter().map(|s| s.to_string()).collect(),
            },
        };
        let mut docs = tbl("docs", &["id", "c"], None);
        docs.schema = Some("dbo".into());
        let nodes = [LoadedNode {
            database: "main".into(),
            schema: Some(Arc::new(DbSchema {
                tables: vec![docs],
                tsql_objects: vec![
                    syn("docs_syn", &["dbo", "docs"]),
                    syn("far_syn", &["other", "dbo", "t"]),
                ],
                ..Default::default()
            })),
        }];
        let ix = index::build(&nodes, &HashSet::new(), Some("main"));
        for name in ["docs_syn", "far_syn"] {
            assert!(
                ix.tables.iter().any(|(t, _)| t == name),
                "{name}: {:?}",
                ix.tables
            );
            assert!(ix.tables_by_db["main"].iter().any(|t| t == name));
        }
        let names = |cols: &[ColMeta]| cols.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&ix.columns["docs_syn"]), ["id", "c"]);
        assert_eq!(
            names(&ix.columns_by_db[&("main".to_string(), "docs_syn".to_string())]),
            ["id", "c"]
        );
        assert!(!ix.columns.contains_key("far_syn"));
    }

    /// **A synonym is offered the columns of the table it resolves to**
    /// (S4.2-L1-05), not of whichever same-named table the index met last:
    /// over `dbo.docs (x, y)` beside a later `sales.docs (a, b)`, `s` offered
    /// `a, b`, which diagnostics then squiggled.
    #[test]
    fn a_synonym_is_offered_its_resolved_tables_columns() {
        use schemaic_core::schema::{TsqlObject, TsqlObjectKind};
        let mut dbo = tbl("docs", &["x", "y"], None);
        dbo.schema = Some("dbo".into());
        let mut sales = tbl("docs", &["a", "b"], None);
        sales.schema = Some("sales".into());
        let nodes = [LoadedNode {
            database: "main".into(),
            schema: Some(Arc::new(DbSchema {
                tables: vec![dbo, sales],
                tsql_objects: vec![TsqlObject {
                    schema: Some("dbo".into()),
                    name: "s".into(),
                    kind: TsqlObjectKind::Synonym {
                        target: vec!["dbo".into(), "docs".into()],
                    },
                }],
                ..Default::default()
            })),
        }];
        let ix = index::build(&nodes, &HashSet::new(), Some("main"));
        let names = |cols: &[ColMeta]| cols.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&ix.columns["s"]), ["x", "y"]);
        assert_eq!(
            names(&ix.columns_by_db[&("main".to_string(), "s".to_string())]),
            ["x", "y"]
        );
    }

    /// The database list dedupes case-insensitively — two connections onto the
    /// same server spelling one database differently are one row in the popup.
    #[test]
    fn the_database_list_dedupes_by_case() {
        let nodes = [node("Shop", vec![]), node("shop", vec![])];
        let ix = index::build(&nodes, &HashSet::new(), None);
        assert_eq!(ix.databases, vec!["Shop".to_string()]);
    }
}
