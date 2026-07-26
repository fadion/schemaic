//! SQL intelligence: the structure-aware layer over a real per-dialect AST.
//!
//! This is the bounded module that turns *cursor + catalog* into structured
//! facts — scope resolution (which tables/aliases/CTEs are visible), completion
//! context, and catalog-aware diagnostics. It sits on two engines by design:
//!
//! - **`sqlparser`** (per-dialect AST) parses *complete, valid* statements and
//!   answers structural questions the token stream can't (this ident is a table
//!   ref in FROM vs a column in SELECT; these are the CTE names in scope).
//! - **The `skip_noncode` lexer** ([`crate::sql`]) stays the source of *byte
//!   positions* (squiggle placement, word boundaries) and the *mid-edit
//!   fallback*: sqlparser isn't error-tolerant, so while a statement is being
//!   typed (and doesn't parse) we degrade to the lexer heuristics rather than
//!   losing all structure.
//!
//! The live database remains the semantic authority — offline diagnostics here
//! cover the instant, catalog-only cases (unknown table/column); dialect-exact
//! validation via PREPARE/EXPLAIN is a later, additive tier.

use sqlparser::dialect::{Dialect, MySqlDialect};

use crate::sql::skip_noncode;

/// Which SQL dialect a connection speaks. Only [`SqlDialect::MySql`] is wired
/// today; Postgres/SQLite are future arms — the point of the seam is that adding
/// them is a dialect swap, not a rewrite (sqlparser already ships those dialects).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SqlDialect {
    #[default]
    MySql,
}

impl SqlDialect {
    /// The `sqlparser` dialect backing this connection kind.
    pub(crate) fn parser(self) -> Box<dyn Dialect> {
        match self {
            SqlDialect::MySql => Box::new(MySqlDialect {}),
        }
    }
}

// ── Keyword data ─────────────────────────────────────────────────────────────
// The single home for the completion/analysis keyword sets (previously in the UI
// crate's `completion.rs`, which core can't depend on). `sql_highlight`'s coloring
// list is intentionally separate — it's a broader set that also tints data-type
// and DDL words, a different role from suggestion/analysis.

/// Common SQL keywords offered by autocomplete (identifiers come from the
/// introspected schema).
pub const SQL_KEYWORDS: &[&str] = &[
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
    // functions, not keywords) — deduped to remove the tier ambiguity.
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

/// SQL functions offered in value/column position. Kept distinct from keywords so
/// the editor's typo checker treats them as known words.
pub const SQL_FUNCTIONS: &[&str] = &[
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
pub const STMT_KEYWORDS: &[&str] = &[
    "SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "ALTER", "TRUNCATE", "WITH", "SHOW",
    "EXPLAIN", "DESCRIBE", "USE", "REPLACE", "CALL",
];

/// Clause keywords that determine the completion context (see [`clause_context`]).
const CLAUSE_KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "JOIN", "ON", "AND", "OR", "HAVING", "SET", "USING", "BY", "GROUP",
    "ORDER", "DISTINCT", "INTO", "UPDATE", "TABLE", "TRUNCATE", "DESCRIBE", "VALUES", "LIMIT",
    "OFFSET", "WHEN", "THEN", "ELSE",
];

/// Case-insensitive membership in the SQL keyword set (used to reject a keyword
/// as an implicit table alias).
pub fn is_sql_keyword(word: &str) -> bool {
    let up = word.to_ascii_uppercase();
    SQL_KEYWORDS.iter().any(|k| *k == up)
}

// ── Byte-position lexer (word/punctuation scan over `skip_noncode`) ───────────
// This is the *positional* engine: it agrees with the statement splitter / WHERE
// guard on string/comment/backtick boundaries and yields absolute byte offsets,
// which the AST (whose spans are still maturing) can't reliably provide.

pub(crate) fn is_word_byte(b: u8) -> bool {
    // `>= 0x80` = any UTF-8 lead/continuation byte, so Unicode identifiers count
    // as one word instead of splitting at the first non-ASCII byte.
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
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

/// A token plus its absolute byte offset in `sql`.
struct Token {
    at: usize,
    kind: TkKind,
}

/// Tokenize `sql[lo..hi]` into words + `. , ( )`, skipping string literals,
/// backtick identifiers, and comments via the shared [`skip_noncode`] primitive.
fn tokenize_range(sql: &str, lo: usize, hi: usize) -> Vec<Token> {
    let b = sql.as_bytes();
    let mut out = Vec::new();
    let mut i = lo;
    let push = |out: &mut Vec<Token>, at: usize, kind: TkKind| out.push(Token { at, kind });
    while i < hi {
        if let Some(j) = skip_noncode(b, i) {
            i = j.min(hi);
            continue;
        }
        let c = b[i];
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
/// order; `0` is the top level and always included).
fn caret_scope_chain(toks: &[Token], caret: usize) -> std::collections::HashSet<usize> {
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
    let mut chain: std::collections::HashSet<usize> = open.into_iter().collect();
    chain.insert(0);
    chain
}

// ── Scope + context ──────────────────────────────────────────────────────────

/// A table reference visible in a statement's scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRef {
    pub name: String,
    pub alias: Option<String>,
    /// Qualifying database, if written `db.table`.
    pub db: Option<String>,
}

/// The tables and CTE names in scope at a caret position.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    pub tables: Vec<TableRef>,
    pub ctes: Vec<String>,
}

/// What kind of token is expected at the caret, deciding which suggestions to
/// rank first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClauseCtx {
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

/// Byte offset where the identifier ending at `offset` begins.
pub fn word_start(text: &str, offset: usize) -> usize {
    let bytes = text.as_bytes();
    let mut start = offset.min(text.len());
    while start > 0 && is_word_byte(bytes[start - 1]) {
        start -= 1;
    }
    start
}

/// Classify the caret context from the statement `sql[lo..hi]`. `word_lo` is the
/// byte offset where the caret's current word begins. Lexer-based, so it stays
/// correct mid-edit (when the statement doesn't parse).
pub fn clause_context(sql: &str, lo: usize, word_lo: usize) -> ClauseCtx {
    // Qualified reference: the char just before the word is a `.`.
    if word_lo > lo && sql.as_bytes()[word_lo - 1] == b'.' {
        let q_start = word_start(sql, word_lo - 1);
        let qualifier = sql.get(q_start..word_lo - 1).unwrap_or("").to_string();
        if !qualifier.is_empty() {
            return ClauseCtx::Qualified(qualifier);
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
            if toks.iter().any(|t| matches!(t.kind, TkKind::Word(_))) {
                ClauseCtx::Other
            } else {
                ClauseCtx::Start
            }
        }
        Some("FROM" | "JOIN" | "INTO" | "UPDATE" | "TABLE" | "TRUNCATE" | "DESCRIBE") => {
            ClauseCtx::Table
        }
        Some(
            "SELECT" | "WHERE" | "ON" | "AND" | "OR" | "HAVING" | "SET" | "USING" | "BY" | "GROUP"
            | "ORDER" | "DISTINCT" | "WHEN" | "THEN" | "ELSE",
        ) => ClauseCtx::Column,
        _ => ClauseCtx::Other,
    }
}

/// Parse the tables (and aliases) visible at `caret` using the byte-position
/// lexer + paren-scope chain — the fallback used when the statement doesn't parse
/// as a complete AST. Handles `db.table`, `AS alias`, implicit `table alias`, and
/// comma FROM-lists; scopes to the caret's query or an enclosing one.
fn lexer_scope(sql: &str, lo: usize, hi: usize, caret: usize) -> Vec<TableRef> {
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
                while let Some(mut name) = toks.get(i).and_then(|t| word(&t.kind)) {
                    if is_sql_keyword(&name) {
                        break;
                    }
                    let mut db = None;
                    i += 1;
                    if matches!(toks.get(i).map(|t| &t.kind), Some(TkKind::Dot))
                        && let Some(second) = toks.get(i + 1).and_then(|t| word(&t.kind))
                    {
                        db = Some(name);
                        name = second;
                        i += 2;
                    }
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
                        out.push(TableRef { name, alias, db });
                    }
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

/// The tables + CTE names visible at `caret` within the statement `sql[lo..hi]`.
///
/// Uses the real AST when the statement parses (robust alias/CTE/`db.table`/
/// derived-table resolution the paren hack got wrong); falls back to the
/// byte-position lexer scope when it doesn't (mid-edit). AST resolution returns
/// the union of table refs across the statement — a superset is the safe
/// direction for both completion (extra candidates rank low) and diagnostics
/// (never a false "unknown column").
pub fn statement_scope(
    sql: &str,
    lo: usize,
    hi: usize,
    caret: usize,
    dialect: SqlDialect,
) -> Scope {
    let stmt = sql.get(lo..hi).unwrap_or("");
    if let Ok(mut asts) = sqlparser::parser::Parser::parse_sql(&*dialect.parser(), stmt)
        && asts.len() == 1
    {
        let mut scope = Scope::default();
        ast_scope::collect_statement(&asts.pop().unwrap(), &mut scope);
        if !scope.tables.is_empty() || !scope.ctes.is_empty() {
            return scope;
        }
    }
    Scope {
        tables: lexer_scope(sql, lo, hi, caret),
        ctes: Vec::new(),
    }
}

/// AST-walk helpers: collect the table refs + CTE names from a parsed statement.
/// Recurses into subqueries/CTE bodies (union of all refs).
mod ast_scope {
    use super::{Scope, TableRef};
    use sqlparser::ast::{
        Cte, FromTable, Query, Select, SetExpr, Statement, TableFactor, TableWithJoins,
    };

    pub(super) fn collect_statement(stmt: &Statement, out: &mut Scope) {
        match stmt {
            Statement::Query(q) => collect_query(q, out),
            Statement::Insert(insert) => {
                push_object_name(&insert.table.to_string(), None, out);
                if let Some(src) = &insert.source {
                    collect_query(src, out);
                }
            }
            Statement::Update(u) => {
                // The target table (and any `UPDATE t JOIN …`) — the FROM-source
                // variants are dialect-specific and not needed for scope here.
                collect_twj(&u.table, out);
            }
            Statement::Delete(del) => {
                let tables = match &del.from {
                    FromTable::WithFromKeyword(v) | FromTable::WithoutKeyword(v) => v,
                };
                for twj in tables {
                    collect_twj(twj, out);
                }
            }
            _ => {}
        }
    }

    fn collect_query(q: &Query, out: &mut Scope) {
        if let Some(with) = &q.with {
            for cte in &with.cte_tables {
                collect_cte(cte, out);
            }
        }
        collect_setexpr(&q.body, out);
    }

    fn collect_cte(cte: &Cte, out: &mut Scope) {
        out.ctes.push(cte.alias.name.value.clone());
        collect_query(&cte.query, out);
    }

    fn collect_setexpr(body: &SetExpr, out: &mut Scope) {
        match body {
            SetExpr::Select(sel) => collect_select(sel, out),
            SetExpr::Query(q) => collect_query(q, out),
            SetExpr::SetOperation { left, right, .. } => {
                collect_setexpr(left, out);
                collect_setexpr(right, out);
            }
            _ => {}
        }
    }

    fn collect_select(sel: &Select, out: &mut Scope) {
        for twj in &sel.from {
            collect_twj(twj, out);
        }
    }

    fn collect_twj(twj: &TableWithJoins, out: &mut Scope) {
        collect_factor(&twj.relation, out);
        for join in &twj.joins {
            collect_factor(&join.relation, out);
        }
    }

    fn collect_factor(factor: &TableFactor, out: &mut Scope) {
        match factor {
            TableFactor::Table { name, alias, .. } => {
                let parts: Vec<String> = name.0.iter().map(|p| p.to_string()).collect();
                let (db, table) = match parts.as_slice() {
                    [t] => (None, t.clone()),
                    [d, t] => (Some(d.clone()), t.clone()),
                    // db.schema.table etc. → last is the table, prior is its db.
                    [.., d, t] => (Some(d.clone()), t.clone()),
                    [] => return,
                };
                let alias = alias.as_ref().map(|a| a.name.value.clone());
                push_ref(
                    TableRef {
                        name: table,
                        alias,
                        db,
                    },
                    out,
                );
            }
            TableFactor::Derived {
                subquery, alias, ..
            } => {
                // A derived table's columns aren't in the catalog; keep recursing
                // so the inner real tables still resolve. The alias itself has no
                // catalog columns, so we don't push it as a TableRef.
                let _ = alias;
                collect_query(subquery, out);
            }
            TableFactor::NestedJoin {
                table_with_joins, ..
            } => collect_twj(table_with_joins, out),
            _ => {}
        }
    }

    fn push_object_name(name: &str, alias: Option<String>, out: &mut Scope) {
        let table = name.rsplit('.').next().unwrap_or(name).trim_matches('`');
        let db = name
            .rsplit_once('.')
            .map(|(d, _)| d.trim_matches('`').to_string());
        push_ref(
            TableRef {
                name: table.to_string(),
                alias,
                db,
            },
            out,
        );
    }

    fn push_ref(r: TableRef, out: &mut Scope) {
        if !out
            .tables
            .iter()
            .any(|e| e.name.eq_ignore_ascii_case(&r.name) && e.alias == r.alias)
        {
            out.tables.push(r);
        }
    }
}

// ── Catalog ──────────────────────────────────────────────────────────────────

use std::collections::{HashMap, HashSet};

use crate::schema::DbSchema;

/// Whether a referenced table can be judged against the catalog.
enum TableStatus {
    /// The table exists in the (loaded) catalog.
    Found,
    /// The relevant database is loaded and does not contain the table.
    NotFound,
    /// The relevant database's schema isn't loaded — we can't judge (no squiggle).
    Unknown,
}

/// A case-folded view over the introspected schema, answering the existence /
/// column questions offline diagnostics need. Built from the *loaded* database
/// schemas plus the tab's active database (which scopes unqualified references).
pub struct Catalog {
    /// (db_lower, table_lower) → column names (original case).
    qualified: HashMap<(String, String), Vec<String>>,
    /// table_lower → column names, for unqualified refs (active-db scoped).
    unqualified: HashMap<String, Vec<String>>,
    /// table_lower → its foreign-key edges (active-db scoped), for the FK-aware
    /// `JOIN … ON` completion.
    fks: HashMap<String, Vec<FkEdge>>,
    /// Loaded database names (lower).
    loaded_dbs: HashSet<String>,
    /// The active database (lower), if any.
    active_db: Option<String>,
    /// All known identifier names (lower) — dbs + tables + columns — used to keep
    /// the keyword-typo check from flagging real schema names.
    known_idents: HashSet<String>,
}

/// A foreign-key edge (this table's `columns` reference `ref_table`'s
/// `ref_columns`, aligned by position).
#[derive(Clone)]
struct FkEdge {
    columns: Vec<String>,
    ref_table: String,
    ref_columns: Vec<String>,
}

impl Catalog {
    /// Build from the loaded `(database, schema)` pairs and the active database.
    /// Only databases whose introspection has completed should be passed — an
    /// absent database is treated as "can't judge" rather than "not found".
    pub fn build(loaded: &[(&str, &DbSchema)], active_db: Option<&str>) -> Catalog {
        let mut qualified = HashMap::new();
        let mut unqualified: HashMap<String, Vec<String>> = HashMap::new();
        let mut fks: HashMap<String, Vec<FkEdge>> = HashMap::new();
        let mut loaded_dbs = HashSet::new();
        let mut known_idents = HashSet::new();
        let active_lower = active_db.map(|d| d.to_ascii_lowercase());
        for (db, schema) in loaded {
            let db_lower = db.to_ascii_lowercase();
            loaded_dbs.insert(db_lower.clone());
            known_idents.insert(db_lower.clone());
            let in_scope = active_lower
                .as_deref()
                .is_none_or(|a| a == db_lower.as_str());
            for t in &schema.tables {
                let cols: Vec<String> = t.columns.iter().map(|c| c.name.clone()).collect();
                known_idents.insert(t.name.to_ascii_lowercase());
                for c in &cols {
                    known_idents.insert(c.to_ascii_lowercase());
                }
                qualified.insert(
                    (db_lower.clone(), t.name.to_ascii_lowercase()),
                    cols.clone(),
                );
                if in_scope {
                    let entry = unqualified.entry(t.name.to_ascii_lowercase()).or_default();
                    for c in cols {
                        if !entry.iter().any(|e| e.eq_ignore_ascii_case(&c)) {
                            entry.push(c);
                        }
                    }
                    if !t.foreign_keys.is_empty() {
                        fks.entry(t.name.to_ascii_lowercase()).or_default().extend(
                            t.foreign_keys.iter().map(|fk| FkEdge {
                                columns: fk.columns.clone(),
                                ref_table: fk.ref_table.clone(),
                                ref_columns: fk.ref_columns.clone(),
                            }),
                        );
                    }
                }
            }
        }
        Catalog {
            qualified,
            unqualified,
            fks,
            loaded_dbs,
            active_db: active_lower,
            known_idents,
        }
    }

    /// The database whose schema decides an unqualified reference: the active db,
    /// or (no active db) the sole loaded db if there's exactly one.
    fn unqualified_db_loaded(&self) -> bool {
        match &self.active_db {
            Some(a) => self.loaded_dbs.contains(a),
            None => !self.loaded_dbs.is_empty(),
        }
    }

    fn table_status(&self, r: &TableRef) -> TableStatus {
        match &r.db {
            Some(db) => {
                let db_lower = db.to_ascii_lowercase();
                if !self.loaded_dbs.contains(&db_lower) {
                    return TableStatus::Unknown;
                }
                if self
                    .qualified
                    .contains_key(&(db_lower, r.name.to_ascii_lowercase()))
                {
                    TableStatus::Found
                } else {
                    TableStatus::NotFound
                }
            }
            None => {
                if !self.unqualified_db_loaded() {
                    return TableStatus::Unknown;
                }
                if self.unqualified.contains_key(&r.name.to_ascii_lowercase()) {
                    TableStatus::Found
                } else {
                    TableStatus::NotFound
                }
            }
        }
    }

    /// The columns of a resolved table reference, if known.
    fn columns_of(&self, r: &TableRef) -> Option<&Vec<String>> {
        match &r.db {
            Some(db) => self
                .qualified
                .get(&(db.to_ascii_lowercase(), r.name.to_ascii_lowercase())),
            None => self.unqualified.get(&r.name.to_ascii_lowercase()),
        }
    }
}

// ── Diagnostics ──────────────────────────────────────────────────────────────

/// A diagnostic's severity — drives the squiggle colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// A definite problem (unknown table/column, a syntax error).
    Error,
    /// A heuristic hint (a probable keyword typo).
    Warning,
}

/// One editor diagnostic: a byte range to underline, its severity, and the hover
/// message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub range: (usize, usize),
    pub severity: Severity,
    pub message: String,
}

/// Convert a 1-based (line, column) — as reported by a `sqlparser` / DB error
/// against `text` — into a byte offset. Column counts characters (SQL is ASCII in
/// practice; multi-byte is handled by char stepping). Clamps to `text.len()`.
pub fn offset_of_line_col(text: &str, line: u64, col: u64) -> usize {
    if line == 0 {
        return 0;
    }
    let mut off = 0usize;
    let mut cur_line = 1u64;
    let bytes = text.as_bytes();
    // Advance to the start of `line`.
    while cur_line < line && off < bytes.len() {
        if bytes[off] == b'\n' {
            cur_line += 1;
        }
        off += 1;
    }
    // Advance `col-1` characters into the line.
    let mut remaining = col.saturating_sub(1);
    while remaining > 0 && off < text.len() {
        let ch = text[off..].chars().next().unwrap();
        if ch == '\n' {
            break;
        }
        off += ch.len_utf8();
        remaining -= 1;
    }
    off.min(text.len())
}

/// Pull the ` at Line: L, Column: C` suffix `sqlparser` appends to its error
/// messages, returning `(line, col)` and the message with the suffix stripped.
fn split_error_location(msg: &str) -> (Option<(u64, u64)>, String) {
    if let Some(idx) = msg.find(" at Line: ") {
        let (head, tail) = msg.split_at(idx);
        // tail = " at Line: L, Column: C"
        let nums: Vec<u64> = tail
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        if let [line, col] = nums.as_slice() {
            let clean = head.trim().trim_end_matches(':').to_string();
            return (Some((*line, *col)), clean);
        }
    }
    (None, msg.to_string())
}

/// The byte range of the word at `off` in `sql` (single-char if not on a word).
fn word_range_at(sql: &str, off: usize) -> (usize, usize) {
    let b = sql.as_bytes();
    let n = sql.len();
    let off = off.min(n);
    if off < n && is_word_byte(b[off]) {
        let mut j = off + 1;
        while j < n && is_word_byte(b[j]) {
            j += 1;
        }
        (off, j)
    } else {
        (off, (off + 1).min(n))
    }
}

/// All FROM/JOIN/UPDATE/INTO table references in `sql[lo..hi]`, each with the byte
/// range of its *table-name* token (positions the AST can't reliably give). Unlike
/// [`lexer_scope`] this ignores paren scoping — for a parsed statement we want
/// every table reference in it.
fn table_refs_with_pos(sql: &str, lo: usize, hi: usize) -> Vec<(TableRef, (usize, usize))> {
    let toks = tokenize_range(sql, lo, hi);
    let word = |k: &TkKind| -> Option<String> {
        if let TkKind::Word(w) = k {
            Some(w.clone())
        } else {
            None
        }
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let TkKind::Word(w) = &toks[i].kind else {
            i += 1;
            continue;
        };
        let up = w.to_ascii_uppercase();
        let is_from = up == "FROM";
        if !matches!(up.as_str(), "FROM" | "JOIN" | "INTO" | "UPDATE") {
            i += 1;
            continue;
        }
        i += 1;
        while let Some(mut name) = toks.get(i).and_then(|t| word(&t.kind)) {
            if is_sql_keyword(&name) {
                break;
            }
            let mut pos = (toks[i].at, toks[i].at + name.len());
            let mut db = None;
            i += 1;
            if matches!(toks.get(i).map(|t| &t.kind), Some(TkKind::Dot))
                && let Some(second) = toks.get(i + 1).and_then(|t| word(&t.kind))
            {
                db = Some(name);
                name = second;
                pos = (toks[i + 1].at, toks[i + 1].at + name.len());
                i += 2;
            }
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
            out.push((TableRef { name, alias, db }, pos));
            if is_from && matches!(toks.get(i).map(|t| &t.kind), Some(TkKind::Comma)) {
                i += 1;
                continue;
            }
            break;
        }
    }
    out
}

/// Is `word` a likely misspelled SQL keyword? Not a known word (keyword/function/
/// schema ident) but a near-miss of a keyword. Conservative (short words + distant
/// matches ignored) to avoid flagging legitimate identifiers.
fn is_probable_typo(word: &str, known: &HashSet<String>) -> bool {
    if word.len() < 4 {
        return false;
    }
    let lw = word.to_ascii_lowercase();
    if known.contains(&lw) {
        return false;
    }
    let up = word.to_ascii_uppercase();
    let thresh = if word.len() >= 7 { 2 } else { 1 };
    SQL_KEYWORDS.iter().chain(STMT_KEYWORDS).any(|kw| {
        (kw.len() as isize - up.len() as isize).unsigned_abs() <= thresh
            && crate::sql::edit_distance(&up, kw) <= thresh
    })
}

/// Catalog-aware offline diagnostics: instant, no DB round-trip. For each
/// statement in `sql`:
/// - parses it with `dialect`; a parse failure on a *completed* statement (one
///   that isn't the still-being-typed final fragment) yields one syntax error;
/// - on a clean parse, resolves table references (unknown table) and qualified
///   `alias.col` / `table.col` references (unknown column) against `catalog`;
/// - flags probable keyword typos (a heuristic warning), excluding real schema
///   names.
///
/// Byte ranges come from the [`skip_noncode`] lexer, not AST spans (which are
/// still maturing upstream), so squiggle placement is exact.
pub fn diagnostics(sql: &str, catalog: &Catalog, dialect: SqlDialect) -> Vec<Diagnostic> {
    let ranges = crate::sql::statement_ranges(sql);
    let last = ranges.len().saturating_sub(1);
    let mut out: Vec<Diagnostic> = Vec::new();
    for (idx, &(lo, hi)) in ranges.iter().enumerate() {
        let stmt = &sql[lo..hi];
        let terminated = sql.as_bytes().get(hi - 1) == Some(&b';');
        let is_typing_tail = idx == last && !terminated;
        match sqlparser::parser::Parser::parse_sql(&*dialect.parser(), stmt) {
            Ok(_) => catalog_checks(sql, lo, hi, catalog, &mut out),
            Err(e) => {
                // Don't nag about the fragment the user is still typing.
                if !is_typing_tail {
                    let (loc, msg) = split_error_location(&e.to_string());
                    let range = match loc {
                        Some((line, col)) => {
                            let at = lo + offset_of_line_col(stmt, line, col);
                            word_range_at(sql, at)
                        }
                        None => (lo, hi),
                    };
                    out.push(Diagnostic {
                        range,
                        severity: Severity::Error,
                        message: friendly_syntax_message(&msg),
                    });
                }
            }
        }
        typo_checks(sql, lo, hi, catalog, &mut out);
    }
    dedup_diagnostics(out)
}

/// Table-existence + qualified-column checks for a cleanly-parsed statement.
fn catalog_checks(sql: &str, lo: usize, hi: usize, catalog: &Catalog, out: &mut Vec<Diagnostic>) {
    let refs = table_refs_with_pos(sql, lo, hi);
    for (r, pos) in &refs {
        if let TableStatus::NotFound = catalog.table_status(r) {
            let where_db =
                r.db.as_deref()
                    .map(|d| format!(" in `{d}`"))
                    .unwrap_or_default();
            out.push(Diagnostic {
                range: *pos,
                severity: Severity::Error,
                message: format!("Table `{}` not found{where_db}", r.name),
            });
        }
    }
    // Qualified `alias.col` / `table.col`: flag a column that definitively isn't
    // in the resolved table (only when we know that table's columns).
    let toks = tokenize_range(sql, lo, hi);
    for w in toks.windows(3) {
        let (TkKind::Word(q), TkKind::Dot, TkKind::Word(col)) =
            (&w[0].kind, &w[1].kind, &w[2].kind)
        else {
            continue;
        };
        let Some(table) = resolve_qualifier(q, &refs) else {
            continue;
        };
        if let Some(cols) = catalog.columns_of(&table)
            && !cols.iter().any(|c| c.eq_ignore_ascii_case(col))
        {
            out.push(Diagnostic {
                range: (w[2].at, w[2].at + col.len()),
                severity: Severity::Error,
                message: format!("Column `{col}` not found in `{}`", table.name),
            });
        }
    }
}

/// Resolve a `qualifier` (before a `.`) to a table reference: an in-scope alias
/// first, else a table referenced by that bare name.
fn resolve_qualifier(q: &str, refs: &[(TableRef, (usize, usize))]) -> Option<TableRef> {
    for (r, _) in refs {
        if r.alias
            .as_deref()
            .is_some_and(|a| a.eq_ignore_ascii_case(q))
        {
            return Some(r.clone());
        }
    }
    for (r, _) in refs {
        if r.alias.is_none() && r.name.eq_ignore_ascii_case(q) {
            return Some(r.clone());
        }
    }
    None
}

/// Probable keyword-typo warnings across `sql[lo..hi]` (skips the identifier after
/// a `.` and real schema names).
fn typo_checks(sql: &str, lo: usize, hi: usize, catalog: &Catalog, out: &mut Vec<Diagnostic>) {
    let mut known: HashSet<String> = SQL_KEYWORDS
        .iter()
        .chain(SQL_FUNCTIONS.iter())
        .chain(STMT_KEYWORDS.iter())
        .map(|k| k.to_ascii_lowercase())
        .collect();
    known.extend(catalog.known_idents.iter().cloned());

    let b = sql.as_bytes();
    let mut i = lo;
    while i < hi {
        if let Some(j) = skip_noncode(b, i) {
            i = j.min(hi);
            continue;
        }
        let c = b[i];
        if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 {
            let s = i;
            let mut j = i + 1;
            while j < hi && is_word_byte(b[j]) {
                j += 1;
            }
            let qualified = s > 0 && b[s - 1] == b'.';
            if !qualified && is_probable_typo(&sql[s..j], &known) {
                out.push(Diagnostic {
                    range: (s, j),
                    severity: Severity::Warning,
                    message: format!("`{}` looks like a misspelled keyword", &sql[s..j]),
                });
            }
            i = j;
            continue;
        }
        i += 1;
    }
}

/// Tidy a raw `sqlparser` error into a short editor message.
fn friendly_syntax_message(msg: &str) -> String {
    let m = msg
        .trim()
        .strip_prefix("sql parser error:")
        .unwrap_or(msg)
        .trim();
    if m.is_empty() {
        "Syntax error".to_string()
    } else {
        format!("Syntax error: {m}")
    }
}

/// Drop diagnostics whose range is fully covered by an earlier, higher-or-equal
/// severity one (e.g. a keyword typo under a syntax squiggle), so a token isn't
/// double-underlined.
fn dedup_diagnostics(mut v: Vec<Diagnostic>) -> Vec<Diagnostic> {
    // Errors first so a Warning on the same span is the one dropped.
    v.sort_by(|a, b| {
        a.range
            .0
            .cmp(&b.range.0)
            .then((a.severity == Severity::Warning).cmp(&(b.severity == Severity::Warning)))
    });
    let mut out: Vec<Diagnostic> = Vec::new();
    for d in v {
        let covered = out
            .iter()
            .any(|e| e.range.0 <= d.range.0 && d.range.1 <= e.range.1);
        if !covered {
            out.push(d);
        }
    }
    out
}

// ── FK-aware JOIN … ON completion ────────────────────────────────────────────

impl Catalog {
    /// The foreign-key edges declared on `table` (case-insensitive), if any.
    fn fks_of(&self, table: &str) -> Option<&Vec<FkEdge>> {
        self.fks.get(&table.to_ascii_lowercase())
    }
}

/// The qualifier to write for a table reference in a predicate: its alias if it
/// has one, else its (bare) name.
fn ref_qualifier(r: &TableRef) -> &str {
    r.alias.as_deref().unwrap_or(&r.name)
}

/// Build the equality predicate joining `left`'s `lcols` to `right`'s `rcols`
/// (aligned by position), e.g. `o.customer_id = c.id` (composite → `AND`-joined).
fn build_predicate(
    left: &TableRef,
    lcols: &[String],
    right: &TableRef,
    rcols: &[String],
) -> String {
    lcols
        .iter()
        .zip(rcols)
        .map(|(lc, rc)| {
            format!(
                "{}.{lc} = {}.{rc}",
                ref_qualifier(left),
                ref_qualifier(right)
            )
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// The FK predicate linking two table references, in either direction (`a`→`b` or
/// `b`→`a`), or `None` if no foreign key connects them.
fn fk_predicate(catalog: &Catalog, a: &TableRef, b: &TableRef) -> Option<String> {
    if let Some(edges) = catalog.fks_of(&a.name) {
        for e in edges {
            if e.ref_table.eq_ignore_ascii_case(&b.name) {
                return Some(build_predicate(a, &e.columns, b, &e.ref_columns));
            }
        }
    }
    if let Some(edges) = catalog.fks_of(&b.name) {
        for e in edges {
            if e.ref_table.eq_ignore_ascii_case(&a.name) {
                return Some(build_predicate(b, &e.columns, a, &e.ref_columns));
            }
        }
    }
    None
}

/// If the caret sits in a *fresh* `JOIN … ON` (right after `ON`, nothing typed
/// yet), and a foreign key connects the just-joined table to another table in
/// scope, return the ready-to-insert join predicate (e.g. `o.customer_id = c.id`).
/// This is the DataGrip-style auto-join: the completion layer offers it as the top
/// suggestion. `None` when the caret isn't in an empty ON, or no FK links the
/// tables (the user then completes the condition by hand).
pub fn join_condition(
    sql: &str,
    lo: usize,
    hi: usize,
    caret: usize,
    catalog: &Catalog,
) -> Option<String> {
    let toks = tokenize_range(sql, lo, hi);
    // The last clause keyword strictly before the caret must be `ON`.
    let mut last_kw_idx = None;
    for (i, t) in toks.iter().enumerate() {
        if t.at >= caret {
            break;
        }
        if let TkKind::Word(w) = &t.kind
            && CLAUSE_KEYWORDS.contains(&w.to_ascii_uppercase().as_str())
        {
            last_kw_idx = Some(i);
        }
    }
    let on_idx = last_kw_idx?;
    let TkKind::Word(kw) = &toks[on_idx].kind else {
        return None;
    };
    if !kw.eq_ignore_ascii_case("ON") {
        return None;
    }
    // The ON expression must be empty (no word typed between `ON` and the caret) —
    // we only auto-fill a blank condition, never overwrite a hand-typed one.
    for t in &toks[on_idx + 1..] {
        if t.at >= caret {
            break;
        }
        if matches!(t.kind, TkKind::Word(_)) {
            return None;
        }
    }
    // The JOIN this ON belongs to, and the table it introduced.
    let join_idx = toks[..on_idx]
        .iter()
        .rposition(|t| matches!(&t.kind, TkKind::Word(w) if w.eq_ignore_ascii_case("JOIN")))?;
    let join_at = toks[join_idx].at;
    let refs = table_refs_with_pos(sql, lo, hi);
    let joined = refs
        .iter()
        .filter(|(_, p)| p.0 > join_at)
        .min_by_key(|(_, p)| p.0)
        .map(|(r, _)| r.clone())?;
    // Pair it with each other in-scope table; first FK match wins.
    for (other, _) in &refs {
        if other.name.eq_ignore_ascii_case(&joined.name) && other.alias == joined.alias {
            continue;
        }
        if let Some(pred) = fk_predicate(catalog, &joined, other) {
            return Some(pred);
        }
    }
    None
}

// ── DB-validated diagnostics (Tier 2) ────────────────────────────────────────

/// Does `stmt` parse cleanly as SQL in `dialect`? The live DB validation gates on
/// this so it only round-trips syntactically-complete statements — a half-typed
/// fragment (which the server would reject) never triggers a spurious error.
pub fn parses(stmt: &str, dialect: SqlDialect) -> bool {
    sqlparser::parser::Parser::parse_sql(&*dialect.parser(), stmt).is_ok()
}

/// Case-insensitive search for `needle` in `hay`, returning its byte range.
fn find_ci(hay: &str, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    (0..=h.len().saturating_sub(n.len())).find_map(|i| {
        if h[i..i + n.len()]
            .iter()
            .zip(n)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            Some((i, i + n.len()))
        } else {
            None
        }
    })
}

/// Generic phrases MySQL/MariaDB puts in quotes that aren't object names.
fn is_error_phrase(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "field list"
            | "where clause"
            | "on clause"
            | "order clause"
            | "having clause"
            | "group statement"
    )
}

/// Locate the offending token of a DB error `message` within the statement text
/// `stmt`: the `near '<tok>'` clause of a syntax error, or the first quoted object
/// name of a name error (`Unknown column 'x'`, `Table 'db.t' doesn't exist`).
fn locate_db_error(stmt: &str, message: &str) -> Option<(usize, usize)> {
    // Syntax error: `... near 'FRM employees' at line 1` → first word `FRM`.
    if let Some(idx) = message.find("near '") {
        let rest = &message[idx + 6..];
        if let Some(end) = rest.find('\'') {
            let tok = rest[..end].split_whitespace().next().unwrap_or("");
            let tok = tok.trim_matches('`');
            if !tok.is_empty() {
                return find_ci(stmt, tok);
            }
        }
    }
    // Name error: first single-quoted object name, last `.`-segment (skip generic
    // phrases like 'field list').
    let mut i = 0;
    while let Some(open) = message[i..].find('\'') {
        let start = i + open + 1;
        if let Some(close_rel) = message[start..].find('\'') {
            let content = &message[start..start + close_rel];
            i = start + close_rel + 1;
            if is_error_phrase(content) {
                continue;
            }
            let seg = content
                .rsplit('.')
                .next()
                .unwrap_or(content)
                .trim_matches('`');
            if let Some(r) = find_ci(stmt, seg) {
                return Some(r);
            }
        } else {
            break;
        }
    }
    None
}

/// Strip the boilerplate MySQL/MariaDB pads its syntax errors with, and cut to the
/// first line, so the hover message stays short.
fn clean_db_message(message: &str) -> String {
    let first_line = message.lines().next().unwrap_or(message);
    let cleaned = first_line
        .replace(
            "check the manual that corresponds to your MariaDB server version for the right syntax to use ",
            "",
        )
        .replace(
            "check the manual that corresponds to your MySQL server version for the right syntax to use ",
            "",
        );
    cleaned.trim().to_string()
}

/// Turn a database validation error (`message`, from a failed PREPARE/EXPLAIN) into
/// a [`Diagnostic`] positioned within the statement `sql[lo..hi]`. The server names
/// the offending token — a `near '<tok>'` clause or a quoted object name — which we
/// locate to place the squiggle; failing that, the statement's leading token is
/// underlined (never the whole statement, which reads as noise).
pub fn db_error_diagnostic(sql: &str, lo: usize, hi: usize, message: &str) -> Diagnostic {
    let stmt = sql.get(lo..hi).unwrap_or("");
    let range = match locate_db_error(stmt, message) {
        Some((s, e)) => (lo + s, lo + e),
        // Fall back to the statement's first token, not the whole statement.
        None => {
            let (_, first_end) = word_range_at(sql, lo);
            (lo, first_end.max(lo + 1).min(hi))
        }
    };
    Diagnostic {
        range,
        severity: Severity::Error,
        message: clean_db_message(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::parser::Parser;

    fn names(scope: &Scope) -> Vec<String> {
        let mut v: Vec<String> = scope.tables.iter().map(|t| t.name.clone()).collect();
        v.sort();
        v
    }

    #[test]
    fn sqlparser_parses_a_mysql_statement() {
        // Smoke test: the dependency + dialect seam parse a backtick-quoted,
        // `LIMIT offset, count` MySQL statement (a construct the hand lexer can't
        // structure). Proves the AST engine is wired before we build on it.
        let d = SqlDialect::MySql.parser();
        let ast = Parser::parse_sql(&*d, "SELECT `id` FROM `users` u LIMIT 10, 5").unwrap();
        assert_eq!(ast.len(), 1);
    }

    // ── statement_scope: AST path ─────────────────────────────────────────────

    fn scope(sql: &str) -> Scope {
        statement_scope(sql, 0, sql.len(), sql.len(), SqlDialect::MySql)
    }

    #[test]
    fn scope_resolves_simple_from_alias() {
        let s = scope("SELECT * FROM employees e");
        assert_eq!(names(&s), vec!["employees"]);
        assert_eq!(s.tables[0].alias.as_deref(), Some("e"));
    }

    #[test]
    fn scope_resolves_db_qualified_table() {
        let s = scope("SELECT * FROM sakila.actor a");
        assert_eq!(s.tables[0].name, "actor");
        assert_eq!(s.tables[0].db.as_deref(), Some("sakila"));
        assert_eq!(s.tables[0].alias.as_deref(), Some("a"));
    }

    #[test]
    fn scope_collects_joins() {
        let s = scope(
            "SELECT * FROM orders o JOIN customers c ON o.cust_id = c.id LEFT JOIN items i ON i.order_id = o.id",
        );
        assert_eq!(names(&s), vec!["customers", "items", "orders"]);
    }

    #[test]
    fn scope_collects_comma_join_list() {
        let s = scope("SELECT * FROM a, b, c");
        assert_eq!(names(&s), vec!["a", "b", "c"]);
    }

    #[test]
    fn scope_resolves_cte_names_and_inner_tables() {
        // The CTE name is recorded; the real table inside its body resolves too.
        let s = scope("WITH recent AS (SELECT * FROM orders) SELECT * FROM recent");
        assert!(s.ctes.iter().any(|c| c == "recent"));
        assert!(s.tables.iter().any(|t| t.name == "orders"));
    }

    #[test]
    fn scope_recurses_into_derived_table() {
        // The paren-hack over-scoped derived tables; the AST resolves the inner
        // real table (`events`) while not inventing a catalog table for `d`.
        let s = scope("SELECT * FROM (SELECT id FROM events) d");
        assert_eq!(names(&s), vec!["events"]);
    }

    #[test]
    fn scope_handles_update_and_delete() {
        assert!(
            scope("UPDATE employees SET salary = 1")
                .tables
                .iter()
                .any(|t| t.name == "employees")
        );
        assert!(
            scope("DELETE FROM logs WHERE id = 1")
                .tables
                .iter()
                .any(|t| t.name == "logs")
        );
    }

    #[test]
    fn scope_falls_back_to_lexer_when_unparseable() {
        // Mid-edit, incomplete SQL doesn't parse — the lexer scope still resolves
        // the FROM table + alias so completion keeps working.
        let sql = "SELECT e. FROM employees e WHERE ";
        let s = statement_scope(sql, 0, sql.len(), 9, SqlDialect::MySql);
        assert!(
            s.tables
                .iter()
                .any(|t| t.name == "employees" && t.alias.as_deref() == Some("e"))
        );
    }

    // ── clause_context ────────────────────────────────────────────────────────

    fn ctx_at(sql: &str, caret: usize) -> ClauseCtx {
        let word_lo = word_start(sql, caret);
        clause_context(sql, 0, word_lo)
    }

    #[test]
    fn context_start_then_table_then_column() {
        assert_eq!(ctx_at("SEL", 3), ClauseCtx::Start);
        // Caret after the trailing space (offset 14) → in FROM's target position.
        assert_eq!(ctx_at("SELECT * FROM ", 14), ClauseCtx::Table);
        // Caret in the projection (between SELECT and FROM) → column context.
        assert_eq!(ctx_at("SELECT  FROM t", 7), ClauseCtx::Column);
    }

    #[test]
    fn context_qualified_after_dot() {
        // `e.` → qualified on `e`.
        assert_eq!(
            ctx_at("SELECT e. FROM employees e", 9),
            ClauseCtx::Qualified("e".to_string())
        );
    }

    // ── diagnostics ───────────────────────────────────────────────────────────

    use crate::schema::{ColumnInfo, DbSchema, TableInfo};

    fn tbl(name: &str, cols: &[&str]) -> TableInfo {
        TableInfo {
            name: name.to_string(),
            columns: cols
                .iter()
                .map(|c| ColumnInfo {
                    name: c.to_string(),
                    type_name: "int".to_string(),
                    nullable: true,
                    primary_key: false,
                })
                .collect(),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            is_view: false,
            view_definition: None,
        }
    }

    fn sample_catalog() -> (DbSchema, &'static str) {
        (
            DbSchema {
                tables: vec![
                    tbl("employees", &["id", "name", "salary", "dept_id"]),
                    tbl("departments", &["id", "name"]),
                ],
            },
            "company",
        )
    }

    fn diag(sql: &str) -> Vec<Diagnostic> {
        let (schema, db) = sample_catalog();
        let cat = Catalog::build(&[(db, &schema)], Some(db));
        diagnostics(sql, &cat, SqlDialect::MySql)
    }

    #[test]
    fn diag_clean_query_has_none() {
        assert!(diag("SELECT id, name FROM employees WHERE salary > 100").is_empty());
        assert!(
            diag("SELECT e.name FROM employees e JOIN departments d ON e.dept_id = d.id")
                .is_empty()
        );
    }

    #[test]
    fn diag_unknown_table() {
        let d = diag("SELECT * FROM employes");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].severity, Severity::Error);
        assert!(d[0].message.contains("Table `employes` not found"));
        // The squiggle covers the table name, not the whole statement.
        assert_eq!(
            &"SELECT * FROM employes"[d[0].range.0..d[0].range.1],
            "employes"
        );
    }

    #[test]
    fn diag_unknown_column_qualified() {
        let d = diag("SELECT e.salery FROM employees e");
        assert_eq!(d.len(), 1);
        assert!(
            d[0].message
                .contains("Column `salery` not found in `employees`")
        );
        assert_eq!(
            &"SELECT e.salery FROM employees e"[d[0].range.0..d[0].range.1],
            "salery"
        );
    }

    #[test]
    fn diag_known_column_qualified_is_clean() {
        assert!(diag("SELECT e.salary FROM employees e").is_empty());
        // Table-qualified (no alias) also resolves.
        assert!(diag("SELECT employees.name FROM employees").is_empty());
    }

    #[test]
    fn diag_syntax_error_on_terminated_statement() {
        // A completed (`;`-terminated) broken statement → a syntax error.
        let d = diag("SELECT FROM WHERE;");
        assert!(
            d.iter()
                .any(|x| x.severity == Severity::Error && x.message.starts_with("Syntax error"))
        );
    }

    #[test]
    fn diag_suppresses_syntax_on_typing_tail() {
        // The final, unterminated fragment being typed must not throw a red error.
        let d = diag("SELECT id FROM employees WHERE ");
        assert!(!d.iter().any(|x| x.message.starts_with("Syntax error")));
    }

    #[test]
    fn diag_multi_statement_isolates() {
        // First statement valid, second references an unknown table.
        let d = diag("SELECT * FROM employees; SELECT * FROM nope;");
        assert_eq!(
            d.iter().filter(|x| x.message.contains("not found")).count(),
            1
        );
    }

    #[test]
    fn diag_no_false_positive_when_schema_unloaded() {
        // No loaded schema → we can't judge existence, so no unknown-table noise.
        let cat = Catalog::build(&[], Some("company"));
        assert!(diagnostics("SELECT * FROM anything", &cat, SqlDialect::MySql).is_empty());
    }

    #[test]
    fn diag_keyword_typo_is_a_warning() {
        let sql = "SELCT * FROM employees";
        let d = diag(sql);
        // `SELCT` → the statement doesn't parse; a typo warning still pinpoints it.
        assert!(
            d.iter().any(|x| {
                x.severity == Severity::Warning && &sql[x.range.0..x.range.1] == "SELCT"
            })
        );
    }

    #[test]
    fn diag_qualified_unloaded_db_not_flagged() {
        // `otherdb.t` — that db isn't loaded, so no unknown-table false positive.
        assert!(diag("SELECT * FROM otherdb.things t").is_empty());
    }

    #[test]
    fn offset_of_line_col_maps_positions() {
        let sql = "SELECT 1\nFROM t\nWHERE x";
        assert_eq!(offset_of_line_col(sql, 1, 1), 0);
        assert_eq!(offset_of_line_col(sql, 2, 1), 9); // start of "FROM"
        assert_eq!(&sql[offset_of_line_col(sql, 2, 1)..][..4], "FROM");
        assert_eq!(offset_of_line_col(sql, 3, 7), sql.len() - 1); // the `x`
        assert_eq!(offset_of_line_col(sql, 3, 99), sql.len()); // past EOL clamps
    }

    // ── FK-aware JOIN … ON ────────────────────────────────────────────────────

    fn fk_catalog() -> (DbSchema, &'static str) {
        use crate::schema::ForeignKeyInfo;
        let mut orders = tbl("orders", &["id", "customer_id", "total"]);
        orders.foreign_keys = vec![ForeignKeyInfo {
            columns: vec!["customer_id".to_string()],
            ref_schema: None,
            ref_table: "customers".to_string(),
            ref_columns: vec!["id".to_string()],
        }];
        let customers = tbl("customers", &["id", "name"]);
        // Composite FK: order_items(order_id, item_id) → … just test single here,
        // plus a composite pair below.
        let mut line_items = tbl("line_items", &["order_id", "sku", "qty"]);
        line_items.foreign_keys = vec![ForeignKeyInfo {
            columns: vec!["order_id".to_string()],
            ref_schema: None,
            ref_table: "orders".to_string(),
            ref_columns: vec!["id".to_string()],
        }];
        (
            DbSchema {
                tables: vec![orders, customers, line_items],
            },
            "shop",
        )
    }

    fn join_at(sql: &str, caret: usize) -> Option<String> {
        let (schema, db) = fk_catalog();
        let cat = Catalog::build(&[(db, &schema)], Some(db));
        join_condition(sql, 0, sql.len(), caret, &cat)
    }

    #[test]
    fn join_suggests_fk_predicate_with_aliases() {
        // orders.customer_id → customers.id; caret right after `ON `.
        let sql = "SELECT * FROM orders o JOIN customers c ON ";
        assert_eq!(
            join_at(sql, sql.len()),
            Some("o.customer_id = c.id".to_string())
        );
    }

    #[test]
    fn join_predicate_uses_table_names_without_aliases() {
        let sql = "SELECT * FROM orders JOIN customers ON ";
        assert_eq!(
            join_at(sql, sql.len()),
            Some("orders.customer_id = customers.id".to_string())
        );
    }

    #[test]
    fn join_works_when_fk_points_the_other_way() {
        // The just-joined table (`orders`) is referenced BY `line_items` — the
        // predicate still resolves (direction-agnostic).
        let sql = "SELECT * FROM line_items l JOIN orders o ON ";
        assert_eq!(
            join_at(sql, sql.len()),
            Some("l.order_id = o.id".to_string())
        );
    }

    #[test]
    fn join_none_when_condition_already_typed() {
        // Something already typed after ON → don't overwrite it.
        let sql = "SELECT * FROM orders o JOIN customers c ON o.";
        assert_eq!(join_at(sql, sql.len()), None);
    }

    #[test]
    fn join_none_without_a_foreign_key() {
        // customers ⟷ line_items have no FK between them.
        let sql = "SELECT * FROM customers c JOIN line_items l ON ";
        assert_eq!(join_at(sql, sql.len()), None);
    }

    #[test]
    fn join_none_outside_on_context() {
        // Caret in the projection, not an ON clause.
        let sql = "SELECT  FROM orders o JOIN customers c ON x";
        assert_eq!(join_at(sql, 7), None);
    }

    // ── DB-validated diagnostics ──────────────────────────────────────────────

    #[test]
    fn db_error_locates_syntax_near_token() {
        let sql = "SELECT * FRM employees";
        let msg = "You have an error in your SQL syntax; check the manual that corresponds to your MariaDB server version for the right syntax to use near 'FRM employees' at line 1";
        let d = db_error_diagnostic(sql, 0, sql.len(), msg);
        assert_eq!(&sql[d.range.0..d.range.1], "FRM");
        assert!(!d.message.contains("check the manual")); // boilerplate stripped
    }

    #[test]
    fn db_error_locates_unknown_column() {
        let sql = "SELECT salery FROM employees";
        let d = db_error_diagnostic(sql, 0, sql.len(), "Unknown column 'salery' in 'field list'");
        assert_eq!(&sql[d.range.0..d.range.1], "salery");
    }

    #[test]
    fn db_error_locates_qualified_table_last_segment() {
        let sql = "SELECT * FROM employes";
        let d = db_error_diagnostic(sql, 0, sql.len(), "Table 'company.employes' doesn't exist");
        assert_eq!(&sql[d.range.0..d.range.1], "employes");
    }

    #[test]
    fn db_error_falls_back_to_leading_token() {
        // A message naming nothing findable → underline the first token, not all.
        let sql = "SELECT 1";
        let d = db_error_diagnostic(sql, 0, sql.len(), "Some opaque server error");
        assert_eq!(&sql[d.range.0..d.range.1], "SELECT");
    }

    #[test]
    fn db_error_positions_within_a_later_statement() {
        // Statement offset by `lo` — the range is absolute in the full buffer.
        let sql = "SELECT 1;\nSELECT salery FROM employees";
        let (lo, hi) = (10, sql.len());
        let d = db_error_diagnostic(sql, lo, hi, "Unknown column 'salery' in 'field list'");
        assert_eq!(&sql[d.range.0..d.range.1], "salery");
    }
}
