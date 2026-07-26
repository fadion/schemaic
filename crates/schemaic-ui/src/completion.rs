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

use std::collections::{HashMap, HashSet};

use floem::kurbo::Point;
use floem::prelude::*;
use floem::views::editor::Editor;
use floem::views::editor::core::cursor::CursorAffinity;
use floem::views::editor::core::editor::EditType;
use floem::views::editor::core::selection::Selection;

use schemaic_core::intel::{self, ClauseCtx, SqlDialect};
use schemaic_core::schema::{DbSchema, SchemaState};
use schemaic_core::sql::statement_range;

// The keyword/function sets now live in `schemaic_core::intel` (so the core
// analysis + diagnostics share one authoritative copy); used here to seed the
// suggestion pool.
use schemaic_core::intel::{SQL_FUNCTIONS, SQL_KEYWORDS, STMT_KEYWORDS};

use floem::AnyView;

use crate::consts::*;
use crate::{ConnNode, icons, theme};

// ===== moved from lib.rs (autocomplete) =====
// ── Autocomplete ────────────────────────────────────────────────────────────

/// Autocomplete popup state, shared between the editor key handler, the
/// per-edit recompute, and the popup view.
#[derive(Clone, Copy)]
pub(crate) struct Completion {
    pub(crate) items: RwSignal<Vec<Suggestion>>,
    pub(crate) sel: RwSignal<usize>,
    pub(crate) open: RwSignal<bool>,
    /// Caret position in editor-content coordinates (drives popup placement).
    pub(crate) point: RwSignal<Point>,
    /// Set right after accepting, so the edit that follows doesn't re-open the
    /// popup on the just-inserted word.
    pub(crate) suppress: RwSignal<bool>,
}

/// What an autocomplete row represents (drives its color + the detail shown).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum SuggestKind {
    Keyword,
    Function,
    Table,
    Column,
    Database,
}

/// One ranked autocomplete row: the text inserted, its kind, a dim detail (a
/// column's type + nullability, or a table's database), and whether it's a primary
/// key (drives the gold key glyph on column rows).
#[derive(Clone)]
pub(crate) struct Suggestion {
    text: String,
    kind: SuggestKind,
    detail: String,
    pk: bool,
}

fn is_word_byte(b: u8) -> bool {
    // `>= 0x80` = any UTF-8 lead/continuation byte, so Unicode identifiers count
    // as one word instead of splitting at the first non-ASCII byte (review B6).
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// Byte offset where the identifier ending at `offset` begins.
fn word_start(text: &str, offset: usize) -> usize {
    let bytes = text.as_bytes();
    let mut start = offset.min(text.len());
    while start > 0 && is_word_byte(bytes[start - 1]) {
        start -= 1;
    }
    start
}

/// Build the `schemaic_core::intel::Catalog` from the loaded connection schemas
/// and the tab's active database. Shared by the FK-aware `JOIN … ON` completion
/// and the editor's diagnostics (`editor_pane::compute_diagnostics`), so both read
/// the same catalog view.
pub(crate) fn build_catalog(
    db_nodes: RwSignal<Vec<ConnNode>>,
    active_db: Option<&str>,
) -> intel::Catalog {
    let loaded: Vec<(String, DbSchema)> = db_nodes
        .get_untracked()
        .into_iter()
        .filter_map(|node| match node.schema.get_untracked() {
            SchemaState::Loaded(schema) => Some((node.database, schema)),
            _ => None,
        })
        .collect();
    let refs: Vec<(&str, &DbSchema)> = loaded.iter().map(|(d, s)| (d.as_str(), s)).collect();
    intel::Catalog::build(&refs, active_db)
}

/// One column's completion-relevant metadata.
#[derive(Clone)]
struct ColMeta {
    name: String,
    type_name: String,
    nullable: bool,
    primary_key: bool,
}

/// A schema view built once per recompute: which databases/tables exist and each
/// table's columns, all indexed case-insensitively. Columns of same-named tables
/// across databases are merged (dedup by column name).
struct SchemaIndex {
    databases: Vec<String>,
    /// (table name, database it lives in).
    tables: Vec<(String, String)>,
    /// table name (lowercase) → its columns.
    columns: HashMap<String, Vec<ColMeta>>,
    /// database name (lowercase) → its table names.
    tables_by_db: HashMap<String, Vec<String>>,
}

impl SchemaIndex {
    /// Build the completion index. When `active_db` is `Some`, the *unqualified*
    /// suggestion pool (`tables`/`columns`) is scoped to that database — now that
    /// a tab has a selected database, suggestions shouldn't be polluted by every
    /// other database on the connection (TODO). `databases` and `tables_by_db`
    /// stay complete so an explicit `otherdb.table` qualifier still completes.
    fn build(db_nodes: RwSignal<Vec<ConnNode>>, active_db: Option<&str>) -> SchemaIndex {
        let mut databases = Vec::new();
        let mut tables = Vec::new();
        let mut columns: HashMap<String, Vec<ColMeta>> = HashMap::new();
        let mut tables_by_db: HashMap<String, Vec<String>> = HashMap::new();
        for node in db_nodes.get_untracked() {
            if !databases
                .iter()
                .any(|d: &String| d.eq_ignore_ascii_case(&node.database))
            {
                databases.push(node.database.clone());
            }
            if let SchemaState::Loaded(schema) = node.schema.get_untracked() {
                let by_db = tables_by_db
                    .entry(node.database.to_ascii_lowercase())
                    .or_default();
                for t in &schema.tables {
                    by_db.push(t.name.clone());
                }
                // Unqualified pool: only the selected database (or all, if none).
                let in_scope = active_db.is_none_or(|db| db.eq_ignore_ascii_case(&node.database));
                if in_scope {
                    for t in &schema.tables {
                        tables.push((t.name.clone(), node.database.clone()));
                        let entry = columns.entry(t.name.to_ascii_lowercase()).or_default();
                        for c in &t.columns {
                            if !entry.iter().any(|m| m.name.eq_ignore_ascii_case(&c.name)) {
                                entry.push(ColMeta {
                                    name: c.name.clone(),
                                    type_name: c.type_name.clone(),
                                    nullable: c.nullable,
                                    primary_key: c.primary_key,
                                });
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

/// A raw completion candidate before scoring: `tier` is its context priority
/// (lower ranks higher; ties break by fuzzy score then length).
struct Cand {
    text: String,
    kind: SuggestKind,
    detail: String,
    tier: u8,
    pk: bool,
}

/// Recompute context-aware suggestions for the word at the caret. Ranks the most
/// relevant kind first (columns of the in-scope tables after SELECT/WHERE, tables
/// after FROM, a qualifier's columns after `x.`, statement keywords at the
/// start), then functions/keywords; within a tier, best fuzzy match wins. Empty
/// prefix closes the popup unless `force` (Ctrl+Space) or the caret is right
/// after a `.`.
pub(crate) fn recompute_completions(
    ed: &Editor,
    db_nodes: RwSignal<Vec<ConnNode>>,
    comp: Completion,
    active_db: Option<&str>,
    force: bool,
) {
    if comp.suppress.get_untracked() {
        comp.suppress.set(false);
        if !force {
            comp.open.set(false);
            comp.items.set(Vec::new());
            return;
        }
    }
    let offset = ed.cursor.get_untracked().offset();
    let text = ed.doc().text().to_string();
    let word_lo = word_start(&text, offset);
    let prefix = text.get(word_lo..offset).unwrap_or("").to_string();

    let (lo, hi) = statement_range(&text, offset);
    // Context is lexer-based (correct mid-edit); scope prefers the real AST
    // (robust CTE/alias/derived-table resolution), falling back to the lexer.
    let ctx = intel::clause_context(&text, lo, word_lo);
    let qualified = matches!(ctx, ClauseCtx::Qualified(_));

    // FK-aware auto-join: right after a fresh `JOIN … ON `, offer the foreign-key
    // join predicate as a single, ready-to-insert suggestion (DataGrip-style). Only
    // on an empty ON expression (`prefix` empty, in a column/ON context), so it
    // never fights manual typing.
    if prefix.is_empty() && matches!(ctx, ClauseCtx::Column) {
        let catalog = build_catalog(db_nodes, active_db);
        if let Some(pred) = intel::join_condition(&text, lo, hi, offset, &catalog) {
            let mut cpoint = ed.points_of_offset(offset, CursorAffinity::Backward).1;
            cpoint.y += EDITOR_PAD_TOP;
            comp.point.set(cpoint);
            comp.items.set(vec![Suggestion {
                text: pred,
                kind: SuggestKind::Column,
                detail: "foreign key".to_string(),
                pk: false,
            }]);
            comp.sel.set(0);
            comp.open.set(true);
            return;
        }
    }

    // Don't pop the list on every space: an empty prefix only shows suggestions
    // right after a `.` or when explicitly requested (Ctrl+Space).
    if prefix.is_empty() && !qualified && !force {
        comp.open.set(false);
        comp.items.set(Vec::new());
        return;
    }

    let schema = SchemaIndex::build(db_nodes, active_db);
    let scope = intel::statement_scope(&text, lo, hi, offset, SqlDialect::MySql).tables;
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
            tier,
            pk: false,
        });
    };
    // Column candidates carry PK + nullability. `detail` shows the type, suffixed
    // with a dim `· NULL` for nullable columns (NOT NULL stays clean); PK columns
    // get the gold key glyph via `Cand.pk` (and don't repeat the type detail when
    // `type_detail` is false — e.g. the no-FROM pool shows the owning table instead).
    let add_col = |cands: &mut Vec<Cand>,
                   seen: &mut HashSet<String>,
                   c: &ColMeta,
                   detail: String,
                   tier: u8| {
        let tl = c.name.to_ascii_lowercase();
        if tl == pl || !seen.insert(tl) {
            return;
        }
        cands.push(Cand {
            text: c.name.clone(),
            kind: SuggestKind::Column,
            detail,
            tier,
            pk: c.primary_key,
        });
    };
    // The detail string for a column typed against its own metadata.
    let col_type_detail = |c: &ColMeta| -> String {
        if c.nullable {
            format!("{} · NULL", c.type_name)
        } else {
            c.type_name.clone()
        }
    };
    let cols_of = |name: &str| -> Vec<ColMeta> {
        schema
            .columns
            .get(&name.to_ascii_lowercase())
            .cloned()
            .unwrap_or_default()
    };
    // A qualifier resolves to a table via an in-scope alias, else a bare table
    // name (whether or not it's in FROM).
    let resolve = |q: &str| -> Option<String> {
        for r in &scope {
            if r.alias
                .as_deref()
                .is_some_and(|a| a.eq_ignore_ascii_case(q))
            {
                return Some(r.name.clone());
            }
        }
        if schema.columns.contains_key(&q.to_ascii_lowercase()) {
            return Some(q.to_string());
        }
        None
    };

    match &ctx {
        ClauseCtx::Qualified(q) => {
            if let Some(table) = resolve(q) {
                for c in cols_of(&table) {
                    let d = col_type_detail(&c);
                    add_col(&mut cands, &mut seen, &c, d, 0);
                }
            } else if let Some(tbls) = schema.tables_by_db.get(&q.to_ascii_lowercase()) {
                for t in tbls {
                    add(&mut cands, &mut seen, t, SuggestKind::Table, q.clone(), 0);
                }
            }
        }
        ClauseCtx::Table => {
            for (name, db) in &schema.tables {
                add(
                    &mut cands,
                    &mut seen,
                    name,
                    SuggestKind::Table,
                    db.clone(),
                    0,
                );
            }
            for db in &schema.databases {
                add(
                    &mut cands,
                    &mut seen,
                    db,
                    SuggestKind::Database,
                    String::new(),
                    1,
                );
            }
        }
        ClauseCtx::Column => {
            if scope.is_empty() {
                // No FROM yet: offer every column, disambiguated by its table
                // (shown as the detail) so the broader list stays navigable.
                for (name, _) in &schema.tables {
                    for c in cols_of(name) {
                        add_col(&mut cands, &mut seen, &c, name.clone(), 1);
                    }
                }
            } else {
                for r in &scope {
                    for c in cols_of(&r.name) {
                        let d = col_type_detail(&c);
                        add_col(&mut cands, &mut seen, &c, d, 0);
                    }
                }
            }
            for &f in SQL_FUNCTIONS {
                add(
                    &mut cands,
                    &mut seen,
                    f,
                    SuggestKind::Function,
                    String::new(),
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
            for r in &scope {
                for c in cols_of(&r.name) {
                    let d = col_type_detail(&c);
                    add_col(&mut cands, &mut seen, &c, d, 0);
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

    // Score by fuzzy match; sort by tier (context priority), then score, then a
    // shorter candidate. Non-matches drop out.
    let mut scored: Vec<(u8, i32, Cand)> = cands
        .into_iter()
        .filter_map(|c| fuzzy_score(&c.text, &prefix).map(|s| (c.tier, s, c)))
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
            pk: c.pk,
        })
        .collect();

    // `.1` = the point BELOW the caret (line bottom) in editor-area coords, +the
    // editor's top padding (which `points_of_offset` doesn't count).
    let mut cpoint = ed.points_of_offset(offset, CursorAffinity::Backward).1;
    cpoint.y += EDITOR_PAD_TOP;
    comp.point.set(cpoint);
    let open = !items.is_empty();
    comp.items.set(items);
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
    if let Some(word) = comp
        .items
        .with_untracked(|v| v.get(idx).map(|s| s.text.clone()))
    {
        comp.suppress.set(true);
        doc.edit_single(
            Selection::region(start, offset),
            &word,
            EditType::Completion,
        );
        // `edit_single` doesn't move the caret, so place it after the insert.
        let new_offset = start + word.len();
        ed.cursor.update(|c| c.set_offset(new_offset, false, false));
    }
    comp.open.set(false);
    comp.items.set(Vec::new());
}

/// Row text color for a suggestion kind (columns stay neutral; the rest are
/// tinted so the kind reads at a glance).
fn suggest_color(kind: SuggestKind) -> floem::peniko::Color {
    match kind {
        SuggestKind::Keyword => theme::suggest_keyword(),
        SuggestKind::Function => theme::suggest_function(),
        SuggestKind::Table => theme::suggest_table(),
        SuggestKind::Database => theme::suggest_database(),
        SuggestKind::Column => theme::text(),
    }
}

// Floating suggestion list, positioned just below the caret.
pub(crate) fn completion_popup(comp: Completion) -> impl IntoView {
    dyn_container(
        move || (comp.open.get(), comp.items.get(), comp.sel.get()),
        move |(open, items, sel)| {
            if !open || items.is_empty() {
                return empty().into_any();
            }
            let rows = items.into_iter().enumerate().map(move |(i, item)| {
                let selected = i == sel;
                let Suggestion {
                    text: name,
                    kind,
                    detail,
                    pk,
                } = item;
                let color = suggest_color(kind);
                // A gold key glyph marks a primary-key column (same colour as the
                // schema tree); other rows get no leading slot.
                let lead: AnyView = if pk {
                    icons::icon(icons::KEY_ROUND, 12.0)
                        .style(|s| s.color(theme::key_primary()).margin_right(5.0))
                        .into_any()
                } else {
                    empty().into_any()
                };
                // Name (kind-tinted) on the left; the dim detail (a column's type +
                // nullability, a table's database) right-aligned. The selected/
                // hovered background spans the full row width. 14px matches the editor.
                h_stack((
                    lead,
                    text(name).style(move |s| s.font_size(14.0).color(color)),
                    empty().style(|s| s.flex_grow(1.0_f32)),
                    text(detail)
                        .style(|s| s.font_size(12.0).color(theme::text_dim()).margin_left(16.0)),
                ))
                .style(move |s| {
                    let s = s
                        .flex_row()
                        .items_center()
                        .width_full()
                        .padding_horiz(10.0)
                        .padding_vert(5.0)
                        .hover(|s| s.background(theme::completion_active()));
                    if selected {
                        s.background(theme::completion_active())
                    } else {
                        s
                    }
                })
            });
            // The surface (bg #14151A, #373942 outline, rounded) lives on the
            // inner box, and `.clip()` rounds the full-width row highlights to the
            // corners — the outer container only positions (absolute), so clipping
            // here doesn't disturb the anchor.
            v_stack_from_iter(rows)
                .style(|s| {
                    s.flex_col()
                        .width_full()
                        .max_height(260.0)
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
        if comp.open.get() {
            let p = comp.point.get();
            s.absolute()
                .inset_left(COMPLETION_GUTTER + p.x)
                .inset_top(p.y + COMPLETION_LINE_H)
                .min_width(240.0)
                .max_width(460.0)
        } else {
            s
        }
    })
}
