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

/// A broad set of MySQL/MariaDB built-in function names (upper-case) — the
/// authority for the "unknown function" typo check. It is intentionally *much*
/// larger than [`SQL_FUNCTIONS`] (the small suggestion set): a call like `POWR(x)`
/// is only flagged when its name is a near-miss of some entry here and isn't itself
/// one, so real builtins outside the suggestion set (e.g. `POWER`, `COALESCE`,
/// `JSON_EXTRACT`) never false-positive. Not used for completion.
pub(crate) const KNOWN_FUNCTIONS: &[&str] = &[
    // Aggregate / window.
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "GROUP_CONCAT",
    "STD",
    "STDDEV",
    "STDDEV_POP",
    "STDDEV_SAMP",
    "VARIANCE",
    "VAR_POP",
    "VAR_SAMP",
    "BIT_AND",
    "BIT_OR",
    "BIT_XOR",
    "JSON_ARRAYAGG",
    "JSON_OBJECTAGG",
    "ROW_NUMBER",
    "RANK",
    "DENSE_RANK",
    "PERCENT_RANK",
    "CUME_DIST",
    "NTILE",
    "LAG",
    "LEAD",
    "FIRST_VALUE",
    "LAST_VALUE",
    "NTH_VALUE",
    // String.
    "CONCAT",
    "CONCAT_WS",
    "LENGTH",
    "OCTET_LENGTH",
    "CHAR_LENGTH",
    "CHARACTER_LENGTH",
    "LOWER",
    "LCASE",
    "UPPER",
    "UCASE",
    "TRIM",
    "LTRIM",
    "RTRIM",
    "SUBSTRING",
    "SUBSTR",
    "MID",
    "SUBSTRING_INDEX",
    "REPLACE",
    "REVERSE",
    "REPEAT",
    "LEFT",
    "RIGHT",
    "LPAD",
    "RPAD",
    "LOCATE",
    "POSITION",
    "INSTR",
    "FIELD",
    "FIND_IN_SET",
    "SPACE",
    "ELT",
    "ORD",
    "ASCII",
    "CHAR",
    "HEX",
    "UNHEX",
    "BIN",
    "OCT",
    "FORMAT",
    "QUOTE",
    "SOUNDEX",
    "TO_BASE64",
    "FROM_BASE64",
    "REGEXP_REPLACE",
    "REGEXP_SUBSTR",
    "REGEXP_INSTR",
    "REGEXP_LIKE",
    "MAKE_SET",
    "EXPORT_SET",
    // Numeric.
    "ABS",
    "CEIL",
    "CEILING",
    "FLOOR",
    "ROUND",
    "TRUNCATE",
    "MOD",
    "POW",
    "POWER",
    "SQRT",
    "EXP",
    "LN",
    "LOG",
    "LOG2",
    "LOG10",
    "SIGN",
    "RAND",
    "PI",
    "DEGREES",
    "RADIANS",
    "SIN",
    "COS",
    "TAN",
    "ASIN",
    "ACOS",
    "ATAN",
    "ATAN2",
    "COT",
    "CRC32",
    "CONV",
    "GREATEST",
    "LEAST",
    // Date / time.
    "NOW",
    "CURDATE",
    "CURRENT_DATE",
    "CURTIME",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "SYSDATE",
    "UTC_DATE",
    "UTC_TIME",
    "UTC_TIMESTAMP",
    "DATE",
    "TIME",
    "YEAR",
    "MONTH",
    "DAY",
    "DAYOFMONTH",
    "HOUR",
    "MINUTE",
    "SECOND",
    "MICROSECOND",
    "WEEK",
    "WEEKDAY",
    "WEEKOFYEAR",
    "DAYOFWEEK",
    "DAYOFYEAR",
    "DAYNAME",
    "MONTHNAME",
    "QUARTER",
    "LAST_DAY",
    "DATE_FORMAT",
    "TIME_FORMAT",
    "STR_TO_DATE",
    "DATE_ADD",
    "DATE_SUB",
    "ADDDATE",
    "SUBDATE",
    "ADDTIME",
    "SUBTIME",
    "DATEDIFF",
    "TIMEDIFF",
    "TIMESTAMP",
    "TIMESTAMPADD",
    "TIMESTAMPDIFF",
    "EXTRACT",
    "MAKEDATE",
    "MAKETIME",
    "PERIOD_ADD",
    "PERIOD_DIFF",
    "SEC_TO_TIME",
    "TIME_TO_SEC",
    "TO_DAYS",
    "FROM_DAYS",
    "TO_SECONDS",
    "UNIX_TIMESTAMP",
    "FROM_UNIXTIME",
    "CONVERT_TZ",
    "GET_FORMAT",
    // Control / cast / null.
    "IF",
    "IFNULL",
    "NULLIF",
    "COALESCE",
    "CAST",
    "CONVERT",
    "ISNULL",
    "NANVL",
    // JSON.
    "JSON_EXTRACT",
    "JSON_OBJECT",
    "JSON_ARRAY",
    "JSON_VALID",
    "JSON_TYPE",
    "JSON_KEYS",
    "JSON_LENGTH",
    "JSON_CONTAINS",
    "JSON_CONTAINS_PATH",
    "JSON_SET",
    "JSON_INSERT",
    "JSON_REPLACE",
    "JSON_REMOVE",
    "JSON_MERGE",
    "JSON_MERGE_PATCH",
    "JSON_MERGE_PRESERVE",
    "JSON_UNQUOTE",
    "JSON_QUOTE",
    "JSON_SEARCH",
    "JSON_DEPTH",
    "JSON_PRETTY",
    // Info / misc.
    "DATABASE",
    "SCHEMA",
    "USER",
    "CURRENT_USER",
    "SESSION_USER",
    "SYSTEM_USER",
    "VERSION",
    "CONNECTION_ID",
    "LAST_INSERT_ID",
    "ROW_COUNT",
    "FOUND_ROWS",
    "UUID",
    "UUID_SHORT",
    "MD5",
    "SHA",
    "SHA1",
    "SHA2",
    "PASSWORD",
    "AES_ENCRYPT",
    "AES_DECRYPT",
    "COMPRESS",
    "UNCOMPRESS",
    "BENCHMARK",
    "SLEEP",
    "INET_ATON",
    "INET_NTOA",
    "INET6_ATON",
    "INET6_NTOA",
    "IS_IPV4",
    "IS_IPV6",
    "NULLIF",
    "BIT_COUNT",
    "BIT_LENGTH",
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

// ── Expected-token continuation model ────────────────────────────────────────
// Beyond "which clause are we in" ([`clause_context`], which schema *candidates* to
// rank), the completion popup needs "which *keyword* comes next" from SQL's fixed
// clause grammar — so `WHERE` outranks the generic keyword bag after a complete
// table ref, `FROM` is #1 after the projection, and multi-word phrases (`GROUP BY`,
// `LEFT JOIN`, `IS NOT NULL`) are single suggestions. SQL's clause order is small +
// stable, so a hand table (grown case-by-case, each with a test) beats a generic
// parser here and stays dialect-perfect.

/// The leading statement kind, which decides the clause grammar to follow.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StmtKind {
    Select,
    Insert,
    Update,
    Delete,
    Other,
}

/// The clause the caret currently sits in, tracked by [`scan_clauses`]. Multi-word
/// clauses have both an incomplete arm (`Group` = `GROUP` typed, `BY` expected) and
/// a complete one (`GroupBy`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Clause {
    None,
    Select,
    From,
    JoinMod, // a bare `LEFT`/`INNER`/… — `JOIN` still expected
    Join,
    On,
    Using,
    Where,
    Group,
    GroupBy,
    Having,
    Order,
    OrderBy,
    Limit,
    Offset,
    Update,
    Set,
    Insert,
    Into,
    Values,
}

/// Ranked keyword/phrase continuations the SQL grammar expects at the caret, plus
/// whether the completion popup should auto-open on an empty prefix here. Feeds the
/// **top tier** of the completion popup so a legal next clause keyword outranks the
/// generic keyword bag (and, after a complete table ref, the schema table names).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Continuation {
    /// Expected next keyword/phrase suggestions, best first.
    pub keywords: Vec<String>,
    /// True when the caret sits right after a clause keyword (or comma) that takes
    /// an operand — `WHERE`/`ON`/`SET`/`ORDER BY`/`FROM`/… — so the popup opens
    /// without a typed prefix (DataGrip-style: columns after `WHERE`, tables after
    /// `FROM`).
    pub auto_show: bool,
}

/// The byte offset to start scanning the caret's local clause sequence from: just
/// inside the innermost unclosed `(` before the caret (so a subquery's clauses
/// don't bleed in from the outer query), else `lo`.
fn local_scope_start(sql: &str, lo: usize, caret: usize) -> usize {
    let toks = tokenize_range(sql, lo, caret);
    let mut stack: Vec<usize> = Vec::new();
    for t in &toks {
        match t.kind {
            TkKind::LParen => stack.push(t.at),
            TkKind::RParen => {
                stack.pop();
            }
            _ => {}
        }
    }
    stack.last().map(|&at| at + 1).unwrap_or(lo)
}

/// Reduce the top-level token sequence before the caret into the current clause,
/// how much of its operand slot is filled, and which clauses have already appeared.
/// Returns `(kind, current_clause, operand_count, seen, select_has_content,
/// select_has_star, distinct_seen)`. `operand_count` resets at each clause keyword
/// *and* comma, so it answers "is the current slot non-empty" (a trailing comma →
/// empty again); `select_has_content` persists across commas (any projection item
/// ever seen), so DISTINCT is only offered before the very first one.
fn scan_clauses(
    sql: &str,
    toks: &[Token],
    scan_end: usize,
) -> (StmtKind, Clause, usize, Vec<Clause>, bool, bool, bool) {
    let word_up = |t: &Token| -> Option<String> {
        if let TkKind::Word(w) = &t.kind {
            Some(w.to_ascii_uppercase())
        } else {
            None
        }
    };
    let mut kind = StmtKind::Other;
    let mut cur = Clause::None;
    let mut operand = 0usize;
    let mut seen: Vec<Clause> = Vec::new();
    let mut select_has_content = false;
    let mut distinct_seen = false;
    let mut select_kw_end = 0usize;
    let mut first = true;
    let push_seen = |seen: &mut Vec<Clause>, c: Clause| {
        if !seen.contains(&c) {
            seen.push(c);
        }
    };
    let mut i = 0;
    while i < toks.len() {
        if let Some(up) = word_up(&toks[i]) {
            if first {
                kind = match up.as_str() {
                    "SELECT" | "WITH" => StmtKind::Select,
                    "INSERT" | "REPLACE" => StmtKind::Insert,
                    "UPDATE" => StmtKind::Update,
                    "DELETE" => StmtKind::Delete,
                    _ => StmtKind::Other,
                };
                first = false;
            }
            match up.as_str() {
                "SELECT" => {
                    cur = Clause::Select;
                    operand = 0;
                    select_kw_end = toks[i].at + 6;
                    push_seen(&mut seen, Clause::Select);
                }
                "FROM" => {
                    cur = Clause::From;
                    operand = 0;
                    push_seen(&mut seen, Clause::From);
                }
                "WHERE" => {
                    cur = Clause::Where;
                    operand = 0;
                    push_seen(&mut seen, Clause::Where);
                }
                "HAVING" => {
                    cur = Clause::Having;
                    operand = 0;
                    push_seen(&mut seen, Clause::Having);
                }
                "LIMIT" => {
                    cur = Clause::Limit;
                    operand = 0;
                    push_seen(&mut seen, Clause::Limit);
                }
                "OFFSET" => {
                    cur = Clause::Offset;
                    operand = 0;
                    push_seen(&mut seen, Clause::Offset);
                }
                "SET" => {
                    cur = Clause::Set;
                    operand = 0;
                    push_seen(&mut seen, Clause::Set);
                }
                "VALUES" => {
                    cur = Clause::Values;
                    operand = 0;
                }
                "ON" => {
                    cur = Clause::On;
                    operand = 0;
                }
                "USING" => {
                    cur = Clause::Using;
                    operand = 0;
                }
                "JOIN" | "STRAIGHT_JOIN" => {
                    cur = Clause::Join;
                    operand = 0;
                    push_seen(&mut seen, Clause::Join);
                }
                "LEFT" | "RIGHT" | "INNER" | "OUTER" | "CROSS" | "FULL" => {
                    cur = Clause::JoinMod;
                    operand = 0;
                }
                "UPDATE" => {
                    cur = Clause::Update;
                    operand = 0;
                }
                "INSERT" | "REPLACE" => {
                    cur = Clause::Insert;
                    operand = 0;
                }
                "INTO" => {
                    cur = Clause::Into;
                    operand = 0;
                }
                "GROUP" => {
                    if toks
                        .get(i + 1)
                        .and_then(&word_up)
                        .as_deref()
                        .is_some_and(|w| w == "BY")
                    {
                        cur = Clause::GroupBy;
                        push_seen(&mut seen, Clause::GroupBy);
                        i += 1;
                    } else {
                        cur = Clause::Group;
                    }
                    operand = 0;
                }
                "ORDER" => {
                    if toks
                        .get(i + 1)
                        .and_then(&word_up)
                        .as_deref()
                        .is_some_and(|w| w == "BY")
                    {
                        cur = Clause::OrderBy;
                        push_seen(&mut seen, Clause::OrderBy);
                        i += 1;
                    } else {
                        cur = Clause::Order;
                    }
                    operand = 0;
                }
                "BY" => {
                    // A `BY` reached on its own (e.g. `GROUP  BY` with extra space):
                    // complete whichever partial clause preceded it.
                    cur = match cur {
                        Clause::Group => {
                            push_seen(&mut seen, Clause::GroupBy);
                            Clause::GroupBy
                        }
                        Clause::Order => {
                            push_seen(&mut seen, Clause::OrderBy);
                            Clause::OrderBy
                        }
                        other => other,
                    };
                    operand = 0;
                }
                // Boolean connectives keep us in the current clause but reopen the
                // operand slot (a new column/value is expected after them).
                "AND" | "OR" => operand = 0,
                // Modifiers that don't fill the operand slot on their own.
                "AS" | "NOT" | "ALL" | "ASC" | "DESC" => {}
                "DISTINCT" => distinct_seen = true,
                _ => {
                    operand += 1;
                    if cur == Clause::Select {
                        select_has_content = true;
                    }
                }
            }
        } else if matches!(toks[i].kind, TkKind::Comma) {
            // A new list item (projection / FROM list / ORDER BY / VALUES) reopens
            // the operand slot.
            operand = 0;
        }
        i += 1;
    }
    // `*` (and `count(*)`) is a projection but the tokenizer drops it, so scan the
    // raw SELECT-clause span (up to the caret's word) for one to know the projection
    // is non-empty.
    let select_has_star = matches!(cur, Clause::Select)
        && sql
            .get(select_kw_end..scan_end)
            .is_some_and(|s| s.contains('*'));
    (
        kind,
        cur,
        operand,
        seen,
        select_has_content,
        select_has_star,
        distinct_seen,
    )
}

/// The keyword/phrase continuations SQL grammar expects at the caret, and whether
/// the popup should auto-open on an empty prefix. `word_lo` is the byte offset where
/// the caret's current (possibly empty) word begins; only tokens strictly before it
/// are considered, so a phrase like `GROUP BY` is offered while `GROUP` is still
/// being typed. Lexer-based (correct mid-edit) and microsecond-cheap — it runs every
/// keystroke.
pub fn clause_continuation(sql: &str, lo: usize, word_lo: usize) -> Continuation {
    let start = local_scope_start(sql, lo, word_lo);
    let toks = tokenize_range(sql, start, word_lo);
    let (kind, cur, operand, seen, select_has_content, select_has_star, distinct_seen) =
        scan_clauses(sql, &toks, word_lo);
    let filled = operand >= 1;
    let has = |c: Clause| seen.contains(&c);
    let mut kws: Vec<&str> = Vec::new();

    // The downstream single-statement clauses, in canonical order, each offered only
    // if not already present — shared by the several "after a complete operand"
    // positions (post table ref, post-condition, …).
    let downstream = |kws: &mut Vec<&str>, from_where: bool| {
        if from_where && !has(Clause::Where) {
            kws.push("WHERE");
        }
        if !has(Clause::GroupBy) {
            kws.push("GROUP BY");
        }
        if !has(Clause::Having) {
            kws.push("HAVING");
        }
        if !has(Clause::OrderBy) {
            kws.push("ORDER BY");
        }
        if !has(Clause::Limit) {
            kws.push("LIMIT");
        }
    };

    match (kind, cur) {
        (StmtKind::Delete, Clause::None) => kws.push("FROM"),
        (_, Clause::Select) => {
            // FROM once the *current* projection item is complete (a column typed,
            // or `*`); a trailing comma reopens the slot (`filled` false → no FROM).
            if filled || select_has_star {
                kws.push("FROM");
            } else if !select_has_content && !distinct_seen {
                // Only right after SELECT with nothing projected yet.
                kws.push("DISTINCT");
            }
        }
        (_, Clause::From) if filled => {
            downstream(&mut kws, true);
            kws.extend(["JOIN", "LEFT JOIN", "RIGHT JOIN", "INNER JOIN", "AS"]);
        }
        (_, Clause::JoinMod) => kws.extend(["JOIN", "OUTER JOIN"]),
        (_, Clause::Join) if filled => {
            kws.extend(["ON", "USING", "AS"]);
            downstream(&mut kws, true);
        }
        (_, Clause::On | Clause::Using) if filled => {
            kws.extend(["AND", "OR"]);
            kws.extend(["JOIN", "LEFT JOIN", "RIGHT JOIN", "INNER JOIN"]);
            downstream(&mut kws, true);
        }
        (_, Clause::Where) if filled => {
            kws.extend([
                "AND",
                "OR",
                "IS NULL",
                "IS NOT NULL",
                "LIKE",
                "IN",
                "BETWEEN",
            ]);
            downstream(&mut kws, false);
        }
        (_, Clause::Group) | (_, Clause::Order) => kws.push("BY"),
        (_, Clause::GroupBy) if filled => downstream(&mut kws, false),
        (_, Clause::Having) if filled => {
            kws.extend(["AND", "OR"]);
            downstream(&mut kws, false);
        }
        (_, Clause::OrderBy) if filled => kws.extend(["ASC", "DESC", "LIMIT"]),
        (_, Clause::Limit) if filled => kws.push("OFFSET"),
        (StmtKind::Update, Clause::Update) if filled => kws.push("SET"),
        (StmtKind::Update, Clause::Set) if filled => kws.push("WHERE"),
        (StmtKind::Insert, Clause::Insert) if !filled => kws.push("INTO"),
        (StmtKind::Insert, Clause::Into) if filled => kws.push("VALUES"),
        _ => {}
    }

    // Auto-open on an empty prefix right after a clause keyword (or comma) that
    // takes an operand — columns after WHERE/ON/BY/SET/HAVING/SELECT, tables after
    // FROM/JOIN/INTO. Not after LIMIT/OFFSET (a bare number) or a filled slot.
    let auto_show = !filled
        && matches!(
            cur,
            Clause::Select
                | Clause::From
                | Clause::Join
                | Clause::On
                | Clause::Using
                | Clause::Where
                | Clause::GroupBy
                | Clause::Having
                | Clause::OrderBy
                | Clause::Set
                | Clause::Into
        );

    Continuation {
        keywords: kws.into_iter().map(|s| s.to_string()).collect(),
        auto_show,
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
                    if is_reserved_word(&name) {
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
                                // A reserved keyword after AS isn't a valid alias
                                // (needs backticks) — don't register it, matching the
                                // implicit-alias arm below. Still consume both tokens.
                                if !is_reserved_word(&al) {
                                    alias = Some(al);
                                }
                                i += 2;
                            }
                        }
                        Some(TkKind::Word(a)) if !is_reserved_word(a) => {
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
    /// table_lower → its primary-key column names (lower, active-db scoped), for
    /// the `only_full_group_by` functional-dependency exemption.
    pks: HashMap<String, Vec<String>>,
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
        let mut pks: HashMap<String, Vec<String>> = HashMap::new();
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
                    let pk: Vec<String> = t
                        .columns
                        .iter()
                        .filter(|c| c.primary_key)
                        .map(|c| c.name.to_ascii_lowercase())
                        .collect();
                    if !pk.is_empty() {
                        pks.insert(t.name.to_ascii_lowercase(), pk);
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
            pks,
            loaded_dbs,
            active_db: active_lower,
            known_idents,
        }
    }

    /// The primary-key columns (lower) of an active-db table, if known and non-empty.
    fn pk_of(&self, table_lower: &str) -> Option<&Vec<String>> {
        self.pks.get(table_lower)
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
            if is_reserved_word(&name) {
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
                        // A reserved keyword after AS isn't a valid alias — don't
                        // register it (matches the implicit arm + `lexer_scope`).
                        if !is_reserved_word(&al) {
                            alias = Some(al);
                        }
                        i += 2;
                    }
                }
                Some(TkKind::Word(a)) if !is_reserved_word(a) => {
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
            Ok(asts) => {
                table_existence_checks(sql, lo, hi, catalog, &mut out);
                match asts.as_slice() {
                    // A single SELECT/query → per-scope column resolution (aware of
                    // subqueries / derived tables / CTEs; qualified + unqualified).
                    [ast @ sqlparser::ast::Statement::Query(_)] => {
                        colres::check(sql, lo, hi, catalog, ast, &mut out)
                    }
                    // Other statements (UPDATE/DELETE/…) → the flat qualified scan.
                    _ => qualified_column_checks(sql, lo, hi, catalog, &mut out),
                }
            }
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
        function_typo_checks(sql, lo, hi, catalog, &mut out);
        // Reserved-keyword aliases (`orders AS or`, `orders or`) run unconditionally:
        // sqlparser is laxer than MySQL here (it *accepts* `AS or`), so gating on a
        // parse failure would miss the very case we want to flag.
        alias_checks(sql, lo, hi, &mut out);
    }
    dedup_diagnostics(out)
}

/// Keywords that legitimately follow `AS` without being an alias: a query/CTAS body
/// (`CREATE TABLE t AS SELECT …`, `CREATE VIEW v AS SELECT …`, `cte AS (SELECT …)`).
fn is_query_body_keyword(word: &str) -> bool {
    let up = word.to_ascii_uppercase();
    matches!(up.as_str(), "SELECT" | "WITH" | "VALUES")
}

/// MySQL/MariaDB **reserved** words — those that can't be used as a bare (unquoted)
/// identifier (table/column/alias) without backticks. This is the authoritative list
/// for [`is_reserved_word`] (the alias check + scope's alias resolution); it's a
/// superset of the small [`SQL_KEYWORDS`] completion set and deliberately excludes
/// non-reserved keywords (`OFFSET`, `VIEW`, `TRUNCATE`, …), which *are* legal aliases.
/// Sourced from MySQL 8.0's "Keywords and Reserved Words" (the `(R)` entries), plus
/// `INTERSECT`. When Postgres joins the [`SqlDialect`] seam this becomes per-dialect.
const MYSQL_RESERVED: &[&str] = &[
    "ACCESSIBLE",
    "ADD",
    "ALL",
    "ALTER",
    "ANALYZE",
    "AND",
    "AS",
    "ASC",
    "ASENSITIVE",
    "BEFORE",
    "BETWEEN",
    "BIGINT",
    "BINARY",
    "BLOB",
    "BOTH",
    "BY",
    "CALL",
    "CASCADE",
    "CASE",
    "CHANGE",
    "CHAR",
    "CHARACTER",
    "CHECK",
    "COLLATE",
    "COLUMN",
    "CONDITION",
    "CONSTRAINT",
    "CONTINUE",
    "CONVERT",
    "CREATE",
    "CROSS",
    "CUBE",
    "CUME_DIST",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "CURRENT_USER",
    "CURSOR",
    "DATABASE",
    "DATABASES",
    "DAY_HOUR",
    "DAY_MICROSECOND",
    "DAY_MINUTE",
    "DAY_SECOND",
    "DEC",
    "DECIMAL",
    "DECLARE",
    "DEFAULT",
    "DELAYED",
    "DELETE",
    "DENSE_RANK",
    "DESC",
    "DESCRIBE",
    "DETERMINISTIC",
    "DISTINCT",
    "DISTINCTROW",
    "DIV",
    "DOUBLE",
    "DROP",
    "DUAL",
    "EACH",
    "ELSE",
    "ELSEIF",
    "EMPTY",
    "ENCLOSED",
    "ESCAPED",
    "EXCEPT",
    "EXISTS",
    "EXIT",
    "EXPLAIN",
    "FALSE",
    "FETCH",
    "FIRST_VALUE",
    "FLOAT",
    "FLOAT4",
    "FLOAT8",
    "FOR",
    "FORCE",
    "FOREIGN",
    "FROM",
    "FULLTEXT",
    "FUNCTION",
    "GENERATED",
    "GET",
    "GRANT",
    "GROUP",
    "GROUPING",
    "GROUPS",
    "HAVING",
    "HIGH_PRIORITY",
    "HOUR_MICROSECOND",
    "HOUR_MINUTE",
    "HOUR_SECOND",
    "IF",
    "IGNORE",
    "IN",
    "INDEX",
    "INFILE",
    "INNER",
    "INOUT",
    "INSENSITIVE",
    "INSERT",
    "INT",
    "INT1",
    "INT2",
    "INT3",
    "INT4",
    "INT8",
    "INTEGER",
    "INTERSECT",
    "INTERVAL",
    "INTO",
    "IO_AFTER_GTIDS",
    "IO_BEFORE_GTIDS",
    "IS",
    "ITERATE",
    "JOIN",
    "JSON_TABLE",
    "KEY",
    "KEYS",
    "KILL",
    "LAG",
    "LAST_VALUE",
    "LATERAL",
    "LEAD",
    "LEADING",
    "LEAVE",
    "LEFT",
    "LIKE",
    "LIMIT",
    "LINEAR",
    "LINES",
    "LOAD",
    "LOCALTIME",
    "LOCALTIMESTAMP",
    "LOCK",
    "LONG",
    "LONGBLOB",
    "LONGTEXT",
    "LOOP",
    "LOW_PRIORITY",
    "MASTER_BIND",
    "MASTER_SSL_VERIFY_SERVER_CERT",
    "MATCH",
    "MAXVALUE",
    "MEDIUMBLOB",
    "MEDIUMINT",
    "MEDIUMTEXT",
    "MIDDLEINT",
    "MINUTE_MICROSECOND",
    "MINUTE_SECOND",
    "MOD",
    "MODIFIES",
    "NATURAL",
    "NOT",
    "NO_WRITE_TO_BINLOG",
    "NTH_VALUE",
    "NTILE",
    "NULL",
    "NUMERIC",
    "OF",
    "ON",
    "OPTIMIZE",
    "OPTIMIZER_COSTS",
    "OPTION",
    "OPTIONALLY",
    "OR",
    "ORDER",
    "OUT",
    "OUTER",
    "OUTFILE",
    "OVER",
    "PARTITION",
    "PERCENT_RANK",
    "PRECISION",
    "PRIMARY",
    "PROCEDURE",
    "PURGE",
    "RANGE",
    "RANK",
    "READ",
    "READS",
    "READ_WRITE",
    "REAL",
    "RECURSIVE",
    "REFERENCES",
    "REGEXP",
    "RELEASE",
    "RENAME",
    "REPEAT",
    "REPLACE",
    "REQUIRE",
    "RESIGNAL",
    "RESTRICT",
    "RETURN",
    "REVOKE",
    "RIGHT",
    "RLIKE",
    "ROW",
    "ROWS",
    "ROW_NUMBER",
    "SCHEMA",
    "SCHEMAS",
    "SECOND_MICROSECOND",
    "SELECT",
    "SENSITIVE",
    "SEPARATOR",
    "SET",
    "SHOW",
    "SIGNAL",
    "SMALLINT",
    "SPATIAL",
    "SPECIFIC",
    "SQL",
    "SQLEXCEPTION",
    "SQLSTATE",
    "SQLWARNING",
    "SQL_BIG_RESULT",
    "SQL_CALC_FOUND_ROWS",
    "SQL_SMALL_RESULT",
    "SSL",
    "STARTING",
    "STORED",
    "STRAIGHT_JOIN",
    "SYSTEM",
    "TABLE",
    "TERMINATED",
    "THEN",
    "TINYBLOB",
    "TINYINT",
    "TINYTEXT",
    "TO",
    "TRAILING",
    "TRIGGER",
    "TRUE",
    "UNDO",
    "UNION",
    "UNIQUE",
    "UNLOCK",
    "UNSIGNED",
    "UPDATE",
    "USAGE",
    "USE",
    "USING",
    "UTC_DATE",
    "UTC_TIME",
    "UTC_TIMESTAMP",
    "VALUES",
    "VARBINARY",
    "VARCHAR",
    "VARCHARACTER",
    "VARYING",
    "VIRTUAL",
    "WHEN",
    "WHERE",
    "WHILE",
    "WINDOW",
    "WITH",
    "WRITE",
    "XOR",
    "YEAR_MONTH",
    "ZEROFILL",
];

/// A word that can't be a bare (unquoted) identifier/alias in MySQL — reserved.
/// Backs the alias diagnostic and the scope's alias resolution so they agree on what
/// counts as a valid alias. See [`MYSQL_RESERVED`].
pub(crate) fn is_reserved_word(word: &str) -> bool {
    let up = word.to_ascii_uppercase();
    MYSQL_RESERVED.contains(&up.as_str())
}

/// Keywords that legitimately follow a *table reference* (so a reserved keyword here
/// is a clause/join continuation, not a botched implicit alias): the join family,
/// the clause boundaries, set operations, MySQL locking/index-hint words, `WINDOW`,
/// and `AS` itself. Anything else reserved in that slot (`OR`, `AND`, `IN`, …) was
/// meant as an alias.
fn is_table_ref_continuation(word: &str) -> bool {
    matches!(
        word.to_ascii_uppercase().as_str(),
        "JOIN"
            | "INNER"
            | "LEFT"
            | "RIGHT"
            | "OUTER"
            | "CROSS"
            | "FULL"
            | "NATURAL"
            | "STRAIGHT_JOIN"
            | "ON"
            | "USING"
            | "WHERE"
            | "GROUP"
            | "ORDER"
            | "HAVING"
            | "LIMIT"
            | "OFFSET"
            | "UNION"
            | "EXCEPT"
            | "INTERSECT"
            | "FOR"
            | "LOCK"
            | "USE"
            | "FORCE"
            | "IGNORE"
            | "PARTITION"
            | "WINDOW"
            | "AS"
    )
}

/// Flag a reserved keyword used as an alias — explicit (`orders AS or`, `id AS key`)
/// or implicit (`orders or`) — a syntax error unless backtick-quoted. Runs
/// unconditionally: sqlparser is laxer than MySQL here (it *accepts* `AS or`), so
/// gating on a parse failure would miss it. Only genuinely-reserved words are flagged
/// ([`is_reserved_word`]) and only where an alias is actually expected, so well-formed
/// SQL isn't squiggled.
fn alias_checks(sql: &str, lo: usize, hi: usize, out: &mut Vec<Diagnostic>) {
    let toks = tokenize_range(sql, lo, hi);
    let flag = |out: &mut Vec<Diagnostic>, at: usize, kw: &str| {
        out.push(Diagnostic {
            range: (at, at + kw.len()),
            severity: Severity::Error,
            message: format!(
                "`{kw}` is a reserved keyword and can't be used as an alias (quote it with backticks)"
            ),
        });
    };
    // Only a CTAS / view (`CREATE … AS SELECT`) legitimately puts a query *body*
    // after `AS`; in a plain SELECT, `col AS select` is a reserved-word alias mistake.
    let is_create = toks
        .iter()
        .find_map(|t| match &t.kind {
            TkKind::Word(w) => Some(w.to_ascii_uppercase()),
            _ => None,
        })
        .is_some_and(|w| w == "CREATE");
    // Explicit `AS <reserved>` — table OR column alias, anywhere.
    for w in toks.windows(2) {
        let (TkKind::Word(a), TkKind::Word(b)) = (&w[0].kind, &w[1].kind) else {
            continue;
        };
        if !a.eq_ignore_ascii_case("AS") || !is_reserved_word(b) {
            continue;
        }
        // A CTAS/view body isn't an alias (`CREATE TABLE t AS SELECT …`).
        if is_create && is_query_body_keyword(b) {
            continue;
        }
        // The alias must sit immediately after AS (whitespace only between). A skipped
        // backtick/quote/comment in the gap means the alias was quoted (`AS `select``)
        // and `b` is the *following* token (e.g. FROM) — never flag that.
        let as_end = w[0].at + a.len();
        if sql
            .get(as_end..w[1].at)
            .is_some_and(|g| g.trim().is_empty())
        {
            flag(out, w[1].at, b);
        }
    }
    // Implicit `<table> <reserved>` in a FROM / JOIN ref position. Restricted to
    // FROM/JOIN (where implicit aliases are idiomatic — not INSERT INTO / UPDATE,
    // whose VALUES/SET/SELECT would false-trigger) and to keywords that aren't a
    // legitimate table-ref continuation, so `WHERE`/`ORDER`/`ON`/`JOIN`/… are safe.
    let word = |k: &TkKind| -> Option<String> {
        if let TkKind::Word(w) = k {
            Some(w.clone())
        } else {
            None
        }
    };
    let mut i = 0;
    while i < toks.len() {
        let Some(kw) = word(&toks[i].kind) else {
            i += 1;
            continue;
        };
        if !matches!(kw.to_ascii_uppercase().as_str(), "FROM" | "JOIN") {
            i += 1;
            continue;
        }
        let is_from = kw.eq_ignore_ascii_case("FROM");
        i += 1;
        while let Some(name) = toks.get(i).and_then(|t| word(&t.kind)) {
            if is_reserved_word(&name) {
                break; // a clause keyword, not a table name (`FROM WHERE …` etc.)
            }
            i += 1;
            // Optional `db.table`.
            if matches!(toks.get(i).map(|t| &t.kind), Some(TkKind::Dot))
                && toks.get(i + 1).and_then(|t| word(&t.kind)).is_some()
            {
                i += 2;
            }
            // The alias slot right after the table name.
            match toks.get(i).map(|t| &t.kind) {
                Some(TkKind::Word(a)) if a.eq_ignore_ascii_case("AS") => {
                    // Handled by the `AS` scan above; consume `AS` + the next token.
                    i += toks.get(i + 1).map_or(1, |_| 2);
                }
                // A clause/join keyword ends this ref — not an alias (check before
                // `is_reserved_word`, since these are reserved too).
                Some(TkKind::Word(a)) if is_table_ref_continuation(a) => break,
                Some(TkKind::Word(a)) if is_reserved_word(a) => {
                    flag(out, toks[i].at, a);
                    break;
                }
                Some(TkKind::Word(_)) => i += 1, // a valid alias (identifier or non-reserved word)
                _ => {}
            }
            // A comma continues the FROM list with another table reference.
            if is_from && matches!(toks.get(i).map(|t| &t.kind), Some(TkKind::Comma)) {
                i += 1;
                continue;
            }
            break;
        }
    }
}

/// Unknown-table checks: flag a FROM/JOIN/UPDATE/INTO table reference the catalog
/// definitively doesn't contain (only when the relevant database is loaded).
fn table_existence_checks(
    sql: &str,
    lo: usize,
    hi: usize,
    catalog: &Catalog,
    out: &mut Vec<Diagnostic>,
) {
    for (r, pos) in table_refs_with_pos(sql, lo, hi) {
        if let TableStatus::NotFound = catalog.table_status(&r) {
            let where_db =
                r.db.as_deref()
                    .map(|d| format!(" in `{d}`"))
                    .unwrap_or_default();
            out.push(Diagnostic {
                range: pos,
                severity: Severity::Error,
                message: format!("Table `{}` not found{where_db}", r.name),
            });
        }
    }
}

/// Flat qualified-column check (`alias.col` / `table.col`): flag a column that
/// definitively isn't in the resolved table. Used for non-SELECT statements
/// (UPDATE/DELETE/…); a SELECT goes through the per-scope resolver instead.
fn qualified_column_checks(
    sql: &str,
    lo: usize,
    hi: usize,
    catalog: &Catalog,
    out: &mut Vec<Diagnostic>,
) {
    let refs = table_refs_with_pos(sql, lo, hi);
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

/// Per-scope unknown-column resolver for a `SELECT`. Unlike the flat
/// [`qualified_column_checks`], this walks the query's scope tree (subqueries,
/// derived tables, CTEs each get their own scope) and resolves every column
/// reference — qualified *and* unqualified — against the sources visible at that
/// point, honouring correlation (an inner scope can see outer columns).
///
/// Design (kept conservative — never squiggle valid SQL):
/// - **AST for classification, spans for position.** sqlparser 0.62 emits accurate
///   per-identifier spans; each column ref is positioned by its own span (so the same
///   name in an inner vs outer scope is placed independently) — a step past the older
///   "lexer for positions" note now that spans are reliable.
/// - Each `Query` node becomes a scope keyed by its byte range; a column ref is
///   resolved against the innermost containing scope and its ancestors.
/// - A source whose columns can't be fully enumerated (unloaded/unknown base table,
///   a derived table / CTE projecting `*` or an unnamed expression) is **open** — any
///   column against it is allowed, so uncertainty never yields a false positive.
mod colres {
    use std::collections::{HashMap, HashSet};
    use std::ops::ControlFlow;

    use sqlparser::ast::{
        Cte, Expr, GroupByExpr, JoinConstraint, JoinOperator, Query, Select, SelectItem, SetExpr,
        Spanned, Statement, TableAlias, TableFactor, TableWithJoins, Visit, Visitor,
    };
    use sqlparser::tokenizer::Span;

    use super::{Catalog, Diagnostic, Severity, TableRef, TableStatus, offset_of_line_col};

    /// The columns a FROM source exposes, or `Open` when they can't be enumerated.
    #[derive(Clone)]
    enum Cols {
        Known(HashSet<String>),
        Open,
    }

    /// One FROM source: the names it can be qualified by (alias / table name, lower),
    /// its columns, and the base-table name for the diagnostic message (if any).
    struct Src {
        quals: Vec<String>,
        cols: Cols,
        table: Option<String>,
    }

    /// A resolution scope: the byte range it spans, its FROM sources, and the output
    /// aliases of its projection (referenceable unqualified in ORDER BY / HAVING).
    struct Scope {
        range: (usize, usize),
        sources: Vec<Src>,
        proj_aliases: HashSet<String>,
        /// Columns coalesced by a `USING(...)` join — an unqualified reference to
        /// one is unambiguous even when several sources expose it.
        coalesced: HashSet<String>,
        /// A `NATURAL` join is present → its coalesced (common) columns can't be
        /// enumerated cheaply, so ambiguity checks are suppressed for this scope.
        natural: bool,
    }

    /// A column reference to resolve, positioned by its identifier span.
    struct Ref {
        qualifier: Option<String>,
        col: String,
        range: (usize, usize),
    }

    /// Entry point: resolve every column ref in the parsed `SELECT` statement.
    pub(super) fn check(
        sql: &str,
        lo: usize,
        hi: usize,
        catalog: &Catalog,
        ast: &Statement,
        out: &mut Vec<Diagnostic>,
    ) {
        let stmt = &sql[lo..hi];
        let mut c = Collector {
            stmt,
            lo,
            catalog,
            ctes: HashMap::new(),
            scopes: Vec::new(),
            refs: Vec::new(),
            gb: Vec::new(),
        };
        let _ = ast.visit(&mut c);
        for r in &c.refs {
            if let Some(d) = resolve(r, &c.scopes) {
                out.push(d);
            }
        }
        out.append(&mut c.gb);
    }

    struct Collector<'a> {
        stmt: &'a str,
        lo: usize,
        catalog: &'a Catalog,
        /// CTE name (lower) → its output columns. Populated as queries are visited.
        ctes: HashMap<String, Cols>,
        scopes: Vec<Scope>,
        refs: Vec<Ref>,
        /// `only_full_group_by` warnings collected per SELECT scope as it's pushed.
        gb: Vec<Diagnostic>,
    }

    impl Visitor for Collector<'_> {
        type Break = ();

        fn pre_visit_query(&mut self, q: &Query) -> ControlFlow<()> {
            // Register this query's CTEs first (visible to its own FROM + siblings).
            if let Some(with) = &q.with {
                for cte in &with.cte_tables {
                    let cols = cte_cols(cte);
                    self.ctes
                        .insert(cte.alias.name.value.to_ascii_lowercase(), cols);
                }
            }
            match q.body.as_ref() {
                // A single SELECT body: the scope spans the whole *query*, so its
                // ORDER BY / LIMIT (which hang off the Query, not the Select) are
                // covered, with the SELECT's sources.
                SetExpr::Select(sel) => {
                    let range = to_range(self.stmt, self.lo, q.span());
                    self.push_scope(range, sel);
                }
                // A set operation (UNION/EXCEPT/INTERSECT): each branch SELECT is its
                // own scope, keyed by its own (disjoint) span, so a column resolves
                // against the branch it sits in. The union's own ORDER BY (which
                // references the *output* columns, positioned past every branch) falls
                // outside all branch scopes → left unchecked, safely.
                body @ SetExpr::SetOperation { .. } => {
                    let mut selects = Vec::new();
                    collect_selects(body, &mut selects);
                    for sel in selects {
                        let range = to_range(self.stmt, self.lo, sel.span());
                        self.push_scope(range, sel);
                    }
                }
                // A parenthesized inner query self-handles via its own
                // pre_visit_query; VALUES/… have no columns to resolve.
                _ => {}
            }
            ControlFlow::Continue(())
        }

        fn pre_visit_expr(&mut self, e: &Expr) -> ControlFlow<()> {
            match e {
                Expr::Identifier(id) => self.refs.push(Ref {
                    qualifier: None,
                    col: id.value.to_ascii_lowercase(),
                    range: to_range(self.stmt, self.lo, id.span),
                }),
                Expr::CompoundIdentifier(parts) if parts.len() >= 2 => {
                    let col = &parts[parts.len() - 1];
                    let qual = &parts[parts.len() - 2];
                    self.refs.push(Ref {
                        qualifier: Some(qual.value.to_ascii_lowercase()),
                        col: col.value.to_ascii_lowercase(),
                        range: to_range(self.stmt, self.lo, col.span),
                    });
                }
                _ => {}
            }
            ControlFlow::Continue(())
        }
    }

    impl Collector<'_> {
        /// Record a scope for `sel` covering byte `range`, resolving its FROM sources
        /// against the catalog + the CTEs registered so far.
        fn push_scope(&mut self, range: (usize, usize), sel: &Select) {
            let (sources, proj_aliases) = build_sources(sel, self.catalog, &self.ctes);
            let (coalesced, natural) = coalesced_cols(sel);
            group_by_check(
                sel,
                &sources,
                self.catalog,
                self.stmt,
                self.lo,
                &mut self.gb,
            );
            cartesian_check(sel, self.stmt, self.lo, &mut self.gb);
            self.scopes.push(Scope {
                range,
                sources,
                proj_aliases,
                coalesced,
                natural,
            });
        }
    }

    /// Gather the leaf SELECTs of a set-expression tree (UNION/EXCEPT/INTERSECT). A
    /// parenthesized branch (`SetExpr::Query`) is left for its own `pre_visit_query`.
    fn collect_selects<'a>(body: &'a SetExpr, out: &mut Vec<&'a Select>) {
        match body {
            SetExpr::Select(s) => out.push(s),
            SetExpr::SetOperation { left, right, .. } => {
                collect_selects(left, out);
                collect_selects(right, out);
            }
            _ => {}
        }
    }

    /// Resolve one column ref against the scopes containing its position (innermost
    /// first). Returns a diagnostic only when the column definitively doesn't exist.
    fn resolve(r: &Ref, scopes: &[Scope]) -> Option<Diagnostic> {
        // Scopes whose range contains this ref, smallest (innermost) first.
        let mut chain: Vec<&Scope> = scopes
            .iter()
            .filter(|s| s.range.0 <= r.range.0 && r.range.1 <= s.range.1)
            .collect();
        chain.sort_by_key(|s| s.range.1 - s.range.0);
        if chain.is_empty() {
            return None;
        }

        match &r.qualifier {
            Some(q) => {
                // Find the source this qualifier names, in this or an enclosing scope.
                for s in &chain {
                    for src in &s.sources {
                        if src.quals.iter().any(|x| x == q) {
                            return match &src.cols {
                                Cols::Open => None,
                                Cols::Known(cols) if cols.contains(&r.col) => None,
                                Cols::Known(_) => Some(err(
                                    r,
                                    &format!(
                                        "Column `{}` not found in `{}`",
                                        r.col,
                                        src.table.clone().unwrap_or_else(|| q.clone())
                                    ),
                                )),
                            };
                        }
                    }
                }
                // Unknown qualifier (db-qualified, or an outer name we didn't model) —
                // don't flag; the table-existence check covers a bad table name.
                None
            }
            None => {
                // Resolve innermost-first (MySQL name resolution): the first scope
                // that supplies the column wins. Within that scope, two *concrete*
                // sources exposing it is an ambiguity error; an unenumerable (`Open`)
                // source means we can't judge, so we stay silent.
                for s in &chain {
                    let mut known_matches = 0usize;
                    let mut has_open = false;
                    for src in &s.sources {
                        match &src.cols {
                            Cols::Open => has_open = true,
                            Cols::Known(cols) if cols.contains(&r.col) => known_matches += 1,
                            Cols::Known(_) => {}
                        }
                    }
                    // Two known sources with the column → ambiguous, unless a
                    // `USING`/`NATURAL` join coalesced it into a single output column.
                    if known_matches >= 2 && !s.natural && !s.coalesced.contains(&r.col) {
                        return Some(err(r, &format!("Column `{}` is ambiguous", r.col)));
                    }
                    if known_matches >= 1 {
                        return None; // resolved unambiguously in this scope
                    }
                    if has_open {
                        return None; // an unenumerable source might provide it
                    }
                    if s.proj_aliases.contains(&r.col) {
                        return None;
                    }
                    // Not in this scope → try the enclosing (correlated) scope.
                }
                // Not found and no open source anywhere in the chain → flag. Name the
                // table only when the innermost scope has a single base-table source.
                let where_tbl = match chain[0].sources.as_slice() {
                    [Src { table: Some(t), .. }] => format!(" in `{t}`"),
                    _ => String::new(),
                };
                Some(err(r, &format!("Column `{}` not found{where_tbl}", r.col)))
            }
        }
    }

    fn err(r: &Ref, message: &str) -> Diagnostic {
        Diagnostic {
            range: r.range,
            severity: Severity::Error,
            message: message.to_string(),
        }
    }

    /// A SELECT's FROM sources + projection output aliases.
    fn build_sources(
        sel: &Select,
        catalog: &Catalog,
        ctes: &HashMap<String, Cols>,
    ) -> (Vec<Src>, HashSet<String>) {
        let mut sources = Vec::new();
        for twj in &sel.from {
            add_source(&twj.relation, &mut sources, catalog, ctes);
            for join in &twj.joins {
                add_source(&join.relation, &mut sources, catalog, ctes);
            }
        }
        (sources, proj_aliases(sel))
    }

    /// Warn on an inner `JOIN`/`INNER JOIN`/`STRAIGHT_JOIN` with no `ON`/`USING`
    /// condition — MySQL treats it as a cross join (every row combined), almost
    /// always an accident. An explicit `CROSS JOIN` (a distinct operator) and a
    /// comma-join (a separate `TableWithJoins`, no join node) are intentional and
    /// left alone. Squiggles the joined relation.
    fn cartesian_check(sel: &Select, stmt: &str, lo: usize, out: &mut Vec<Diagnostic>) {
        fn walk(twj: &TableWithJoins, stmt: &str, lo: usize, out: &mut Vec<Diagnostic>) {
            walk_factor(&twj.relation, stmt, lo, out);
            for join in &twj.joins {
                walk_factor(&join.relation, stmt, lo, out);
                if is_unconstrained_inner_join(&join.join_operator) {
                    out.push(Diagnostic {
                        range: to_range(stmt, lo, join.relation.span()),
                        severity: Severity::Warning,
                        message: "JOIN has no ON/USING condition — this is a cross join; \
                                  use CROSS JOIN if that's intended"
                            .to_string(),
                    });
                }
            }
        }
        fn walk_factor(f: &TableFactor, stmt: &str, lo: usize, out: &mut Vec<Diagnostic>) {
            if let TableFactor::NestedJoin {
                table_with_joins, ..
            } = f
            {
                walk(table_with_joins, stmt, lo, out);
            }
        }
        for twj in &sel.from {
            walk(twj, stmt, lo, out);
        }
    }

    /// A `JOIN`/`INNER JOIN`/`STRAIGHT_JOIN` carrying no constraint (the valid-but-
    /// cartesian forms). `CROSS JOIN` is a separate operator; outer joins without a
    /// constraint don't parse in MySQL, so they never reach here.
    fn is_unconstrained_inner_join(op: &JoinOperator) -> bool {
        use JoinOperator::*;
        matches!(
            op,
            Join(JoinConstraint::None)
                | Inner(JoinConstraint::None)
                | StraightJoin(JoinConstraint::None)
        )
    }

    /// `only_full_group_by`: with a `GROUP BY`, every projected column must be a
    /// grouping column or live inside an aggregate. Deliberately conservative — it
    /// fires only for a *single, fully-known base table* (multi-source / derived /
    /// unloaded FROMs are skipped), skips non-column grouping expressions and
    /// wildcards, and exempts the whole query when the grouping set contains the
    /// table's full primary key (functional dependency, MySQL's own exemption).
    fn group_by_check(
        sel: &Select,
        sources: &[Src],
        catalog: &Catalog,
        stmt: &str,
        lo: usize,
        out: &mut Vec<Diagnostic>,
    ) {
        let GroupByExpr::Expressions(exprs, _) = &sel.group_by else {
            return;
        };
        if exprs.is_empty() {
            return;
        }
        // The grouping set, as simple column names. A non-column grouping expression
        // (e.g. `GROUP BY YEAR(hired)`) is too complex to reason about → bail.
        let mut grouped: HashSet<String> = HashSet::new();
        for e in exprs {
            match e {
                Expr::Identifier(id) => {
                    grouped.insert(id.value.to_ascii_lowercase());
                }
                Expr::CompoundIdentifier(parts) if !parts.is_empty() => {
                    grouped.insert(parts[parts.len() - 1].value.to_ascii_lowercase());
                }
                _ => return,
            }
        }
        // Need exactly one base table with a known column set to judge safely.
        let [src] = sources else { return };
        let Cols::Known(known) = &src.cols else {
            return;
        };
        let Some(table) = &src.table else { return };
        // Grouping by the full primary key functionally determines every column.
        if let Some(pk) = catalog.pk_of(&table.to_ascii_lowercase())
            && pk.iter().all(|k| grouped.contains(k))
        {
            return;
        }
        for item in &sel.projection {
            let expr = match item {
                SelectItem::UnnamedExpr(e) => e,
                SelectItem::ExprWithAlias { expr, .. } => expr,
                _ => continue, // `*` / `t.*` → can't enumerate, skip
            };
            let (name, span) = match expr {
                Expr::Identifier(id) => (id.value.to_ascii_lowercase(), id.span),
                Expr::CompoundIdentifier(parts) if !parts.is_empty() => {
                    let last = &parts[parts.len() - 1];
                    (last.value.to_ascii_lowercase(), last.span)
                }
                _ => continue, // aggregate / expression / literal → not a bare column
            };
            if grouped.contains(&name) || !known.contains(&name) {
                // Grouped (fine) or not a real column of this table (an
                // unknown-column error already covers it — don't double-flag).
                continue;
            }
            out.push(Diagnostic {
                range: to_range(stmt, lo, span),
                severity: Severity::Warning,
                message: format!(
                    "Column `{name}` must appear in GROUP BY or be used in an aggregate"
                ),
            });
        }
    }

    /// Columns coalesced by a `USING(...)` join in this SELECT's FROM (lowercased),
    /// plus whether a `NATURAL` join is present. A reference to a coalesced column is
    /// unambiguous even when several sources expose it; NATURAL suppresses the check
    /// entirely (its common-column set isn't enumerated here).
    fn coalesced_cols(sel: &Select) -> (HashSet<String>, bool) {
        let mut cols = HashSet::new();
        let mut natural = false;
        fn walk(twj: &TableWithJoins, cols: &mut HashSet<String>, natural: &mut bool) {
            walk_factor(&twj.relation, cols, natural);
            for join in &twj.joins {
                walk_factor(&join.relation, cols, natural);
                constraint(&join.join_operator, cols, natural);
            }
        }
        fn walk_factor(f: &TableFactor, cols: &mut HashSet<String>, natural: &mut bool) {
            if let TableFactor::NestedJoin {
                table_with_joins, ..
            } = f
            {
                walk(table_with_joins, cols, natural);
            }
        }
        fn constraint(op: &JoinOperator, cols: &mut HashSet<String>, natural: &mut bool) {
            match join_constraint(op) {
                Some(JoinConstraint::Using(names)) => {
                    for n in names {
                        if let Some(id) = n.0.last().and_then(|p| p.as_ident()) {
                            cols.insert(id.value.to_ascii_lowercase());
                        }
                    }
                }
                Some(JoinConstraint::Natural) => *natural = true,
                _ => {}
            }
        }
        for twj in &sel.from {
            walk(twj, &mut cols, &mut natural);
        }
        (cols, natural)
    }

    /// The `JoinConstraint` carried by a join operator, if any (the `APPLY`/`ARRAY
    /// JOIN` forms carry none).
    fn join_constraint(op: &JoinOperator) -> Option<&JoinConstraint> {
        use JoinOperator::*;
        match op {
            Join(c) | Inner(c) | Left(c) | LeftOuter(c) | Right(c) | RightOuter(c)
            | FullOuter(c) | CrossJoin(c) | Semi(c) | LeftSemi(c) | RightSemi(c) | Anti(c)
            | LeftAnti(c) | RightAnti(c) | StraightJoin(c) => Some(c),
            AsOf { constraint, .. } => Some(constraint),
            CrossApply | OuterApply | ArrayJoin | LeftArrayJoin | InnerArrayJoin => None,
        }
    }

    fn add_source(
        factor: &TableFactor,
        sources: &mut Vec<Src>,
        catalog: &Catalog,
        ctes: &HashMap<String, Cols>,
    ) {
        match factor {
            TableFactor::Table {
                name, alias, args, ..
            } => {
                // A table-valued function's columns are unknowable → open.
                if args.is_some() {
                    sources.push(open_src());
                    return;
                }
                let parts: Vec<String> = name.0.iter().map(|p| p.to_string()).collect();
                let (db, tname) = match parts.as_slice() {
                    [t] => (None, t.clone()),
                    [.., d, t] => (Some(d.clone()), t.clone()),
                    [] => return,
                };
                let alias_name = alias.as_ref().map(|a| a.name.value.clone());
                let quals: Vec<String> = std::iter::once(&tname)
                    .chain(alias_name.as_ref())
                    .map(|s| s.to_ascii_lowercase())
                    .collect();
                // A bare name matching a CTE resolves to the CTE's columns.
                if db.is_none()
                    && let Some(cols) = ctes.get(&tname.to_ascii_lowercase())
                {
                    sources.push(Src {
                        quals,
                        cols: cols.clone(),
                        table: None,
                    });
                    return;
                }
                let tref = TableRef {
                    name: tname.clone(),
                    alias: alias_name,
                    db,
                };
                let cols = match catalog.table_status(&tref) {
                    TableStatus::Found => catalog
                        .columns_of(&tref)
                        .map(|c| Cols::Known(c.iter().map(|s| s.to_ascii_lowercase()).collect()))
                        .unwrap_or(Cols::Open),
                    _ => Cols::Open, // not loaded / not found → can't judge its columns
                };
                sources.push(Src {
                    quals,
                    cols,
                    table: Some(tname),
                });
            }
            TableFactor::Derived {
                subquery, alias, ..
            } => {
                let quals = alias
                    .as_ref()
                    .map(|a| vec![a.name.value.to_ascii_lowercase()])
                    .unwrap_or_default();
                sources.push(Src {
                    quals,
                    cols: output_cols(subquery),
                    table: None,
                });
            }
            TableFactor::NestedJoin {
                table_with_joins, ..
            } => {
                add_source(&table_with_joins.relation, sources, catalog, ctes);
                for join in &table_with_joins.joins {
                    add_source(&join.relation, sources, catalog, ctes);
                }
            }
            _ => sources.push(open_src()),
        }
    }

    /// The output column names of a subquery/CTE body, or `Open` when they can't be
    /// cleanly enumerated (`SELECT *`, a set-op, or an unnamed non-column expression).
    fn output_cols(q: &Query) -> Cols {
        let SetExpr::Select(sel) = q.body.as_ref() else {
            return Cols::Open;
        };
        let mut names = HashSet::new();
        for item in &sel.projection {
            match item {
                SelectItem::UnnamedExpr(Expr::Identifier(id)) => {
                    names.insert(id.value.to_ascii_lowercase());
                }
                SelectItem::UnnamedExpr(Expr::CompoundIdentifier(parts)) if !parts.is_empty() => {
                    names.insert(parts[parts.len() - 1].value.to_ascii_lowercase());
                }
                SelectItem::ExprWithAlias { alias, .. } => {
                    names.insert(alias.value.to_ascii_lowercase());
                }
                // `*`, `t.*`, multi-alias, or an unnamed expression → can't enumerate.
                _ => return Cols::Open,
            }
        }
        Cols::Known(names)
    }

    /// A CTE's output columns: its explicit column list if given, else its body's.
    fn cte_cols(cte: &Cte) -> Cols {
        let TableAlias { columns, .. } = &cte.alias;
        if !columns.is_empty() {
            return Cols::Known(
                columns
                    .iter()
                    .map(|c| c.name.value.to_ascii_lowercase())
                    .collect(),
            );
        }
        output_cols(&cte.query)
    }

    fn proj_aliases(sel: &Select) -> HashSet<String> {
        let mut out = HashSet::new();
        for item in &sel.projection {
            match item {
                SelectItem::ExprWithAlias { alias, .. } => {
                    out.insert(alias.value.to_ascii_lowercase());
                }
                SelectItem::ExprWithAliases { aliases, .. } => {
                    out.extend(aliases.iter().map(|a| a.value.to_ascii_lowercase()));
                }
                _ => {}
            }
        }
        out
    }

    fn open_src() -> Src {
        Src {
            quals: Vec::new(),
            cols: Cols::Open,
            table: None,
        }
    }

    /// A sqlparser span (line/col, relative to `stmt`) → absolute byte range in the
    /// buffer. Spans are 1-based, end-exclusive; `offset_of_line_col` clamps.
    fn to_range(stmt: &str, lo: usize, s: Span) -> (usize, usize) {
        (
            lo + offset_of_line_col(stmt, s.start.line, s.start.column),
            lo + offset_of_line_col(stmt, s.end.line, s.end.column),
        )
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

/// Flag a word in **function-call position** (`word(`) that is a near-miss of a
/// known builtin but isn't itself one — a probable typo like `COUTN(...)` for
/// `COUNT(...)`. Conservative: the name must be within edit distance of an entry in
/// [`KNOWN_FUNCTIONS`] (the broad builtin set), so user-defined functions and
/// unlisted builtins pass through untouched; qualified calls (`pkg.func(`) and real
/// schema identifiers are skipped too.
fn function_typo_checks(
    sql: &str,
    lo: usize,
    hi: usize,
    catalog: &Catalog,
    out: &mut Vec<Diagnostic>,
) {
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
            // A call only if the next non-blank byte is `(`.
            let mut k = j;
            while k < hi && (b[k] == b' ' || b[k] == b'\t') {
                k += 1;
            }
            let is_call = k < hi && b[k] == b'(';
            let word = &sql[s..j];
            let lw = word.to_ascii_lowercase();
            let qualified = s > lo && b[s - 1] == b'.';
            if is_call
                && !qualified
                && !is_known_function(&lw)
                && !is_sql_keyword(word)
                && !STMT_KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(word))
                && !catalog.known_idents.contains(&lw)
                && is_probable_function_typo(word)
            {
                out.push(Diagnostic {
                    range: (s, j),
                    severity: Severity::Warning,
                    message: format!("`{word}` looks like a misspelled function"),
                });
            }
            i = j;
            continue;
        }
        i += 1;
    }
}

/// Case-insensitive membership in the broad builtin set.
fn is_known_function(word_lower: &str) -> bool {
    KNOWN_FUNCTIONS
        .iter()
        .any(|f| f.eq_ignore_ascii_case(word_lower))
}

/// Is `word` a near-miss of a known builtin function name? A near-miss is a small
/// Levenshtein distance (1, or 2 for longer names) *or* a single adjacent
/// transposition (`COUTN`↔`COUNT`) — the latter is distance 2 under plain
/// Levenshtein but by far the most common typo, so it's matched explicitly rather
/// than by loosening the distance threshold (which would flag names like
/// `format_x` as a typo of `FORMAT`).
fn is_probable_function_typo(word: &str) -> bool {
    if word.len() < 4 {
        return false;
    }
    let up = word.to_ascii_uppercase();
    let thresh = if word.len() >= 7 { 2 } else { 1 };
    KNOWN_FUNCTIONS.iter().any(|f| {
        let close = (f.len() as isize - up.len() as isize).unsigned_abs() <= thresh
            && crate::sql::edit_distance(&up, f) <= thresh;
        close || is_adjacent_transposition(up.as_bytes(), f.as_bytes())
    })
}

/// True when `a` becomes `b` by swapping exactly one adjacent pair of characters.
fn is_adjacent_transposition(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let diffs: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
    diffs.len() == 2
        && diffs[1] == diffs[0] + 1
        && a[diffs[0]] == b[diffs[1]]
        && a[diffs[1]] == b[diffs[0]]
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

    /// All FK entries (`table_lower` → its edges) — for enumerating *reverse* FK
    /// relationships (a table that references one already in scope).
    fn all_fks(&self) -> impl Iterator<Item = (&String, &Vec<FkEdge>)> {
        self.fks.iter()
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

/// A foreign-key-connected table to offer at a `JOIN` slot: the table to insert and
/// the ready-to-write `ON` predicate linking it to a table already in scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinTarget {
    /// Candidate table name to join.
    pub table: String,
    /// The `ON` predicate (without the `ON` keyword), e.g. `orders.customer_id = c.id`.
    pub predicate: String,
}

/// FK-aware JOIN targets for completion. When the caret sits at a `JOIN` table slot
/// (the last clause keyword before the partial name is `JOIN`, nothing else typed),
/// returns every table connected by a foreign key — in either direction — to a table
/// already in scope, each with a ready-to-insert `ON` predicate. The completion layer
/// offers these at the top of the table list, inserting `table ON <predicate>`.
/// Empty when the caret isn't at a JOIN slot or nothing is FK-connected.
pub fn join_targets(
    sql: &str,
    lo: usize,
    hi: usize,
    caret: usize,
    catalog: &Catalog,
) -> Vec<JoinTarget> {
    let toks = tokenize_range(sql, lo, hi);
    let word_lo = word_start(sql, caret);
    // The last clause keyword strictly before the partial table name must be `JOIN`.
    let mut join_idx = None;
    for (i, t) in toks.iter().enumerate() {
        if t.at >= word_lo {
            break;
        }
        if let TkKind::Word(w) = &t.kind
            && CLAUSE_KEYWORDS.contains(&w.to_ascii_uppercase().as_str())
        {
            join_idx = w.eq_ignore_ascii_case("JOIN").then_some(i);
        }
    }
    let Some(join_idx) = join_idx else {
        return Vec::new();
    };
    // No complete table already typed between `JOIN` and the partial prefix.
    for t in &toks[join_idx + 1..] {
        if t.at >= word_lo {
            break;
        }
        if matches!(t.kind, TkKind::Word(_)) {
            return Vec::new();
        }
    }
    let scope = statement_scope(sql, lo, hi, caret, SqlDialect::MySql).tables;
    join_targets_for(&scope, catalog)
}

/// The FK-connected join candidates for a set of in-scope tables (both edge
/// directions), deduped by candidate table.
fn join_targets_for(scope: &[TableRef], catalog: &Catalog) -> Vec<JoinTarget> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let in_scope = |name: &str| scope.iter().any(|s| s.name.eq_ignore_ascii_case(name));
    // Forward: an in-scope table's FK references candidate `T`.
    for s in scope {
        if let Some(edges) = catalog.fks_of(&s.name) {
            for e in edges {
                if in_scope(&e.ref_table) || !seen.insert(e.ref_table.to_ascii_lowercase()) {
                    continue;
                }
                let cand = TableRef {
                    name: e.ref_table.clone(),
                    alias: None,
                    db: None,
                };
                out.push(JoinTarget {
                    table: e.ref_table.clone(),
                    predicate: build_predicate(s, &e.columns, &cand, &e.ref_columns),
                });
            }
        }
    }
    // Reverse: a candidate table `T` has an FK referencing an in-scope table.
    for (t, edges) in catalog.all_fks() {
        if in_scope(t) || seen.contains(t) {
            continue;
        }
        for e in edges {
            if let Some(s) = scope
                .iter()
                .find(|s| e.ref_table.eq_ignore_ascii_case(&s.name))
            {
                let cand = TableRef {
                    name: t.clone(),
                    alias: None,
                    db: None,
                };
                if seen.insert(t.clone()) {
                    out.push(JoinTarget {
                        table: t.clone(),
                        predicate: build_predicate(&cand, &e.columns, s, &e.ref_columns),
                    });
                }
                break;
            }
        }
    }
    out
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

    // ── clause_continuation (expected-token model) ────────────────────────────

    fn cont_at(sql: &str, caret: usize) -> Continuation {
        let word_lo = word_start(sql, caret);
        clause_continuation(sql, 0, word_lo)
    }
    fn cont(sql: &str) -> Continuation {
        cont_at(sql, sql.len())
    }
    fn kws(sql: &str) -> Vec<String> {
        cont(sql).keywords
    }

    #[test]
    fn continuation_from_ranks_after_projection() {
        // `select * f` → FROM is the expected continuation (the projection is
        // complete via `*`), so the ranker can lift it to #1.
        assert_eq!(kws("SELECT * f"), vec!["FROM"]);
        // A named projection column, mid-typing the next keyword.
        assert_eq!(kws("SELECT id f"), vec!["FROM"]);
    }

    #[test]
    fn continuation_distinct_right_after_select() {
        // Right after SELECT (nothing projected yet) → DISTINCT, not FROM.
        assert_eq!(kws("SELECT d"), vec!["DISTINCT"]);
        // Already `DISTINCT` typed → don't re-offer it (and no complete projection
        // yet, so no FROM either).
        assert!(!kws("SELECT DISTINCT ").contains(&"DISTINCT".to_string()));
        // Once a projection column is complete, FROM follows (even with DISTINCT).
        assert_eq!(kws("SELECT DISTINCT id f"), vec!["FROM"]);
    }

    #[test]
    fn continuation_no_from_mid_projection_list() {
        // A trailing comma reopens the projection slot → don't offer FROM (the user
        // is adding another column); auto-show columns instead.
        let c = cont("SELECT id, ");
        assert!(
            !c.keywords.contains(&"FROM".to_string()),
            "got {:?}",
            c.keywords
        );
        assert!(c.auto_show);
    }

    #[test]
    fn continuation_where_after_complete_table_ref() {
        // `select * from table w` → WHERE must be an expected continuation (today's
        // known miss: context stayed in "table" mode).
        let k = kws("SELECT * FROM users w");
        assert!(k.contains(&"WHERE".to_string()), "got {k:?}");
        assert!(k.contains(&"GROUP BY".to_string()));
        assert!(k.contains(&"ORDER BY".to_string()));
        assert!(k.contains(&"LIMIT".to_string()));
        // An alias'd ref is just as "complete".
        assert!(kws("SELECT * FROM users u w").contains(&"WHERE".to_string()));
    }

    #[test]
    fn continuation_no_keywords_immediately_after_from() {
        // Right after FROM (no table yet) → no keyword continuations (tables come
        // from the schema), but auto-show so the table list opens.
        let c = cont("SELECT * FROM ");
        assert!(c.keywords.is_empty(), "got {:?}", c.keywords);
        assert!(c.auto_show);
    }

    #[test]
    fn continuation_group_by_is_one_phrase() {
        // Typing `GROUP` after a complete FROM → offer `GROUP BY` as one item.
        assert!(kws("SELECT * FROM t GROUP").contains(&"GROUP BY".to_string()));
        // After `GROUP ` (space) the partial clause expects `BY`.
        assert_eq!(kws("SELECT * FROM t GROUP "), vec!["BY"]);
    }

    #[test]
    fn continuation_order_by_offers_columns_then_direction() {
        // `order by ` (trailing space) → auto-show columns (no keyword clutter).
        let c = cont("SELECT * FROM t ORDER BY ");
        assert!(c.auto_show);
        assert!(c.keywords.is_empty(), "got {:?}", c.keywords);
        // After a sort column → ASC/DESC/LIMIT.
        let k = kws("SELECT * FROM t ORDER BY name ");
        assert!(k.contains(&"ASC".to_string()) && k.contains(&"DESC".to_string()));
        assert!(k.contains(&"LIMIT".to_string()));
    }

    #[test]
    fn continuation_where_condition_offers_connectives() {
        let k = kws("SELECT * FROM t WHERE id = 1 ");
        assert!(k.contains(&"AND".to_string()) && k.contains(&"OR".to_string()));
        assert!(k.contains(&"ORDER BY".to_string()));
        // A completed WHERE isn't re-offered downstream.
        assert!(!k.contains(&"WHERE".to_string()));
    }

    #[test]
    fn continuation_join_then_on() {
        // After the joined table name → ON/USING.
        let k = kws("SELECT * FROM a JOIN b ");
        assert!(k.contains(&"ON".to_string()) && k.contains(&"USING".to_string()));
        // A bare join modifier still expects JOIN.
        assert_eq!(kws("SELECT * FROM a LEFT "), vec!["JOIN", "OUTER JOIN"]);
    }

    #[test]
    fn continuation_auto_show_after_operand_keywords() {
        // Columns auto-show after WHERE/SET/ON and after a comma in a list.
        assert!(cont("SELECT * FROM t WHERE ").auto_show);
        assert!(cont("SELECT * FROM t ORDER BY a, ").auto_show);
        assert!(cont("UPDATE t SET ").auto_show);
        // But not after a filled slot (a plain trailing space post-table-ref).
        assert!(!cont("SELECT * FROM t ").auto_show);
        // And not after LIMIT (a bare number is expected, not a suggestion).
        assert!(!cont("SELECT * FROM t LIMIT ").auto_show);
    }

    #[test]
    fn continuation_update_and_insert_and_delete() {
        assert!(kws("UPDATE employees ").contains(&"SET".to_string()));
        assert!(kws("UPDATE employees SET salary = 1 ").contains(&"WHERE".to_string()));
        assert!(kws("INSERT ").contains(&"INTO".to_string()));
        assert!(kws("INSERT INTO t ").contains(&"VALUES".to_string()));
        assert!(kws("DELETE ").contains(&"FROM".to_string()));
    }

    #[test]
    fn continuation_scopes_to_subquery() {
        // The caret inside a subquery shouldn't inherit the outer query's FROM as a
        // completed slot — here the inner `SELECT * ` still expects FROM.
        let sql = "SELECT * FROM (SELECT * f";
        assert_eq!(kws(sql), vec!["FROM"]);
    }

    #[test]
    fn continuation_empty_for_non_dml() {
        // A statement start / DDL we don't model → no continuations, no auto-show.
        assert_eq!(cont("CREATE TABLE t "), Continuation::default());
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

    // ── unqualified unknown columns ────────────────────────────────────────────

    fn col_errors(sql: &str) -> Vec<String> {
        diag(sql)
            .into_iter()
            .filter(|d| d.message.starts_with("Column "))
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn diag_unqualified_unknown_column_flagged() {
        let d = diag("SELECT salery FROM employees");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].severity, Severity::Error);
        assert!(
            d[0].message
                .contains("Column `salery` not found in `employees`")
        );
        assert_eq!(
            &"SELECT salery FROM employees"[d[0].range.0..d[0].range.1],
            "salery"
        );
    }

    #[test]
    fn diag_unqualified_unknown_in_where_and_order() {
        // Multiple positions, WHERE + ORDER BY.
        assert!(col_errors("SELECT id FROM employees WHERE nope = 1").len() == 1);
        assert!(col_errors("SELECT id FROM employees ORDER BY nope").len() == 1);
        // Every occurrence squiggles.
        assert_eq!(col_errors("SELECT nope, nope FROM employees").len(), 2);
    }

    #[test]
    fn diag_unqualified_known_columns_clean() {
        for sql in [
            "SELECT id, name, salary, dept_id FROM employees",
            "SELECT name FROM employees WHERE salary > 100 ORDER BY name",
            "SELECT dept_id FROM employees GROUP BY dept_id HAVING COUNT(*) > 1",
            // A column unique to one joined table resolves (and isn't ambiguous).
            "SELECT salary FROM employees e JOIN departments d ON e.dept_id = d.id",
            "SELECT salary FROM employees, departments",
            // Projection alias referenced downstream is valid.
            "SELECT salary AS s FROM employees ORDER BY s",
            // Functions / interval units are not columns (must not false-positive).
            "SELECT COUNT(*), MAX(salary) FROM employees",
            "SELECT DATE_ADD(id, INTERVAL 1 DAY) FROM employees",
            // Columns nested in CASE / functions / arithmetic / operators.
            "SELECT CASE WHEN salary > 100 THEN name ELSE dept_id END FROM employees",
            "SELECT UPPER(TRIM(name)) FROM employees",
            "SELECT id + salary AS total FROM employees ORDER BY total",
            "SELECT id FROM employees WHERE name LIKE '%a%' AND salary BETWEEN 1 AND 2",
            "SELECT id FROM employees WHERE name IS NOT NULL",
            // Mixed qualified + unqualified reference to the same column.
            "SELECT id FROM employees e WHERE e.salary > salary",
        ] {
            assert!(
                col_errors(sql).is_empty(),
                "false positive: {sql} -> {:?}",
                col_errors(sql)
            );
        }
    }

    #[test]
    fn diag_ambiguous_unqualified_column() {
        // `name` exists in BOTH joined tables → ambiguous (MySQL errors here).
        let sql = "SELECT name FROM employees e JOIN departments d ON e.dept_id = d.id";
        let d = diag(sql);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].severity, Severity::Error);
        assert!(d[0].message.contains("ambiguous"), "{}", d[0].message);
        // The squiggle covers the offending column.
        assert_eq!(&sql[d[0].range.0..d[0].range.1], "name");
        // Comma-join is the same shape.
        assert!(
            col_errors("SELECT id FROM employees, departments")
                .iter()
                .any(|m| m.contains("ambiguous"))
        );
        // A column unique to one side resolves cleanly.
        assert!(
            col_errors("SELECT salary FROM employees e JOIN departments d ON e.dept_id = d.id")
                .is_empty()
        );
        // A qualified reference is never ambiguous.
        assert!(
            col_errors("SELECT e.name FROM employees e JOIN departments d ON e.dept_id = d.id")
                .is_empty()
        );
    }

    #[test]
    fn diag_ambiguous_suppressed_when_coalesced_or_uncertain() {
        // `USING(id)` coalesces `id` into one output column → unambiguous.
        assert!(col_errors("SELECT id FROM employees e JOIN departments d USING (id)").is_empty());
        // Both sources unenumerable (`SELECT *`) → can't prove ambiguity → silent.
        assert!(
            col_errors(
                "SELECT id FROM (SELECT * FROM employees) a JOIN (SELECT * FROM departments) b ON a.id = b.id"
            )
            .is_empty()
        );
        // A single table is never ambiguous.
        assert!(col_errors("SELECT id FROM employees").is_empty());
    }

    // ── only_full_group_by ─────────────────────────────────────────────────────

    fn gb_warnings(sql: &str) -> Vec<String> {
        diag(sql)
            .into_iter()
            .filter(|d| d.severity == Severity::Warning && d.message.contains("GROUP BY"))
            .map(|d| d.message)
            .collect()
    }

    /// Like `diag`, but with `employees.id` marked as the primary key.
    fn diag_pk(sql: &str) -> Vec<Diagnostic> {
        let mut emp = tbl("employees", &["id", "name", "salary", "dept_id"]);
        emp.columns[0].primary_key = true;
        let schema = DbSchema {
            tables: vec![emp, tbl("departments", &["id", "name"])],
        };
        let cat = Catalog::build(&[("company", &schema)], Some("company"));
        diagnostics(sql, &cat, SqlDialect::MySql)
    }

    #[test]
    fn diag_only_full_group_by_flags_ungrouped_column() {
        // `name` is neither grouped nor aggregated → warning on the column itself.
        let sql = "SELECT dept_id, name FROM employees GROUP BY dept_id";
        let w: Vec<_> = diag(sql)
            .into_iter()
            .filter(|x| x.severity == Severity::Warning && x.message.contains("GROUP BY"))
            .collect();
        assert_eq!(w.len(), 1);
        assert_eq!(&sql[w[0].range.0..w[0].range.1], "name");
    }

    #[test]
    fn diag_only_full_group_by_clean_cases() {
        for sql in [
            "SELECT dept_id, COUNT(*) FROM employees GROUP BY dept_id",
            "SELECT dept_id, SUM(salary) FROM employees GROUP BY dept_id",
            "SELECT dept_id FROM employees GROUP BY dept_id HAVING COUNT(*) > 1",
            "SELECT COUNT(*) FROM employees", // no GROUP BY at all
            "SELECT name, salary FROM employees GROUP BY name, salary", // all grouped
            "SELECT * FROM employees GROUP BY dept_id", // wildcard → skip
        ] {
            assert!(gb_warnings(sql).is_empty(), "false positive: {sql}");
        }
    }

    #[test]
    fn diag_only_full_group_by_pk_functional_dependency() {
        // Grouping by the primary key determines every column → no violation.
        assert!(
            diag_pk("SELECT id, name, salary FROM employees GROUP BY id")
                .iter()
                .all(|d| !d.message.contains("GROUP BY"))
        );
        // Grouping by a non-PK column still flags the ungrouped projection.
        assert!(
            diag_pk("SELECT id, name FROM employees GROUP BY dept_id")
                .iter()
                .any(|d| d.message.contains("GROUP BY"))
        );
    }

    #[test]
    fn diag_only_full_group_by_conservative_on_multi_source() {
        // Multiple sources → skipped (avoid false positives across functional deps).
        assert!(
            gb_warnings(
                "SELECT e.name, d.name FROM employees e JOIN departments d ON e.dept_id = d.id GROUP BY e.dept_id"
            )
            .is_empty()
        );
        // A non-column grouping expression → skipped.
        assert!(gb_warnings("SELECT name FROM employees GROUP BY UPPER(name)").is_empty());
    }

    #[test]
    fn diag_only_full_group_by_inside_subquery() {
        // The rule applies to a derived table's own SELECT too.
        let sql = "SELECT * FROM (SELECT dept_id, name FROM employees GROUP BY dept_id) d";
        assert!(gb_warnings(sql).iter().any(|m| m.contains("`name`")));
    }

    // ── cartesian joins ────────────────────────────────────────────────────────

    fn cross_warnings(sql: &str) -> Vec<Diagnostic> {
        diag(sql)
            .into_iter()
            .filter(|d| d.message.contains("cross join"))
            .collect()
    }

    #[test]
    fn diag_unconstrained_join_flagged() {
        let sql = "SELECT * FROM employees e JOIN departments d";
        let w = cross_warnings(sql);
        assert_eq!(w.len(), 1, "{:?}", diag(sql));
        assert_eq!(w[0].severity, Severity::Warning);
        assert_eq!(&sql[w[0].range.0..w[0].range.1], "departments d");
        // `INNER JOIN` with no ON is the same.
        assert_eq!(
            cross_warnings("SELECT * FROM employees INNER JOIN departments").len(),
            1
        );
    }

    #[test]
    fn diag_cross_join_and_conditioned_joins_clean() {
        for sql in [
            "SELECT * FROM employees e CROSS JOIN departments d", // explicit → intended
            "SELECT * FROM employees e JOIN departments d ON e.dept_id = d.id",
            "SELECT * FROM employees e JOIN departments d USING (id)",
            "SELECT * FROM employees, departments", // comma-join → not flagged
            "SELECT * FROM employees",
        ] {
            assert!(cross_warnings(sql).is_empty(), "false positive: {sql}");
        }
    }

    #[test]
    fn diag_column_per_scope_subquery() {
        // Unknown in the OUTER scope is flagged; the inner subquery resolves against
        // its own table.
        assert_eq!(
            col_errors("SELECT nope FROM employees WHERE id IN (SELECT id FROM departments)"),
            vec!["Column `nope` not found in `employees`"]
        );
        // Unknown INSIDE the subquery is flagged (positioned within it).
        let sql = "SELECT id FROM employees WHERE id IN (SELECT bogus FROM departments)";
        let d = diag(sql);
        assert!(d.iter().any(|x| x.message.contains("`bogus`")));
        assert_eq!(&sql[d[0].range.0..d[0].range.1], "bogus");
        // A clean nested query has no errors.
        assert!(
            col_errors("SELECT id FROM employees WHERE id IN (SELECT id FROM departments)")
                .is_empty()
        );
    }

    #[test]
    fn diag_column_correlated_subquery_sees_outer() {
        // The inner subquery may reference an outer table's column (correlation).
        assert!(
            col_errors(
                "SELECT id FROM employees e WHERE EXISTS \
                 (SELECT 1 FROM departments d WHERE d.id = e.dept_id)"
            )
            .is_empty()
        );
        // …but a genuinely unknown correlated column is still flagged.
        assert!(
            col_errors(
                "SELECT id FROM employees e WHERE EXISTS \
                 (SELECT 1 FROM departments d WHERE d.id = e.nope)"
            )
            .iter()
            .any(|m| m.contains("`nope`"))
        );
    }

    #[test]
    fn diag_column_derived_table() {
        // A derived table's columns are its projection's outputs.
        assert!(col_errors("SELECT x FROM (SELECT id AS x FROM employees) sub").is_empty());
        assert!(col_errors("SELECT sub.x FROM (SELECT id AS x FROM employees) sub").is_empty());
        assert!(
            col_errors("SELECT nope FROM (SELECT id AS x FROM employees) sub")
                .iter()
                .any(|m| m.contains("`nope`"))
        );
        assert!(
            col_errors("SELECT sub.nope FROM (SELECT id AS x FROM employees) sub")
                .iter()
                .any(|m| m.contains("`nope`"))
        );
        // A `*` projection makes the derived table open → no false positives.
        assert!(col_errors("SELECT anything FROM (SELECT * FROM employees) sub").is_empty());
    }

    #[test]
    fn diag_column_cte() {
        assert!(
            col_errors("WITH c AS (SELECT id, name FROM employees) SELECT id, name FROM c")
                .is_empty()
        );
        assert!(
            col_errors("WITH c AS (SELECT id FROM employees) SELECT nope FROM c")
                .iter()
                .any(|m| m.contains("`nope`"))
        );
        // Explicit CTE column list defines the output names.
        assert!(
            col_errors("WITH c (a, b) AS (SELECT id, name FROM employees) SELECT a, b FROM c")
                .is_empty()
        );
    }

    #[test]
    fn diag_column_nested_valid_stays_clean() {
        // False-positive sweep over valid nested queries.
        for sql in [
            "SELECT id, (SELECT COUNT(*) FROM departments) AS cnt FROM employees",
            "SELECT e.name FROM employees e WHERE e.dept_id IN (SELECT id FROM departments)",
            "SELECT name FROM employees WHERE salary > (SELECT AVG(salary) FROM employees)",
            "SELECT sub.total FROM (SELECT dept_id, SUM(salary) AS total FROM employees GROUP BY dept_id) sub",
            "WITH d AS (SELECT id FROM departments) SELECT id FROM employees WHERE dept_id IN (SELECT id FROM d)",
            "WITH a AS (SELECT id FROM employees), b AS (SELECT id FROM departments) SELECT a.id FROM a JOIN b ON a.id = b.id",
            "SELECT e.name, d.name FROM employees e JOIN departments d ON e.dept_id = d.id WHERE e.salary > 100",
        ] {
            assert!(
                col_errors(sql).is_empty(),
                "false positive: {sql} -> {:?}",
                col_errors(sql)
            );
        }
    }

    #[test]
    fn diag_column_union_branches() {
        // Each branch resolves against its own FROM — a clean union is clean.
        assert!(col_errors("SELECT id FROM employees UNION SELECT id FROM departments").is_empty());
        assert!(
            col_errors("SELECT name FROM employees UNION ALL SELECT name FROM departments")
                .is_empty()
        );
        // An unknown column in the FIRST branch is flagged there.
        let a = "SELECT nope FROM employees UNION SELECT id FROM departments";
        let da = diag(a);
        assert!(da.iter().any(|x| x.message.contains("`nope`")));
        assert_eq!(&a[da[0].range.0..da[0].range.1], "nope");
        // …and in the SECOND branch, positioned within it.
        let b = "SELECT id FROM employees UNION SELECT bogus FROM departments";
        let db = diag(b);
        assert!(db.iter().any(|x| x.message.contains("`bogus`")));
        assert_eq!(&b[db[0].range.0..db[0].range.1], "bogus");
        // A column valid in one branch's table but not the other is only flagged in
        // the branch where it's actually unknown (`salary` ∈ employees, ∉ departments).
        let c = "SELECT salary FROM employees UNION SELECT salary FROM departments";
        let dc = diag(c);
        assert_eq!(
            dc.iter().filter(|x| x.message.contains("`salary`")).count(),
            1
        );
        assert_eq!(&c[dc[0].range.0..dc[0].range.1], "salary");
        assert!(dc[0].range.0 > c.find("UNION").unwrap()); // the second occurrence
    }

    #[test]
    fn diag_column_open_on_unknown_source() {
        // Unknown table → open → columns unjudgeable (only the table error surfaces).
        let d = diag("SELECT nope FROM nonexistent");
        assert!(
            d.iter()
                .any(|x| x.message.contains("Table `nonexistent` not found"))
        );
        assert!(!d.iter().any(|x| x.message.starts_with("Column ")));
        // A derived table over an unknown table is likewise open.
        assert!(col_errors("SELECT whatever FROM (SELECT * FROM mystery) sub").is_empty());
    }

    #[test]
    fn diag_unqualified_column_needs_loaded_schema() {
        // No loaded schema → can't judge columns → no false positives.
        let cat = Catalog::build(&[], Some("company"));
        let d = diagnostics("SELECT whatever FROM anything", &cat, SqlDialect::MySql);
        assert!(!d.iter().any(|x| x.message.starts_with("Column ")));
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
    fn diag_function_typo_flagged() {
        // Transposition (`COUTN` ↔ `COUNT`) and insertion (`LENGHT`... transposition of
        // `LENGTH`) are both caught, and pinpoint the call name.
        let sql = "SELECT COUTN(*) FROM employees";
        let w: Vec<_> = diag(sql)
            .into_iter()
            .filter(|x| x.message.contains("misspelled function"))
            .collect();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].severity, Severity::Warning);
        assert_eq!(&sql[w[0].range.0..w[0].range.1], "COUTN");
        assert!(
            diag("SELECT LENGHT(name) FROM employees")
                .iter()
                .any(|x| x.message.contains("misspelled function"))
        );
        // A dropped letter (`SUBSTRIN`) too.
        assert!(
            diag("SELECT SUBSTRIN(name, 1) FROM employees")
                .iter()
                .any(|x| x.message.contains("misspelled function"))
        );
    }

    #[test]
    fn diag_function_typo_no_false_positives() {
        for sql in [
            "SELECT COUNT(*) FROM employees",         // correct
            "SELECT POWER(salary, 2) FROM employees", // real builtin, not in suggestion set
            "SELECT JSON_EXTRACT(name, '$.a') FROM employees",
            "SELECT COALESCE(name, 'x') FROM employees",
            "SELECT my_custom_func(id) FROM employees", // UDF, not near a builtin
            "SELECT id FROM employees WHERE id IN (1, 2)", // `IN (` is a keyword
            "SELECT * FROM employees e JOIN departments d ON e.dept_id = d.id",
            "SELECT db.helper(id) FROM employees", // qualified call → skip
        ] {
            let msgs: Vec<String> = diag(sql)
                .into_iter()
                .filter(|x| x.message.contains("misspelled function"))
                .map(|x| x.message)
                .collect();
            assert!(msgs.is_empty(), "false positive: {sql} -> {msgs:?}");
        }
    }

    #[test]
    fn diag_qualified_unloaded_db_not_flagged() {
        // `otherdb.t` — that db isn't loaded, so no unknown-table false positive.
        assert!(diag("SELECT * FROM otherdb.things t").is_empty());
    }

    // ── reserved-keyword aliases ───────────────────────────────────────────────

    fn has_reserved_alias(d: &[Diagnostic], sql: &str, tok: &str) -> bool {
        d.iter().any(|x| {
            x.severity == Severity::Error
                && &sql[x.range.0..x.range.1] == tok
                && x.message.contains("reserved keyword")
        })
    }

    #[test]
    fn diag_reserved_keyword_table_alias_flagged() {
        // `AS or` — OR is reserved, so it can't be a bare alias → error on `or`.
        let sql = "SELECT * FROM employees AS or";
        assert!(has_reserved_alias(&diag(sql), sql, "or"));
    }

    #[test]
    fn diag_reserved_keyword_column_alias_flagged() {
        // Column aliases can't be reserved words either.
        let sql = "SELECT id AS or FROM employees";
        assert!(has_reserved_alias(&diag(sql), sql, "or"));
    }

    #[test]
    fn diag_implicit_reserved_keyword_alias_flagged() {
        // The shorthand form `orders or` must squiggle `or` too — parity with the
        // explicit `AS or` (this was the reported gap).
        let sql = "SELECT * FROM employees or";
        assert!(has_reserved_alias(&diag(sql), sql, "or"));
    }

    #[test]
    fn diag_implicit_alias_parity_with_as_form() {
        for sql in [
            "SELECT * FROM employees or",
            "SELECT * FROM employees AS or",
        ] {
            assert!(
                diag(sql).iter().any(|x| {
                    &sql[x.range.0..x.range.1] == "or" && x.message.contains("reserved keyword")
                }),
                "missed: {sql}"
            );
        }
    }

    #[test]
    fn diag_clause_keywords_after_table_not_flagged() {
        // Legitimate continuations after a table ref aren't alias mistakes.
        for sql in [
            "SELECT * FROM employees WHERE id = 1",
            "SELECT * FROM employees GROUP BY id",
            "SELECT * FROM employees ORDER BY id",
            "SELECT * FROM employees LIMIT 5",
            "SELECT * FROM employees e JOIN departments d ON e.dept_id = d.id",
            "SELECT * FROM employees, departments",
        ] {
            assert!(
                !diag(sql)
                    .iter()
                    .any(|x| x.message.contains("reserved keyword")),
                "false positive: {sql}"
            );
        }
    }

    #[test]
    fn diag_reserved_word_outside_small_keyword_set_flagged() {
        // Words reserved in MySQL but NOT in the small `SQL_KEYWORDS` completion set
        // (`RANK`, `OVER`, `SYSTEM`) — the full reserved list now catches them.
        for sql in [
            "SELECT id AS rank FROM employees",
            "SELECT * FROM employees AS system",
            "SELECT * FROM employees over",
        ] {
            assert!(
                diag(sql)
                    .iter()
                    .any(|x| x.message.contains("reserved keyword")),
                "missed reserved word: {sql}"
            );
        }
    }

    // ── Broad corpus: false-positive / false-negative sweep ────────────────────
    // A wide net of queries to surface anything the diagnostics get wrong. Valid SQL
    // must stay clean; known-bad SQL must be flagged. Uses an *empty* catalog so the
    // unknown-table/column checks are inert — this isolates syntax / alias / typo
    // reporting (the catalog checks have their own focused tests above).

    fn diag_bare(sql: &str) -> Vec<Diagnostic> {
        let cat = Catalog::build(&[], None);
        diagnostics(sql, &cat, SqlDialect::MySql)
    }

    #[test]
    fn corpus_valid_queries_produce_no_diagnostics() {
        let valid = [
            // Basics.
            "SELECT 1;",
            "SELECT * FROM employees;",
            "SELECT id, name FROM employees;",
            "SELECT id, name FROM employees WHERE salary > 100;",
            "SELECT * FROM employees WHERE name IS NULL;",
            "SELECT * FROM employees WHERE name IS NOT NULL;",
            "SELECT * FROM employees WHERE name LIKE '%a%';",
            "SELECT * FROM employees WHERE id IN (1, 2, 3);",
            "SELECT * FROM employees WHERE salary BETWEEN 100 AND 200;",
            "SELECT * FROM employees WHERE dept_id = 1 AND salary > 100 OR name = 'x';",
            "SELECT DISTINCT dept_id FROM employees;",
            // Aliases (explicit, implicit, non-reserved keyword, backtick-quoted).
            "SELECT e.id FROM employees e;",
            "SELECT e.id FROM employees AS e;",
            "SELECT id AS offset FROM employees;",
            // NOTE: `SELECT * FROM employees view` (VIEW as an implicit alias) is valid
            // MySQL but sqlparser rejects it, so our syntax diagnostic false-positives
            // on it — a known upstream-parser gap, not our bug. Omitted here.
            "SELECT id AS `select` FROM employees;",
            "SELECT `order`, `group` FROM employees;",
            // Ordering / grouping / limits.
            "SELECT * FROM employees ORDER BY name;",
            "SELECT * FROM employees ORDER BY name DESC, id ASC;",
            "SELECT dept_id, COUNT(*) FROM employees GROUP BY dept_id;",
            "SELECT dept_id, COUNT(*) c FROM employees GROUP BY dept_id HAVING c > 5;",
            "SELECT * FROM employees LIMIT 10;",
            "SELECT * FROM employees LIMIT 10 OFFSET 5;",
            "SELECT * FROM employees LIMIT 5, 10;",
            // Joins.
            "SELECT e.name, d.name FROM employees e JOIN departments d ON e.dept_id = d.id;",
            "SELECT * FROM employees e LEFT JOIN departments d ON e.dept_id = d.id;",
            "SELECT * FROM employees e INNER JOIN departments d ON e.dept_id = d.id;",
            "SELECT * FROM employees, departments;",
            "SELECT * FROM employees e, departments d WHERE e.dept_id = d.id;",
            // Subqueries / derived / CTE / set ops.
            "SELECT * FROM (SELECT id FROM employees) AS sub;",
            "SELECT * FROM (SELECT id FROM employees) sub;",
            "WITH recent AS (SELECT * FROM employees) SELECT * FROM recent;",
            "SELECT id FROM employees UNION SELECT id FROM departments;",
            "SELECT id FROM employees UNION ALL SELECT id FROM departments;",
            "SELECT id FROM employees e WHERE EXISTS (SELECT 1 FROM departments d WHERE d.id = e.dept_id);",
            "SELECT * FROM employees WHERE salary > (SELECT AVG(salary) FROM employees);",
            // Expressions / functions / CASE.
            "SELECT CASE WHEN salary > 100 THEN 'high' ELSE 'low' END FROM employees;",
            "SELECT COALESCE(name, 'n/a') FROM employees;",
            "SELECT COUNT(*), MAX(salary), MIN(salary), AVG(salary) FROM employees;",
            "SELECT NOW();",
            "SELECT 'it''s a test' FROM employees;",
            // DML / DDL.
            "INSERT INTO employees (id, name) VALUES (1, 'a');",
            "INSERT INTO employees VALUES (1, 'a', 100, 2);",
            "INSERT INTO employees SELECT * FROM employees;",
            "UPDATE employees SET salary = 200 WHERE id = 1;",
            "DELETE FROM employees WHERE id = 1;",
            "CREATE VIEW v AS SELECT id FROM employees;",
            "CREATE TABLE t2 AS SELECT id FROM employees;",
            // Comments, strings containing keywords, multi-statement.
            "SELECT /* block */ id FROM employees;",
            "SELECT id FROM employees WHERE id = 1; SELECT 2;",
            "SELECT 'FROM WHERE OR AS' AS lit FROM employees;",
            // Reserved words, correctly backtick-quoted as identifiers.
            "SELECT * FROM `order`;",
            "SELECT t.`order` FROM employees t;",
            "SELECT `select` FROM employees;",
            // More joins / CTEs / set positions.
            "SELECT * FROM employees e CROSS JOIN departments d;",
            "SELECT * FROM employees e RIGHT JOIN departments d ON e.dept_id = d.id;",
            "WITH a AS (SELECT 1 AS x), b AS (SELECT 2 AS y) SELECT * FROM a JOIN b ON a.x = b.y;",
            "SELECT * FROM employees e JOIN departments d ON e.dept_id = d.id WHERE d.id > 1;",
        ];
        let failures: Vec<String> = valid
            .iter()
            .filter_map(|q| {
                let d = diag_bare(q);
                (!d.is_empty()).then(|| {
                    let msgs: Vec<&str> = d.iter().map(|x| x.message.as_str()).collect();
                    format!("  {q}\n      -> {msgs:?}")
                })
            })
            .collect();
        assert!(
            failures.is_empty(),
            "false positives on valid SQL ({} of {}):\n{}",
            failures.len(),
            valid.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn corpus_invalid_queries_are_flagged() {
        // Each must produce at least one Error diagnostic.
        let invalid = [
            "SELECT * FROM employees AS or;",
            "SELECT * FROM employees or;",
            "SELECT id AS select FROM employees;",
            "SELECT id AS rank FROM employees;",
            "SELECT * FROM employees AS and;",
            "SELECT * FROM employees e JOIN departments or ON 1 = 1;",
            "SELECT FROM WHERE;",
            "SELECT * FROM;",
            "SELECT * FROM employees WHERE;",
            "SELECT * FROM employees GROUP BY;",
            "SELECT a, FROM employees;",
        ];
        let misses: Vec<&str> = invalid
            .iter()
            .copied()
            .filter(|q| !diag_bare(q).iter().any(|x| x.severity == Severity::Error))
            .collect();
        assert!(
            misses.is_empty(),
            "missed errors on invalid SQL:\n{misses:#?}"
        );
    }

    #[test]
    fn diag_nonreserved_keyword_alias_not_flagged() {
        // `OFFSET`/`VIEW` are non-reserved in MySQL — legal as bare aliases, so no
        // squiggle (explicit or implicit).
        for sql in [
            "SELECT id AS offset FROM employees",
            "SELECT * FROM employees view",
        ] {
            assert!(
                !diag(sql)
                    .iter()
                    .any(|x| x.message.contains("reserved keyword")),
                "false positive: {sql}"
            );
        }
    }

    #[test]
    fn diag_insert_and_update_bodies_not_flagged() {
        // INSERT/UPDATE aren't checked for implicit aliases → VALUES/SET are safe.
        for sql in [
            "INSERT INTO employees VALUES (1)",
            "UPDATE employees SET id = 1",
        ] {
            assert!(
                !diag(sql)
                    .iter()
                    .any(|x| x.message.contains("reserved keyword")),
                "false positive: {sql}"
            );
        }
    }

    #[test]
    fn diag_valid_alias_not_flagged() {
        // A non-keyword alias is fine (explicit and implicit).
        assert!(
            !diag("SELECT id AS ord FROM employees")
                .iter()
                .any(|x| x.message.contains("reserved keyword"))
        );
        assert!(
            !diag("SELECT * FROM employees emp")
                .iter()
                .any(|x| x.message.contains("reserved keyword"))
        );
    }

    #[test]
    fn diag_ctas_and_view_body_not_flagged() {
        // `AS SELECT` / `AS (SELECT …)` bodies aren't aliases.
        for sql in [
            "CREATE TABLE t AS SELECT id FROM employees",
            "CREATE VIEW v AS SELECT id FROM employees",
        ] {
            assert!(
                !diag(sql)
                    .iter()
                    .any(|x| x.message.contains("reserved keyword")),
                "false positive on: {sql}"
            );
        }
    }

    #[test]
    fn scope_rejects_reserved_keyword_alias_after_as() {
        // `orders AS or` must NOT resolve `or` as an alias (parity with the implicit
        // form `orders or`) — so completion won't offer `or.`'s columns for it.
        let sql = "SELECT or. FROM employees AS or WHERE ";
        let s = statement_scope(sql, 0, sql.len(), 10, SqlDialect::MySql);
        let emp = s.tables.iter().find(|t| t.name == "employees").unwrap();
        assert_eq!(emp.alias, None);
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

    // ── FK-aware JOIN completion targets ──────────────────────────────────────

    fn jt(sql: &str) -> Vec<JoinTarget> {
        let (schema, db) = fk_catalog();
        let cat = Catalog::build(&[(db, &schema)], Some(db));
        join_targets(sql, 0, sql.len(), sql.len(), &cat)
    }

    #[test]
    fn join_targets_offers_fk_table_and_predicate() {
        // After `FROM orders o JOIN `, `customers` is FK-connected (forward) and
        // `line_items` is FK-connected (reverse).
        let ts = jt("SELECT * FROM orders o JOIN ");
        let names: Vec<&str> = ts.iter().map(|t| t.table.as_str()).collect();
        assert!(names.contains(&"customers"), "{names:?}");
        assert!(names.contains(&"line_items"), "{names:?}");
        let cust = ts.iter().find(|t| t.table == "customers").unwrap();
        assert_eq!(cust.predicate, "o.customer_id = customers.id");
    }

    #[test]
    fn join_targets_reverse_edge_predicate() {
        // `line_items.order_id → orders.id`; joining onto in-scope `orders o`.
        let ts = jt("SELECT * FROM orders o JOIN ");
        let li = ts.iter().find(|t| t.table == "line_items").unwrap();
        assert_eq!(li.predicate, "line_items.order_id = o.id");
    }

    #[test]
    fn join_targets_partial_prefix_still_offered() {
        // A partial table name being typed doesn't disable the suggestions.
        let ts = jt("SELECT * FROM orders o JOIN cust");
        assert!(ts.iter().any(|t| t.table == "customers"));
    }

    #[test]
    fn join_targets_empty_outside_join_slot() {
        // A FROM slot (not JOIN) → no ON-predicate suggestions.
        assert!(jt("SELECT * FROM ").is_empty());
        // After the table is fully typed and an ON is expected, not a table slot.
        let (schema, db) = fk_catalog();
        let cat = Catalog::build(&[(db, &schema)], Some(db));
        let sql = "SELECT * FROM orders o JOIN customers c ";
        assert!(join_targets(sql, 0, sql.len(), sql.len(), &cat).is_empty());
    }

    #[test]
    fn join_targets_excludes_already_in_scope() {
        // `customers` already joined → don't re-suggest it.
        let ts = jt("SELECT * FROM orders o JOIN customers c JOIN ");
        assert!(!ts.iter().any(|t| t.table == "customers"));
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
