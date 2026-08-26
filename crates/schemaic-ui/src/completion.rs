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
use schemaic_core::schema::{DbSchema, SchemaState, classify_column_type, db_contributes};
use schemaic_core::snippet;
use schemaic_core::sql::statement_range;

use crate::schema_tree::column_type_icon;

// The keyword/function sets now live in `schemaic_core::intel` (so the core
// analysis + diagnostics share one authoritative copy); used here to seed the
// suggestion pool.
use schemaic_core::intel::{FUNCTIONS, SQL_KEYWORDS, STMT_KEYWORDS};

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

/// What an autocomplete row represents (drives its color + the detail shown).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum SuggestKind {
    Keyword,
    Function,
    Table,
    Column,
    Database,
    /// A saved snippet, offered by its abbrev. Its row inserts the snippet's
    /// *body*, not the abbrev — the `insert` override every FK-JOIN row uses.
    Snippet,
}

/// Whether a column suggestion participates in a key — tints its leading icon gold
/// (PK) / purple (FK), mirroring the schema tree. Non-columns are always `None`.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum KeyKind {
    None,
    Primary,
    Foreign,
}

/// One ranked autocomplete row: the text inserted, its kind, a dim detail (a
/// column's type + nullability, or a table's database), the owning table + in-scope
/// alias for a column (so a column row reads `id   orders o   int`), and whether
/// it's a primary key (drives the gold key glyph on column rows).
#[derive(Clone)]
pub(crate) struct Suggestion {
    text: String,
    kind: SuggestKind,
    detail: String,
    /// Owning table of a column suggestion (empty otherwise) — the mid annotation.
    table: String,
    /// In-scope alias of that table, if any (empty otherwise).
    alias: String,
    /// The leading schema-style glyph (a column's type family, a table/db icon, or
    /// the `square-function` mark for keywords/functions).
    icon: &'static str,
    /// Key membership — tints a column's icon gold/purple like the schema tree.
    key: KeyKind,
    /// Text spliced on accept when it differs from `text` — e.g. an FK JOIN target
    /// displays `orders` but inserts `orders ON o.customer_id = orders.id`. `None`
    /// inserts `text` verbatim.
    insert: Option<String>,
    /// Absolute byte range to replace on accept, overriding the default word range —
    /// used by `SELECT *` expansion to swap the `*` (or `t.*`) for the column list.
    replace: Option<(usize, usize)>,
}

use schemaic_core::sql::{is_word_byte, is_word_start};

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
}

/// The `schemaic_core::intel::Catalog` for the loaded connection schemas and the
/// tab's active database. Shared by the FK-aware `JOIN … ON` completion and the
/// editor's diagnostics (`editor_pane::compute_diagnostics`), so both read the same
/// catalog view.
pub(crate) fn build_catalog(
    db_nodes: RwSignal<Vec<ConnNode>>,
    active_db: Option<&str>,
) -> Arc<intel::Catalog> {
    // Each `schema` is the `Arc` out of `SchemaState`, so this walk is refcount
    // bumps rather than a deep copy of every table and column of every loaded
    // database — which is what it was, several times per keystroke.
    let loaded: Vec<(String, Arc<DbSchema>)> = db_nodes
        .get_untracked()
        .into_iter()
        .filter_map(|node| match node.schema.get_untracked() {
            SchemaState::Loaded(schema) => Some((node.database, schema)),
            _ => None,
        })
        .collect();
    CATALOG.with(|c| c.borrow_mut().get(&loaded, active_db))
}

/// One column's completion-relevant metadata.
#[derive(Clone)]
struct ColMeta {
    name: String,
    type_name: String,
    nullable: bool,
    primary_key: bool,
    foreign_key: bool,
}

/// A schema view built once per recompute: which databases/tables exist and each
/// table's columns, all indexed case-insensitively. Columns of same-named tables
/// across databases are merged (dedup by column name).
struct SchemaIndex {
    databases: Vec<String>,
    /// (table name, database it lives in).
    tables: Vec<(String, String)>,
    /// table name (lowercase) → its columns — the *active-database* unqualified pool.
    columns: HashMap<String, Vec<ColMeta>>,
    /// (database, table) (both lowercase) → its columns, for *every* loaded database.
    /// Backs qualified completion of a cross-database table (`otherdb.t` or an alias
    /// pointing at one), which the active-db-only `columns` map can't answer.
    ///
    /// `Rc` because the same `Vec` also feeds the unqualified merge below, and
    /// this map is populated for every loaded database on every recompute —
    /// storing it by value cloned each table's columns a second time.
    columns_by_db: HashMap<(String, String), Rc<Vec<ColMeta>>>,
    /// database name (lowercase) → its table names.
    tables_by_db: HashMap<String, Vec<String>>,
}

impl SchemaIndex {
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
    fn build(
        db_nodes: RwSignal<Vec<ConnNode>>,
        hidden: &HashSet<String>,
        active_db: Option<&str>,
    ) -> SchemaIndex {
        let mut databases = Vec::new();
        let mut tables = Vec::new();
        let mut columns: HashMap<String, Vec<ColMeta>> = HashMap::new();
        let mut columns_by_db: HashMap<(String, String), Rc<Vec<ColMeta>>> = HashMap::new();
        let mut tables_by_db: HashMap<String, Vec<String>> = HashMap::new();
        for node in db_nodes.get_untracked() {
            if !db_contributes(hidden, &node.database, active_db) {
                continue;
            }
            if !databases
                .iter()
                .any(|d: &String| d.eq_ignore_ascii_case(&node.database))
            {
                databases.push(node.database.clone());
            }
            if let SchemaState::Loaded(schema) = node.schema.get_untracked() {
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

/// Fuzzy subsequence score of `query` against `cand` (case-insensitive), or None
/// if `query`'s chars don't appear in order in `cand`. Higher is better: prefix,
/// word-boundary (after `_`), and contiguous matches are rewarded; a later first
/// match and a longer candidate are penalized. Empty query matches everything at
/// score 0 (so ranking falls to the caller's tiers).
fn fuzzy_score(cand: &str, query: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let c = cand.as_bytes();
    let q = query.as_bytes();
    let lc = |x: u8| x.to_ascii_lowercase();
    let mut score = 0i32;
    let mut qi = 0usize;
    let mut prev: Option<usize> = None;
    let mut first: Option<usize> = None;
    for ci in 0..c.len() {
        if qi >= q.len() {
            break;
        }
        if lc(c[ci]) == lc(q[qi]) {
            if first.is_none() {
                first = Some(ci);
            }
            let boundary = ci == 0 || c[ci - 1] == b'_';
            score += if boundary { 18 } else { 4 };
            if let Some(p) = prev {
                if ci == p + 1 {
                    score += 12;
                } else {
                    score -= (ci - p - 1).min(10) as i32;
                }
            }
            prev = Some(ci);
            qi += 1;
        }
    }
    if qi < q.len() {
        return None;
    }
    let is_prefix = c.len() >= q.len() && (0..q.len()).all(|k| lc(c[k]) == lc(q[k]));
    if is_prefix {
        score += 40;
    }
    score -= first.unwrap_or(0) as i32;
    score -= (c.len() as i32) / 5;
    Some(score)
}

/// Lowercased identifier words already present in `text[lo..hi]`, excluding the
/// word being typed at `skip` — a recency signal for ranking (you tend to reference
/// the same columns/tables again in a statement). Strings/comments aren't filtered
/// out; a stray hit only mildly reorders suggestions, never changes correctness.
fn statement_identifiers(
    text: &str,
    lo: usize,
    hi: usize,
    skip: (usize, usize),
) -> HashSet<String> {
    let b = text.as_bytes();
    let mut out = HashSet::new();
    let mut i = lo;
    while i < hi {
        let c = b[i];
        if is_word_start(c) {
            let s = i;
            let mut j = i + 1;
            while j < hi && is_word_byte(b[j]) {
                j += 1;
            }
            if (s, j) != skip {
                out.insert(text[s..j].to_ascii_lowercase());
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// Whether a database name should be offered at a table position. Only once the
/// user has typed a prefix — so an empty `FROM`/`JOIN` list stays tables-only — and
/// never the database already in use (qualifying a table with the current database is
/// redundant). Cross-database `otherdb.table` completion stays reachable: start
/// typing the other database's name and it surfaces.
fn database_suggestion_visible(db: &str, prefix: &str, active_db: Option<&str>) -> bool {
    !prefix.is_empty() && active_db.is_none_or(|a| !a.eq_ignore_ascii_case(db))
}

/// Ranking bonus for a candidate identifier (table/column/database) already used
/// elsewhere in the statement. Keywords/functions don't get it — repeating `SELECT`
/// or `COUNT` isn't a relevance signal. Modest, so a strong prefix match on a fresh
/// name still wins.
fn recency_bonus(text: &str, kind: SuggestKind, used: &HashSet<String>) -> i32 {
    let is_ident = matches!(
        kind,
        SuggestKind::Table | SuggestKind::Column | SuggestKind::Database
    );
    if is_ident && used.contains(&text.to_ascii_lowercase()) {
        18
    } else {
        0
    }
}

/// A raw completion candidate before scoring: `tier` is its context priority
/// (lower ranks higher; ties break by fuzzy score then length).
struct Cand {
    text: String,
    kind: SuggestKind,
    detail: String,
    table: String,
    alias: String,
    icon: &'static str,
    key: KeyKind,
    tier: u8,
    /// Splice-on-accept override (see [`Suggestion::insert`]).
    insert: Option<String>,
    /// Replace-range override (see [`Suggestion::replace`]).
    replace: Option<(usize, usize)>,
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
        let catalog = build_catalog(db_nodes, active_db);
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
                    icon: icons::KEY_SQUARE,
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

    let schema = hidden_dbs.with_untracked(|h| SchemaIndex::build(db_nodes, h, active_db));
    let scope = intel::statement_scope(&text, lo, hi, offset, dialect).tables;
    let pl = prefix.to_ascii_lowercase();

    // Collect raw candidates (dedup by text, first/lowest tier wins), then score.
    let mut cands: Vec<Cand> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let add = |cands: &mut Vec<Cand>,
               seen: &mut HashSet<String>,
               text: &str,
               kind: SuggestKind,
               detail: String,
               tier: u8| {
        let tl = text.to_ascii_lowercase();
        if tl == pl || !seen.insert(tl) {
            return;
        }
        cands.push(Cand {
            text: text.to_string(),
            kind,
            detail,
            table: String::new(),
            alias: String::new(),
            icon: kind_icon(kind),
            key: KeyKind::None,
            tier,
            insert: None,
            replace: None,
        });
    };
    // The detail string for a column typed against its own metadata: the type,
    // suffixed with a dim `· NULL` for nullable columns (NOT NULL stays clean).
    let col_type_detail = |c: &ColMeta| -> String {
        if c.nullable {
            format!("{} · NULL", c.type_name)
        } else {
            c.type_name.clone()
        }
    };
    // Column candidates carry their owning `table` (+ in-scope `alias`) as the mid
    // annotation and the type as `detail`. The leading glyph is the column's *type
    // family* (schema-tree style), tinted gold (PK) / purple (FK) via `key`. Deduped
    // by column name (first table in scope wins).
    let add_col = |cands: &mut Vec<Cand>,
                   seen: &mut HashSet<String>,
                   c: &ColMeta,
                   table: &str,
                   alias: Option<&str>,
                   tier: u8| {
        let tl = c.name.to_ascii_lowercase();
        if tl == pl || !seen.insert(tl) {
            return;
        }
        let key = if c.primary_key {
            KeyKind::Primary
        } else if c.foreign_key {
            KeyKind::Foreign
        } else {
            KeyKind::None
        };
        cands.push(Cand {
            text: c.name.clone(),
            kind: SuggestKind::Column,
            detail: col_type_detail(c),
            table: table.to_string(),
            alias: alias.unwrap_or("").to_string(),
            icon: column_type_icon(classify_column_type(&c.type_name)),
            key,
            tier,
            insert: None,
            replace: None,
        });
    };
    // In-scope table references as qualifier candidates: an alias (`ac`, tag icon,
    // detail = the table it stands for) or, for an unaliased table, its name (table
    // icon). Offered in column contexts so `ON a` suggests `ac` before you type `.`.
    let add_aliases = |cands: &mut Vec<Cand>, seen: &mut HashSet<String>| {
        for r in &scope {
            let qtext = r.alias.as_deref().unwrap_or(&r.name);
            let tl = qtext.to_ascii_lowercase();
            if tl == pl || !seen.insert(tl) {
                continue;
            }
            let detail = match (&r.alias, &r.db) {
                (Some(_), Some(db)) => format!("{db}.{}", r.name),
                (Some(_), None) => r.name.clone(),
                (None, _) => String::new(),
            };
            cands.push(Cand {
                text: qtext.to_string(),
                kind: SuggestKind::Table,
                detail,
                table: String::new(),
                alias: String::new(),
                icon: if r.alias.is_some() {
                    icons::TAG
                } else {
                    icons::TABLE
                },
                key: KeyKind::None,
                tier: 0,
                insert: None,
                replace: None,
            });
        }
    };
    // A table's columns: keyed by (db, table) when the table is database-qualified
    // (incl. a cross-database one), else the active-database unqualified pool.
    let cols_of = |db: Option<&str>, name: &str| -> Vec<ColMeta> {
        match db {
            Some(db) => schema
                .columns_by_db
                .get(&(db.to_ascii_lowercase(), name.to_ascii_lowercase()))
                .map(|m| m.as_ref().clone())
                .unwrap_or_default(),
            None => schema
                .columns
                .get(&name.to_ascii_lowercase())
                .cloned()
                .unwrap_or_default(),
        }
    };
    // Snippet abbrevs, in the top tier and ahead of the keyword continuations: an
    // abbrev is a name its owner chose *in order to type it*, so when one matches
    // what is being typed it is not a guess the way a ranked keyword is.
    //
    // Not offered after a `qualifier.` — there the only sensible answers are that
    // table's columns — and one row per distinct spelling, resolved through
    // `snippet::by_abbrev` so the narrowest scope wins a shared abbrev by the
    // same rule everywhere rather than by whichever the list happened to reach
    // first.
    if !qualified {
        let all = snippets.get_untracked();
        let mut spellings: Vec<String> = Vec::new();
        for s in all.iter().filter(|s| snippet::applies(s, dialect, conn_id)) {
            if let Some(a) = s.abbrev.as_deref().filter(|a| !a.is_empty()) {
                let lower = a.to_ascii_lowercase();
                if !spellings.iter().any(|s| s.eq_ignore_ascii_case(&lower)) {
                    spellings.push(a.to_string());
                }
            }
        }
        for spelling in spellings {
            let Some(s) = snippet::by_abbrev(&all, &spelling, dialect, conn_id) else {
                continue;
            };
            let tl = spelling.to_ascii_lowercase();
            if !seen.insert(tl) {
                continue;
            }
            cands.push(Cand {
                text: spelling,
                kind: SuggestKind::Snippet,
                detail: s.name.clone(),
                table: String::new(),
                alias: String::new(),
                icon: kind_icon(SuggestKind::Snippet),
                key: KeyKind::None,
                tier: 0,
                // The row shows the abbrev and inserts the query.
                insert: Some(s.body.clone()),
                replace: None,
            });
        }
    }
    // Expected clause-keyword continuations go in the *top* tier (above columns,
    // functions, and — after a complete table ref — schema table names), so the
    // legal next keyword the grammar predicts wins ties. Added before the
    // per-context candidates so they claim tier 0 (dedup keeps the first entry).
    // Skipped after a `qualifier.` (there we want only that table's columns).
    if !qualified {
        for kw in &cont.keywords {
            add(
                &mut cands,
                &mut seen,
                kw,
                SuggestKind::Keyword,
                String::new(),
                0,
            );
        }
    }
    // Once a clause continuation is expected (a complete table ref sits before the
    // caret), the schema table names are no longer the primary suggestion — demote
    // them below the keyword continuations.
    let table_tier: u8 = if cont.keywords.is_empty() { 0 } else { 1 };

    // A qualifier resolves to a table — (name, its database, the alias to annotate
    // with) — via an in-scope alias, else a bare table name (whether or not it's in
    // FROM). The database is carried so a cross-database table's columns resolve.
    let resolve = |q: &str| -> Option<(String, Option<String>, Option<String>)> {
        for r in &scope {
            if r.alias
                .as_deref()
                .is_some_and(|a| a.eq_ignore_ascii_case(q))
            {
                return Some((r.name.clone(), r.db.clone(), r.alias.clone()));
            }
        }
        for r in &scope {
            if r.alias.is_none() && r.name.eq_ignore_ascii_case(q) {
                return Some((r.name.clone(), r.db.clone(), None));
            }
        }
        if schema.columns.contains_key(&q.to_ascii_lowercase()) {
            return Some((q.to_string(), None, None));
        }
        None
    };

    match &ctx {
        ClauseCtx::Qualified(q) => {
            // A qualifier is either a table/alias (→ its columns) or a database name
            // (→ its tables, for `db.table`). The scope resolver no longer misparses a
            // dangling `db.` as a table (fixed in `intel::lexer_scope`), so the natural
            // table-first order is safe.
            if let Some((table, db, alias)) = resolve(q) {
                for c in cols_of(db.as_deref(), &table) {
                    add_col(&mut cands, &mut seen, &c, &table, alias.as_deref(), 0);
                }
            } else if let Some(tbls) = schema.tables_by_db.get(&q.to_ascii_lowercase()) {
                for t in tbls {
                    add(&mut cands, &mut seen, t, SuggestKind::Table, q.clone(), 0);
                }
            }
        }
        ClauseCtx::Table => {
            // FK-aware JOIN targets first (top tier): a table connected by a foreign
            // key to something in scope, inserting `table ON <predicate>` in one go.
            let catalog = build_catalog(db_nodes, active_db);
            let mut fk_added = false;
            for jt in intel::join_targets(&text, lo, hi, offset, &catalog, dialect) {
                let tl = jt.table.to_ascii_lowercase();
                if tl == pl || !seen.insert(tl) {
                    continue;
                }
                fk_added = true;
                cands.push(Cand {
                    text: jt.table.clone(),
                    kind: SuggestKind::Table,
                    detail: "foreign key".to_string(),
                    table: String::new(),
                    alias: String::new(),
                    icon: icons::KEY_SQUARE,
                    key: KeyKind::Foreign,
                    tier: 0,
                    insert: Some(format!("{} ON {}", jt.table_sql, jt.predicate)),
                    replace: None,
                });
            }
            // With FK targets present, keep them strictly above the plain table list.
            let plain_tier = if fk_added {
                table_tier.max(1)
            } else {
                table_tier
            };
            for (name, db) in &schema.tables {
                add(
                    &mut cands,
                    &mut seen,
                    name,
                    SuggestKind::Table,
                    db.clone(),
                    plain_tier,
                );
            }
            // Databases are offered only once a prefix is typed (so an empty
            // FROM/JOIN list stays tables-only) and never the active one — cross-db
            // `otherdb.table` stays reachable by typing the other database's name.
            for db in &schema.databases {
                if database_suggestion_visible(db, &prefix, active_db) {
                    add(
                        &mut cands,
                        &mut seen,
                        db,
                        SuggestKind::Database,
                        String::new(),
                        table_tier + 1,
                    );
                }
            }
        }
        ClauseCtx::Column => {
            if scope.is_empty() {
                // No FROM yet: offer every column, annotated by its owning table so
                // the broader list stays navigable.
                for (name, _) in &schema.tables {
                    for c in cols_of(None, name) {
                        add_col(&mut cands, &mut seen, &c, name, None, 1);
                    }
                }
            } else {
                // In-scope aliases/table names as qualifier candidates (`ac`, `ord`).
                add_aliases(&mut cands, &mut seen);
                // Bias toward the most recently added (last) table in the FROM/JOIN
                // list — the one you're most likely about to reference: its columns
                // rank first (tier 0) and claim shared names; earlier tables fall to
                // tier 1 (still above functions/keywords).
                let last = scope.len() - 1;
                for (i, r) in scope.iter().enumerate().rev() {
                    let tier = if i == last { 0 } else { 1 };
                    for c in cols_of(r.db.as_deref(), &r.name) {
                        add_col(&mut cands, &mut seen, &c, &r.name, r.alias.as_deref(), tier);
                    }
                }
            }
            for fun in FUNCTIONS {
                add(
                    &mut cands,
                    &mut seen,
                    fun.name,
                    SuggestKind::Function,
                    fun.signature.to_string(),
                    2,
                );
            }
            for &k in SQL_KEYWORDS {
                add(
                    &mut cands,
                    &mut seen,
                    k,
                    SuggestKind::Keyword,
                    String::new(),
                    3,
                );
            }
        }
        ClauseCtx::Start => {
            for &k in STMT_KEYWORDS {
                add(
                    &mut cands,
                    &mut seen,
                    k,
                    SuggestKind::Keyword,
                    String::new(),
                    0,
                );
            }
            for &k in SQL_KEYWORDS {
                add(
                    &mut cands,
                    &mut seen,
                    k,
                    SuggestKind::Keyword,
                    String::new(),
                    1,
                );
            }
        }
        ClauseCtx::Other => {
            add_aliases(&mut cands, &mut seen);
            for r in &scope {
                for c in cols_of(r.db.as_deref(), &r.name) {
                    add_col(&mut cands, &mut seen, &c, &r.name, r.alias.as_deref(), 0);
                }
            }
            for (name, db) in &schema.tables {
                add(
                    &mut cands,
                    &mut seen,
                    name,
                    SuggestKind::Table,
                    db.clone(),
                    1,
                );
            }
            for &k in SQL_KEYWORDS {
                add(
                    &mut cands,
                    &mut seen,
                    k,
                    SuggestKind::Keyword,
                    String::new(),
                    2,
                );
            }
        }
    }

    // SELECT * expansion: when the caret sits right after a projection `*`/`t.*`,
    // offer an item that rewrites it into the explicit column list (shown when the
    // popup opens here — e.g. via Ctrl+Space, since the list doesn't auto-open on `*`).
    if let Some(exp) = star_expansion(&text, lo, hi, offset, db_nodes, active_db, dialect) {
        let ncols = exp.replacement.matches(',').count() + 1;
        cands.push(Cand {
            text: "expand *".to_string(),
            kind: SuggestKind::Column,
            detail: format!("{ncols} columns"),
            table: String::new(),
            alias: String::new(),
            icon: icons::TABLE,
            key: KeyKind::None,
            tier: 0,
            insert: Some(exp.replacement),
            replace: Some(exp.range),
        });
    }

    // Identifiers already written in this statement rank a little higher (recency):
    // you tend to reference the same columns/tables again.
    let used = statement_identifiers(&text, lo, hi, (word_lo, offset));

    // Score by fuzzy match (+ recency bonus); sort by tier (context priority), then
    // score, then a shorter candidate. Non-matches drop out.
    let mut scored: Vec<(u8, i32, Cand)> = cands
        .into_iter()
        .filter_map(|c| {
            fuzzy_score(&c.text, &prefix)
                .map(|s| (c.tier, s + recency_bonus(&c.text, c.kind, &used), c))
        })
        .collect();
    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(b.1.cmp(&a.1))
            .then(a.2.text.len().cmp(&b.2.text.len()))
    });
    let items: Vec<Suggestion> = scored
        .into_iter()
        .take(40)
        .map(|(_, _, c)| Suggestion {
            text: c.text,
            kind: c.kind,
            detail: c.detail,
            table: c.table,
            alias: c.alias,
            icon: c.icon,
            key: c.key,
            insert: c.insert,
            replace: c.replace,
        })
        .collect();

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
fn star_expansion(
    text: &str,
    lo: usize,
    hi: usize,
    offset: usize,
    db_nodes: RwSignal<Vec<ConnNode>>,
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
    let catalog = build_catalog(db_nodes, active_db);
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
pub(crate) fn completion_popup(
    comp: Completion,
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
                        icon,
                        key,
                        insert: _,
                        replace: _,
                    } = item;
                    let color = suggest_color(kind);
                    // Schema-style leading glyph, coloured by kind/key (see
                    // `suggest_icon_color`): a column's type family tinted gold (PK) /
                    // purple (FK), a table/db icon, or the muted `square-function`
                    // mark for keywords/functions.
                    let lead: AnyView = icons::icon(icon, COMPLETION_ICON_BASE)
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
                        text(name).style(move |s| {
                            s.font_size(completion_name_size())
                                .color(color)
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
                .max_width(560.0)
        } else {
            s
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        KeyKind, SuggestKind, Suggestion, call_parens_follow, completion_insertion,
        database_suggestion_visible, natural_width, popup_may_open, popup_placement, popup_w,
        popup_x, recency_bonus, row_width, statement_identifiers, types_a_character,
    };
    use crate::consts::{
        COMPLETION_BORDER, COMPLETION_GUTTER, COMPLETION_LINE_H, completion_detail_gap,
        completion_edge_pad, completion_gap_w, completion_icon_w, completion_max_h,
        completion_max_w, completion_min_h, completion_min_w, completion_row_h, completion_row_pad,
        completion_slack_w,
    };
    use floem::keyboard::{Key, NamedKey};
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
            icon: "",
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
}
