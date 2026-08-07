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
    Statement, TableFactor, Value, visit_expressions_mut,
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
    // named table (not a derived subquery / table function).
    if query.with.is_some() {
        return Ok(None);
    }
    let SetExpr::Select(select) = query.body.as_mut() else {
        return Ok(None);
    };
    if select.from.len() != 1 || !select.from[0].joins.is_empty() {
        return Ok(None);
    }
    if !matches!(select.from[0].relation, TableFactor::Table { .. }) {
        return Ok(None);
    }

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
/// Postgres) — used when we emit an `ORDER BY` column via the AST.
fn quote_char(dialect: SqlDialect) -> char {
    match dialect {
        SqlDialect::MySql => '`',
        SqlDialect::Postgres => '"',
    }
}

/// Quote an identifier as a string, doubling any embedded quote character.
fn quote_ident(name: &str, dialect: SqlDialect) -> String {
    match dialect {
        SqlDialect::MySql => format!("`{}`", name.replace('`', "``")),
        SqlDialect::Postgres => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

/// Quote a value as a single-quoted SQL string literal. Single quotes are doubled
/// for both dialects; backslashes are additionally doubled on MySQL (where `\` is
/// an escape character in string literals by default, unlike standard-conforming
/// Postgres strings).
fn quote_value(v: &str, dialect: SqlDialect) -> String {
    let escaped = match dialect {
        SqlDialect::MySql => v.replace('\\', "\\\\").replace('\'', "''"),
        SqlDialect::Postgres => v.replace('\'', "''"),
    };
    format!("'{escaped}'")
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
/// `key_cols` is normally the primary key, in key order; empty (no PK, or the
/// schema hasn't been introspected yet) means no `ORDER BY` at all rather than a
/// guess at some column.
///
/// Dialect-aware, matching the write path: MySQL qualifies `` `db`.`table` ``
/// (its connection is server-level), Postgres emits the unqualified,
/// double-quoted name (its connection is already bound to the database, and the
/// name resolves through `search_path`).
pub fn table_query(
    dialect: SqlDialect,
    database: &str,
    table: &str,
    key_cols: &[String],
    order: Order,
    limit: usize,
) -> String {
    let name = match dialect {
        SqlDialect::MySql => format!(
            "{}.{}",
            quote_ident(database, dialect),
            quote_ident(table, dialect)
        ),
        SqlDialect::Postgres => quote_ident(table, dialect),
    };
    let order_by = if key_cols.is_empty() {
        String::new()
    } else {
        let cols = key_cols
            .iter()
            .map(|c| format!("{} {}", quote_ident(c, dialect), order.keyword()))
            .collect::<Vec<_>>()
            .join(", ");
        format!(" ORDER BY {cols}")
    };
    format!("SELECT * FROM {name}{order_by} LIMIT {limit}")
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
        table_query(d, "shop", table, &pk, order, limit)
    }

    #[test]
    fn table_query_mysql_qualifies_and_orders() {
        assert_eq!(
            tq(SqlDialect::MySql, "orders", &["id"], Order::Asc, 100),
            "SELECT * FROM `shop`.`orders` ORDER BY `id` ASC LIMIT 100"
        );
    }

    #[test]
    fn table_query_postgres_is_unqualified() {
        // A PG connection is already bound to the database; the table resolves
        // through `search_path`, exactly like the write path.
        assert_eq!(
            tq(SqlDialect::Postgres, "orders", &["id"], Order::Asc, 100),
            "SELECT * FROM \"orders\" ORDER BY \"id\" ASC LIMIT 100"
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
            "SELECT * FROM `shop`.`line_items` ORDER BY `order_id` ASC, `line_no` ASC LIMIT 50"
        );
    }

    #[test]
    fn table_query_without_a_key_is_unordered() {
        // No PK (or the schema hasn't loaded) → no ORDER BY at all, rather than
        // guessing at a column.
        assert_eq!(
            tq(SqlDialect::MySql, "logs", &[], Order::Asc, 100),
            "SELECT * FROM `shop`.`logs` LIMIT 100"
        );
        assert_eq!(
            tq(SqlDialect::Postgres, "logs", &[], Order::Asc, 100),
            "SELECT * FROM \"logs\" LIMIT 100"
        );
    }

    #[test]
    fn table_query_descending() {
        // The AI seed sample wants the most recent rows.
        assert_eq!(
            tq(SqlDialect::MySql, "orders", &["id"], Order::Desc, 20),
            "SELECT * FROM `shop`.`orders` ORDER BY `id` DESC LIMIT 20"
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
                "ta`ble",
                &["i`d".to_string()],
                Order::Asc,
                10
            ),
            "SELECT * FROM `we``ird`.`ta``ble` ORDER BY `i``d` ASC LIMIT 10"
        );
        assert_eq!(
            table_query(
                SqlDialect::Postgres,
                "db",
                "ta\"ble",
                &["i\"d".to_string()],
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
            "SELECT * FROM `shop`.`orders` ORDER BY `total` DESC LIMIT 100"
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
        let reparsed = Parser::parse_sql(&*SqlDialect::MySql.parser(), &out).unwrap();
        assert_eq!(reparsed.len(), 1);
        assert!(out.contains("'x''); DROP TABLE t; --'"));
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
