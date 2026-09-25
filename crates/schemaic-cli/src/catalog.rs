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
/// not have. Sequences are listed on none.
///
/// On SQLite `database` is ignored: the file is the database, and every
/// operation opens its own connection, so nothing else is ever attached.
pub fn tables_sql(dialect: SqlDialect, database: Option<&str>) -> Option<String> {
    let q = |n: &str| ident_sql(n, dialect);
    Some(match dialect {
        // `DATABASE()` rather than a literal of the name: it is the database
        // the statement runs in, which is the one `-d` named. Without one it
        // is NULL and the listing is empty, so a database is required.
        // MariaDB's `TABLE_TYPE` has two more spellings of the same two
        // things — a system-versioned table is a table — and a `SEQUENCE`,
        // left out as PostgreSQL's listing leaves its sequences out.
        SqlDialect::MySql => {
            database?;
            format!(
                "SELECT CAST(TABLE_NAME AS CHAR) AS {name}, \
                 CASE TABLE_TYPE WHEN 'BASE TABLE' THEN 'table' \
                 WHEN 'SYSTEM VERSIONED' THEN 'table' \
                 WHEN 'VIEW' THEN 'view' WHEN 'SYSTEM VIEW' THEN 'view' END AS {ty} \
                 FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() \
                 AND TABLE_TYPE IN ('BASE TABLE', 'SYSTEM VERSIONED', 'VIEW', 'SYSTEM VIEW') \
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
        // The file is the database, so there is always one: `main`.
        SqlDialect::Sqlite => format!(
            "SELECT name AS {name}, type AS {ty} FROM \"main\".sqlite_master \
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
             ORDER BY name",
            name = q("name"),
            ty = q("type"),
        ),
    })
}

/// The columns `describe` prints, on every engine.
pub const DESCRIBE_COLUMNS: [&str; 5] = ["column", "type", "nullable", "default", "key"];

/// One table's (or view's) columns, in their declared order: name, type as
/// the engine spells it, `YES`/`NO` for nullable, the default's expression,
/// and a key marker in MySQL's own `COLUMN_KEY` words. How much of it an
/// engine can say differs: MySQL gives its own `PRI`/`UNI`/`MUL`, PostgreSQL
/// `PRI` and `UNI` (a single-column unique index, not a partial one), and
/// SQLite `PRI` alone, since `pragma_table_xinfo` says nothing of unique
/// indexes. `None` when the dialect needs a database and there is none; on
/// SQLite `database` is ignored, as in [`tables_sql`].
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
        // cannot find is NULL, which matches no row. It also finds an index
        // or a sequence, so the relation has to be of a kind `tables` lists.
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
                   AND i.indpred IS NULL AND i.indkey[0] = a.attnum) THEN 'UNI' \
                 ELSE '' END AS {key} \
                 FROM pg_catalog.pg_attribute a \
                 JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
                 LEFT JOIN pg_catalog.pg_attrdef d \
                   ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
                 WHERE a.attrelid = pg_catalog.to_regclass({table}) \
                 AND c.relkind IN ('r', 'p', 'v', 'm', 'f') \
                 AND a.attnum > 0 AND NOT a.attisdropped \
                 ORDER BY a.attnum",
                table = lit(table),
            )
        }
        // `pragma_table_xinfo` as a table-valued function, so it can be a
        // `SELECT` — the only head the read gate lets through — and take the
        // same aliases as the other two. `xinfo`, not `info`: only it reports
        // a generated column (`hidden` 2 or 3), and `hidden = 1` is a virtual
        // table's hidden column, which is no column of the table's rows (as
        // `db::sqlite`'s own column read has it). Its `pk` is the column's
        // position in the primary key, `0` when it is not in one.
        SqlDialect::Sqlite => format!(
            "SELECT name AS {column}, type AS {ty}, \
             CASE WHEN \"notnull\" THEN 'NO' ELSE 'YES' END AS {nullable}, \
             dflt_value AS {default}, \
             CASE WHEN pk > 0 THEN 'PRI' ELSE '' END AS {key} \
             FROM pragma_table_xinfo({table}, 'main') WHERE hidden <> 1 ORDER BY cid",
            table = lit(table),
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
            sqlite.contains("pragma_table_xinfo('t', 'main')"),
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

    /// Run a canned SQLite statement against an in-memory database built by
    /// `setup`, and return its rows as text.
    fn sqlite_rows(setup: &str, sql: &str) -> Vec<Vec<String>> {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(setup).unwrap();
        let mut stmt = db.prepare(sql).unwrap();
        let n = stmt.column_count();
        stmt.query_map([], |r| {
            (0..n)
                .map(|i| {
                    Ok(match r.get_ref(i)? {
                        rusqlite::types::ValueRef::Null => "NULL".to_string(),
                        rusqlite::types::ValueRef::Integer(v) => v.to_string(),
                        v => v.as_str().unwrap_or("?").to_string(),
                    })
                })
                .collect()
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
    }

    /// **The file is the database.** Every operation opens its own connection,
    /// so nothing but `main` is ever attached, and a `-d` (or a
    /// `SCHEMAIC_DATABASE` meant for another engine) is ignored as `query`
    /// ignores it — it read as a schema name and failed with exit 4, "retry
    /// may work", on a command line that never could.
    #[test]
    fn a_sqlite_listing_ignores_the_database_it_is_given() {
        let setup = "CREATE TABLE t (id INTEGER PRIMARY KEY); CREATE VIEW v AS SELECT 1;";
        for db in [None, Some("shop"), Some("a\"b")] {
            let tables = sqlite_rows(setup, &tables_sql(SqlDialect::Sqlite, db).unwrap());
            assert_eq!(tables, [["t", "table"], ["v", "view"]], "{db:?}");
            let columns = sqlite_rows(setup, &describe_sql(SqlDialect::Sqlite, db, "t").unwrap());
            assert_eq!(columns.len(), 1, "{db:?}");
        }
    }

    /// **A generated column is a column.** `pragma_table_info` leaves it out
    /// — only `table_xinfo` reports one — so `describe` answered a different
    /// column set on SQLite than on the servers, successfully.
    #[test]
    fn a_sqlite_describe_lists_generated_columns() {
        let rows = sqlite_rows(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, a INT NOT NULL, \
             b INT GENERATED ALWAYS AS (a * 2) VIRTUAL, \
             c INT GENERATED ALWAYS AS (a + 1) STORED, s TEXT DEFAULT 'x');",
            &describe_sql(SqlDialect::Sqlite, None, "t").unwrap(),
        );
        let names: Vec<&str> = rows.iter().map(|r| r[0].as_str()).collect();
        assert_eq!(names, ["id", "a", "b", "c", "s"]);
        assert_eq!(rows[0][4], "PRI");
        assert_eq!(rows[1][2], "NO");
        assert_eq!(rows[4][3], "'x'");
    }

    /// **PostgreSQL describes what `tables` lists**, and nothing else:
    /// `to_regclass` finds an index or a sequence too, which was printed as a
    /// table. And a partial unique index is no uniqueness of the column.
    #[test]
    fn a_postgres_describe_is_of_a_listed_relation_only() {
        let sql = describe_sql(SqlDialect::Postgres, Some("app"), "orders").unwrap();
        assert!(
            sql.contains("c.relkind IN ('r', 'p', 'v', 'm', 'f')"),
            "{sql}"
        );
        let tables = tables_sql(SqlDialect::Postgres, Some("app")).unwrap();
        assert!(tables.contains("c.relkind IN ('r', 'p', 'v', 'm', 'f')"));
        assert!(sql.contains("i.indpred IS NULL"), "{sql}");
    }

    /// **One spelling per type on MySQL and MariaDB too.** MariaDB's
    /// `TABLE_TYPE` also says `SYSTEM VERSIONED` (a table) and `SYSTEM VIEW`,
    /// and lists sequences, which PostgreSQL's listing leaves out; each went
    /// through as its own lower-cased word.
    #[test]
    fn a_mysql_listing_spells_every_type_one_way() {
        let sql = tables_sql(SqlDialect::MySql, Some("app")).unwrap();
        assert!(
            sql.contains("WHEN 'SYSTEM VERSIONED' THEN 'table'"),
            "{sql}"
        );
        assert!(sql.contains("WHEN 'SYSTEM VIEW' THEN 'view'"), "{sql}");
        assert!(!sql.contains("LOWER("), "no type passes through: {sql}");
        assert!(
            sql.contains("TABLE_TYPE IN ('BASE TABLE', 'SYSTEM VERSIONED', 'VIEW', 'SYSTEM VIEW')"),
            "{sql}"
        );
    }
}
