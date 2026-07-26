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
use schemaic_core::schema::{DbSchema, SchemaState, classify_column_type};
use schemaic_core::sql::statement_range;

use crate::schema_tree::column_type_icon;

// The keyword/function sets now live in `schemaic_core::intel` (so the core
// analysis + diagnostics share one authoritative copy); used here to seed the
// suggestion pool.
use schemaic_core::intel::{SQL_FUNCTIONS, SQL_KEYWORDS, STMT_KEYWORDS};

use floem::AnyView;

use crate::consts::*;
use crate::widgets::autohide;
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
    columns_by_db: HashMap<(String, String), Vec<ColMeta>>,
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
    fn build(db_nodes: RwSignal<Vec<ConnNode>>, active_db: Option<&str>) -> SchemaIndex {
        let mut databases = Vec::new();
        let mut tables = Vec::new();
        let mut columns: HashMap<String, Vec<ColMeta>> = HashMap::new();
        let mut columns_by_db: HashMap<(String, String), Vec<ColMeta>> = HashMap::new();
        let mut tables_by_db: HashMap<String, Vec<String>> = HashMap::new();
        for node in db_nodes.get_untracked() {
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
                    // Columns covered by a foreign key (case-folded) → the FK tint.
                    let fk_cols: HashSet<String> = t
                        .foreign_keys
                        .iter()
                        .flat_map(|fk| fk.columns.iter())
                        .map(|c| c.to_ascii_lowercase())
                        .collect();
                    let metas: Vec<ColMeta> = t
                        .columns
                        .iter()
                        .map(|c| ColMeta {
                            name: c.name.clone(),
                            type_name: c.type_name.clone(),
                            nullable: c.nullable,
                            primary_key: c.primary_key,
                            foreign_key: fk_cols.contains(&c.name.to_ascii_lowercase()),
                        })
                        .collect();
                    // Every database's columns are keyed by (db, table) for qualified
                    // (incl. cross-database) completion.
                    columns_by_db.insert(
                        (db_lower.clone(), t.name.to_ascii_lowercase()),
                        metas.clone(),
                    );
                    if in_scope {
                        tables.push((t.name.clone(), node.database.clone()));
                        let entry = columns.entry(t.name.to_ascii_lowercase()).or_default();
                        for m in metas {
                            if !entry.iter().any(|e| e.name.eq_ignore_ascii_case(&m.name)) {
                                entry.push(m);
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
        if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 {
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
    let offset = ed.cursor.get_untracked().offset();
    let text = ed.doc().text().to_string();
    // A `.` just typed is a qualifier trigger — reveal the qualifier's members even
    // right after accepting it (otherwise `suppress`, set by the accept, would swallow
    // the very next keystroke and the popup wouldn't reopen until another char).
    let after_dot = offset > 0 && text.as_bytes().get(offset - 1) == Some(&b'.');
    if comp.suppress.get_untracked() {
        comp.suppress.set(false);
        if !force && !after_dot {
            comp.open.set(false);
            comp.items.set(Vec::new());
            return;
        }
    }
    let word_lo = word_start(&text, offset);
    let prefix = text.get(word_lo..offset).unwrap_or("").to_string();

    let (lo, hi) = statement_range(&text, offset);
    // Context is lexer-based (correct mid-edit); scope prefers the real AST
    // (robust CTE/alias/derived-table resolution), falling back to the lexer.
    let ctx = intel::clause_context(&text, lo, word_lo);
    let qualified = matches!(ctx, ClauseCtx::Qualified(_));
    // Expected next keyword/phrase continuations from SQL clause grammar (the
    // `WHERE` after a complete table ref, `FROM` after the projection, `GROUP BY`
    // as one item). These seed the top suggestion tier; `auto_show` opens the popup
    // on an empty prefix right after an operand-taking clause keyword.
    let cont = intel::clause_continuation(&text, lo, word_lo);

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
                table: String::new(),
                alias: String::new(),
                // A purple key-square marks the ready-to-insert FK join predicate.
                icon: icons::KEY_SQUARE,
                key: KeyKind::Foreign,
                insert: None,
                replace: None,
            }]);
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
                .cloned()
                .unwrap_or_default(),
            None => schema
                .columns
                .get(&name.to_ascii_lowercase())
                .cloned()
                .unwrap_or_default(),
        }
    };
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
            for jt in intel::join_targets(&text, lo, hi, offset, &catalog) {
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
                    insert: Some(format!("{} ON {}", jt.table, jt.predicate)),
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
    if let Some(exp) = star_expansion(&text, lo, hi, offset, db_nodes, active_db) {
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
            let followed_by_paren = text[offset..].trim_start().starts_with('(');
            completion_insertion(&word, kind == SuggestKind::Function, followed_by_paren)
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
    comp.items.set(Vec::new());
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
    intel::expand_star(text, lo, hi, offset, &catalog)
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
    }
}

// Floating suggestion list, positioned just below the caret.
pub(crate) fn completion_popup(comp: Completion) -> impl IntoView {
    dyn_container(
        // Keyed on open/items only — NOT `sel`. The selection highlight reads
        // `comp.sel` reactively per row (below), so moving the selection repaints in
        // place instead of rebuilding the list (which would reset the scroll offset).
        move || (comp.open.get(), comp.items.get()),
        move |(open, items)| {
            if !open || items.is_empty() {
                return empty().into_any();
            }
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
                    let lead: AnyView = icons::icon(icon, 13.0)
                        .style(move |s| {
                            s.color(suggest_icon_color(kind, key))
                                .margin_right(7.0)
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
                        let label = if alias.is_empty() {
                            table
                        } else {
                            format!("{table} {alias}")
                        };
                        text(label)
                            .style(|s| s.font_size(12.0).color(theme::text_dim()))
                            .into_any()
                    };
                    // Name (kind-tinted) on the left; annotations right-aligned. The
                    // selected/hovered background spans the full row width. 14px
                    // matches the editor.
                    h_stack((
                        lead,
                        text(name).style(move |s| s.font_size(14.0).color(color)),
                        empty().style(|s| s.flex_grow(1.0_f32).min_width(24.0)),
                        table_ref,
                        text(detail).style(|s| {
                            s.font_size(12.0)
                                .color(theme::text_muted())
                                .margin_left(18.0)
                        }),
                    ))
                    .style(move |s| {
                        let s = s
                            .flex_row()
                            .items_center()
                            .width_full()
                            .padding_horiz(10.0)
                            .padding_vert(5.0)
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
            let list = v_stack_from_iter(rows).style(|s| s.flex_col().width_full());
            // The scroll makes an overflowing list navigable by wheel; `scroll_to_view`
            // keeps the keyboard-selected row visible. `autohide` gives it the shared
            // thin, auto-hiding scrollbar (same as the schema tree / history / etc.).
            // The surface (bg #14151A, #373942 outline, rounded) + `.clip()` live on
            // the wrapping container so the full-width row highlights round to the
            // corners.
            container(
                autohide(scroll(list).scroll_to_view(move || row_ids.get(comp.sel.get()).copied()))
                    .style(|s| s.width_full().max_height(260.0)),
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
        // A high z-index lifts the popup above the results pane below it: the popup
        // overflows the (unclipped) editor pane and, without this, the later-painted
        // results grid draws over it (paint order = tree order). z-index gives the
        // vger renderer a global ordering so the popup composites last. Set
        // unconditionally so it applies whenever the popup is shown.
        let s = s.z_index(1000);
        if comp.open.get() {
            let p = comp.point.get();
            s.absolute()
                .inset_left(COMPLETION_GUTTER + p.x)
                .inset_top(p.y + COMPLETION_LINE_H)
                .min_width(320.0)
                .max_width(640.0)
        } else {
            s
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        SuggestKind, completion_insertion, database_suggestion_visible, recency_bonus,
        statement_identifiers,
    };
    use std::collections::HashSet;

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
    fn non_function_completion_is_verbatim() {
        let (s, c) = completion_insertion("orders", false, false);
        assert_eq!(s, "orders");
        assert_eq!(c, 6);
    }
}
