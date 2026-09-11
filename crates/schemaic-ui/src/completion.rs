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
use schemaic_core::sql::statement_range;

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
/// Shares [`CATALOG`]'s cache: the filtered `loaded` list is a different key, so
/// the two views coexist without a second cache or a second walk.
pub(crate) fn build_offer_catalog(
    db_nodes: RwSignal<Vec<ConnNode>>,
    hidden: &HashSet<String>,
    active_db: Option<&str>,
) -> Arc<intel::Catalog> {
    catalog_of(db_nodes, Some(hidden), active_db)
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
        let mut columns: HashMap<String, Vec<ColMeta>> = HashMap::new();
        let mut columns_by_db: HashMap<(String, String), Rc<Vec<ColMeta>>> = HashMap::new();
        let mut tables_by_db: HashMap<String, Vec<String>> = HashMap::new();
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
                for t in &schema.tables {
                    by_db.push(t.name.clone());
                    // Is this column covered by a foreign key (→ the FK tint)? A
                    // linear scan of the table's FK columns, which number a handful:
                    // building a lowercased `HashSet` cost an allocation per FK
                    // column *and* one per column looked up.
                    let is_fk = |name: &str| {
                        t.foreign_keys
                            .iter()
                            .flat_map(|fk| fk.columns.iter())
                            .any(|c| c.eq_ignore_ascii_case(name))
                    };
                    let metas: Rc<Vec<ColMeta>> = Rc::new(
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
                    );
                    // Every database's columns are keyed by (db, table) for qualified
                    // (incl. cross-database) completion.
                    columns_by_db.insert(
                        (db_lower.clone(), t.name.to_ascii_lowercase()),
                        metas.clone(),
                    );
                    if in_scope {
                        tables.push((t.name.clone(), node.database.clone()));
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
            }
        }
        SchemaIndex {
            databases,
            tables,
            columns,
            columns_by_db,
            tables_by_db,
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
pub(crate) fn update_signature_help(ed: &Editor, comp: Completion, dialect: SqlDialect) {
    let offset = ed.cursor.get_untracked().offset();
    let text = ed.doc().text().to_string();
    let (lo, hi) = statement_range(&text, offset, dialect);
    let help = intel::signature_help(&text, lo, hi, offset, dialect);
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
}

pub(crate) fn recompute_completions(
    ed: &Editor,
    ctx: CompletionCtx<'_>,
    comp: Completion,
    force: bool,
) {
    let CompletionCtx {
        db_nodes,
        hidden_dbs,
        active_db,
        dialect,
        snippets,
        conn_id,
    } = ctx;
    let offset = ed.cursor.get_untracked().offset();
    let text = ed.doc().text().to_string();
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
    let word_lo = word_start(&text, offset);
    let prefix = text.get(word_lo..offset).unwrap_or("").to_string();

    let (lo, hi) = statement_range(&text, offset, dialect);
    // Context is lexer-based (correct mid-edit); scope prefers the real AST
    // (robust CTE/alias/derived-table resolution), falling back to the lexer.
    let ctx = intel::clause_context(&text, lo, word_lo, dialect);
    let qualified = matches!(ctx, ClauseCtx::Qualified(_));
    // Expected next keyword/phrase continuations from SQL clause grammar (the
    // `WHERE` after a complete table ref, `FROM` after the projection, `GROUP BY`
    // as one item). These seed the top suggestion tier; `auto_show` opens the popup
    // on an empty prefix right after an operand-taking clause keyword.
    let cont = intel::clause_continuation(&text, lo, word_lo, dialect);

    // FK-aware auto-join: right after a fresh `JOIN … ON `, offer the foreign-key
    // join predicate as a single, ready-to-insert suggestion (DataGrip-style). Only
    // on an empty ON expression (`prefix` empty, in a column/ON context), so it
    // never fights manual typing.
    if prefix.is_empty() && matches!(ctx, ClauseCtx::Column) {
        // The *offer* catalog: a hidden database must not be joined to.
        let catalog = hidden_dbs.with_untracked(|h| build_offer_catalog(db_nodes, h, active_db));
        if let Some(pred) = intel::join_condition(&text, lo, hi, offset, &catalog, dialect) {
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
            comp.open.set(true);
            return;
        }
    }

    // Don't pop the list on every space: an empty prefix only shows suggestions
    // right after a `.`, right after an operand-taking clause keyword (`cont.
    // auto_show` — columns after WHERE/ON/BY/SET, tables after FROM), or when
    // explicitly requested (Ctrl+Space).
    if prefix.is_empty() && !qualified && !force && !cont.auto_show {
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
    let scope = intel::statement_scope(&text, lo, hi, offset, dialect).tables;

    // Gathered on exactly the paths that used to gather them, which is what keeps
    // the cost where it was: the FK catalog is a walk of the whole schema tree,
    // and only a table slot ever wanted it.
    let join_targets = if matches!(ctx, ClauseCtx::Table) {
        let catalog = hidden_dbs.with_untracked(|h| build_offer_catalog(db_nodes, h, active_db));
        intel::join_targets(&text, lo, hi, offset, &catalog, dialect)
    } else {
        Vec::new()
    };
    let star = hidden_dbs
        .with_untracked(|h| star_expansion(&text, lo, hi, offset, db_nodes, h, active_db, dialect));
    let snippet_rows = rank::snippet_abbrev_rows(
        &snippets.get_untracked(),
        &prefix,
        matches!(ctx, ClauseCtx::Qualified(_)),
        dialect,
        conn_id,
    );
    // Identifiers already written in this statement rank a little higher (recency):
    // you tend to reference the same columns/tables again.
    let used = rank::statement_identifiers(&text, lo, hi, (word_lo, offset));

    let items = rank::rank(
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
        },
    );

    set_anchor(ed, comp, offset);
    let open = !items.is_empty();
    set_items(comp, items);
    comp.sel.set(0);
    comp.open.set(open);
}

/// Replace the word at the caret with the selected suggestion.
pub(crate) fn accept_completion(ed: &Editor, comp: Completion) {
    let offset = ed.cursor.get_untracked().offset();
    let doc = ed.doc();
    let text = doc.text().to_string();
    let start = word_start(&text, offset);
    let idx = comp.sel.get_untracked();
    if let Some((word, kind, over, replace)) = comp.items.with_untracked(|v| {
        v.get(idx)
            .map(|s| (s.text.clone(), s.kind, s.insert.clone(), s.replace))
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
                call_parens_follow(&text[offset..]),
            )
        };
        // Most completions replace the word being typed; a `replace` override (star
        // expansion) swaps a specific range instead (the `*` / `t.*`).
        let (from, to) = replace.unwrap_or((start, offset));
        doc.edit_single(Selection::region(from, to), &insert, EditType::Completion);
        // `edit_single` doesn't move the caret, so place it explicitly.
        ed.cursor
            .update(|c| c.set_offset(from + caret, false, false));
    }
    comp.open.set(false);
    set_items(comp, Vec::new());
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
                    .on_click_stop({
                        let editor = editor.clone();
                        move |_| {
                            comp.sel.set(i);
                            accept_completion(&editor, comp);
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
            container(
                autohide(scroll(list).scroll_to_view(move || row_ids.get(comp.sel.get()).copied()))
                    .style(move |s| {
                        let (line_top, line_bot, _) = anchor();
                        let place = popup_placement(line_top, line_bot, rows_n, area_h.get());
                        s.width_full().max_height(place.max_h)
                    }),
            )
            .style(|s| {
                s.width_full()
                    .background(theme::bg_deepest())
                    .border(1.0)
                    .border_color(theme::completion_border())
                    .border_radius(6.0)
            })
            .clip()
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
            let rows = comp.items.with(Vec::len);
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
mod tests {
    use super::{
        KeyKind, SuggestGlyph, SuggestKind, Suggestion, call_parens_follow, completion_insertion,
        natural_width, popup_may_open, popup_placement, popup_w, popup_x, row_width,
        types_a_character,
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

    /// **The composition, not the function.** The seam that matters is
    /// `fuzzy_score` plus the caller's comparator, since it is the sort that
    /// decides what Enter inserts: highest score first, ties broken by the
    /// shorter text.
    #[test]
    fn the_callers_comparator_preselects_the_prefix_match() {
        let mut rows: Vec<(i32, &str)> = ["account_customs", "customer_id", "custom", "orders"]
            .into_iter()
            .filter_map(|t| fuzzy_score(t, "cus").map(|s| (s, t)))
            .collect();
        // `recompute_completions`' comparator, minus the tier every row here
        // shares: score descending, then the shorter text.
        rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.len().cmp(&b.1.len())));
        assert_eq!(rows.first().map(|r| r.1), Some("custom"));
        // `orders` is not a match and never reaches the list.
        assert!(!rows.iter().any(|r| r.1 == "orders"));
        // And the interior match sorts last of the three that do.
        assert_eq!(rows.last().map(|r| r.1), Some("account_customs"));
    }
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
