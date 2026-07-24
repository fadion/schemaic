//! Schema model: a database's tables, and each table's columns and indexes
//! (ARCHITECTURE §11). No IO here — the DB crate fills these in via
//! `information_schema`; the UI renders them as the collapsible schema tree and
//! (later) uses them as the autocomplete substrate.

use crate::model::Value;

/// A single column of a table.
#[derive(Clone, Debug)]
pub struct ColumnInfo {
    pub name: String,
    /// Full SQL type as reported by `information_schema` (e.g. `varchar(45)`,
    /// `int(11) unsigned`).
    pub type_name: String,
    pub nullable: bool,
    /// True if this column is part of the primary key.
    pub primary_key: bool,
}

/// An index on a table (its ordered key columns).
#[derive(Clone, Debug)]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    /// True if this index backs a FOREIGN KEY constraint.
    pub foreign: bool,
}

impl IndexInfo {
    /// Is this the table's PRIMARY KEY?
    pub fn is_primary(&self) -> bool {
        self.name == "PRIMARY"
    }
}

/// A foreign-key constraint: which local columns reference which columns of which
/// table. Populated from `information_schema.KEY_COLUMN_USAGE`; `columns` (the
/// referencing columns, in this table) and `ref_columns` (the referenced columns)
/// are aligned by key position. Drives "Follow" navigation from the data grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignKeyInfo {
    /// Referencing columns in *this* table, in key order.
    pub columns: Vec<String>,
    /// Referenced schema/database. `None` when the server reports none (treated
    /// as the same database as the referencing table).
    pub ref_schema: Option<String>,
    /// Referenced table.
    pub ref_table: String,
    /// Referenced columns, aligned to [`ForeignKeyInfo::columns`].
    pub ref_columns: Vec<String>,
}

/// Backtick-quote a SQL identifier, doubling any embedded backtick.
fn ddl_ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// A table with its columns and indexes.
#[derive(Clone, Debug)]
pub struct TableInfo {
    pub name: String,
    pub columns: Vec<ColumnInfo>,
    pub indexes: Vec<IndexInfo>,
    /// Foreign-key constraints declared on this table (with their targets).
    pub foreign_keys: Vec<ForeignKeyInfo>,
    /// True if this is a VIEW rather than a base table (`TABLE_TYPE = 'VIEW'`).
    pub is_view: bool,
    /// For views, the stored SELECT (`information_schema.VIEWS.VIEW_DEFINITION`),
    /// used to emit `CREATE VIEW`. `None` for base tables (and views whose
    /// definition couldn't be read).
    pub view_definition: Option<String>,
}

impl TableInfo {
    /// A `CREATE TABLE`/`CREATE VIEW` skeleton from the introspected schema. Not
    /// a round-trip of the server's DDL (no FK references, engine, charset, or
    /// column defaults — foreign keys appear as plain `KEY` indexes since we
    /// don't introspect their references), but a valid, useful skeleton.
    /// Identifiers are backtick-escaped.
    pub fn create_ddl(&self) -> String {
        if self.is_view {
            return match &self.view_definition {
                Some(def) => {
                    format!(
                        "CREATE OR REPLACE VIEW {} AS\n{};",
                        ddl_ident(&self.name),
                        def
                    )
                }
                // View flagged but its definition wasn't readable (e.g. privileges).
                None => format!(
                    "-- View definition for {} was not available.\nCREATE OR REPLACE VIEW {} AS\nSELECT ...;",
                    ddl_ident(&self.name),
                    ddl_ident(&self.name)
                ),
            };
        }
        let mut lines: Vec<String> = Vec::new();
        for c in &self.columns {
            let null = if c.nullable { "" } else { " NOT NULL" };
            lines.push(format!("  {} {}{}", ddl_ident(&c.name), c.type_name, null));
        }
        let pk: Vec<String> = self
            .columns
            .iter()
            .filter(|c| c.primary_key)
            .map(|c| ddl_ident(&c.name))
            .collect();
        if !pk.is_empty() {
            lines.push(format!("  PRIMARY KEY ({})", pk.join(", ")));
        }
        for ix in &self.indexes {
            if ix.is_primary() {
                continue;
            }
            let kw = if ix.unique { "UNIQUE KEY" } else { "KEY" };
            let cols = ix
                .columns
                .iter()
                .map(|c| ddl_ident(c))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("  {kw} {} ({cols})", ddl_ident(&ix.name)));
        }
        format!(
            "CREATE TABLE {} (\n{}\n);",
            ddl_ident(&self.name),
            lines.join(",\n")
        )
    }

    /// Does any of this table's column names contain `needle_lower`
    /// (case-insensitive)? `needle_lower` must already be lower-cased by the caller.
    pub fn any_column_matches(&self, needle_lower: &str) -> bool {
        self.columns
            .iter()
            .any(|c| c.name.to_lowercase().contains(needle_lower))
    }

    /// Does this table match a schema-search term — by its own name OR by any of
    /// its column names? `needle_lower` must already be lower-cased. An empty
    /// needle matches nothing (callers treat "no filter" separately).
    pub fn matches_search(&self, needle_lower: &str) -> bool {
        if needle_lower.is_empty() {
            return false;
        }
        self.name.to_lowercase().contains(needle_lower) || self.any_column_matches(needle_lower)
    }

    /// The foreign key whose referencing columns include `column`, if any — the
    /// FK the data grid follows when right-clicking a cell in `column`. Works for
    /// single- and composite-column keys.
    pub fn fk_for_column(&self, column: &str) -> Option<&ForeignKeyInfo> {
        self.foreign_keys
            .iter()
            .find(|fk| fk.columns.iter().any(|c| c == column))
    }
}

/// A resolved "follow this foreign key" navigation target: the referenced table
/// (so the new tab's grid stays editable, sourced from that table) plus a
/// ready-to-run `SELECT` filtered to the referenced row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FollowTarget {
    pub database: String,
    pub table: String,
    pub sql: String,
}

/// Build a [`FollowTarget`] opening the table `fk` references, filtered to the
/// row keyed by `values` — the referencing row's values for `fk.columns`, in that
/// order (one value for a single-column FK; several for a composite). A NULL value
/// filters with `IS NULL`. `default_schema` is used when the FK names no schema
/// (a same-database reference). Returns `None` if `values` doesn't cover every
/// key column (can't build a safe, unambiguous `WHERE`).
///
/// Identifiers are backtick-escaped and values rendered as SQL literals via the
/// shared [`crate::export`] helpers, so the query is safe to run verbatim.
pub fn follow_target(
    fk: &ForeignKeyInfo,
    values: &[Value],
    default_schema: &str,
) -> Option<FollowTarget> {
    if fk.ref_columns.is_empty() || values.len() != fk.ref_columns.len() {
        return None;
    }
    let database = fk
        .ref_schema
        .clone()
        .unwrap_or_else(|| default_schema.to_string());
    let table = fk.ref_table.clone();
    let where_sql = fk
        .ref_columns
        .iter()
        .zip(values)
        .map(|(col, v)| {
            let ident = crate::export::ident_sql(col);
            if v.is_null() {
                format!("{ident} IS NULL")
            } else {
                format!("{ident} = {}", crate::export::sql_literal(v))
            }
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let sql = format!(
        "SELECT * FROM {}.{} WHERE {where_sql}",
        crate::export::ident_sql(&database),
        crate::export::ident_sql(&table),
    );
    Some(FollowTarget {
        database,
        table,
        sql,
    })
}

/// The introspected schema of one database.
#[derive(Clone, Debug, Default)]
pub struct DbSchema {
    pub tables: Vec<TableInfo>,
}

impl DbSchema {
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }
}

/// Broad category of a column's SQL type, for picking a schema-tree icon. The UI
/// maps each variant to a Lucide glyph; keeping the classification here makes it
/// pure and testable (and reusable beyond the tree).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnTypeClass {
    /// `char`/`varchar`/`text`/`enum`/`set` — string types.
    Text,
    /// `int`/`decimal`/`float`/… — numeric types.
    Numeric,
    /// `bool`/`boolean`.
    Boolean,
    /// `date`/`datetime`/`time`/`timestamp`/`year`.
    DateTime,
    /// `json` and the spatial/geometry types.
    Json,
    /// `blob`/`binary`/`varbinary` — raw bytes.
    Binary,
    /// Anything unrecognised.
    Other,
}

/// Classify a column's declared SQL `type_name` (e.g. `varchar(45)`,
/// `int(11) unsigned`, `decimal(10,2)`) by its leading type keyword. Case- and
/// modifier-insensitive; note MySQL `bool`/`boolean` is a `tinyint(1)` alias, so
/// only the literal `bool`/`boolean` spelling maps to [`ColumnTypeClass::Boolean`]
/// (a bare `tinyint` is [`ColumnTypeClass::Numeric`]).
pub fn classify_column_type(type_name: &str) -> ColumnTypeClass {
    // Leading keyword: up to the first `(`, space, or end.
    let base: String = type_name
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect::<String>()
        .to_ascii_lowercase();
    match base.as_str() {
        "bool" | "boolean" => ColumnTypeClass::Boolean,
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint" | "decimal" | "dec"
        | "numeric" | "fixed" | "float" | "double" | "real" | "bit" => ColumnTypeClass::Numeric,
        "char" | "varchar" | "tinytext" | "text" | "mediumtext" | "longtext" | "enum" | "set" => {
            ColumnTypeClass::Text
        }
        "date" | "datetime" | "time" | "timestamp" | "year" => ColumnTypeClass::DateTime,
        "json" | "geometry" | "geomcollection" | "geometrycollection" | "point" | "linestring"
        | "polygon" | "multipoint" | "multilinestring" | "multipolygon" => ColumnTypeClass::Json,
        "blob" | "tinyblob" | "mediumblob" | "longblob" | "binary" | "varbinary" => {
            ColumnTypeClass::Binary
        }
        _ => ColumnTypeClass::Other,
    }
}

/// Per-connection introspection lifecycle, shared loader→UI through a signal.
#[derive(Clone, Debug)]
pub enum SchemaState {
    /// Introspection query is in flight.
    Loading,
    Loaded(DbSchema),
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str, nullable: bool, pk: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.to_string(),
            type_name: ty.to_string(),
            nullable,
            primary_key: pk,
        }
    }

    fn fk(cols: &[&str], schema: Option<&str>, table: &str, ref_cols: &[&str]) -> ForeignKeyInfo {
        ForeignKeyInfo {
            columns: cols.iter().map(|s| s.to_string()).collect(),
            ref_schema: schema.map(|s| s.to_string()),
            ref_table: table.to_string(),
            ref_columns: ref_cols.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn classify_column_type_covers_each_family() {
        use ColumnTypeClass::*;
        assert_eq!(classify_column_type("varchar(45)"), Text);
        assert_eq!(classify_column_type("CHAR(2)"), Text);
        assert_eq!(classify_column_type("longtext"), Text);
        assert_eq!(classify_column_type("enum('a','b')"), Text);
        assert_eq!(classify_column_type("int(11) unsigned"), Numeric);
        assert_eq!(classify_column_type("tinyint"), Numeric);
        assert_eq!(classify_column_type("decimal(10,2)"), Numeric);
        assert_eq!(classify_column_type("DOUBLE"), Numeric);
        // bool/boolean spelling → Boolean; a bare tinyint stays Numeric.
        assert_eq!(classify_column_type("boolean"), Boolean);
        assert_eq!(classify_column_type("bool"), Boolean);
        assert_eq!(classify_column_type("datetime"), DateTime);
        assert_eq!(classify_column_type("timestamp"), DateTime);
        assert_eq!(classify_column_type("date"), DateTime);
        assert_eq!(classify_column_type("json"), Json);
        assert_eq!(classify_column_type("geometry"), Json);
        assert_eq!(classify_column_type("longblob"), Binary);
        assert_eq!(classify_column_type("varbinary(16)"), Binary);
        assert_eq!(classify_column_type("weird_custom_type"), Other);
        assert_eq!(classify_column_type(""), Other);
    }

    #[test]
    fn matches_search_by_name_or_column() {
        let t = TableInfo {
            name: "orders".to_string(),
            columns: vec![
                col("id", "int", false, true),
                col("customer_email", "varchar(255)", true, false),
            ],
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            is_view: false,
            view_definition: None,
        };
        // By table name (case-insensitive substring).
        assert!(t.matches_search("ord"));
        assert!(t.matches_search("orders"));
        // By a column name, even when the table name doesn't match.
        assert!(t.matches_search("email"));
        assert!(t.any_column_matches("customer"));
        // No match anywhere.
        assert!(!t.matches_search("zzz"));
        assert!(!t.any_column_matches("zzz"));
        // Empty needle matches nothing (callers handle "no filter" separately).
        assert!(!t.matches_search(""));
    }

    #[test]
    fn create_ddl_base_table_with_pk_and_index() {
        let t = TableInfo {
            name: "users".to_string(),
            columns: vec![
                col("id", "int", false, true),
                col("email", "varchar(255)", true, false),
            ],
            indexes: vec![
                IndexInfo {
                    name: "PRIMARY".to_string(),
                    columns: vec!["id".to_string()],
                    unique: true,
                    foreign: false,
                },
                IndexInfo {
                    name: "email_uq".to_string(),
                    columns: vec!["email".to_string()],
                    unique: true,
                    foreign: false,
                },
            ],
            foreign_keys: Vec::new(),
            is_view: false,
            view_definition: None,
        };
        let ddl = t.create_ddl();
        assert!(ddl.starts_with("CREATE TABLE `users` ("));
        assert!(ddl.contains("`id` int NOT NULL"));
        assert!(ddl.contains("`email` varchar(255)\n") || ddl.contains("`email` varchar(255),"));
        assert!(ddl.contains("PRIMARY KEY (`id`)"));
        assert!(ddl.contains("UNIQUE KEY `email_uq` (`email`)"));
        // The PRIMARY index is emitted via PRIMARY KEY(...), not repeated as KEY.
        assert!(!ddl.contains("KEY `PRIMARY`"));
    }

    #[test]
    fn create_ddl_view_uses_definition() {
        let t = TableInfo {
            name: "v".to_string(),
            columns: Vec::new(),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            is_view: true,
            view_definition: Some("SELECT 1".to_string()),
        };
        assert_eq!(t.create_ddl(), "CREATE OR REPLACE VIEW `v` AS\nSELECT 1;");
    }

    #[test]
    fn create_ddl_escapes_backticks() {
        let t = TableInfo {
            name: "we`ird".to_string(),
            columns: vec![col("a`b", "int", true, false)],
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            is_view: false,
            view_definition: None,
        };
        let ddl = t.create_ddl();
        assert!(ddl.contains("CREATE TABLE `we``ird`"));
        assert!(ddl.contains("`a``b` int"));
    }

    #[test]
    fn create_ddl_view_without_definition_emits_placeholder() {
        let t = TableInfo {
            name: "v".to_string(),
            columns: Vec::new(),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            is_view: true,
            view_definition: None,
        };
        let ddl = t.create_ddl();
        assert!(ddl.contains("-- View definition for `v` was not available."));
        assert!(ddl.contains("CREATE OR REPLACE VIEW `v` AS\nSELECT ...;"));
    }

    #[test]
    fn is_primary_only_for_the_primary_index() {
        let ix = |name: &str| IndexInfo {
            name: name.to_string(),
            columns: vec!["id".to_string()],
            unique: true,
            foreign: false,
        };
        assert!(ix("PRIMARY").is_primary());
        assert!(!ix("primary").is_primary()); // case-sensitive: only literal PRIMARY
        assert!(!ix("email_uq").is_primary());
    }

    #[test]
    fn db_schema_table_count() {
        assert_eq!(DbSchema::default().table_count(), 0);
        let s = DbSchema {
            tables: vec![
                TableInfo {
                    name: "a".to_string(),
                    columns: Vec::new(),
                    indexes: Vec::new(),
                    foreign_keys: Vec::new(),
                    is_view: false,
                    view_definition: None,
                },
                TableInfo {
                    name: "b".to_string(),
                    columns: Vec::new(),
                    indexes: Vec::new(),
                    foreign_keys: Vec::new(),
                    is_view: true,
                    view_definition: None,
                },
            ],
        };
        assert_eq!(s.table_count(), 2);
    }

    fn table_with_fks(fks: Vec<ForeignKeyInfo>) -> TableInfo {
        TableInfo {
            name: "orders".to_string(),
            columns: Vec::new(),
            indexes: Vec::new(),
            foreign_keys: fks,
            is_view: false,
            view_definition: None,
        }
    }

    #[test]
    fn fk_for_column_matches_any_referencing_column() {
        let t = table_with_fks(vec![
            fk(&["customer_id"], None, "customers", &["id"]),
            fk(&["a", "b"], None, "other", &["x", "y"]),
        ]);
        assert_eq!(
            t.fk_for_column("customer_id").unwrap().ref_table,
            "customers"
        );
        // A composite FK matches on any of its member columns.
        assert_eq!(t.fk_for_column("b").unwrap().ref_table, "other");
        // A column that's in no FK.
        assert!(t.fk_for_column("note").is_none());
        // No FKs at all.
        assert!(table_with_fks(Vec::new()).fk_for_column("x").is_none());
    }

    #[test]
    fn follow_target_single_column_uses_default_schema() {
        let f = fk(&["customer_id"], None, "customers", &["id"]);
        let ft = follow_target(&f, &[Value::Int(42)], "shop").unwrap();
        assert_eq!(ft.database, "shop"); // ref_schema None → default
        assert_eq!(ft.table, "customers");
        assert_eq!(ft.sql, "SELECT * FROM `shop`.`customers` WHERE `id` = 42");
    }

    #[test]
    fn follow_target_honors_explicit_ref_schema() {
        let f = fk(&["c"], Some("other_db"), "customers", &["id"]);
        let ft = follow_target(&f, &[Value::UInt(7)], "shop").unwrap();
        assert_eq!(ft.database, "other_db");
        assert_eq!(
            ft.sql,
            "SELECT * FROM `other_db`.`customers` WHERE `id` = 7"
        );
    }

    #[test]
    fn follow_target_escapes_idents_and_values_and_handles_null_composite() {
        // Composite FK, backtick-y identifiers, a string value (escaped) and a NULL.
        let f = fk(&["a", "b"], None, "t`x", &["r`1", "r2"]);
        let ft = follow_target(&f, &[Value::Str("O'Hara".into()), Value::Null], "db").unwrap();
        assert_eq!(ft.table, "t`x");
        assert_eq!(
            ft.sql,
            "SELECT * FROM `db`.`t``x` WHERE `r``1` = 'O''Hara' AND `r2` IS NULL"
        );
    }

    #[test]
    fn follow_target_rejects_wrong_arity() {
        // Fewer values than key columns → can't build a safe WHERE.
        let f = fk(&["a", "b"], None, "t", &["x", "y"]);
        assert!(follow_target(&f, &[Value::Int(1)], "db").is_none());
        // A FK with no columns.
        let empty = fk(&[], None, "t", &[]);
        assert!(follow_target(&empty, &[], "db").is_none());
    }
}
