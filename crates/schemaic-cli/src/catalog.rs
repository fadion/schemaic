//! The canned SQL behind `schemaic tables` and `schemaic describe`.
//!
//! **Text, run through the one headless read path**, rather than a `Db` method
//! of its own: what these print is rows like any query's, so they go through
//! `query::read_only_query` — its gate, its read-only session, its deadline
//! and its cap — and `--format` means what it means everywhere. Only the SQL
//! is per dialect, because the question is: PostgreSQL and SQLite have no
//! `DESCRIBE`, and MySQL's `SHOW` output names its columns after the database.
//! Every dialect answers under the **same column names**, so a script reading
//! one engine's output reads the others'.
//!
//! **A name from the command line reaches the SQL only as a literal**, through
//! `export::sql_literal`, never spliced as an identifier. MySQL's doubles a
//! backslash, which is right because the read path makes it right: the
//! session `read_only_query` runs on has `NO_BACKSLASH_ESCAPES` taken out of
//! its `sql_mode` and read back (`mysql::enforce_session`), or the statement
//! is refused.

use schemaic_core::export::{ident_sql, sql_literal};
use schemaic_core::intel::SqlDialect;
use schemaic_core::model::Value;

/// The column names `tables` prints — `schema` first on PostgreSQL alone,
/// the one engine whose database holds more than one namespace.
///
/// Each type is spelled one way on every engine: `table`, `view`, and on
/// PostgreSQL `materialized view` and `foreign table`, which the other two do
/// not have.
pub fn tables_sql(dialect: SqlDialect, database: Option<&str>) -> Option<String> {
    let q = |n: &str| ident_sql(n, dialect);
    Some(match dialect {
        // `DATABASE()` rather than a literal of the name: it is the database
        // the statement runs in, which is the one `-d` named. Without one it
        // is NULL and the listing is empty, so a database is required.
        SqlDialect::MySql => {
            database?;
            format!(
                "SELECT CAST(TABLE_NAME AS CHAR) AS {name}, \
                 CASE TABLE_TYPE WHEN 'BASE TABLE' THEN 'table' WHEN 'VIEW' THEN 'view' \
                 ELSE LOWER(CAST(TABLE_TYPE AS CHAR)) END AS {ty} \
                 FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() \
                 ORDER BY TABLE_NAME",
                name = q("name"),
                ty = q("type"),
            )
        }
        // The catalog rather than `information_schema.tables`, which has no
        // materialized views. Partitions are left out — their parent is the
        // table — and so are the system and per-session temporary schemas.
        // Without a database this would list the maintenance database's
        // tables as though they were the connection's.
        SqlDialect::Postgres => {
            database?;
            format!(
                "SELECT n.nspname AS {schema}, c.relname AS {name}, \
                 CASE c.relkind WHEN 'r' THEN 'table' WHEN 'p' THEN 'table' \
                 WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized view' \
                 WHEN 'f' THEN 'foreign table' END AS {ty} \
                 FROM pg_catalog.pg_class c \
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f') AND NOT c.relispartition \
                 AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
                 AND n.nspname NOT LIKE 'pg\\_toast%' AND n.nspname NOT LIKE 'pg\\_temp\\_%' \
                 ORDER BY 1, 2",
                schema = q("schema"),
                name = q("name"),
                ty = q("type"),
            )
        }
        // The file is the database, so there is always one: `main`, unless
        // `-d` names another the connection has attached.
        SqlDialect::Sqlite => format!(
            "SELECT name AS {name}, type AS {ty} FROM {db}.sqlite_master \
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
             ORDER BY name",
            name = q("name"),
            ty = q("type"),
            db = q(database.unwrap_or("main")),
        ),
    })
}

/// The columns `describe` prints, on every engine.
pub const DESCRIBE_COLUMNS: [&str; 5] = ["column", "type", "nullable", "default", "key"];

/// One table's (or view's) columns, in their declared order: name, type as
/// the engine spells it, `YES`/`NO` for nullable, the default's expression,
/// and a key marker in MySQL's own `COLUMN_KEY` words. How much of it an
/// engine can say differs: MySQL gives its own `PRI`/`UNI`/`MUL`, PostgreSQL
/// `PRI` and `UNI` (a single-column unique index), and SQLite `PRI` alone,
/// since `pragma_table_info` says nothing of unique indexes. `None` when the
/// dialect needs a database and there is none.
///
/// **No rows means no such table**, which the caller reports; a relation with
/// no columns at all (PostgreSQL allows `CREATE TABLE t ()`) reads the same,
/// and is rare enough to take the wrong message.
pub fn describe_sql(dialect: SqlDialect, database: Option<&str>, table: &str) -> Option<String> {
    let q = |n: &str| ident_sql(n, dialect);
    let lit = |s: &str| sql_literal(&Value::Str(s.to_string()), dialect);
    let [column, ty, nullable, default, key] = DESCRIBE_COLUMNS.map(q);
    Some(match dialect {
        SqlDialect::MySql => {
            database?;
            format!(
                "SELECT CAST(COLUMN_NAME AS CHAR) AS {column}, \
                 CAST(COLUMN_TYPE AS CHAR) AS {ty}, \
                 CAST(IS_NULLABLE AS CHAR) AS {nullable}, \
                 CAST(COLUMN_DEFAULT AS CHAR) AS {default}, \
                 CAST(COLUMN_KEY AS CHAR) AS {key} \
                 FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = {table} \
                 ORDER BY ORDINAL_POSITION",
                table = lit(table),
            )
        }
        // `to_regclass` reads the name the way the server reads one: through
        // the search path when bare, `schema.table` when qualified, and
        // case-folded unless double-quoted — so `describe public.orders` and
        // `describe '"Mixed"'` both mean what they would in a query. A name it
        // cannot find is NULL, which matches no row.
        SqlDialect::Postgres => {
            database?;
            format!(
                "SELECT a.attname AS {column}, \
                 pg_catalog.format_type(a.atttypid, a.atttypmod) AS {ty}, \
                 CASE WHEN a.attnotnull THEN 'NO' ELSE 'YES' END AS {nullable}, \
                 pg_catalog.pg_get_expr(d.adbin, d.adrelid) AS {default}, \
                 CASE WHEN EXISTS (SELECT 1 FROM pg_catalog.pg_index i \
                   WHERE i.indrelid = a.attrelid AND i.indisprimary \
                   AND a.attnum = ANY (i.indkey)) THEN 'PRI' \
                 WHEN EXISTS (SELECT 1 FROM pg_catalog.pg_index i \
                   WHERE i.indrelid = a.attrelid AND i.indisunique AND i.indnatts = 1 \
                   AND i.indkey[0] = a.attnum) THEN 'UNI' \
                 ELSE '' END AS {key} \
                 FROM pg_catalog.pg_attribute a \
                 LEFT JOIN pg_catalog.pg_attrdef d \
                   ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
                 WHERE a.attrelid = pg_catalog.to_regclass({table}) \
                 AND a.attnum > 0 AND NOT a.attisdropped \
                 ORDER BY a.attnum",
                table = lit(table),
            )
        }
        // `pragma_table_info` as a table-valued function, so it can be a
        // `SELECT` — the only head the read gate lets through — and take the
        // same aliases as the other two. Its `pk` is the column's position in
        // the primary key, `0` when it is not in one.
        SqlDialect::Sqlite => format!(
            "SELECT name AS {column}, type AS {ty}, \
             CASE WHEN \"notnull\" THEN 'NO' ELSE 'YES' END AS {nullable}, \
             dflt_value AS {default}, \
             CASE WHEN pk > 0 THEN 'PRI' ELSE '' END AS {key} \
             FROM pragma_table_info({table}, {db}) ORDER BY cid",
            table = lit(table),
            db = lit(database.unwrap_or("main")),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [SqlDialect; 3] = [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite];

    /// **What these run is a read the gate lets through**, on every engine —
    /// the same gate a typed `query` meets, so a canned statement it refused
    /// would fail every `tables` before it reached a server.
    #[test]
    fn every_canned_statement_passes_the_read_gate() {
        for d in ALL {
            let tables = tables_sql(d, Some("app")).unwrap();
            assert_eq!(crate::query::gate(&tables, d), Ok(tables.as_str()), "{d:?}");
            let describe = describe_sql(d, Some("app"), "orders").unwrap();
            assert_eq!(
                crate::query::gate(&describe, d),
                Ok(describe.as_str()),
                "{d:?}"
            );
        }
    }

    /// **Without a database MySQL and PostgreSQL have no answer** — MySQL's
    /// would be empty and PostgreSQL's the maintenance database's — while a
    /// SQLite file always is one.
    #[test]
    fn only_a_file_database_needs_no_database_named() {
        assert!(tables_sql(SqlDialect::MySql, None).is_none());
        assert!(tables_sql(SqlDialect::Postgres, None).is_none());
        assert!(describe_sql(SqlDialect::MySql, None, "t").is_none());
        assert!(describe_sql(SqlDialect::Postgres, None, "t").is_none());
        let sqlite = tables_sql(SqlDialect::Sqlite, None).unwrap();
        assert!(sqlite.contains("\"main\".sqlite_master"), "{sqlite}");
        let sqlite = describe_sql(SqlDialect::Sqlite, None, "t").unwrap();
        assert!(
            sqlite.contains("pragma_table_info('t', 'main')"),
            "{sqlite}"
        );
    }

    /// **A table name is a literal, never an identifier spliced in** — a
    /// quote in it is doubled, so it cannot end the string and begin a
    /// statement.
    #[test]
    fn a_table_name_reaches_the_sql_only_as_a_literal() {
        for d in ALL {
            let sql = describe_sql(d, Some("app"), "o'; DROP TABLE x; --").unwrap();
            assert!(sql.contains("'o''; DROP TABLE x; --'"), "{d:?}: {sql}");
            assert_eq!(crate::query::gate(&sql, d), Ok(sql.as_str()), "{d:?}");
        }
    }

    /// Every engine answers under one set of names, quoted, since `default`,
    /// `key` and `column` are reserved words on at least one of them.
    #[test]
    fn every_engine_answers_under_the_same_column_names() {
        for d in ALL {
            let sql = describe_sql(d, Some("app"), "t").unwrap();
            for name in DESCRIBE_COLUMNS {
                assert!(sql.contains(&ident_sql(name, d)), "{d:?} {name}: {sql}");
            }
            let sql = tables_sql(d, Some("app")).unwrap();
            for name in ["name", "type"] {
                assert!(sql.contains(&ident_sql(name, d)), "{d:?} {name}: {sql}");
            }
        }
    }

    /// SQLite's `-d` names an attached database; it is quoted as a name, and
    /// passed to the pragma as the literal it takes.
    #[test]
    fn a_sqlite_database_is_quoted_where_it_is_used() {
        let sql = tables_sql(SqlDialect::Sqlite, Some("a\"b")).unwrap();
        assert!(sql.contains("\"a\"\"b\".sqlite_master"), "{sql}");
        let sql = describe_sql(SqlDialect::Sqlite, Some("a'b"), "t").unwrap();
        assert!(sql.contains("pragma_table_info('t', 'a''b')"), "{sql}");
    }
}
