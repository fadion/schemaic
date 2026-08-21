//! Server-side grid **filter / sort** — the DataGrip-style "filter from the
//! header" feature.
//!
//! The idea: take the `SELECT` that produced a result, splice a `WHERE`
//! condition and/or an `ORDER BY` into it, and hand the rewritten SQL back to be
//! re-run against the DB — so the filter/sort cover the *whole* table, not just
//! the loaded (capped) page. This is a pure, AST-based rewrite built on
//! `sqlparser` (dialect via [`SqlDialect`]); the live DB stays the semantic
//! authority (we don't type-check the condition — the re-run does).
//!
//! Eligibility (is this result attributable to one real base table?) is decided
//! by the caller via [`crate::edit::EditModel::insert_target`]; here we only
//! confirm the SQL is *structurally* rewritable — a single, join-free, CTE-free
//! `SELECT` over a named table — and otherwise return `Ok(None)` ("leave the
//! filter bar disabled") rather than an error.

use crate::intel::SqlDialect;
use sqlparser::ast::{
    BinaryOperator, Expr, Ident, OrderBy, OrderByExpr, OrderByKind, OrderByOptions, SetExpr,
    Statement, Value, visit_expressions_mut,
};
use sqlparser::parser::Parser;
use sqlparser::tokenizer::Token;
use std::ops::ControlFlow;

/// The view-layer filter/sort attached to a result. Persists across result
/// reloads (it lives on the tab, not the per-result grid state) and is cleared on
/// a fresh manual editor run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridQuery {
    /// A raw `WHERE`-condition fragment (DataGrip-style free text), ANDed onto the
    /// base statement's existing `WHERE`. Empty = no filter.
    pub filter: String,
    /// Ordering as `(real column name, ascending)` pairs, applied as `ORDER BY`
    /// (replacing the base statement's). Empty = keep the base `ORDER BY`.
    pub sort: Vec<(String, bool)>,
}

impl GridQuery {
    /// No filter and no sort — the grid shows the base result untouched.
    pub fn is_empty(&self) -> bool {
        self.filter.trim().is_empty() && self.sort.is_empty()
    }
}

/// Why a filter/sort couldn't be turned into runnable SQL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilterError {
    /// The user's `WHERE` fragment didn't parse (message is the parser's, shown
    /// inline under the filter field). The base query is left as-is.
    BadCondition(String),
}

/// Rewrite `base_sql` with the given filter condition and/or sort, returning the
/// SQL to re-run.
///
/// - `Ok(Some(sql))` — the rewritten statement.
/// - `Ok(None)` — `base_sql` isn't a structurally-rewritable single-table
///   `SELECT` (join / CTE / set-operation / derived table / non-`SELECT` /
///   multi-statement); the caller should treat the result as not filterable.
/// - `Err(BadCondition)` — the filter fragment didn't parse; don't run.
///
/// The base statement's projection, `LIMIT`/`OFFSET`, and (when `sort` is empty)
/// its `ORDER BY` are preserved. An existing `WHERE` is preserved and combined
/// with the filter via `AND`, each side parenthesized so operator precedence
/// never changes its meaning.
pub fn build_query(
    base_sql: &str,
    filter: &str,
    sort: &[(String, bool)],
    dialect: SqlDialect,
) -> Result<Option<String>, FilterError> {
    // The base already ran against the DB, so a parse failure here just means
    // sqlparser can't model it — degrade to "not rewritable", never an error.
    let Ok(mut stmts) = Parser::parse_sql(&*dialect.parser(), base_sql) else {
        return Ok(None);
    };
    if stmts.len() != 1 {
        return Ok(None);
    }
    let Statement::Query(query) = &mut stmts[0] else {
        return Ok(None);
    };
    // No CTE, a plain SELECT body (not UNION/…), one table, no joins, and a plain
    // named table (not a derived subquery / table function). The test belongs to
    // `intel`, which is where structure-aware SQL analysis lives and where the
    // *other* caller of it is — SQLite's write-back has to answer the same
    // question about the same statement, and a second copy of the rule that
    // agreed on the day it was written is what the one-quoter invariant exists to
    // prevent.
    if crate::intel::simple_select_source(query).is_none() {
        return Ok(None);
    }
    let SetExpr::Select(select) = query.body.as_mut() else {
        // Unreachable: `simple_select_source` just established this is a Select.
        return Ok(None);
    };

    // WHERE: AND the parsed condition onto any existing selection, parenthesizing
    // both sides so e.g. a base `a OR b` + filter `c` becomes `(a OR b) AND (c)`.
    // A leading `WHERE` the user typed out of habit is stripped first.
    let filter = strip_leading_where(filter.trim());
    if !filter.is_empty() {
        let mut cond = parse_condition(filter, dialect)?;
        // MySQL/MariaDB treat "x" as a string literal (unlike Postgres, where it's a
        // quoted identifier). Normalize any double-quoted string in the condition to a
        // single-quoted literal so `name = "x"` behaves like `name = 'x'` regardless
        // of the server's ANSI_QUOTES mode. Postgres keeps its identifier semantics.
        if dialect == SqlDialect::MySql {
            let _ = visit_expressions_mut(&mut cond, |e| {
                if let Expr::Value(vws) = e
                    && let Value::DoubleQuotedString(s) = &vws.value
                {
                    vws.value = Value::SingleQuotedString(s.clone());
                }
                ControlFlow::<()>::Continue(())
            });
        }
        select.selection = Some(match select.selection.take() {
            Some(existing) => Expr::BinaryOp {
                left: Box::new(Expr::Nested(Box::new(existing))),
                op: BinaryOperator::And,
                right: Box::new(Expr::Nested(Box::new(cond))),
            },
            None => cond,
        });
    }

    // ORDER BY: replace with our sort spec when present; otherwise keep the base's.
    if !sort.is_empty() {
        let q = quote_char(dialect);
        let exprs = sort
            .iter()
            .map(|(col, asc)| OrderByExpr {
                expr: Expr::Identifier(Ident::with_quote(q, col.clone())),
                options: OrderByOptions {
                    asc: Some(*asc),
                    nulls_first: None,
                },
                with_fill: None,
            })
            .collect();
        query.order_by = Some(OrderBy {
            kind: OrderByKind::Expressions(exprs),
            interpolate: None,
        });
    }

    Ok(Some(stmts[0].to_string()))
}

/// Build a `WHERE`-fragment for the cell right-click "Filter by / Exclude value"
/// actions, ready to append (with ` AND `) into the filter field. `value` is the
/// cell's raw text, or `None` for a NULL cell.
///
/// - `value = Some(v)`: `` `col` = 'v' `` (or `<> 'v'` when `negate`).
/// - `value = None`: `` `col` IS NULL `` (or `IS NOT NULL` when `negate`).
///
/// Identifiers and string literals are quoted/escaped per dialect so the fragment
/// re-parses cleanly (reserved-word columns, embedded quotes/backslashes).
pub fn eq_condition(col: &str, value: Option<&str>, negate: bool, dialect: SqlDialect) -> String {
    let ident = quote_ident(col, dialect);
    match value {
        None => {
            if negate {
                format!("{ident} IS NOT NULL")
            } else {
                format!("{ident} IS NULL")
            }
        }
        Some(v) => {
            let op = if negate { "<>" } else { "=" };
            format!("{ident} {op} {}", quote_value(v, dialect))
        }
    }
}

/// Strip a leading `WHERE` keyword (case-insensitive, followed by whitespace) from
/// a filter fragment, so `WHERE name = 'x'` behaves like `name = 'x'`. A column
/// literally named `where…` (e.g. `whereabouts`) is left untouched.
fn strip_leading_where(s: &str) -> &str {
    let s = s.trim_start();
    if let Some(head) = s.get(..5)
        && head.eq_ignore_ascii_case("where")
    {
        let rest = &s[5..];
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            return rest.trim_start();
        }
    }
    s
}

/// Parse a free-text `WHERE` fragment into an expression, requiring the *whole*
/// fragment to be consumed (so `id = 5; DROP …` is rejected, not silently
/// truncated to `id = 5`).
fn parse_condition(cond: &str, dialect: SqlDialect) -> Result<Expr, FilterError> {
    let bad = |e: sqlparser::parser::ParserError| FilterError::BadCondition(e.to_string());
    let d = dialect.parser();
    let mut parser = Parser::new(&*d).try_with_sql(cond).map_err(bad)?;
    let expr = parser.parse_expr().map_err(bad)?;
    if parser.peek_token().token != Token::EOF {
        return Err(FilterError::BadCondition(
            "unexpected trailing input after condition".to_string(),
        ));
    }
    Ok(expr)
}

/// Identifier quote character for the dialect (backtick on MySQL, double-quote on
/// Postgres and SQLite) — used when we emit an `ORDER BY` column via the AST.
/// Matches what [`crate::export::ident_sql`] emits, which is the authority.
fn quote_char(dialect: SqlDialect) -> char {
    match dialect {
        SqlDialect::MySql => '`',
        SqlDialect::Postgres | SqlDialect::Sqlite => '"',
    }
}

/// Quote an identifier as a string, doubling any embedded quote character.
///
/// Delegates: identifier quoting is [`crate::export::ident_sql`]'s, here as
/// everywhere. This was a second implementation that had independently reached
/// the same rules — which is the drift hazard, not the reassurance.
use crate::export::ident_sql as quote_ident;

/// What this module quotes identifiers with, exposed so the workspace-wide
/// agreement test can reach it. Not a second implementation — it *is* the
/// import above, which is the point being asserted.
#[cfg(test)]
pub(crate) fn quoted_ident_for_test(name: &str, dialect: SqlDialect) -> String {
    quote_ident(name, dialect)
}

/// Quote a value as a single-quoted SQL string literal. Single quotes are doubled
/// for both dialects; backslashes are additionally doubled on MySQL (where `\` is
/// an escape character in string literals by default, unlike standard-conforming
/// Postgres strings).
fn quote_value(v: &str, dialect: SqlDialect) -> String {
    let escaped = match dialect {
        SqlDialect::MySql => v.replace('\\', "\\\\").replace('\'', "''"),
        SqlDialect::Postgres | SqlDialect::Sqlite => v.replace('\'', "''"),
    };
    format!("'{escaped}'")
}

/// Does `name` have to be quoted to survive a round-trip through the server?
///
/// Only when it isn't a bare identifier, or collides with a reserved word. The
/// dialects differ on case: Postgres folds an unquoted identifier to lower case,
/// so anything with an upper-case letter must be quoted or it resolves to a
/// different (probably missing) table; MySQL doesn't fold, so mixed case is fine
/// bare. **SQLite doesn't fold either** — it compares identifiers
/// case-insensitively and stores them as written, so `ArtistId` bare resolves to
/// the same column as `artistid` and quoting it would only be decoration.
///
/// Used to keep generated SQL free of decorative quoting — which reads better,
/// and keeps names visible to the mid-edit tokenizer that backs completion.
fn needs_quoting(name: &str, dialect: SqlDialect) -> bool {
    if name.is_empty() {
        return true;
    }
    let bare = match dialect {
        // MySQL bare identifiers: letters, digits, `_`, `$`; no leading digit.
        SqlDialect::MySql => {
            !name.starts_with(|c: char| c.is_ascii_digit())
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        }
        // Postgres: lower-case only (anything else folds), plus `_` and digits.
        SqlDialect::Postgres => {
            name.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        }
        // SQLite: letters, digits, `_`, `$`; no leading digit — MySQL's rule, and
        // case-preserving like it, so mixed case needs no quoting.
        SqlDialect::Sqlite => {
            !name.starts_with(|c: char| c.is_ascii_digit())
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        }
    };
    // `must_quote_ident`, not `is_reserved_word`: this name goes into a statement
    // as a table or column, where SQLite refuses a few words it would accept as
    // an alias.
    !bare || crate::intel::must_quote_ident(name, dialect)
}

/// Quote `name` only if it needs it.
fn quote_if_needed(name: &str, dialect: SqlDialect) -> String {
    if needs_quoting(name, dialect) {
        quote_ident(name, dialect)
    } else {
        name.to_string()
    }
}

/// Direction for a generated `ORDER BY`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Asc,
    Desc,
}

impl Order {
    fn keyword(self) -> &'static str {
        match self {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
        }
    }
}

/// What a generated browse query ([`table_query`]) keys and orders its page on.
///
/// The two keyed cases are separate variants rather than one list of names, and
/// that distinction is the point: a key made of the table's **own** columns is
/// already in a `SELECT *`, while one that isn't has to be named in the
/// projection or the result simply won't contain it — and then nothing can write
/// the rows back. Collapsing them would make the projection depend on a fact the
/// name alone doesn't carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowseKey<'a> {
    /// The table's own key columns, in key order — normally the primary key.
    /// Already projected by the `*`.
    Columns(&'a [String]),
    /// A row identity that is **not** one of the table's columns, so the
    /// statement must name it: SQLite's `rowid`, via
    /// [`crate::schema::TableInfo::implicit_key`].
    Implicit(&'a str),
    /// Nothing to key on — no primary key, or the schema hasn't been
    /// introspected yet. The page is unordered rather than ordered by a guess at
    /// some column, and the result is read-only.
    None,
}

impl<'a> BrowseKey<'a> {
    /// The key to browse a table by: its own columns when it has any, else the
    /// implicit one, else nothing.
    ///
    /// **One definition of that precedence, deliberately.** It has to agree with
    /// [`crate::edit::analyze_edit`], which resolves the *write* key from the
    /// result and reaches for an implicit key only once a real one has failed. A
    /// statement projecting a rowid the write path then ignored would carry a
    /// column for no reason; one projecting nothing the write path needed would
    /// be read-only for no reason.
    /// `key_cols` is [`crate::schema::browse_key_columns`]'s answer, not the
    /// primary key alone — the middle arm (a unique, non-foreign, all-`NOT NULL`
    /// index) is exactly what `analyze_edit` reaches for second, and leaving it
    /// out made a `(email TEXT NOT NULL UNIQUE, name TEXT)` table browse by
    /// rowid while the write path keyed on `email`.
    pub fn pick(key_cols: &'a [String], implicit: Option<&'a str>) -> Self {
        // An empty name is not a key. `pick` is the only constructor callers are
        // supposed to use, and this is where the two illegal states — a `Columns`
        // with nothing in it, an `Implicit` spelled `""` — are turned into the
        // `None` they mean, so nothing downstream has to re-check.
        match (
            key_cols.is_empty(),
            implicit.filter(|k| !k.trim().is_empty()),
        ) {
            (false, _) => Self::Columns(key_cols),
            (true, Some(k)) => Self::Implicit(k),
            (true, None) => Self::None,
        }
    }
}

/// How a table is named in generated SQL, in `dialect` — **the** spelling, so
/// two statements the app generates for one table can't disagree about what it
/// is called.
///
/// MySQL qualifies `` `db`.`table` `` (its connection is server-level).
/// PostgreSQL emits the name its connection can already reach: bare in `public`,
/// which resolves through `search_path`, and `"schema"."table"` in any other
/// namespace — `schema` is the table's PostgreSQL namespace and is always `None`
/// on MySQL, where the database *is* the namespace. **SQLite names the table
/// alone**: a connection is one file, so there is no server-level qualifier to
/// add, and `database` there is the schema name SQLite itself uses (`main`, or
/// an `ATTACH`ed one) — qualifying with it would emit `main.t`, correct but noise
/// on every generated query, and wrong the moment the statement is copied into a
/// session where the file is attached under another name.
///
/// Quoted only where the name needs it ([`needs_quoting`]), which is what keeps
/// generated SQL free of decorative quoting and its names visible to the
/// mid-edit tokenizer behind completion.
pub fn qualified_table_name(
    dialect: SqlDialect,
    database: &str,
    schema: Option<&str>,
    table: &str,
) -> String {
    match dialect {
        SqlDialect::MySql => format!(
            "{}.{}",
            quote_if_needed(database, dialect),
            quote_if_needed(table, dialect)
        ),
        SqlDialect::Postgres => match crate::schema::sql_qualifier(schema) {
            Some(s) => format!(
                "{}.{}",
                quote_if_needed(s, dialect),
                quote_if_needed(table, dialect)
            ),
            None => quote_if_needed(table, dialect),
        },
        SqlDialect::Sqlite => quote_if_needed(table, dialect),
    }
}

/// The `SELECT` Schemaic generates for a table — opening one from the schema
/// tree, and the AI's bottom-sample.
///
/// **Why it orders.** Without an `ORDER BY` neither engine promises anything, so
/// "the first `limit` rows" is an undefined set: InnoDB usually walks the
/// primary key, but Postgres returns heap order, where an `UPDATE` can move a
/// row to the end of the table and change what you see between runs. Ordering by
/// the key makes the capped page deterministic. The key is also the cheap column
/// to sort by — on InnoDB it *is* the clustered order.
///
/// The `ORDER BY` is written into the statement the user sees rather than
/// spliced in on the way to the server: the editor text has to stay what
/// actually runs, or history, EXPLAIN and copy-as-SQL all quietly disagree with
/// it. It's ordinary SQL in the editor — deletable, editable, and replaced (not
/// appended to) when a column header is clicked, since [`build_query`] rewrites
/// an existing `ORDER BY`.
///
/// Dialect-aware, matching the write path — the name itself is
/// [`qualified_table_name`]'s, which every statement the app generates for a
/// table shares.
///
/// See [`BrowseKey`] for what `key` does to the projection — a key that is not one
/// of the table's columns has to be named in it, and that is what makes a keyless
/// SQLite table editable.
pub fn table_query(
    dialect: SqlDialect,
    database: &str,
    schema: Option<&str>,
    table: &str,
    key: BrowseKey<'_>,
    order: Order,
    limit: usize,
) -> String {
    let name = qualified_table_name(dialect, database, schema, table);
    let (projection, order_cols): (String, Vec<String>) = match key {
        // Not one of the table's columns, so `*` won't return it and it has to be
        // named — nothing can key a write on a value the result doesn't contain.
        BrowseKey::Implicit(k) => {
            let k = quote_if_needed(k, dialect);
            (format!("{k}, *"), vec![k])
        }
        BrowseKey::Columns(cols) => (
            "*".to_string(),
            cols.iter().map(|c| quote_if_needed(c, dialect)).collect(),
        ),
        BrowseKey::None => ("*".to_string(), Vec::new()),
    };
    let order_by = if order_cols.is_empty() {
        String::new()
    } else {
        let cols = order_cols
            .iter()
            .map(|c| format!("{c} {}", order.keyword()))
            .collect::<Vec<_>>()
            .join(", ");
        format!(" ORDER BY {cols}")
    };
    format!("SELECT {projection} FROM {name}{order_by} LIMIT {limit}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(base: &str, filter: &str, sort: &[(&str, bool)], d: SqlDialect) -> Option<String> {
        let sort: Vec<(String, bool)> = sort.iter().map(|(c, a)| (c.to_string(), *a)).collect();
        build_query(base, filter, &sort, d).unwrap()
    }

    // ── table_query: the generated "open this table" statement ────────────

    fn tq(d: SqlDialect, table: &str, pk: &[&str], order: Order, limit: usize) -> String {
        let pk: Vec<String> = pk.iter().map(|c| c.to_string()).collect();
        let key = BrowseKey::pick(&pk, None);
        table_query(d, "shop", None, table, key, order, limit)
    }

    /// As [`tq`], but for a table in an explicit PostgreSQL namespace.
    fn tq_in(d: SqlDialect, schema: &str, table: &str, pk: &[&str]) -> String {
        let pk: Vec<String> = pk.iter().map(|c| c.to_string()).collect();
        let key = BrowseKey::pick(&pk, None);
        table_query(d, "shop", Some(schema), table, key, Order::Asc, 100)
    }

    /// As [`tq`], with an implicit key the table may or may not need — routed
    /// through `BrowseKey::pick`, which is where the precedence lives.
    fn tq_implicit(table: &str, pk: &[&str], implicit: Option<&str>) -> String {
        let pk: Vec<String> = pk.iter().map(|c| c.to_string()).collect();
        let key = BrowseKey::pick(&pk, implicit);
        table_query(
            SqlDialect::Sqlite,
            "main",
            None,
            table,
            key,
            Order::Asc,
            100,
        )
    }

    #[test]
    fn browse_key_prefers_the_tables_own_columns() {
        let pk = vec!["id".to_string()];
        assert_eq!(BrowseKey::pick(&pk, Some("rowid")), BrowseKey::Columns(&pk));
        assert_eq!(
            BrowseKey::pick(&[], Some("rowid")),
            BrowseKey::Implicit("rowid")
        );
        assert_eq!(BrowseKey::pick(&[], None), BrowseKey::None);
    }

    /// A table with no key of its own is opened with its rowid projected — the
    /// statement that makes it editable. `SELECT *` would not return one.
    #[test]
    fn table_query_projects_an_implicit_key_for_a_table_with_no_key() {
        assert_eq!(
            tq_implicit("notes", &[], Some("rowid")),
            "SELECT rowid, * FROM notes ORDER BY rowid ASC LIMIT 100"
        );
        // The chosen spelling is used verbatim: a table declaring its own `rowid`
        // column reaches the real one only as `_rowid_`.
        assert_eq!(
            tq_implicit("notes", &[], Some("_rowid_")),
            "SELECT _rowid_, * FROM notes ORDER BY _rowid_ ASC LIMIT 100"
        );
    }

    /// A real key makes the implicit one noise: it is neither projected nor
    /// ordered on, so a keyed table's statement is byte-for-byte what it was.
    #[test]
    fn table_query_ignores_an_implicit_key_when_the_table_has_a_real_one() {
        assert_eq!(
            tq_implicit("notes", &["id"], Some("rowid")),
            "SELECT * FROM notes ORDER BY id ASC LIMIT 100"
        );
        assert_eq!(
            tq_implicit("notes", &["id"], None),
            tq_implicit("notes", &["id"], Some("rowid"))
        );
    }

    /// No implicit key — every MySQL and PostgreSQL table, and a SQLite
    /// `WITHOUT ROWID` one — reads exactly as it always did.
    #[test]
    fn table_query_without_an_implicit_key_is_unchanged() {
        assert_eq!(
            tq_implicit("notes", &[], None),
            "SELECT * FROM notes LIMIT 100"
        );
    }

    #[test]
    fn table_query_postgres_qualifies_a_non_public_schema() {
        assert_eq!(
            tq_in(SqlDialect::Postgres, "sales", "orders", &["id"]),
            "SELECT * FROM sales.orders ORDER BY id ASC LIMIT 100"
        );
        // `public` is on the search_path → the statement stays exactly what it
        // was before multi-schema browsing existed.
        assert_eq!(
            tq_in(SqlDialect::Postgres, "public", "orders", &["id"]),
            "SELECT * FROM orders ORDER BY id ASC LIMIT 100"
        );
    }

    #[test]
    fn table_query_quotes_a_schema_that_needs_it() {
        // The namespace goes through the same quoting rules as the table, so a
        // mixed-case or reserved-word schema still resolves.
        assert_eq!(
            tq_in(SqlDialect::Postgres, "Sales Ops", "orders", &["id"]),
            "SELECT * FROM \"Sales Ops\".orders ORDER BY id ASC LIMIT 100"
        );
        assert_eq!(
            table_query(
                SqlDialect::Postgres,
                "shop",
                Some("we\"ird"),
                "orders",
                BrowseKey::None,
                Order::Asc,
                10
            ),
            "SELECT * FROM \"we\"\"ird\".orders LIMIT 10"
        );
    }

    #[test]
    fn table_query_mysql_qualifies_and_orders() {
        assert_eq!(
            tq(SqlDialect::MySql, "orders", &["id"], Order::Asc, 100),
            "SELECT * FROM shop.orders ORDER BY id ASC LIMIT 100"
        );
    }

    #[test]
    fn table_query_postgres_is_unqualified() {
        // A PG connection is already bound to the database; the table resolves
        // through `search_path`, exactly like the write path.
        assert_eq!(
            tq(SqlDialect::Postgres, "orders", &["id"], Order::Asc, 100),
            "SELECT * FROM orders ORDER BY id ASC LIMIT 100"
        );
    }

    #[test]
    fn table_query_orders_by_a_composite_key_in_order() {
        assert_eq!(
            tq(
                SqlDialect::MySql,
                "line_items",
                &["order_id", "line_no"],
                Order::Asc,
                50
            ),
            "SELECT * FROM shop.line_items ORDER BY order_id ASC, line_no ASC LIMIT 50"
        );
    }

    #[test]
    fn table_query_without_a_key_is_unordered() {
        // No PK (or the schema hasn't loaded) → no ORDER BY at all, rather than
        // guessing at a column.
        assert_eq!(
            tq(SqlDialect::MySql, "logs", &[], Order::Asc, 100),
            "SELECT * FROM shop.logs LIMIT 100"
        );
        assert_eq!(
            tq(SqlDialect::Postgres, "logs", &[], Order::Asc, 100),
            "SELECT * FROM logs LIMIT 100"
        );
    }

    #[test]
    fn table_query_descending() {
        // The AI seed sample wants the most recent rows.
        assert_eq!(
            tq(SqlDialect::MySql, "orders", &["id"], Order::Desc, 20),
            "SELECT * FROM shop.orders ORDER BY id DESC LIMIT 20"
        );
    }

    #[test]
    fn table_query_leaves_ordinary_names_unquoted() {
        // Quoting every name is noisy, and it costs real functionality: the
        // mid-edit tokenizer (the fallback that runs the moment a statement stops
        // parsing) can't see inside a Postgres double-quoted name, so a quoted
        // table loses column completion as soon as you type `WHERE`.
        assert_eq!(
            tq(SqlDialect::MySql, "orders", &["id"], Order::Asc, 100),
            "SELECT * FROM shop.orders ORDER BY id ASC LIMIT 100"
        );
        assert_eq!(
            tq(SqlDialect::Postgres, "orders", &["id"], Order::Asc, 100),
            "SELECT * FROM orders ORDER BY id ASC LIMIT 100"
        );
    }

    #[test]
    fn table_query_quotes_a_name_that_needs_it() {
        // Reserved word.
        assert_eq!(
            tq(SqlDialect::MySql, "order", &["key"], Order::Asc, 10),
            "SELECT * FROM shop.`order` ORDER BY `key` ASC LIMIT 10"
        );
        // Not a bare identifier.
        assert_eq!(
            tq(SqlDialect::MySql, "my table", &["id"], Order::Asc, 10),
            "SELECT * FROM shop.`my table` ORDER BY id ASC LIMIT 10"
        );
    }

    #[test]
    fn table_query_quotes_uppercase_only_on_postgres() {
        // Postgres folds an unquoted name to lower case, so `MyTable` unquoted
        // would resolve to `mytable` and fail. MySQL doesn't fold.
        assert_eq!(
            tq(SqlDialect::Postgres, "MyTable", &["Id"], Order::Asc, 10),
            "SELECT * FROM \"MyTable\" ORDER BY \"Id\" ASC LIMIT 10"
        );
        assert_eq!(
            tq(SqlDialect::MySql, "MyTable", &["Id"], Order::Asc, 10),
            "SELECT * FROM shop.MyTable ORDER BY Id ASC LIMIT 10"
        );
    }

    #[test]
    fn table_query_escapes_identifier_quotes() {
        // A backtick in a MySQL name and a double-quote in a PG one are doubled,
        // so a hostile name can't break out of the quoting.
        assert_eq!(
            table_query(
                SqlDialect::MySql,
                "we`ird",
                None,
                "ta`ble",
                BrowseKey::Columns(&["i`d".to_string()]),
                Order::Asc,
                10
            ),
            "SELECT * FROM `we``ird`.`ta``ble` ORDER BY `i``d` ASC LIMIT 10"
        );
        assert!(needs_quoting("we`ird", SqlDialect::MySql));
        assert_eq!(
            table_query(
                SqlDialect::Postgres,
                "db",
                None,
                "ta\"ble",
                BrowseKey::Columns(&["i\"d".to_string()]),
                Order::Asc,
                10
            ),
            "SELECT * FROM \"ta\"\"ble\" ORDER BY \"i\"\"d\" ASC LIMIT 10"
        );
    }

    #[test]
    fn table_query_output_is_rewritable_by_the_filter() {
        // The generated statement is the *base* the header filter/sort rewrites,
        // so it has to survive a round-trip: sorting by another column replaces
        // the generated ORDER BY rather than appending to it.
        let base = tq(SqlDialect::MySql, "orders", &["id"], Order::Asc, 100);
        let out = build(&base, "", &[("total", false)], SqlDialect::MySql).unwrap();
        assert_eq!(
            out,
            "SELECT * FROM shop.orders ORDER BY `total` DESC LIMIT 100"
        );
    }

    #[test]
    fn passthrough_when_no_filter_or_sort() {
        // Structurally rewritable, but nothing to change → re-serialized base.
        let out = build("SELECT * FROM users", "", &[], SqlDialect::MySql).unwrap();
        assert_eq!(out, "SELECT * FROM users");
    }

    #[test]
    fn appends_where_when_none_exists() {
        let out = build("SELECT * FROM users", "id = 5", &[], SqlDialect::MySql).unwrap();
        assert_eq!(out, "SELECT * FROM users WHERE id = 5");
    }

    #[test]
    fn ands_onto_existing_where_with_parens() {
        // Base WHERE preserved; both sides parenthesized so precedence is safe.
        let out = build(
            "SELECT * FROM users WHERE a = 1 OR b = 2",
            "c = 3",
            &[],
            SqlDialect::MySql,
        )
        .unwrap();
        assert_eq!(
            out,
            "SELECT * FROM users WHERE (a = 1 OR b = 2) AND (c = 3)"
        );
    }

    #[test]
    fn adds_order_by() {
        let out = build("SELECT * FROM t", "", &[("name", true)], SqlDialect::MySql).unwrap();
        assert_eq!(out, "SELECT * FROM t ORDER BY `name` ASC");
    }

    #[test]
    fn replaces_existing_order_by() {
        let out = build(
            "SELECT * FROM t ORDER BY id",
            "",
            &[("name", false)],
            SqlDialect::MySql,
        )
        .unwrap();
        assert_eq!(out, "SELECT * FROM t ORDER BY `name` DESC");
    }

    #[test]
    fn multi_column_order_by() {
        let out = build(
            "SELECT * FROM t",
            "",
            &[("a", true), ("b", false)],
            SqlDialect::MySql,
        )
        .unwrap();
        assert_eq!(out, "SELECT * FROM t ORDER BY `a` ASC, `b` DESC");
    }

    #[test]
    fn preserves_limit() {
        let out = build("SELECT * FROM t LIMIT 100", "a = 1", &[], SqlDialect::MySql).unwrap();
        assert_eq!(out, "SELECT * FROM t WHERE a = 1 LIMIT 100");
    }

    #[test]
    fn filter_and_sort_together() {
        let out = build(
            "SELECT id, name FROM t LIMIT 50",
            "name IS NOT NULL",
            &[("id", true)],
            SqlDialect::MySql,
        )
        .unwrap();
        assert_eq!(
            out,
            "SELECT id, name FROM t WHERE name IS NOT NULL ORDER BY `id` ASC LIMIT 50"
        );
    }

    #[test]
    fn not_eligible_join() {
        assert_eq!(
            build(
                "SELECT * FROM a JOIN b ON a.id = b.id",
                "a.x = 1",
                &[],
                SqlDialect::MySql,
            ),
            None
        );
    }

    #[test]
    fn not_eligible_derived_table() {
        assert_eq!(
            build(
                "SELECT * FROM (SELECT * FROM t) x",
                "id = 1",
                &[],
                SqlDialect::MySql,
            ),
            None
        );
    }

    #[test]
    fn not_eligible_cte() {
        assert_eq!(
            build(
                "WITH c AS (SELECT * FROM t) SELECT * FROM c",
                "id = 1",
                &[],
                SqlDialect::MySql,
            ),
            None
        );
    }

    #[test]
    fn not_eligible_union() {
        assert_eq!(
            build(
                "SELECT * FROM a UNION SELECT * FROM b",
                "x = 1",
                &[],
                SqlDialect::MySql,
            ),
            None
        );
    }

    #[test]
    fn not_eligible_multi_statement() {
        assert_eq!(
            build(
                "SELECT * FROM t; SELECT 1",
                "id = 1",
                &[],
                SqlDialect::MySql
            ),
            None
        );
    }

    #[test]
    fn not_eligible_non_select() {
        assert_eq!(
            build("UPDATE t SET a = 1", "id = 1", &[], SqlDialect::MySql),
            None
        );
    }

    #[test]
    fn bad_condition_is_error() {
        let err = build_query("SELECT * FROM t", "id = = 5", &[], SqlDialect::MySql).unwrap_err();
        assert!(matches!(err, FilterError::BadCondition(_)));
    }

    #[test]
    fn trailing_input_rejected() {
        // parse_expr would happily stop after `id = 5`; we reject the rest.
        let err = build_query(
            "SELECT * FROM t",
            "id = 5; DROP TABLE t",
            &[],
            SqlDialect::MySql,
        )
        .unwrap_err();
        assert!(matches!(err, FilterError::BadCondition(_)));
    }

    #[test]
    fn eq_condition_basic() {
        assert_eq!(
            eq_condition("country", Some("USA"), false, SqlDialect::MySql),
            "`country` = 'USA'"
        );
        assert_eq!(
            eq_condition("country", Some("USA"), true, SqlDialect::MySql),
            "`country` <> 'USA'"
        );
    }

    #[test]
    fn eq_condition_null() {
        assert_eq!(
            eq_condition("addr", None, false, SqlDialect::MySql),
            "`addr` IS NULL"
        );
        assert_eq!(
            eq_condition("addr", None, true, SqlDialect::MySql),
            "`addr` IS NOT NULL"
        );
    }

    #[test]
    fn eq_condition_quotes_per_dialect() {
        assert_eq!(
            eq_condition("x", Some("v"), false, SqlDialect::Postgres),
            "\"x\" = 'v'"
        );
    }

    #[test]
    fn eq_condition_escapes_embedded_quote() {
        assert_eq!(
            eq_condition("c", Some("O'Brien"), false, SqlDialect::MySql),
            "`c` = 'O''Brien'"
        );
    }

    #[test]
    fn eq_condition_escapes_backslash_on_mysql_only() {
        assert_eq!(
            eq_condition("c", Some("a\\b"), false, SqlDialect::MySql),
            "`c` = 'a\\\\b'"
        );
        // Postgres standard strings treat backslash literally — don't double it.
        assert_eq!(
            eq_condition("c", Some("a\\b"), false, SqlDialect::Postgres),
            "\"c\" = 'a\\b'"
        );
    }

    #[test]
    fn eq_condition_roundtrips_through_build_query() {
        // A malicious-looking value stays a single string literal — the rewritten
        // SQL still parses to exactly one statement with one WHERE.
        let cond = eq_condition("c", Some("x'); DROP TABLE t; --"), false, SqlDialect::MySql);
        let out = build_query("SELECT * FROM t", &cond, &[], SqlDialect::MySql)
            .unwrap()
            .unwrap();
        let mut reparsed = Parser::parse_sql(&*SqlDialect::MySql.parser(), &out).unwrap();
        assert_eq!(reparsed.len(), 1);
        assert!(out.contains("'x''); DROP TABLE t; --'"));
        // And structurally, not only by text: the WHERE is one comparison whose
        // right-hand side is a single string literal carrying the value verbatim.
        let Statement::Query(query) = reparsed.remove(0) else {
            panic!("expected a query");
        };
        let SetExpr::Select(select) = *query.body else {
            panic!("expected a SELECT");
        };
        let Some(Expr::BinaryOp { right, .. }) = select.selection else {
            panic!("expected one comparison in the WHERE");
        };
        match *right {
            Expr::Value(v) => assert_eq!(
                v.value,
                Value::SingleQuotedString("x'); DROP TABLE t; --".to_string())
            ),
            other => panic!("expected a string literal, got {other:?}"),
        }
    }

    /// The `ORDER BY` expressions of `sql`, re-parsed and read back as **bare**
    /// identifier values (`Ident::value`, unquoted — the spelling the catalog
    /// uses). Panics unless `sql` is exactly one statement, which is itself the
    /// assertion that the emitted quoting closed properly.
    fn reparsed_sort_idents(sql: &str, dialect: SqlDialect) -> Vec<(String, Option<bool>)> {
        let mut stmts = Parser::parse_sql(&*dialect.parser(), sql).expect("output must re-parse");
        assert_eq!(stmts.len(), 1, "expected one statement from {sql:?}");
        let Statement::Query(query) = stmts.remove(0) else {
            panic!("expected a query");
        };
        let Some(OrderBy {
            kind: OrderByKind::Expressions(exprs),
            ..
        }) = query.order_by
        else {
            panic!("expected ORDER BY expressions");
        };
        exprs
            .into_iter()
            .map(|e| match e.expr {
                Expr::Identifier(id) => (id.value, e.options.asc),
                other => panic!("expected a plain identifier, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn sort_column_quoting_survives_a_round_trip_through_the_parser() {
        // The `ORDER BY` identifier's escaping comes from sqlparser's `Display for
        // Ident`, not from this crate — nothing here pinned it, so a dependency
        // bump could have turned a column name into injectable SQL with the whole
        // suite green. Assert the property (re-parses to the same single name)
        // rather than sqlparser's spelling of it.
        let out = build_query(
            "SELECT * FROM t",
            "",
            &[("a`b".to_string(), true)],
            SqlDialect::MySql,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            reparsed_sort_idents(&out, SqlDialect::MySql),
            vec![("a`b".to_string(), Some(true))]
        );

        // PostgreSQL: a name carrying its own quote char plus a comment opener —
        // if the doubling ever stopped, the `--` would swallow the rest.
        let out = build_query(
            "SELECT * FROM t",
            "",
            &[("a\" OR 1=1 --".to_string(), false)],
            SqlDialect::Postgres,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            reparsed_sort_idents(&out, SqlDialect::Postgres),
            vec![("a\" OR 1=1 --".to_string(), Some(false))]
        );
    }

    #[test]
    fn multi_column_sort_keeps_its_order_and_directions() {
        let out = build_query(
            "SELECT * FROM t",
            "",
            &[("b".to_string(), false), ("a".to_string(), true)],
            SqlDialect::MySql,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            reparsed_sort_idents(&out, SqlDialect::MySql),
            vec![
                ("b".to_string(), Some(false)),
                ("a".to_string(), Some(true))
            ]
        );
    }

    #[test]
    fn mysql_double_quoted_value_becomes_string_literal() {
        // `name = "Albania"` should behave like `name = 'Albania'` on MySQL.
        let out = build(
            "SELECT * FROM t",
            "name = \"Albania\"",
            &[],
            SqlDialect::MySql,
        )
        .unwrap();
        assert_eq!(out, "SELECT * FROM t WHERE name = 'Albania'");
        // Also inside an IN list / nested condition.
        let out2 = build(
            "SELECT * FROM t",
            "a = \"x\" OR b IN (\"y\", \"z\")",
            &[],
            SqlDialect::MySql,
        )
        .unwrap();
        assert_eq!(out2, "SELECT * FROM t WHERE a = 'x' OR b IN ('y', 'z')");
    }

    #[test]
    fn postgres_double_quoted_stays_identifier() {
        // In Postgres "x" is a quoted identifier — leave it untouched.
        let out = build(
            "SELECT * FROM t",
            "name = \"Albania\"",
            &[],
            SqlDialect::Postgres,
        )
        .unwrap();
        assert_eq!(out, "SELECT * FROM t WHERE name = \"Albania\"");
    }

    #[test]
    fn strips_leading_where() {
        let out = build(
            "SELECT * FROM t",
            "WHERE name = 'Albania'",
            &[],
            SqlDialect::MySql,
        )
        .unwrap();
        assert_eq!(out, "SELECT * FROM t WHERE name = 'Albania'");
        // Case-insensitive + extra spaces.
        let out2 = build(
            "SELECT * FROM t",
            "  where   id = 5",
            &[],
            SqlDialect::MySql,
        )
        .unwrap();
        assert_eq!(out2, "SELECT * FROM t WHERE id = 5");
    }

    #[test]
    fn does_not_strip_where_prefixed_identifier() {
        // A column named `whereabouts` must not lose its first five letters.
        let out = build(
            "SELECT * FROM t",
            "whereabouts = 'x'",
            &[],
            SqlDialect::MySql,
        )
        .unwrap();
        assert_eq!(out, "SELECT * FROM t WHERE whereabouts = 'x'");
    }

    #[test]
    fn grid_query_is_empty() {
        assert!(GridQuery::default().is_empty());
        assert!(
            GridQuery {
                filter: "   ".to_string(),
                sort: vec![]
            }
            .is_empty()
        );
        assert!(
            !GridQuery {
                filter: "a = 1".to_string(),
                sort: vec![]
            }
            .is_empty()
        );
        assert!(
            !GridQuery {
                filter: String::new(),
                sort: vec![("a".to_string(), true)]
            }
            .is_empty()
        );
    }
}
