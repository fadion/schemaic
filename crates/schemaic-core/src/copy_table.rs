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
//!   column, recomputed from its expression, and a SQL Server `rowversion`
//!   ([`crate::schema::ColumnInfo::is_server_assigned`], the import's own rule).
//!   **An identity is not one of them**, though the server numbers it: its
//!   values are the table's keys, so SQL Server's and PostgreSQL's
//!   `GENERATED ALWAYS` are written as they are ([`carries_identity`]).
//!
//! **Every value comes as it is**, bytes included: a blob is read as text the
//! server makes of it and written back as bytes ([`read_form`]), and a cell
//! the read could not carry stops the copy rather than reaching the target
//! ([`chunk_refusal`]).

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
pub fn refusal(src: &TableInfo, to: &CopyTarget, same_database: bool) -> Option<CopyRefusal> {
    if to.name.trim().is_empty() {
        return Some(CopyRefusal::Unnamed);
    }
    if to.database.trim().is_empty() {
        return Some(CopyRefusal::NoDatabase);
    }
    let same_ns = match (&src.schema, &to.schema) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        (None, None) => true,
        _ => false,
    };
    if same_database && same_ns && src.name.eq_ignore_ascii_case(&to.name) {
        return Some(CopyRefusal::OntoItself);
    }
    None
}

/// [`refusal`]'s answer — a value the interface words; [`Self::message`] is
/// the English.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyRefusal {
    /// The copy has no name.
    Unnamed,
    /// No database to copy into is chosen.
    NoDatabase,
    /// The copy would land on the source itself.
    OntoItself,
}

impl CopyRefusal {
    /// The refusal in English.
    pub fn message(self) -> &'static str {
        match self {
            CopyRefusal::Unnamed => "Name the copy.",
            CopyRefusal::NoDatabase => "Choose the database to copy into.",
            CopyRefusal::OntoItself => {
                "A table can't be copied onto itself — give the copy another name."
            }
        }
    }
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
/// the server assigns and refuses a value for — **except an identity the
/// write can carry** ([`carries_identity`]), whose values are the table's
/// keys.
pub fn columns(src: &TableInfo, dialect: SqlDialect) -> Vec<String> {
    src.columns
        .iter()
        .filter(|c| copies(c, dialect))
        .map(|c| c.name.clone())
        .collect()
}

/// Is `c`'s value read and written by the copy?
fn copies(c: &crate::schema::ColumnInfo, dialect: SqlDialect) -> bool {
    !c.is_server_assigned() || carries_identity(c, dialect)
}

/// Is `c` an identity the server numbers itself and refuses a value for, which
/// the copy writes anyway — so that the copy keeps the source's keys rather
/// than numbering its rows `1..n` in whatever order the read streamed them?
///
/// SQL Server's, under the import's `IDENTITY_INSERT` (the dump's own rule,
/// [`crate::dump::carries_identity`]), and PostgreSQL's `GENERATED ALWAYS`,
/// through an `INSERT … OVERRIDING SYSTEM VALUE`
/// ([`crate::import::overriding_clause`]) — which the dump has no way to write,
/// and so renumbers. A `rowversion` is no identity, and stays the server's.
pub fn carries_identity(c: &crate::schema::ColumnInfo, dialect: SqlDialect) -> bool {
    crate::dump::carries_identity(c, dialect)
        || (c.auto_increment
            && c.generated.is_none()
            && crate::import::overriding_clause(dialect).is_some())
}

/// Everything the rows half needs to read the source and write the target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowsRead {
    /// [`columns`], in table order.
    pub columns: Vec<String>,
    /// How each of them arrives, in the same order — [`read_form`].
    pub forms: Vec<crate::import::CellForm>,
    /// The read on the source — [`select_sql`].
    pub select: String,
    /// Whether the write needs [`crate::import::overriding_clause`]: a carried
    /// identity is among `columns` on an engine that asks for it per `INSERT`.
    pub overriding: bool,
}

/// The read *Copy to* runs on `database`'s `src`, and how its cells are
/// written.
pub fn rows_read(database: &str, src: &TableInfo, dialect: SqlDialect) -> RowsRead {
    rows_read_with(database, src, &[], dialect)
}

/// [`rows_read`] with the source database's SQL Server objects
/// (`DbSchema::tsql_objects`), so a column typed by an **alias** is read by the
/// alias's base, as the dump reads it ([`crate::dump::literal_select`]):
/// without them an alias over `varbinary` arrived as the `<n bytes>`
/// placeholder and stopped every copy of its table, and one over
/// `sql_variant` came back an `nvarchar` variant.
pub fn rows_read_with(
    database: &str,
    src: &TableInfo,
    aliases: &[crate::schema::TsqlObject],
    dialect: SqlDialect,
) -> RowsRead {
    let columns = columns(src, dialect);
    let forms = columns
        .iter()
        .map(|name| {
            src.columns
                .iter()
                .find(|c| &c.name == name)
                .map_or(crate::import::CellForm::Value, |c| {
                    read_form_with(c, aliases, dialect).1
                })
        })
        .collect();
    let overriding = crate::import::overriding_clause(dialect).is_some()
        && src
            .columns
            .iter()
            .any(|c| c.is_server_assigned() && carries_identity(c, dialect));
    RowsRead {
        select: select_sql_with(database, src, &columns, aliases, dialect),
        columns,
        forms,
        overriding,
    }
}

/// How the copy reads `c`: the expression in the `SELECT` and the form the
/// value arrives in.
///
/// **A raw-bytes value has no text of its own** — every engine's read renders
/// it as the `<n bytes>` placeholder, which the copy wrote into the target as
/// the data — so it is read as text the server makes of it, and written back
/// as bytes: hex on MySQL and PostgreSQL, SQL Server's own literal (as its
/// dump reads it, `dump::literal_select`, a `sql_variant` too), and on SQLite
/// `quote()` of *every* column, since a declared type there is only an
/// affinity and a blob can sit in any of them. A column this misses still
/// arrives as the placeholder, and [`chunk_refusal`] stops the copy on it.
///
/// **A MySQL-family `FLOAT` is read as a double** ([`crate::dump::exact_float_select`]):
/// the text protocol spells one with six significant digits, so the copy
/// wrote another float under a success report.
pub fn read_form(
    c: &crate::schema::ColumnInfo,
    dialect: SqlDialect,
) -> (String, crate::import::CellForm) {
    read_form_with(c, &[], dialect)
}

/// [`read_form`], with SQL Server's alias types to judge a column by — see
/// [`rows_read_with`].
pub fn read_form_with(
    c: &crate::schema::ColumnInfo,
    aliases: &[crate::schema::TsqlObject],
    dialect: SqlDialect,
) -> (String, crate::import::CellForm) {
    use crate::import::CellForm;
    if let Some(expr) = crate::dump::exact_float_select(c, dialect) {
        return (expr, CellForm::Value);
    }
    let col = ident_sql(&c.name, dialect);
    let head = c
        .type_name
        .split(|ch: char| ch == '(' || ch.is_whitespace())
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match dialect {
        SqlDialect::MySql
            if crate::model::type_is_binary(&head) || MYSQL_SPATIAL.contains(&head.as_str()) =>
        {
            (format!("HEX({col}) AS {col}"), CellForm::Hex)
        }
        SqlDialect::Postgres if head == "bytea" => {
            (format!("encode({col}, 'hex') AS {col}"), CellForm::Hex)
        }
        SqlDialect::MsSql => match crate::dump::literal_select(c, aliases, dialect) {
            Some((expr, form)) => (expr, CellForm::Server(form)),
            None => (col, CellForm::Value),
        },
        SqlDialect::Sqlite => (format!("quote({col}) AS {col}"), CellForm::Quoted),
        SqlDialect::MySql | SqlDialect::Postgres => (col, CellForm::Value),
    }
}

/// MySQL's and MariaDB's spatial types, which the server sends as their
/// internal bytes and takes back as the same.
const MYSQL_SPATIAL: &[&str] = &[
    "geometry",
    "point",
    "linestring",
    "polygon",
    "multipoint",
    "multilinestring",
    "multipolygon",
    "geometrycollection",
    "geomcollection",
];

/// The read on the source: `columns` of the table, in that order, each as
/// [`read_form`] reads it, unsorted — the rows arrive in whatever order the
/// server streams them, which is all an insert needs.
pub fn select_sql(
    database: &str,
    src: &TableInfo,
    columns: &[String],
    dialect: SqlDialect,
) -> String {
    select_sql_with(database, src, columns, &[], dialect)
}

/// [`select_sql`] with SQL Server's alias types — see [`rows_read_with`].
pub fn select_sql_with(
    database: &str,
    src: &TableInfo,
    columns: &[String],
    aliases: &[crate::schema::TsqlObject],
    dialect: SqlDialect,
) -> String {
    let list = columns
        .iter()
        .map(|name| match src.columns.iter().find(|c| &c.name == name) {
            Some(c) => read_form_with(c, aliases, dialect).0,
            None => ident_sql(name, dialect),
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT {list} FROM {}",
        qualified_table(database, src.schema.as_deref(), &src.name, dialect)
    )
}

/// Why a chunk of the copy's read must not be written, or `None`.
///
/// The safety net under [`read_form`]: a cell that still arrived as the
/// `<n bytes>` placeholder in a column holding bytes (`model::is_binary_display`
/// — a type `read_form` does not know, a SQL Server alias over `varbinary`) is
/// a value the copy does not have, and so is every cell of a column whose text
/// outgrew the chunk's arena (`ResultSet::capped_columns`), which reads blank
/// past that point. Written, either is a wrong value under a success report.
pub fn chunk_refusal(rs: &crate::model::ResultSet) -> Option<String> {
    if let Some(&c) = rs.capped_columns.first() {
        let name = rs.columns.get(c).map_or("a column", |c| c.name.as_str());
        return Some(format!(
            "{name} holds more text than one read can carry, so the copy stopped before writing it"
        ));
    }
    for (c, col) in rs.columns.iter().enumerate() {
        if !(rs.binary_columns.contains(&c) || col.is_binary()) {
            continue;
        }
        let placeholder = (0..rs.row_count()).any(|r| {
            rs.cell(r, c)
                .is_some_and(|v| !v.is_null() && crate::model::is_binary_display(v.text()))
        });
        if placeholder {
            return Some(format!(
                "{} holds bytes the copy cannot read as they are, so it stopped before writing them",
                col.name
            ));
        }
    }
    None
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
        .filter(|c| c.auto_increment && copies(c, dialect))
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

/// Put a running copy's cancel handle in the one slot Stop and ✕ read,
/// returning the launch's number for [`release`].
pub fn launch<T>(slot: &mut Option<(u64, T)>, handle: T) -> u64 {
    let n = LAUNCHES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    *slot = Some((n, handle));
    n
}

/// Every launch's number is new, the slot emptied or not.
static LAUNCHES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A finished copy's report: empty the slot **only if it still holds this
/// launch's handle**. A copy closed mid-way can still be rolling back when the
/// next one starts, and its report clearing the slot left the newer copy's
/// Stop and ✕ cancelling nothing.
pub fn release<T>(slot: &mut Option<(u64, T)>, launch: u64) {
    if slot.as_ref().is_some_and(|(n, _)| *n == launch) {
        *slot = None;
    }
}

/// The change-list line for the rows half, beside the `CREATE TABLE`'s own.
/// The rows are streamed once the table has committed, not run from the
/// preview's script, so the line says so — without it the preview read as a
/// structure-only copy while Apply went on to write every row.
pub fn rows_change(src: &TableInfo) -> String {
    format!(
        "Then copy every row of {} into it (not shown as SQL)",
        crate::schema::display_name(src.schema.as_deref(), &src.name)
    )
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
            ..Default::default()
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
        for d in [PG, SqlDialect::MySql, SqlDialect::MsSql, SqlDialect::Sqlite] {
            assert_eq!(
                columns(&orders(), d),
                vec!["id".to_string(), "total".to_string()],
                "{d:?}"
            );
        }
    }

    /// **A copy keeps the source's keys.** SQL Server reads every identity as
    /// one the server assigns (`identity_always`), and leaving it out of the
    /// copied columns numbered the copy's rows `1..n` in the order the read
    /// streamed them — ids 5, 10, 42 became 1, 2, 3 under a success report.
    /// PostgreSQL's `GENERATED ALWAYS` was renumbered the same way; it is
    /// written through `OVERRIDING SYSTEM VALUE`, and its counter settled.
    #[test]
    fn an_always_identity_is_copied_with_its_values() {
        let mut t = orders();
        t.columns[0].identity_always = true;
        for d in [PG, SqlDialect::MsSql] {
            assert_eq!(
                columns(&t, d),
                vec!["id".to_string(), "total".to_string()],
                "{d:?}"
            );
        }
        assert!(rows_read("shop", &t, PG).overriding);
        assert!(!rows_read("shop", &t, SqlDialect::MsSql).overriding);
        assert_eq!(
            settle_sql(&to("o"), &t, PG).len(),
            1,
            "the counter moves past them"
        );
    }

    /// A `rowversion` is the server's and no identity: it stays out, and
    /// nothing asks for an override on its account.
    #[test]
    fn a_rowversion_is_not_copied() {
        let mut t = orders();
        t.columns[1].identity_always = true;
        assert_eq!(
            columns(&t, SqlDialect::MsSql),
            vec!["id".to_string()],
            "total stood in for a rowversion"
        );
        assert!(!rows_read("shop", &t, PG).overriding);
    }

    #[test]
    fn the_read_names_the_source_and_its_columns_in_order() {
        let t = orders();
        assert_eq!(
            select_sql("shop", &t, &columns(&t, PG), PG),
            "SELECT \"id\", \"total\" FROM \"public\".\"orders\""
        );
        assert_eq!(
            select_sql(
                "shop",
                &TableInfo {
                    schema: None,
                    ..t.clone()
                },
                &columns(&t, SqlDialect::MySql),
                SqlDialect::MySql
            ),
            "SELECT `id`, `total` FROM `shop`.`orders`"
        );
    }

    fn blob_table(ty: &str) -> TableInfo {
        let mut t = orders();
        t.columns[1].type_name = ty.into();
        t.columns[1].name = "body".into();
        t
    }

    /// **A blob is read as text the server makes of it**, never as the
    /// `<n bytes>` the grid shows, and written back as bytes.
    #[test]
    fn a_blob_is_read_as_text_the_write_turns_back_into_bytes() {
        use crate::export::ServerLiteral;
        use crate::import::CellForm;
        let my = rows_read("shop", &blob_table("longblob"), SqlDialect::MySql);
        assert_eq!(
            my.select,
            "SELECT `id`, HEX(`body`) AS `body` FROM `public`.`orders`"
        );
        assert_eq!(my.forms, vec![CellForm::Value, CellForm::Hex]);
        let pg = rows_read("shop", &blob_table("bytea"), PG);
        assert!(
            pg.select.contains("encode(\"body\", 'hex') AS \"body\""),
            "{}",
            pg.select
        );
        assert_eq!(pg.forms[1], CellForm::Hex);
        let ms = rows_read("shop", &blob_table("varbinary(max)"), SqlDialect::MsSql);
        assert!(ms.select.contains("CONVERT(varchar(max)"), "{}", ms.select);
        assert_eq!(ms.forms[1], CellForm::Server(ServerLiteral::Hex));
        // SQLite: every column, since any of them can hold a blob.
        let lite = rows_read("main", &blob_table("TEXT"), SqlDialect::Sqlite);
        assert_eq!(lite.forms, vec![CellForm::Quoted, CellForm::Quoted]);
        assert!(
            lite.select.contains("quote(\"body\") AS \"body\""),
            "{}",
            lite.select
        );
        // A MySQL spatial value is bytes on the wire as well.
        assert_eq!(
            rows_read("shop", &blob_table("point"), SqlDialect::MySql).forms[1],
            CellForm::Hex
        );
        // And text stays text.
        assert_eq!(
            rows_read("shop", &blob_table("text"), PG).forms[1],
            CellForm::Value
        );
    }

    /// **A MySQL-family `FLOAT` is read as the double it is**: the text
    /// protocol spells a `FLOAT` with six significant digits, so `16777215`
    /// read `16777200` and `1.2345678` `1.23457`, and the copy wrote those —
    /// other floats — under a success report. Widened to a double the value
    /// arrives exact (`16777215`, `1.2345677614212036`) and the `FLOAT` column
    /// takes it back as the same float (measured on MariaDB 10.11 and
    /// MySQL 8.4: a join on the column matches every row). A `DOUBLE` reads
    /// with its own 17 digits and is left as it is.
    #[test]
    fn a_mysql_float_is_read_as_a_double() {
        for ty in ["float", "float(7,4)", "float unsigned"] {
            let my = rows_read("shop", &blob_table(ty), SqlDialect::MySql);
            assert!(
                my.select.contains("(`body` + 0e0) AS `body`"),
                "{ty}: {}",
                my.select
            );
            assert_eq!(my.forms[1], crate::import::CellForm::Value);
        }
        let double = rows_read("shop", &blob_table("double"), SqlDialect::MySql);
        assert!(!double.select.contains("0e0"), "{}", double.select);
        let pg = rows_read("shop", &blob_table("real"), PG);
        assert!(!pg.select.contains("0e0"), "{}", pg.select);
    }

    /// **A SQL Server alias type is read by its base**, as the dump reads it:
    /// with no aliases passed, a column typed `[dbo].[Hash]` over `varbinary`
    /// matched no head, arrived as the `<n bytes>` placeholder and stopped
    /// every copy of its table, and one over `sql_variant` came back an
    /// `nvarchar` variant.
    #[test]
    fn a_sql_server_alias_type_is_read_by_its_base() {
        use crate::export::ServerLiteral;
        use crate::import::CellForm;
        use crate::schema::{TsqlObject, TsqlObjectKind};
        let alias = |name: &str, base: &str| TsqlObject {
            schema: Some("dbo".into()),
            name: name.into(),
            kind: TsqlObjectKind::AliasType {
                base: base.into(),
                nullable: true,
            },
        };
        let aliases = [alias("Hash", "varbinary(32)"), alias("Any", "sql_variant")];
        let hash = rows_read_with(
            "shop",
            &blob_table("[dbo].[Hash]"),
            &aliases,
            SqlDialect::MsSql,
        );
        assert_eq!(
            hash.forms[1],
            CellForm::Server(ServerLiteral::Hex),
            "{}",
            hash.select
        );
        let any = rows_read_with(
            "shop",
            &blob_table("[dbo].[Any]"),
            &aliases,
            SqlDialect::MsSql,
        );
        assert_eq!(
            any.forms[1],
            CellForm::Server(ServerLiteral::Variant),
            "{}",
            any.select
        );
        // Without them, as before: the alias is no type it knows.
        let blind = rows_read("shop", &blob_table("[dbo].[Hash]"), SqlDialect::MsSql);
        assert_eq!(blind.forms[1], CellForm::Value);
    }

    fn chunk(ty: &str, cells: Vec<crate::model::Value>) -> crate::model::ResultSet {
        let col = crate::model::Column {
            name: "body".into(),
            type_name: ty.into(),
            origin: None,
        };
        crate::model::ResultSet::from_rows(vec![col], cells.into_iter().map(|v| vec![v]).collect())
    }

    /// **The safety net**: a placeholder that reached the read anyway stops
    /// the copy before it is written — and a row of prose that merely looks
    /// like one in a text column does not.
    #[test]
    fn a_chunk_holding_a_bytes_placeholder_is_refused() {
        use crate::model::Value;
        let blob = chunk("varbinary", vec![Value::Str("<3 bytes>".into())]);
        assert!(chunk_refusal(&blob).is_some_and(|w| w.contains("body")));
        let text = chunk("text", vec![Value::Str("<3 bytes>".into())]);
        assert_eq!(chunk_refusal(&text), None);
        let hex = chunk("varbinary", vec![Value::Str("00ff".into()), Value::Null]);
        assert_eq!(chunk_refusal(&hex), None);
        let mut capped = chunk("text", vec![Value::Str("x".into())]);
        capped.capped_columns = vec![0];
        assert!(chunk_refusal(&capped).is_some());
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
    fn the_rows_step_names_its_source_and_says_it_is_not_in_the_sql() {
        let t = TableInfo {
            schema: Some("sales".into()),
            ..orders()
        };
        assert_eq!(
            rows_change(&t),
            "Then copy every row of sales.orders into it (not shown as SQL)"
        );
    }

    #[test]
    fn the_rows_step_names_a_schemaless_source_bare() {
        let t = TableInfo {
            schema: None,
            ..orders()
        };
        assert_eq!(
            rows_change(&t),
            "Then copy every row of orders into it (not shown as SQL)"
        );
    }

    /// **A finished copy releases only its own cancel handle.** Copy A, closed
    /// mid-copy, can still be rolling back when copy B starts; A's report
    /// landing then cleared the one shared slot, and B's Stop and ✕ cancelled
    /// nothing.
    #[test]
    fn a_finished_copy_does_not_clear_a_newer_copys_token() {
        let mut slot = None;
        let a = launch(&mut slot, "A");
        let b = launch(&mut slot, "B");
        release(&mut slot, a);
        assert_eq!(
            slot.as_ref().map(|(_, t)| *t),
            Some("B"),
            "B can still be stopped"
        );
        release(&mut slot, b);
        assert_eq!(slot, None);
        assert_ne!(a, b);
    }

    #[test]
    fn a_copy_must_be_named_placed_and_not_onto_itself() {
        let t = orders();
        assert_eq!(refusal(&t, &to("orders_copy"), true), None);
        assert_eq!(refusal(&t, &to("orders"), false), None, "another database");
        assert_eq!(
            refusal(&t, &to("ORDERS"), true),
            Some(CopyRefusal::OntoItself)
        );
        assert_eq!(refusal(&t, &to("  "), true), Some(CopyRefusal::Unnamed));
        let mut nowhere = to("x");
        nowhere.database = String::new();
        assert_eq!(refusal(&t, &nowhere, false), Some(CopyRefusal::NoDatabase));
        let mut elsewhere = to("orders");
        elsewhere.schema = Some("archive".into());
        assert_eq!(refusal(&t, &elsewhere, true), None, "another schema");
    }
}
