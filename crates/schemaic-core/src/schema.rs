//! Schema model: a database's tables, and each table's columns and indexes
//! (ARCHITECTURE §11). No IO here — the DB crate fills these in via
//! `information_schema`; the UI renders them as the collapsible schema tree and
//! (later) uses them as the autocomplete substrate.

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
                    is_view: false,
                    view_definition: None,
                },
                TableInfo {
                    name: "b".to_string(),
                    columns: Vec::new(),
                    indexes: Vec::new(),
                    is_view: true,
                    view_definition: None,
                },
            ],
        };
        assert_eq!(s.table_count(), 2);
    }
}
