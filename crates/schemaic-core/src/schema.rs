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

/// PostgreSQL's default namespace. It is always on the stock `search_path`, so a
/// table in it resolves unqualified — which is why [`sql_qualifier`] leaves it
/// off and single-schema statements stay exactly what they were.
pub const PG_DEFAULT_SCHEMA: &str = "public";

/// The namespace to qualify a table with in **user-facing** generated SQL, or
/// `None` when the bare name is right. `schema` is a table's introspected
/// namespace ([`TableInfo::schema`]): `None` on MySQL, which has no level between
/// database and table, and `Some` on PostgreSQL.
///
/// `public` is deliberately dropped: it's on the default `search_path`, so the
/// statement the user sees stays clean and identical to the single-schema case.
/// (The *write* path doesn't use this — `commit_writes`/`refetch_rows` qualify
/// unconditionally, since that SQL is invisible and must not depend on
/// `search_path` at all.)
pub fn sql_qualifier(schema: Option<&str>) -> Option<&str> {
    match schema {
        Some(s) if !s.eq_ignore_ascii_case(PG_DEFAULT_SCHEMA) => Some(s),
        _ => None,
    }
}

/// A table's display name within its database: `table` on MySQL and in
/// PostgreSQL's `public`, `schema.table` elsewhere. This is what the schema tree,
/// tab titles and the "source" label show — never a quoted SQL fragment.
pub fn display_name(schema: Option<&str>, table: &str) -> String {
    match sql_qualifier(schema) {
        Some(s) => format!("{s}.{table}"),
        None => table.to_string(),
    }
}

/// The table a query tab (and therefore its grid) was opened from — the identity
/// that makes a result editable, shows key icons, and lets "open this table" reuse
/// an existing tab. A tab running an arbitrary `SELECT` has none.
///
/// Three parts, because a PostgreSQL database has a namespace level: `sales.orders`
/// and `archive.orders` are different tables and must never compare equal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableSource {
    pub database: String,
    /// PostgreSQL namespace (`None` on MySQL, and for a table in `public` this is
    /// still `Some("public")` — the introspected truth, not the display rule).
    pub schema: Option<String>,
    pub table: String,
}

impl TableSource {
    pub fn new(
        database: impl Into<String>,
        schema: Option<String>,
        table: impl Into<String>,
    ) -> Self {
        Self {
            database: database.into(),
            schema,
            table: table.into(),
        }
    }

    /// How the table is named in the UI: `table`, or `schema.table` outside
    /// PostgreSQL's `public`. See [`display_name`].
    pub fn display(&self) -> String {
        display_name(self.schema.as_deref(), &self.table)
    }
}

/// A table with its columns and indexes.
#[derive(Clone, Debug, Default)]
pub struct TableInfo {
    pub name: String,
    /// The namespace the table lives in — a PostgreSQL schema (`public`,
    /// `sales`, …). `None` on MySQL, where a database *is* the namespace. Part of
    /// the table's identity: two schemas may hold same-named tables, so anything
    /// resolving a table (edit model, catalog, FK follow) must match on this too.
    pub schema: Option<String>,
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
    /// column defaults), but a valid, useful skeleton in the connection's dialect:
    /// MySQL backtick-quotes and inlines `KEY`/`UNIQUE KEY`; PostgreSQL
    /// double-quotes and emits non-PK indexes as separate `CREATE INDEX`
    /// statements (its `CREATE TABLE` can't inline them). A table outside
    /// PostgreSQL's `public` is emitted schema-qualified, so the DDL recreates it
    /// in the namespace it came from rather than wherever `search_path` points.
    pub fn create_ddl(&self, dialect: crate::intel::SqlDialect) -> String {
        let pg = dialect == crate::intel::SqlDialect::Postgres;
        let q = |s: &str| -> String {
            if pg {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                ddl_ident(s)
            }
        };
        // The table's own name, schema-qualified when it isn't in `public`.
        let qname = match sql_qualifier(self.schema.as_deref()) {
            Some(s) => format!("{}.{}", q(s), q(&self.name)),
            None => q(&self.name),
        };
        if self.is_view {
            return match &self.view_definition {
                Some(def) => {
                    format!("CREATE OR REPLACE VIEW {qname} AS\n{def};")
                }
                // View flagged but its definition wasn't readable (e.g. privileges).
                None => format!(
                    "-- View definition for {qname} was not available.\nCREATE OR REPLACE VIEW {qname} AS\nSELECT ...;"
                ),
            };
        }
        let mut lines: Vec<String> = Vec::new();
        for c in &self.columns {
            let null = if c.nullable { "" } else { " NOT NULL" };
            lines.push(format!("  {} {}{}", q(&c.name), c.type_name, null));
        }
        let pk: Vec<String> = self
            .columns
            .iter()
            .filter(|c| c.primary_key)
            .map(|c| q(&c.name))
            .collect();
        if !pk.is_empty() {
            lines.push(format!("  PRIMARY KEY ({})", pk.join(", ")));
        }
        let non_pk = self.indexes.iter().filter(|ix| !ix.is_primary());
        if pg {
            // Postgres: indexes are separate statements after the table.
            let mut out = format!("CREATE TABLE {qname} (\n{}\n);", lines.join(",\n"));
            for ix in non_pk {
                let uniq = if ix.unique { "UNIQUE " } else { "" };
                let cols = ix
                    .columns
                    .iter()
                    .map(|c| q(c))
                    .collect::<Vec<_>>()
                    .join(", ");
                // The index name is never qualified — Postgres puts an index in
                // its table's schema automatically, and `CREATE INDEX "s"."i"` is
                // a syntax error.
                out.push_str(&format!(
                    "\nCREATE {uniq}INDEX {} ON {qname} ({cols});",
                    q(&ix.name),
                ));
            }
            out
        } else {
            // MySQL: inline KEY / UNIQUE KEY.
            for ix in non_pk {
                let kw = if ix.unique { "UNIQUE KEY" } else { "KEY" };
                let cols = ix
                    .columns
                    .iter()
                    .map(|c| q(c))
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(format!("  {kw} {} ({cols})", q(&ix.name)));
            }
            format!("CREATE TABLE {qname} (\n{}\n);", lines.join(",\n"))
        }
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
    /// PostgreSQL namespace the referenced table lives in (`None` on MySQL, where
    /// `database` already is the namespace). Carried so the opened tab's source —
    /// and therefore its edit model — points at the right table when two schemas
    /// hold same-named ones.
    pub schema: Option<String>,
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
/// Dialect-aware: on **MySQL** the query qualifies `` `db`.`table` `` (using the
/// FK's `ref_schema` for a cross-database reference) and backtick-escapes idents.
/// On **PostgreSQL** the referenced table lives in the *same* database (a FK can't
/// cross databases; `ref_schema` there is a namespace like `public`), so the
/// target database is `default_schema` and the table is double-quoted — bare when
/// the reference lands in `public` (resolved via `search_path`, as before) and
/// `"schema"."table"` when it crosses into another namespace. Values are rendered
/// as safe SQL literals so the query runs verbatim.
pub fn follow_target(
    fk: &ForeignKeyInfo,
    values: &[Value],
    default_schema: &str,
    dialect: crate::intel::SqlDialect,
) -> Option<FollowTarget> {
    if fk.ref_columns.is_empty() || values.len() != fk.ref_columns.len() {
        return None;
    }
    let postgres = dialect == crate::intel::SqlDialect::Postgres;
    // Postgres: the reference is same-database; open the current DB, not the schema.
    let database = if postgres {
        default_schema.to_string()
    } else {
        fk.ref_schema
            .clone()
            .unwrap_or_else(|| default_schema.to_string())
    };
    // On Postgres the FK's `ref_schema` *is* the namespace, so it becomes the
    // target's schema. On MySQL it was already consumed as the database above.
    let schema = postgres.then(|| fk.ref_schema.clone()).flatten();
    let table = fk.ref_table.clone();
    let quote = |s: &str| {
        if postgres {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            crate::export::ident_sql(s)
        }
    };
    let literal = |v: &Value| -> String {
        if postgres {
            match v {
                Value::Null => "NULL".to_string(),
                Value::Int(i) => i.to_string(),
                Value::UInt(u) => u.to_string(),
                Value::Float(f) => f.to_string(),
                Value::Str(s) => format!("'{}'", s.replace('\'', "''")),
            }
        } else {
            crate::export::sql_literal(v)
        }
    };
    let where_sql = fk
        .ref_columns
        .iter()
        .zip(values)
        .map(|(col, v)| {
            let ident = quote(col);
            if v.is_null() {
                format!("{ident} IS NULL")
            } else {
                format!("{ident} = {}", literal(v))
            }
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let sql = if postgres {
        // Connected to `database` directly → the name only needs the namespace,
        // and only when that isn't the search-path default.
        let name = match sql_qualifier(schema.as_deref()) {
            Some(s) => format!("{}.{}", quote(s), quote(&table)),
            None => quote(&table),
        };
        format!("SELECT * FROM {name} WHERE {where_sql}")
    } else {
        format!(
            "SELECT * FROM {}.{} WHERE {where_sql}",
            crate::export::ident_sql(&database),
            crate::export::ident_sql(&table),
        )
    };
    Some(FollowTarget {
        database,
        schema,
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

    /// The introspected table with this `(namespace, name)` identity.
    ///
    /// An exact namespace match wins. When the caller has no namespace to offer —
    /// MySQL, or a tab restored from a session file written before multi-schema
    /// browsing — it falls back to the name alone, preferring `public` so the
    /// common case resolves the way it always did rather than to whichever
    /// same-named table happens to come first.
    pub fn find_table(&self, schema: Option<&str>, name: &str) -> Option<&TableInfo> {
        if schema.is_some() {
            return self
                .tables
                .iter()
                .find(|t| t.name == name && t.schema.as_deref() == schema);
        }
        let by_name = || self.tables.iter().filter(|t| t.name == name);
        by_name()
            .find(|t| t.schema.as_deref() == Some(PG_DEFAULT_SCHEMA))
            .or_else(|| by_name().next())
    }

    /// Every namespace present, in display order (`public` first, then
    /// alphabetical). Empty on MySQL, where tables carry no namespace — which is
    /// how the schema tree decides whether to render a schema level at all.
    pub fn schemas(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .tables
            .iter()
            .filter_map(|t| t.schema.clone())
            .collect();
        out.sort_by(|a, b| {
            let key = |s: &str| (s != PG_DEFAULT_SCHEMA, s.to_string());
            key(a).cmp(&key(b))
        });
        out.dedup();
        out
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
            schema: None,
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
            schema: None,
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
        let ddl = t.create_ddl(crate::intel::SqlDialect::MySql);
        assert!(ddl.starts_with("CREATE TABLE `users` ("));
        assert!(ddl.contains("`id` int NOT NULL"));
        assert!(ddl.contains("`email` varchar(255)\n") || ddl.contains("`email` varchar(255),"));
        assert!(ddl.contains("PRIMARY KEY (`id`)"));
        assert!(ddl.contains("UNIQUE KEY `email_uq` (`email`)"));
        // The PRIMARY index is emitted via PRIMARY KEY(...), not repeated as KEY.
        assert!(!ddl.contains("KEY `PRIMARY`"));
    }

    #[test]
    fn create_ddl_postgres_double_quotes_and_separate_indexes() {
        let t = TableInfo {
            schema: None,
            name: "users".to_string(),
            columns: vec![
                col("id", "integer", false, true),
                col("email", "text", false, false),
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
        let ddl = t.create_ddl(crate::intel::SqlDialect::Postgres);
        assert!(ddl.starts_with("CREATE TABLE \"users\" ("), "{ddl}");
        assert!(ddl.contains("\"id\" integer NOT NULL"));
        assert!(ddl.contains("PRIMARY KEY (\"id\")"));
        // Non-PK index is a separate CREATE INDEX (not an inline KEY), double-quoted.
        assert!(ddl.contains("CREATE UNIQUE INDEX \"email_uq\" ON \"users\" (\"email\");"));
        // No MySQL-isms.
        assert!(!ddl.contains('`'));
        assert!(!ddl.contains("KEY `"));
    }

    #[test]
    fn create_ddl_view_uses_definition() {
        let t = TableInfo {
            schema: None,
            name: "v".to_string(),
            columns: Vec::new(),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            is_view: true,
            view_definition: Some("SELECT 1".to_string()),
        };
        assert_eq!(
            t.create_ddl(crate::intel::SqlDialect::MySql),
            "CREATE OR REPLACE VIEW `v` AS\nSELECT 1;"
        );
    }

    #[test]
    fn create_ddl_escapes_backticks() {
        let t = TableInfo {
            schema: None,
            name: "we`ird".to_string(),
            columns: vec![col("a`b", "int", true, false)],
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            is_view: false,
            view_definition: None,
        };
        let ddl = t.create_ddl(crate::intel::SqlDialect::MySql);
        assert!(ddl.contains("CREATE TABLE `we``ird`"));
        assert!(ddl.contains("`a``b` int"));
    }

    #[test]
    fn create_ddl_view_without_definition_emits_placeholder() {
        let t = TableInfo {
            schema: None,
            name: "v".to_string(),
            columns: Vec::new(),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            is_view: true,
            view_definition: None,
        };
        let ddl = t.create_ddl(crate::intel::SqlDialect::MySql);
        assert!(ddl.contains("-- View definition for `v` was not available."));
        assert!(ddl.contains("CREATE OR REPLACE VIEW `v` AS\nSELECT ...;"));
    }

    // ── multi-schema (PostgreSQL namespaces) ──────────────────────────────

    #[test]
    fn sql_qualifier_drops_the_search_path_default() {
        // MySQL has no namespace level at all.
        assert_eq!(sql_qualifier(None), None);
        // `public` is on the stock search_path → statements stay bare, exactly as
        // they were before multi-schema browsing existed.
        assert_eq!(sql_qualifier(Some("public")), None);
        assert_eq!(sql_qualifier(Some("PUBLIC")), None); // fold case
        // Anything else must be qualified or it resolves somewhere else.
        assert_eq!(sql_qualifier(Some("sales")), Some("sales"));
        // A schema literally named "" is not `public`, so it still qualifies
        // (pathological, but never silently treated as the default).
        assert_eq!(sql_qualifier(Some("")), Some(""));
    }

    #[test]
    fn display_name_qualifies_only_outside_public() {
        assert_eq!(display_name(None, "orders"), "orders");
        assert_eq!(display_name(Some("public"), "orders"), "orders");
        assert_eq!(display_name(Some("sales"), "orders"), "sales.orders");
    }

    #[test]
    fn create_ddl_postgres_qualifies_a_non_public_schema() {
        let t = TableInfo {
            name: "orders".to_string(),
            schema: Some("sales".to_string()),
            columns: vec![col("id", "integer", false, true)],
            indexes: vec![IndexInfo {
                name: "orders_ts".to_string(),
                columns: vec!["id".to_string()],
                unique: false,
                foreign: false,
            }],
            ..Default::default()
        };
        let ddl = t.create_ddl(crate::intel::SqlDialect::Postgres);
        assert!(
            ddl.starts_with("CREATE TABLE \"sales\".\"orders\" ("),
            "{ddl}"
        );
        // The index is created ON the qualified table, but its own name is NOT
        // qualified — Postgres rejects `CREATE INDEX "s"."i"`.
        assert!(
            ddl.contains("CREATE INDEX \"orders_ts\" ON \"sales\".\"orders\" (\"id\");"),
            "{ddl}"
        );
    }

    #[test]
    fn create_ddl_postgres_public_stays_unqualified() {
        let t = TableInfo {
            name: "orders".to_string(),
            schema: Some("public".to_string()),
            columns: vec![col("id", "integer", false, true)],
            ..Default::default()
        };
        let ddl = t.create_ddl(crate::intel::SqlDialect::Postgres);
        assert!(ddl.starts_with("CREATE TABLE \"orders\" ("), "{ddl}");
    }

    #[test]
    fn create_ddl_qualified_view_uses_the_schema() {
        let t = TableInfo {
            name: "daily".to_string(),
            schema: Some("analytics".to_string()),
            is_view: true,
            view_definition: Some("SELECT 1".to_string()),
            ..Default::default()
        };
        assert_eq!(
            t.create_ddl(crate::intel::SqlDialect::Postgres),
            "CREATE OR REPLACE VIEW \"analytics\".\"daily\" AS\nSELECT 1;"
        );
    }

    #[test]
    fn create_ddl_mysql_never_grows_a_qualifier() {
        // MySQL introspection always leaves `schema` unset (the database already
        // is the namespace), so its DDL must stay exactly what it was.
        let t = TableInfo {
            name: "users".to_string(),
            schema: None,
            columns: vec![col("id", "int", false, true)],
            ..Default::default()
        };
        let ddl = t.create_ddl(crate::intel::SqlDialect::MySql);
        assert!(ddl.starts_with("CREATE TABLE `users` ("), "{ddl}");
    }

    #[test]
    fn table_source_display_matches_display_name() {
        let s = TableSource::new("warehouse", Some("sales".into()), "orders");
        assert_eq!(s.display(), "sales.orders");
        let public = TableSource::new("warehouse", Some("public".into()), "orders");
        assert_eq!(public.display(), "orders");
        // Two namespaces are never the same table.
        assert_ne!(s, public);
    }

    #[test]
    fn find_table_prefers_an_exact_namespace_match() {
        let s = DbSchema {
            tables: vec![
                TableInfo {
                    name: "orders".into(),
                    schema: Some("sales".into()),
                    columns: vec![col("total", "int", true, false)],
                    ..Default::default()
                },
                TableInfo {
                    name: "orders".into(),
                    schema: Some("public".into()),
                    columns: vec![col("id", "int", false, true)],
                    ..Default::default()
                },
            ],
        };
        // Exact match wins, even though `sales` comes first in the list.
        assert_eq!(
            s.find_table(Some("public"), "orders")
                .map(|t| t.columns[0].name.as_str()),
            Some("id")
        );
        assert_eq!(
            s.find_table(Some("sales"), "orders")
                .map(|t| t.columns[0].name.as_str()),
            Some("total")
        );
        // A namespace we don't have is a miss, not a silent fallback.
        assert!(s.find_table(Some("archive"), "orders").is_none());
    }

    #[test]
    fn find_table_without_a_namespace_falls_back_to_public() {
        // The caller has no namespace to offer (MySQL, or a session restored from
        // a file written before multi-schema browsing). `public` is the sane pick.
        let s = DbSchema {
            tables: vec![
                TableInfo {
                    name: "orders".into(),
                    schema: Some("sales".into()),
                    ..Default::default()
                },
                TableInfo {
                    name: "orders".into(),
                    schema: Some("public".into()),
                    columns: vec![col("id", "int", false, true)],
                    ..Default::default()
                },
            ],
        };
        assert_eq!(
            s.find_table(None, "orders").map(|t| t.schema.as_deref()),
            Some(Some("public"))
        );
        // With no `public` candidate it still resolves rather than giving up.
        let only_sales = DbSchema {
            tables: vec![TableInfo {
                name: "orders".into(),
                schema: Some("sales".into()),
                ..Default::default()
            }],
        };
        assert_eq!(
            only_sales
                .find_table(None, "orders")
                .map(|t| t.schema.as_deref()),
            Some(Some("sales"))
        );
        assert!(only_sales.find_table(None, "ghosts").is_none());
    }

    #[test]
    fn schemas_lists_public_first_then_alphabetical_deduped() {
        let t = |ns: &str, name: &str| TableInfo {
            name: name.into(),
            schema: Some(ns.into()),
            ..Default::default()
        };
        let s = DbSchema {
            tables: vec![
                t("sales", "orders"),
                t("analytics", "daily"),
                t("public", "staging"),
                t("sales", "line_items"),
            ],
        };
        assert_eq!(s.schemas(), vec!["public", "analytics", "sales"]);
    }

    #[test]
    fn schemas_is_empty_without_namespaces() {
        // MySQL: no namespace level, so the tree renders tables flat.
        let s = DbSchema {
            tables: vec![TableInfo {
                name: "users".into(),
                ..Default::default()
            }],
        };
        assert!(s.schemas().is_empty());
        assert!(DbSchema::default().schemas().is_empty());
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
                    schema: None,
                    name: "a".to_string(),
                    columns: Vec::new(),
                    indexes: Vec::new(),
                    foreign_keys: Vec::new(),
                    is_view: false,
                    view_definition: None,
                },
                TableInfo {
                    schema: None,
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
            schema: None,
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
        use crate::intel::SqlDialect;
        let f = fk(&["customer_id"], None, "customers", &["id"]);
        let ft = follow_target(&f, &[Value::Int(42)], "shop", SqlDialect::MySql).unwrap();
        assert_eq!(ft.database, "shop"); // ref_schema None → default
        assert_eq!(ft.table, "customers");
        assert_eq!(ft.sql, "SELECT * FROM `shop`.`customers` WHERE `id` = 42");
    }

    #[test]
    fn follow_target_honors_explicit_ref_schema() {
        use crate::intel::SqlDialect;
        let f = fk(&["c"], Some("other_db"), "customers", &["id"]);
        let ft = follow_target(&f, &[Value::UInt(7)], "shop", SqlDialect::MySql).unwrap();
        assert_eq!(ft.database, "other_db");
        assert_eq!(
            ft.sql,
            "SELECT * FROM `other_db`.`customers` WHERE `id` = 7"
        );
    }

    #[test]
    fn follow_target_escapes_idents_and_values_and_handles_null_composite() {
        use crate::intel::SqlDialect;
        // Composite FK, backtick-y identifiers, a string value (escaped) and a NULL.
        let f = fk(&["a", "b"], None, "t`x", &["r`1", "r2"]);
        let ft = follow_target(
            &f,
            &[Value::Str("O'Hara".into()), Value::Null],
            "db",
            SqlDialect::MySql,
        )
        .unwrap();
        assert_eq!(ft.table, "t`x");
        assert_eq!(
            ft.sql,
            "SELECT * FROM `db`.`t``x` WHERE `r``1` = 'O''Hara' AND `r2` IS NULL"
        );
    }

    #[test]
    fn follow_target_postgres_same_db_unqualified_double_quoted() {
        use crate::intel::SqlDialect;
        // Postgres: ref_schema is a namespace ('public'), but the target opens the
        // *current* database (default_schema); table is unqualified + double-quoted,
        // string escaped, NULL → IS NULL.
        let f = fk(&["cc"], Some("public"), "country", &["code"]);
        let ft = follow_target(
            &f,
            &[Value::Str("O'Hara".into())],
            "world",
            SqlDialect::Postgres,
        )
        .unwrap();
        assert_eq!(ft.database, "world"); // current DB, NOT the 'public' schema
        assert_eq!(ft.table, "country");
        assert_eq!(
            ft.sql,
            "SELECT * FROM \"country\" WHERE \"code\" = 'O''Hara'"
        );
    }

    #[test]
    fn follow_target_postgres_qualifies_a_cross_schema_reference() {
        use crate::intel::SqlDialect;
        // A FK from a `public` table into `sales`: the target still opens the
        // current database, but the statement must name the namespace or it
        // resolves through search_path to the wrong (or no) table.
        let f = fk(&["order_id"], Some("sales"), "orders", &["id"]);
        let ft = follow_target(&f, &[Value::Int(7)], "warehouse", SqlDialect::Postgres).unwrap();
        assert_eq!(ft.database, "warehouse");
        assert_eq!(ft.schema.as_deref(), Some("sales"));
        assert_eq!(ft.table, "orders");
        assert_eq!(
            ft.sql,
            "SELECT * FROM \"sales\".\"orders\" WHERE \"id\" = 7"
        );
    }

    #[test]
    fn follow_target_mysql_leaves_schema_unset() {
        use crate::intel::SqlDialect;
        // On MySQL `ref_schema` is the *database* and is consumed as such — it
        // must not also leak into the namespace slot and double-qualify.
        let f = fk(&["c"], Some("other_db"), "customers", &["id"]);
        let ft = follow_target(&f, &[Value::Int(1)], "shop", SqlDialect::MySql).unwrap();
        assert_eq!(ft.database, "other_db");
        assert_eq!(ft.schema, None);
    }

    #[test]
    fn follow_target_rejects_wrong_arity() {
        use crate::intel::SqlDialect;
        // Fewer values than key columns → can't build a safe WHERE.
        let f = fk(&["a", "b"], None, "t", &["x", "y"]);
        assert!(follow_target(&f, &[Value::Int(1)], "db", SqlDialect::MySql).is_none());
        // A FK with no columns.
        let empty = fk(&[], None, "t", &[]);
        assert!(follow_target(&empty, &[], "db", SqlDialect::MySql).is_none());
    }
}
