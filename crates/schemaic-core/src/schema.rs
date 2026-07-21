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

/// Per-connection introspection lifecycle, shared loader→UI through a signal.
#[derive(Clone, Debug)]
pub enum SchemaState {
    /// Introspection query is in flight.
    Loading,
    Loaded(DbSchema),
    Failed(String),
}
