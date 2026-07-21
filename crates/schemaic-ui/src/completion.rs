//! SQL autocomplete: the context-aware suggestion engine and its popup view.
//! `recompute_completions` classifies the caret context (statement start / column
//! / table / `qualifier.` / mixed) from a lightweight token scan built on the
//! shared `skip_noncode` lexer, ranks candidates (schema tables/columns via
//! `SchemaIndex`, plus keyword/function tables) by a fuzzy score within
//! context tiers, and drives the `Completion` state that `completion_popup`
//! renders below the caret. `accept_completion` writes the picked word back into
//! the editor. Only `Completion`/`recompute_completions`/`accept_completion`/
//! `completion_popup` (and `SQL_KEYWORDS`, reused by the editor's typo squiggles)
//! are `pub(crate)`; the rest is internal.

use std::collections::{HashMap, HashSet};

use floem::kurbo::Point;
use floem::prelude::*;
use floem::views::editor::Editor;
use floem::views::editor::core::cursor::CursorAffinity;
use floem::views::editor::core::editor::EditType;
use floem::views::editor::core::selection::Selection;

use schemaic_core::schema::SchemaState;
use schemaic_core::sql::{skip_noncode, statement_range};

use crate::consts::*;
use crate::{ConnNode, theme};

// ===== moved from lib.rs (autocomplete) =====
// ── Autocomplete ────────────────────────────────────────────────────────────

/// Common SQL keywords offered by autocomplete (identifiers come from the
/// introspected schema).
pub(crate) const SQL_KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "AND",
    "OR",
    "NOT",
    "NULL",
    "IS",
    "IN",
    "LIKE",
    "BETWEEN",
    "AS",
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "OUTER",
    "CROSS",
    "ON",
    "USING",
    "GROUP",
    "ORDER",
    "BY",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "DISTINCT",
    "UNION",
    "ALL",
    "EXISTS",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "ASC",
    "DESC",
    // COUNT/SUM/AVG/MIN/MAX intentionally live only in `SQL_FUNCTIONS` (they're
    // functions, not keywords) — deduped to remove the tier ambiguity (§7.5).
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE",
    "TABLE",
    "VIEW",
    "INDEX",
    "ALTER",
    "DROP",
    "TRUNCATE",
    "PRIMARY",
    "KEY",
    "FOREIGN",
    "REFERENCES",
    "DEFAULT",
    "AUTO_INCREMENT",
    "UNIQUE",
];

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

/// One ranked autocomplete row: the text inserted, its kind, and a dim detail
/// (a column's type, or a table's database).
#[derive(Clone)]
pub(crate) struct Suggestion {
    text: String,
    kind: SuggestKind,
    detail: String,
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

/// SQL functions offered in value/column position (kind [`SuggestKind::Function`]).
/// `pub(crate)` so the editor's typo-squiggle checker can treat them as known
/// words (they're no longer in `SQL_KEYWORDS`).
pub(crate) const SQL_FUNCTIONS: &[&str] = &[
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "COALESCE",
    "IFNULL",
    "NULLIF",
    "CONCAT",
    "CONCAT_WS",
    "GROUP_CONCAT",
    "LENGTH",
    "CHAR_LENGTH",
    "LOWER",
    "UPPER",
    "TRIM",
    "LTRIM",
    "RTRIM",
    "SUBSTRING",
    "REPLACE",
    "ROUND",
    "FLOOR",
    "CEIL",
    "ABS",
    "MOD",
    "NOW",
    "CURDATE",
    "CURTIME",
    "DATE",
    "YEAR",
    "MONTH",
    "DAY",
    "HOUR",
    "DATE_FORMAT",
    "DATEDIFF",
    "CAST",
    "CONVERT",
    "IF",
    "GREATEST",
    "LEAST",
];

/// Keywords that begin a statement (offered at statement start).
const STMT_KEYWORDS: &[&str] = &[
    "SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER", "TRUNCATE", "WITH", "SHOW",
    "EXPLAIN", "DESCRIBE", "USE", "REPLACE", "CALL",
];

/// Case-insensitive membership in the SQL keyword set (used to reject a keyword
/// as an implicit table alias).
fn is_sql_keyword(word: &str) -> bool {
    let up = word.to_ascii_uppercase();
    SQL_KEYWORDS.iter().any(|k| *k == up)
}

/// A schema view built once per recompute: which databases/tables exist and each
/// table's columns, all indexed case-insensitively. Columns of same-named tables
/// across databases are merged (dedup by column name).
struct SchemaIndex {
    databases: Vec<String>,
    /// (table name, database it lives in).
    tables: Vec<(String, String)>,
    /// table name (lowercase) → its columns `(name, type)`.
    columns: HashMap<String, Vec<(String, String)>>,
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
        let mut columns: HashMap<String, Vec<(String, String)>> = HashMap::new();
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
                            if !entry.iter().any(|(n, _)| n.eq_ignore_ascii_case(&c.name)) {
                                entry.push((c.name.clone(), c.type_name.clone()));
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

/// A table reference parsed from a statement's FROM/JOIN/UPDATE/INTO clause.
struct TableRef {
    alias: Option<String>,
    name: String,
}

/// Lightweight SQL token used by the context analysis (words + the punctuation
/// that matters for it). Strings and comments are skipped by the tokenizer.
#[derive(Clone)]
enum TkKind {
    Word(String),
    Dot,
    Comma,
    LParen,
    RParen,
}

/// A token plus its absolute byte offset in `sql` (offsets drive the caret's
/// paren-scope lookup).
struct Token {
    at: usize,
    kind: TkKind,
}

/// Tokenize `sql[lo..hi]` into words + `. , ( )`, skipping string literals,
/// backtick identifiers, and comments via the shared [`skip_noncode`] primitive
/// (so it agrees with the statement splitter / WHERE guard on those boundaries).
fn tokenize_range(sql: &str, lo: usize, hi: usize) -> Vec<Token> {
    let b = sql.as_bytes();
    let mut out = Vec::new();
    let mut i = lo;
    let push = |out: &mut Vec<Token>, at: usize, kind: TkKind| out.push(Token { at, kind });
    while i < hi {
        if let Some(j) = skip_noncode(b, i) {
            // A construct can run to the buffer end (unterminated string); clamp
            // to this range's `hi`. `skip_noncode` always advances past `i`.
            i = j.min(hi);
            continue;
        }
        let c = b[i];
        // A word starts on an ASCII letter/`_` or any non-ASCII (UTF-8) byte, and
        // runs over word bytes — so Unicode identifiers tokenize whole (review B6).
        if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 {
            let s = i;
            let mut j = i + 1;
            while j < hi && is_word_byte(b[j]) {
                j += 1;
            }
            push(&mut out, s, TkKind::Word(sql[s..j].to_string()));
            i = j;
            continue;
        }
        match c {
            b'.' => push(&mut out, i, TkKind::Dot),
            b',' => push(&mut out, i, TkKind::Comma),
            b'(' => push(&mut out, i, TkKind::LParen),
            b')' => push(&mut out, i, TkKind::RParen),
            _ => {}
        }
        i += 1;
    }
    out
}

/// The set of paren-scope ids open at `caret` (each `(` is numbered by encounter
/// order; `0` is the top level and is always included). A table declared in one
/// of these scopes — the caret's own query or an enclosing one — is visible;
/// tables in sibling/deeper subqueries are not. Same numbering as
/// [`tables_in_scope`], so the ids line up.
fn caret_scope_chain(toks: &[Token], caret: usize) -> HashSet<usize> {
    let mut next_id = 1usize;
    let mut open: Vec<usize> = Vec::new();
    for t in toks {
        if t.at >= caret {
            break;
        }
        match t.kind {
            TkKind::LParen => {
                open.push(next_id);
                next_id += 1;
            }
            TkKind::RParen => {
                open.pop();
            }
            _ => {}
        }
    }
    let mut chain: HashSet<usize> = open.into_iter().collect();
    chain.insert(0);
    chain
}

/// Parse the tables (and aliases) visible at `caret` — those in the caret's
/// query scope or an enclosing one (correlation), not sibling/inner subqueries.
/// Handles `db.table`, `AS alias`, implicit `table alias`, and comma FROM-lists.
fn tables_in_scope(sql: &str, lo: usize, hi: usize, caret: usize) -> Vec<TableRef> {
    let toks = tokenize_range(sql, lo, hi);
    let chain = caret_scope_chain(&toks, caret);
    let word = |k: &TkKind| -> Option<String> {
        if let TkKind::Word(w) = k {
            Some(w.clone())
        } else {
            None
        }
    };
    let mut out = Vec::new();
    let mut next_id = 1usize;
    let mut open: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        match &toks[i].kind {
            TkKind::LParen => {
                open.push(next_id);
                next_id += 1;
                i += 1;
                continue;
            }
            TkKind::RParen => {
                open.pop();
                i += 1;
                continue;
            }
            TkKind::Word(w) => {
                let up = w.to_ascii_uppercase();
                let is_from = up == "FROM";
                if !matches!(up.as_str(), "FROM" | "JOIN" | "INTO" | "UPDATE") {
                    i += 1;
                    continue;
                }
                let scope = *open.last().unwrap_or(&0);
                i += 1;
                // A table name: `word` or `word . word` (db.table → table part).
                while let Some(mut name) = toks.get(i).and_then(|t| word(&t.kind)) {
                    // A keyword here (e.g. `FROM (SELECT…`) isn't a table name.
                    if is_sql_keyword(&name) {
                        break;
                    }
                    i += 1;
                    if matches!(toks.get(i).map(|t| &t.kind), Some(TkKind::Dot))
                        && let Some(second) = toks.get(i + 1).and_then(|t| word(&t.kind)) {
                            name = second;
                            i += 2;
                        }
                    // Optional alias: `AS x` or a bare non-keyword word.
                    let mut alias = None;
                    match toks.get(i).map(|t| &t.kind) {
                        Some(TkKind::Word(a)) if a.eq_ignore_ascii_case("AS") => {
                            if let Some(al) = toks.get(i + 1).and_then(|t| word(&t.kind)) {
                                alias = Some(al);
                                i += 2;
                            }
                        }
                        Some(TkKind::Word(a)) if !is_sql_keyword(a) => {
                            alias = Some(a.clone());
                            i += 1;
                        }
                        _ => {}
                    }
                    if chain.contains(&scope) {
                        out.push(TableRef { alias, name });
                    }
                    // FROM allows a comma list of tables; otherwise one per intro.
                    if is_from && matches!(toks.get(i).map(|t| &t.kind), Some(TkKind::Comma)) {
                        i += 1;
                        continue;
                    }
                    break;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    out
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
}

/// What kind of token is expected at the caret, deciding which suggestions to
/// rank first.
enum CompCtx {
    /// Start of a statement → statement keywords.
    Start,
    /// After SELECT / WHERE / ON / SET / … → columns, functions, keywords.
    Column,
    /// After FROM / JOIN / UPDATE / INTO → tables, databases.
    Table,
    /// Right after `qualifier.` → that table's columns (or that db's tables).
    Qualified(String),
    /// Anything else → the full mixed list (keywords + tables + columns).
    Other,
}

/// Classify the caret context from the statement `sql[lo..hi]`. `word_lo` is the
/// byte offset where the caret's current word begins (so we look only at tokens
/// before it).
fn completion_context(sql: &str, lo: usize, _hi: usize, word_lo: usize) -> CompCtx {
    // Qualified reference: the char just before the word is a `.`.
    if word_lo > lo && sql.as_bytes()[word_lo - 1] == b'.' {
        let q_start = word_start(sql, word_lo - 1);
        let qualifier = sql.get(q_start..word_lo - 1).unwrap_or("").to_string();
        if !qualifier.is_empty() {
            return CompCtx::Qualified(qualifier);
        }
    }
    // The last clause keyword strictly before the caret's word decides the rest.
    let toks = tokenize_range(sql, lo, word_lo);
    let mut last_kw: Option<String> = None;
    for t in &toks {
        if let TkKind::Word(w) = &t.kind {
            let up = w.to_ascii_uppercase();
            if CLAUSE_KEYWORDS.contains(&up.as_str()) {
                last_kw = Some(up);
            }
        }
    }
    match last_kw.as_deref() {
        None => {
            // No clause keyword yet: statement start if there are no words at all,
            // otherwise a mixed context.
            if toks.iter().any(|t| matches!(t.kind, TkKind::Word(_))) {
                CompCtx::Other
            } else {
                CompCtx::Start
            }
        }
        Some("FROM" | "JOIN" | "INTO" | "UPDATE" | "TABLE" | "TRUNCATE" | "DESCRIBE") => {
            CompCtx::Table
        }
        Some(
            "SELECT" | "WHERE" | "ON" | "AND" | "OR" | "HAVING" | "SET" | "USING" | "BY" | "GROUP"
            | "ORDER" | "DISTINCT" | "WHEN" | "THEN" | "ELSE",
        ) => CompCtx::Column,
        _ => CompCtx::Other,
    }
}

/// Clause keywords that determine the completion context (see
/// [`completion_context`]).
const CLAUSE_KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "JOIN", "ON", "AND", "OR", "HAVING", "SET", "USING", "BY", "GROUP",
    "ORDER", "DISTINCT", "INTO", "UPDATE", "TABLE", "TRUNCATE", "DESCRIBE", "VALUES", "LIMIT",
    "OFFSET", "WHEN", "THEN", "ELSE",
];

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
    let ctx = completion_context(&text, lo, hi, word_lo);
    let qualified = matches!(ctx, CompCtx::Qualified(_));

    // Don't pop the list on every space: an empty prefix only shows suggestions
    // right after a `.` or when explicitly requested (Ctrl+Space).
    if prefix.is_empty() && !qualified && !force {
        comp.open.set(false);
        comp.items.set(Vec::new());
        return;
    }

    let schema = SchemaIndex::build(db_nodes, active_db);
    let scope = tables_in_scope(&text, lo, hi, offset);
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
        });
    };
    let cols_of = |name: &str| -> Vec<(String, String)> {
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
        CompCtx::Qualified(q) => {
            if let Some(table) = resolve(q) {
                for (n, ty) in cols_of(&table) {
                    add(&mut cands, &mut seen, &n, SuggestKind::Column, ty, 0);
                }
            } else if let Some(tbls) = schema.tables_by_db.get(&q.to_ascii_lowercase()) {
                for t in tbls {
                    add(&mut cands, &mut seen, t, SuggestKind::Table, q.clone(), 0);
                }
            }
        }
        CompCtx::Table => {
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
        CompCtx::Column => {
            if scope.is_empty() {
                // No FROM yet: offer every column, disambiguated by its table
                // (shown as the detail) so the broader list stays navigable.
                for (name, _) in &schema.tables {
                    for (n, _) in cols_of(name) {
                        add(
                            &mut cands,
                            &mut seen,
                            &n,
                            SuggestKind::Column,
                            name.clone(),
                            1,
                        );
                    }
                }
            } else {
                for r in &scope {
                    for (n, ty) in cols_of(&r.name) {
                        add(&mut cands, &mut seen, &n, SuggestKind::Column, ty, 0);
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
        CompCtx::Start => {
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
        CompCtx::Other => {
            for r in &scope {
                for (n, ty) in cols_of(&r.name) {
                    add(&mut cands, &mut seen, &n, SuggestKind::Column, ty, 0);
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
                } = item;
                let color = suggest_color(kind);
                // Name (kind-tinted) on the left; the dim detail (a column's type,
                // a table's database) right-aligned. The selected/hovered
                // background spans the full row width. 14px matches the editor.
                h_stack((
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
