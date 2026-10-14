//! **Copy a table to another connection or database** — of the same engine.
//!
//! A copy is a `CREATE TABLE` on the target, through the DDL preview like any
//! other generated DDL, then the rows, streamed from the source and written by
//! the import's writer (`Db::import_rows`): one transaction, batched, every
//! batch's row count checked. This module is the pure half — the draft the
//! preview shows, the columns that carry values, the `SELECT` that reads them
//! and the statements that settle an identity afterwards.
//!
//! **Same engine only.** A copy across engines is a type mapping, and nothing
//! here has one: `int` → `integer`, `datetime` → `timestamp`, an `enum` with no
//! counterpart at all. The target picker offers only connections that
//! [`crate::connection::same_engine`] matches.
//!
//! **What does not come along:**
//! - **Foreign keys.** The tables they reference need not exist on the target,
//!   and a key that names a missing table refuses the whole `CREATE`. A copy is
//!   one table; its keys can be redrawn in the designer once its peers are there.
//! - **Triggers**, which are not part of a table's draft and run code the
//!   target may not have.
//! - **The values of a column the server assigns and refuses** — a generated
//!   column, recomputed from its expression, and PostgreSQL's `GENERATED ALWAYS
//!   AS IDENTITY`, which takes fresh numbers ([`crate::schema::ColumnInfo::is_server_assigned`],
//!   the import's own rule).

use crate::ddl::{self, ChangeSet, CheckDraft, ColumnDraft, IndexDraft, TableDraft};
use crate::export::{ident_sql, qualified_table};
use crate::intel::SqlDialect;
use crate::schema::{TableInfo, ddl_string};

/// Where the copy goes: a database on the target connection, a schema in it
/// (PostgreSQL, SQL Server), and the name the copy takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyTarget {
    pub database: String,
    pub schema: Option<String>,
    pub name: String,
}

/// Why the copy cannot be made as asked, or `None`. `same_database` — the
/// target is the source's own connection and database — makes the source's
/// own name a clash.
pub fn refusal(src: &TableInfo, to: &CopyTarget, same_database: bool) -> Option<&'static str> {
    if to.name.trim().is_empty() {
        return Some("Name the copy.");
    }
    if to.database.trim().is_empty() {
        return Some("Choose the database to copy into.");
    }
    let same_ns = match (&src.schema, &to.schema) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        (None, None) => true,
        _ => false,
    };
    if same_database && same_ns && src.name.eq_ignore_ascii_case(&to.name) {
        return Some("A table can't be copied onto itself — give the copy another name.");
    }
    None
}

/// The table the copy creates: the source's columns, key, indexes and checks,
/// all **new** to the target, under `to`'s name and schema, without its
/// foreign keys or its partitioning.
///
/// **An index's or a check's name moves with the table's.** Index names are
/// per schema on PostgreSQL and per database on SQLite, and a check's name per
/// schema on MySQL and SQL Server, so a copy under another name beside its
/// source would collide on every one of them. Where the name holds the table's
/// (`orders_total_idx`), the copy's replaces it (`orders_copy_total_idx`);
/// elsewhere the copy's name is put in front. A copy that keeps the table's
/// name keeps every name: it is in another database, or `refusal` stopped it.
/// SQLite's `sqlite_autoindex_*` are the engine's, re-declared as `UNIQUE`
/// rather than created by name, and stay as they are.
pub fn draft(src: &TableInfo, to: &CopyTarget) -> TableDraft {
    let mut d = TableDraft::from_table(src);
    d.original = None;
    d.name = to.name.clone();
    d.schema = to.schema.clone();
    d.columns = src.columns.iter().cloned().map(ColumnDraft::new).collect();
    d.foreign_keys.clear();
    // **A plain table**, whatever the source's partitioning: it holds every
    // row the source reads, and no parent gives it an index.
    d.partitioning = None;
    d.partition_of = None;
    let renamed = |name: &str| moved_name(name, &src.name, &to.name);
    d.indexes = d
        .indexes
        .into_iter()
        .map(|mut ix| {
            ix.info.name = renamed(&ix.info.name);
            ix.info.inherited = false;
            IndexDraft::new(ix.info)
        })
        .collect();
    d.check_constraints = d
        .check_constraints
        .into_iter()
        .map(|mut ck| {
            if !ck.info.name.is_empty() {
                ck.info.name = renamed(&ck.info.name);
            }
            CheckDraft::new(ck.info)
        })
        .collect();
    d
}

/// `name`, an index's or a check's, for a copy of table `from` called `to`.
fn moved_name(name: &str, from: &str, to: &str) -> String {
    if from == to || name.starts_with("sqlite_autoindex_") {
        return name.to_string();
    }
    match name.find(from) {
        Some(at) => format!("{}{to}{}", &name[..at], &name[at + from.len()..]),
        None => format!("{to}_{name}"),
    }
}

/// The plan the preview shows: [`draft`] created on the target.
pub fn plan(src: &TableInfo, to: &CopyTarget, dialect: SqlDialect) -> ChangeSet {
    ddl::create(&draft(src, to), dialect)
}

/// The columns whose values are copied, in table order: every one but those
/// the server assigns and refuses a value for.
pub fn columns(src: &TableInfo) -> Vec<String> {
    src.columns
        .iter()
        .filter(|c| !c.is_server_assigned())
        .map(|c| c.name.clone())
        .collect()
}

/// The read on the source: `columns` of the table, in that order, unsorted —
/// the rows arrive in whatever order the server streams them, which is all an
/// insert needs.
pub fn select_sql(
    database: &str,
    src: &TableInfo,
    columns: &[String],
    dialect: SqlDialect,
) -> String {
    let list = columns
        .iter()
        .map(|c| ident_sql(c, dialect))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT {list} FROM {}",
        qualified_table(database, src.schema.as_deref(), &src.name, dialect)
    )
}

/// Does an explicit value written into an auto-numbered column move the
/// engine's counter past it? MySQL's `AUTO_INCREMENT`, SQL Server's identity
/// (under the import's `IDENTITY_INSERT`) and SQLite's rowid do. A PostgreSQL
/// identity's sequence does not: it would hand out `1` next, on a table that
/// already holds it.
pub fn counter_follows_written_values(dialect: SqlDialect) -> bool {
    match dialect {
        SqlDialect::MySql | SqlDialect::MsSql | SqlDialect::Sqlite => true,
        SqlDialect::Postgres => false,
    }
}

/// What runs on the target after the rows, to move each auto-numbered
/// column's counter past the values the copy wrote — nothing where the engine
/// does that itself ([`counter_follows_written_values`]). A column whose
/// values were not copied ([`columns`]) took fresh ones, and needs nothing.
pub fn settle_sql(to: &CopyTarget, src: &TableInfo, dialect: SqlDialect) -> Vec<String> {
    if counter_follows_written_values(dialect) {
        return Vec::new();
    }
    let table = qualified_table(&to.database, to.schema.as_deref(), &to.name, dialect);
    src.columns
        .iter()
        .filter(|c| c.auto_increment && !c.is_server_assigned())
        .map(|c| {
            let col = ident_sql(&c.name, dialect);
            // `pg_get_serial_sequence` parses its first argument as a name —
            // quoted, so case and schema survive — and takes the column
            // literally. `is_called = false` makes the next value exactly
            // `MAX + 1`, and `1` on an empty table.
            format!(
                "SELECT setval(pg_get_serial_sequence({}, {}), COALESCE(MAX({col}), 0) + 1, false) FROM {table}",
                ddl_string(&table, dialect),
                ddl_string(&c.name, dialect),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{CheckInfo, ColumnInfo, ForeignKeyInfo, IndexColumn, IndexInfo};

    const PG: SqlDialect = SqlDialect::Postgres;

    fn col(name: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            type_name: "integer".into(),
            ..Default::default()
        }
    }

    fn orders() -> TableInfo {
        let mut id = col("id");
        id.auto_increment = true;
        id.primary_key = true;
        let mut total = col("total");
        total.generated = None;
        let mut doubled = col("doubled");
        doubled.generated = Some("total * 2".into());
        TableInfo {
            name: "orders".into(),
            schema: Some("public".into()),
            columns: vec![id, total, doubled],
            indexes: vec![IndexInfo {
                name: "orders_total_idx".into(),
                columns: vec![IndexColumn {
                    name: "total".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            foreign_keys: vec![ForeignKeyInfo {
                name: "orders_customer_fk".into(),
                columns: vec!["total".into()],
                ref_table: "customers".into(),
                ref_columns: vec!["id".into()],
                ..Default::default()
            }],
            check_constraints: vec![CheckInfo {
                name: "positive_total".into(),
                expression: "total > 0".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn to(name: &str) -> CopyTarget {
        CopyTarget {
            database: "shop".into(),
            schema: Some("public".into()),
            name: name.into(),
        }
    }

    #[test]
    fn the_draft_is_a_new_table_under_the_targets_name_without_foreign_keys() {
        let d = draft(&orders(), &to("orders_copy"));
        assert_eq!(d.original, None);
        assert_eq!(d.name, "orders_copy");
        assert_eq!(d.schema.as_deref(), Some("public"));
        assert!(d.columns.iter().all(|c| c.original.is_none()));
        assert!(d.indexes.iter().all(|ix| ix.original.is_none()));
        assert!(d.check_constraints.iter().all(|ck| ck.original.is_none()));
        assert!(d.foreign_keys.is_empty());
        assert_eq!(d.primary_key, vec!["id".to_string()]);
    }

    /// **A copy is one plain table**: a partitioned source copied partitioned
    /// has no partition for any row it then inserts, and a partition copied
    /// `PARTITION OF` names a parent the target may not have. The rows a
    /// partitioned table reads are its partitions', so the copy holds them
    /// all; and an index the source inherited is the copy's own.
    #[test]
    fn a_partitioned_table_or_a_partition_is_copied_as_a_plain_table() {
        let mut src = orders();
        src.partitioning = Some(crate::schema::Partitioning {
            key: "RANGE (id)".into(),
            ..Default::default()
        });
        src.partition_of = Some(crate::schema::PartitionOf {
            schema: Some("public".into()),
            parent: "all_orders".into(),
            bound: "DEFAULT".into(),
        });
        src.indexes[0].inherited = true;
        let sql = plan(&src, &to("orders_copy"), PG).emit().join("\n");
        assert!(!sql.contains("PARTITION"), "{sql}");
        assert!(
            sql.contains("CREATE INDEX \"orders_copy_total_idx\""),
            "{sql}"
        );
    }

    #[test]
    fn an_index_and_a_check_take_the_copys_name() {
        let d = draft(&orders(), &to("orders_copy"));
        assert_eq!(d.indexes[0].info.name, "orders_copy_total_idx");
        assert_eq!(
            d.check_constraints[0].info.name,
            "orders_copy_positive_total"
        );
    }

    #[test]
    fn a_copy_under_the_same_name_keeps_every_name() {
        let d = draft(&orders(), &to("orders"));
        assert_eq!(d.indexes[0].info.name, "orders_total_idx");
        assert_eq!(d.check_constraints[0].info.name, "positive_total");
    }

    #[test]
    fn sqlites_own_indexes_are_not_renamed() {
        assert_eq!(
            moved_name("sqlite_autoindex_orders_1", "orders", "o2"),
            "sqlite_autoindex_orders_1"
        );
    }

    /// The emitted plan is a `CREATE TABLE` of the copy and nothing that names
    /// the source — the composition the preview shows.
    #[test]
    fn the_plan_creates_the_copy_and_never_names_the_source() {
        let cs = plan(&orders(), &to("orders_copy"), PG);
        let sql = cs.emit().join("\n");
        assert!(
            sql.contains("CREATE TABLE \"public\".\"orders_copy\""),
            "{sql}"
        );
        assert!(sql.contains("orders_copy_total_idx"), "{sql}");
        assert!(
            !sql.contains("customers"),
            "the foreign key came along: {sql}"
        );
        assert!(
            !sql.contains("\"orders\"") && !sql.contains("orders_total_idx"),
            "{sql}"
        );
    }

    #[test]
    fn a_generated_column_is_not_read_or_written() {
        assert_eq!(
            columns(&orders()),
            vec!["id".to_string(), "total".to_string()]
        );
    }

    #[test]
    fn an_always_identity_takes_fresh_values() {
        let mut t = orders();
        t.columns[0].identity_always = true;
        assert_eq!(columns(&t), vec!["total".to_string()]);
        assert!(settle_sql(&to("o"), &t, PG).is_empty());
    }

    #[test]
    fn the_read_names_the_source_and_its_columns_in_order() {
        let t = orders();
        assert_eq!(
            select_sql("shop", &t, &columns(&t), PG),
            "SELECT \"id\", \"total\" FROM \"public\".\"orders\""
        );
        assert_eq!(
            select_sql(
                "shop",
                &TableInfo {
                    schema: None,
                    ..t.clone()
                },
                &columns(&t),
                SqlDialect::MySql
            ),
            "SELECT `id`, `total` FROM `shop`.`orders`"
        );
    }

    #[test]
    fn only_postgres_settles_its_counter_after_the_rows() {
        let t = orders();
        assert_eq!(
            settle_sql(&to("Orders Copy"), &t, PG),
            vec![
                "SELECT setval(pg_get_serial_sequence('\"public\".\"Orders Copy\"', 'id'), \
                 COALESCE(MAX(\"id\"), 0) + 1, false) FROM \"public\".\"Orders Copy\""
                    .to_string()
            ]
        );
        for d in [SqlDialect::MySql, SqlDialect::MsSql, SqlDialect::Sqlite] {
            assert!(settle_sql(&to("o"), &t, d).is_empty(), "{d:?}");
        }
    }

    #[test]
    fn a_copy_must_be_named_placed_and_not_onto_itself() {
        let t = orders();
        assert_eq!(refusal(&t, &to("orders_copy"), true), None);
        assert_eq!(refusal(&t, &to("orders"), false), None, "another database");
        assert!(refusal(&t, &to("ORDERS"), true).is_some());
        assert!(refusal(&t, &to("  "), true).is_some());
        let mut nowhere = to("x");
        nowhere.database = String::new();
        assert!(refusal(&t, &nowhere, false).is_some());
        let mut elsewhere = to("orders");
        elsewhere.schema = Some("archive".into());
        assert_eq!(refusal(&t, &elsewhere, true), None, "another schema");
    }
}
