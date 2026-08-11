//! Schema model: a database's tables, and each table's columns and indexes
//! (ARCHITECTURE §11). No IO here — the DB crate fills these in via
//! `information_schema`; the UI renders them as the collapsible schema tree and
//! (later) uses them as the autocomplete substrate.

use crate::intel::SqlDialect;
use crate::model::Value;

/// A single column of a table.
///
/// Everything past `primary_key` exists because **MySQL's `MODIFY COLUMN`
/// replaces a column's entire definition** — anything not restated is silently
/// dropped. Widening a `varchar` without knowing the column's default, comment,
/// collation and auto-increment would destroy all four, so a schema editor can't
/// be built on a model that doesn't carry them. They are equally what makes
/// [`TableInfo::create_ddl`] emit SQL that actually recreates the table.
///
/// `Default` so the many places that only care about a column's name and type
/// (tests, the MCP surface, the AI context) can spell out those and take the
/// rest — the alternative is every one of them listing eight fields it has no
/// opinion about.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnInfo {
    pub name: String,
    /// Full SQL type as reported by the server (e.g. `varchar(45)`,
    /// `int(11) unsigned`, `numeric(10,2)`) — **with** its parameters, which is
    /// what makes it re-emittable.
    pub type_name: String,
    pub nullable: bool,
    /// True if this column is part of the primary key.
    pub primary_key: bool,
    /// The declared `DEFAULT`, as SQL text ready to emit: a quoted literal
    /// (`'draft'`), a number, or an expression (`CURRENT_TIMESTAMP`, `now()`).
    ///
    /// Normalized at the introspection boundary rather than here, because the
    /// servers disagree about what they hand back — MariaDB and PostgreSQL
    /// already return SQL text, MySQL returns a *raw value* that has to be
    /// quoted by type. Downstream can treat this as "paste after `DEFAULT `".
    pub default: Option<String>,
    /// Server-assigned on insert: MySQL `AUTO_INCREMENT`, PostgreSQL an identity
    /// column or a `serial`'s owned sequence.
    pub auto_increment: bool,
    /// PostgreSQL `GENERATED ALWAYS AS IDENTITY` (`attidentity = 'a'`), as
    /// opposed to `BY DEFAULT` (`'d'`), MySQL `AUTO_INCREMENT` or a `serial`.
    ///
    /// The distinction only matters where something writes the column: `ALWAYS`
    /// **rejects** an explicit value, the others accept one. Collapsing the two
    /// into [`ColumnInfo::auto_increment`] made import offer to write a column
    /// the server would refuse. Always `false` on MySQL, which has no such form.
    pub identity_always: bool,
    /// A generated/computed column's expression, without the `AS (…)` wrapper.
    pub generated: Option<String>,
    /// MySQL's `ON UPDATE CURRENT_TIMESTAMP` (the expression, not the keyword).
    pub on_update: Option<String>,
    pub comment: Option<String>,
    /// Explicit collation, when the server reports one for this column.
    pub collation: Option<String>,
}

/// One key column of an index, with the parts of it that aren't just a name.
///
/// Modelled rather than flattened to a string because both are silently lost
/// otherwise: recreating a MySQL prefix index `KEY (bio(20))` as `KEY (bio)`
/// fails outright on a `TEXT` column, and dropping a `DESC` turns an index that
/// serves an `ORDER BY` into one that doesn't.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexColumn {
    /// The column's name — or, when [`IndexColumn::expression`] is set, the
    /// expression's SQL text (`lower(email)`), which is not a name at all.
    pub name: String,
    /// MySQL prefix length — `KEY (bio(20))`. Always `None` on PostgreSQL.
    pub prefix: Option<u32>,
    pub descending: bool,
    /// This key is an **expression**, not a column (PostgreSQL: `CREATE INDEX …
    /// ON t (lower(email))`).
    ///
    /// It changes three things, and each is a way the two are not
    /// interchangeable: it is emitted parenthesised and **unquoted** (quoting it
    /// would make the whole expression an identifier); it is not a row key, so
    /// [`IndexInfo::column_names`] skips it; and no table column has to exist by
    /// that name, so the designer's validation must not look for one.
    pub expression: bool,
}

impl IndexColumn {
    /// The ordinary case: a whole column, ascending.
    pub fn plain(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// An expression key — `sql` without the wrapping parentheses.
    pub fn expr(sql: impl Into<String>) -> Self {
        Self {
            name: sql.into(),
            expression: true,
            ..Default::default()
        }
    }
}

/// An index on a table (its ordered key columns).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexInfo {
    pub name: String,
    pub columns: Vec<IndexColumn>,
    pub unique: bool,
    /// True if this index backs a FOREIGN KEY constraint.
    pub foreign: bool,
    /// The access method (`btree`, `hash`, `gin`…), when the server names one.
    /// Left `None` for the engine's default so generated DDL stays plain.
    pub method: Option<String>,
    /// A partial index's predicate, without the `WHERE` (PostgreSQL only).
    pub predicate: Option<String>,
    /// The constraint this index *is*, when it backs one — a PostgreSQL
    /// `PRIMARY KEY` or `UNIQUE` constraint. `None` for a plain index (and always
    /// on MySQL, which drops every key by index name).
    ///
    /// Carried because PostgreSQL refuses `DROP INDEX` on a constraint-backed
    /// index and has no `DROP PRIMARY KEY` — the only way to remove one is
    /// `ALTER TABLE … DROP CONSTRAINT <name>`, and the introspected index name
    /// isn't it (the primary index is renamed `PRIMARY` so
    /// [`IndexInfo::is_primary`] works the MySQL way).
    pub constraint: Option<String>,
    /// **This index holds something the model above cannot represent**, so what
    /// is in `columns` is not the whole index.
    ///
    /// It matters because an index edit is a `DROP` plus a `CREATE` built from
    /// this model: recreating a partly-read index silently destroys the parts
    /// that were never read. Measured against PostgreSQL 16, three things do
    /// this and none is exotic —
    ///
    /// - an **expression** key column (`lower(email)`): stored as `0` in
    ///   `pg_index.indkey`, which has no `pg_attribute` row, so it vanishes;
    /// - a non-default **operator class** (`last_name text_pattern_ops`), which
    ///   no per-column catalogue accessor returns;
    /// - a **`NULLS FIRST`/`LAST`** that isn't the default for the direction,
    ///   which lives in a bit of `pg_index.indoption`.
    ///
    /// `false` is the default so a hand-built or MySQL index behaves normally;
    /// the introspection that *can* be lossy is the one that sets it. `ddl::diff`
    /// then refuses to drop-and-recreate such an index as a side effect of an
    /// unrelated edit — the same "uncertainty resolves to don't destroy" rule
    /// `ddl::pg_replaceable` follows for views.
    pub lossy: bool,
}

impl IndexInfo {
    /// Is this the table's PRIMARY KEY?
    pub fn is_primary(&self) -> bool {
        self.name == "PRIMARY"
    }

    /// Just the key column names, for the callers that don't care about prefixes
    /// or sort order (edit-model key selection, the schema tree, the grid's key
    /// icons).
    ///
    /// An **expression** key is skipped: no result column carries its value, so
    /// nothing downstream could match it — and a caller that treated it as a
    /// column name would build a `WHERE lower(email) = …` keyed on a column that
    /// doesn't exist.
    pub fn column_names(&self) -> impl Iterator<Item = &str> {
        self.columns
            .iter()
            .filter(|c| !c.expression)
            .map(|c| c.name.as_str())
    }

    /// An index over whole columns, ascending — the shape most call sites mean.
    pub fn plain<S: Into<String>>(name: impl Into<String>, columns: Vec<S>, unique: bool) -> Self {
        Self {
            name: name.into(),
            columns: columns.into_iter().map(IndexColumn::plain).collect(),
            unique,
            ..Default::default()
        }
    }

    /// The parenthesised key list, with each column's prefix length and sort
    /// direction — `` `bio`(20), `age` DESC ``.
    pub fn key_sql(&self, dialect: crate::intel::SqlDialect) -> String {
        self.columns
            .iter()
            .map(|c| {
                // An expression is SQL, not a name: quoting it would turn the
                // whole thing into one identifier. Parenthesised because
                // PostgreSQL requires it for anything but a bare function call,
                // and accepts it for those too.
                let mut s = if c.expression {
                    format!("({})", c.name)
                } else {
                    ddl_ident_in(&c.name, dialect)
                };
                if let Some(n) = c.prefix {
                    s.push_str(&format!("({n})"));
                }
                if c.descending {
                    s.push_str(" DESC");
                }
                s
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// A foreign-key constraint: which local columns reference which columns of which
/// table. Populated from `information_schema.KEY_COLUMN_USAGE`; `columns` (the
/// referencing columns, in this table) and `ref_columns` (the referenced columns)
/// are aligned by key position. Drives "Follow" navigation from the data grid.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ForeignKeyInfo {
    /// The constraint's name. Without it a foreign key can't be dropped — both
    /// engines drop by name — so it's carried even though FK *navigation* never
    /// needed it.
    pub name: String,
    /// Referencing columns in *this* table, in key order.
    pub columns: Vec<String>,
    /// Referenced schema/database. `None` when the server reports none (treated
    /// as the same database as the referencing table).
    pub ref_schema: Option<String>,
    /// Referenced table.
    pub ref_table: String,
    /// Referenced columns, aligned to [`ForeignKeyInfo::columns`].
    pub ref_columns: Vec<String>,
    /// `ON DELETE` action (`CASCADE`, `SET NULL`, …). `None` means the standard
    /// default, `NO ACTION`, which both engines leave unwritten — so emitting
    /// nothing for `None` round-trips exactly.
    pub on_delete: Option<String>,
    /// `ON UPDATE` action, same rule as [`ForeignKeyInfo::on_delete`].
    pub on_update: Option<String>,
}

/// Backtick-quote a SQL identifier, doubling any embedded backtick.
fn ddl_ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// Quote an identifier for generated DDL in `dialect`.
pub fn ddl_ident_in(name: &str, dialect: crate::intel::SqlDialect) -> String {
    match dialect {
        crate::intel::SqlDialect::Postgres => format!("\"{}\"", name.replace('"', "\"\"")),
        _ => ddl_ident(name),
    }
}

/// Quote a string as a SQL literal for generated DDL (comments, and defaults we
/// had to quote ourselves).
///
/// **Takes the dialect because backslashes are dialect-critical**, and this
/// function once didn't: it doubled only the single quote, so on MySQL — where
/// `\` escapes inside a literal — a column comment of `C:\temp` was written as
/// `C:<TAB>emp`, and a value ending in a backslash escaped the closing quote and
/// malformed the statement. PostgreSQL takes a backslash literally, so doubling
/// there would corrupt the value instead.
///
/// It delegates to [`crate::export::sql_literal`] rather than repeating the
/// rule. Two implementations of one job is exactly how the rule came to be
/// applied in one of them and not the other.
pub fn ddl_string(s: &str, dialect: SqlDialect) -> String {
    crate::export::sql_literal(&crate::model::Value::Str(s.to_string()), dialect)
}

impl ColumnInfo {
    /// Does the server assign this column's value and **reject** an explicit one?
    ///
    /// A generated/computed column always does, on either engine, and so does
    /// PostgreSQL's `GENERATED ALWAYS AS IDENTITY`. `AUTO_INCREMENT`, `serial`
    /// and `GENERATED BY DEFAULT AS IDENTITY` do *not* — they fill the column in
    /// when nothing is supplied but accept a value, which is what someone
    /// re-importing their own keys wants.
    ///
    /// The one predicate for "must not be written", so a write path can't decide
    /// it differently: import reads it, and it is why importing a file Schemaic
    /// exported no longer fails the whole transaction on the first batch.
    pub fn is_server_assigned(&self) -> bool {
        self.generated.is_some() || self.identity_always
    }

    /// This column as it appears inside `CREATE TABLE` — the **whole**
    /// definition, in `dialect`.
    ///
    /// Whole is the point. MySQL's `MODIFY COLUMN` replaces a column outright, so
    /// this is exactly what an `ALTER` has to restate to avoid dropping the
    /// column's default, comment, collation or auto-increment as a side effect of
    /// changing its type. One emitter shared between `CREATE` and `MODIFY` is
    /// what keeps the two from drifting apart.
    pub fn definition_sql(&self, dialect: crate::intel::SqlDialect) -> String {
        let pg = dialect == crate::intel::SqlDialect::Postgres;
        let mut out = format!("{} {}", ddl_ident_in(&self.name, dialect), self.type_name);
        if let Some(col) = &self.collation
            && !pg
        {
            out.push_str(&format!(" COLLATE {col}"));
        }
        // A generated column carries an expression instead of a default.
        if let Some(expr) = &self.generated {
            out.push_str(&format!(" GENERATED ALWAYS AS ({expr})"));
            if pg {
                // PostgreSQL only has the stored form, and requires the keyword.
                out.push_str(" STORED");
            }
        }
        if !self.nullable {
            out.push_str(" NOT NULL");
        }
        if self.generated.is_none() {
            if let Some(d) = &self.default {
                out.push_str(&format!(" DEFAULT {d}"));
            }
            if self.auto_increment {
                // PostgreSQL's identity is a column attribute; MySQL's is a flag.
                // `ALWAYS` vs `BY DEFAULT` is a real difference in what the
                // column accepts, so restate the one the server reported.
                out.push_str(match (pg, self.identity_always) {
                    (true, true) => " GENERATED ALWAYS AS IDENTITY",
                    (true, false) => " GENERATED BY DEFAULT AS IDENTITY",
                    (false, _) => " AUTO_INCREMENT",
                });
            }
        }
        if let Some(u) = &self.on_update
            && !pg
        {
            out.push_str(&format!(" ON UPDATE {u}"));
        }
        // PostgreSQL has no inline column comment — it's a separate `COMMENT ON`
        // statement, which the DDL emitter adds alongside.
        if let Some(c) = &self.comment
            && !pg
            && !c.is_empty()
        {
            out.push_str(&format!(" COMMENT {}", ddl_string(c, dialect)));
        }
        out
    }
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
    /// For views, everything about them that isn't the SELECT — see
    /// [`ViewOptions`], which exists because redefining a view **replaces** it.
    /// `None` for base tables.
    pub view_options: Option<ViewOptions>,
    /// MySQL storage engine (`InnoDB`, `MyISAM`). `None` on PostgreSQL, which has
    /// no equivalent.
    pub engine: Option<String>,
    /// MySQL table collation (which implies its charset). `None` on PostgreSQL.
    pub collation: Option<String>,
    pub comment: Option<String>,
}

/// A view's options — everything about it that isn't the `SELECT`.
///
/// These are modelled at all for the same reason [`ColumnInfo`] carries a
/// column's whole definition: `CREATE OR REPLACE VIEW` **replaces the view**, so
/// anything the statement doesn't restate reverts to the server's default. For
/// `SQL SECURITY` that isn't cosmetic — a view redefined without the clause runs
/// as the *caller* instead of its definer, which is a privilege change nobody
/// asked for. Same for PostgreSQL's `security_barrier`, whose loss makes a view
/// leak rows it was written to hide.
///
/// Half the fields belong to one engine (as [`TableInfo::engine`] does): MySQL
/// has the definer and the security type, PostgreSQL the storage parameters and
/// materialization. `check_option` is the one both spell the same way.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewOptions {
    /// `WITH {CASCADED|LOCAL} CHECK OPTION`, upper-cased. `None` for a view
    /// without one (MySQL reports that as `NONE`).
    pub check_option: Option<String>,
    /// MySQL's `DEFINER`, as the catalogue reports it — `root@localhost`,
    /// unquoted. The two halves quote separately, so it's split at emit time
    /// ([`definer_sql`]) rather than stored pre-quoted.
    pub definer: Option<String>,
    /// MySQL's `SQL SECURITY`: `DEFINER` or `INVOKER`.
    pub security: Option<String>,
    /// MariaDB's `ALGORITHM` (`MERGE`/`TEMPTABLE`; `UNDEFINED` is the default and
    /// stays `None`). MySQL 8 doesn't expose it in `information_schema` at all —
    /// only `SHOW CREATE VIEW` has it — so a MySQL view's non-default algorithm
    /// is the one option a replace can still reset. Known gap.
    pub algorithm: Option<String>,
    /// PostgreSQL storage parameters other than `check_option`, verbatim
    /// (`security_barrier=true`, `security_invoker=true`).
    pub storage: Vec<String>,
    /// PostgreSQL materialized view (`relkind = 'm'`). It has no
    /// `CREATE OR REPLACE` and no check option, so Schemaic shows it rather than
    /// editing it.
    pub materialized: bool,
}

/// A MySQL `DEFINER` clause from the catalogue's `user@host` form.
///
/// The two halves are separate identifiers and quote separately, and the split
/// is on the **last** `@` — a user name may contain one, a host name may not.
pub fn definer_sql(definer: &str) -> String {
    match definer.rsplit_once('@') {
        Some((user, host)) => format!("DEFINER = {}@{}", ddl_ident(user), ddl_ident(host)),
        // No host part: emit the account as given, still quoted.
        None => format!("DEFINER = {}", ddl_ident(definer)),
    }
}

impl TableInfo {
    /// A `CREATE TABLE`/`CREATE VIEW` skeleton from the introspected schema. Not
    /// a round-trip of the server's DDL — no FK references, engine or charset —
    /// but a valid, useful skeleton in the connection's dialect:
    /// MySQL backtick-quotes and inlines `KEY`/`UNIQUE KEY`; PostgreSQL
    /// double-quotes and emits non-PK indexes as separate `CREATE INDEX`
    /// statements (its `CREATE TABLE` can't inline them). A table outside
    /// PostgreSQL's `public` is emitted schema-qualified, so the DDL recreates it
    /// in the namespace it came from rather than wherever `search_path` points.
    ///
    /// A **view** goes through [`crate::ddl::view_ddl`], the same emitter the
    /// apply path uses, so the copied statement carries the options a
    /// re-creation would otherwise reset. This branch used to build its own
    /// statement and drop all of them.
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
            // Through `ddl::view_ddl`, so the copy path and the apply path share
            // one emitter: this branch used to build its own statement and drop
            // every view option on the floor. `None` only when the definition
            // wasn't readable (e.g. privileges), which has nothing to restate.
            return match crate::ddl::view_ddl(self, dialect).filter(|_| {
                self.view_definition
                    .as_deref()
                    .is_some_and(|d| !d.trim().is_empty())
            }) {
                Some(sql) => sql,
                None => format!(
                    "-- View definition for {qname} was not available.\nCREATE VIEW {qname} AS\nSELECT ...;"
                ),
            };
        }
        let mut lines: Vec<String> = Vec::new();
        for c in &self.columns {
            lines.push(format!("  {}", c.definition_sql(dialect)));
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
                let using = match &ix.method {
                    Some(m) => format!(" USING {m}"),
                    None => String::new(),
                };
                let filter = match &ix.predicate {
                    Some(p) => format!(" WHERE {p}"),
                    None => String::new(),
                };
                // The index name is never qualified — Postgres puts an index in
                // its table's schema automatically, and `CREATE INDEX "s"."i"` is
                // a syntax error.
                out.push_str(&format!(
                    "\nCREATE {uniq}INDEX {} ON {qname}{using} ({}){filter};",
                    q(&ix.name),
                    ix.key_sql(dialect),
                ));
            }
            out
        } else {
            // MySQL: inline KEY / UNIQUE KEY.
            for ix in non_pk {
                let kw = if ix.unique { "UNIQUE KEY" } else { "KEY" };
                lines.push(format!("  {kw} {} ({})", q(&ix.name), ix.key_sql(dialect)));
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
    // One dialect-aware quoter/literal for both engines, rather than a second
    // hand-rolled copy here — that's how the two drift (the copy this replaced
    // rendered a non-finite float as `NaN`, which isn't valid SQL).
    let quote = |s: &str| crate::export::ident_sql(s, dialect);
    let literal = |v: &Value| crate::export::sql_literal(v, dialect);
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
            quote(&database),
            quote(&table),
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

    /// The table whose [`display_name`] is `name` — the inverse of the naming used
    /// for tree keys and ER-diagram node ids, for turning one of those back into a
    /// real table. Matches `sales.orders` and, for a `public`/MySQL table, the bare
    /// `orders`.
    pub fn find_by_display(&self, name: &str) -> Option<&TableInfo> {
        self.tables
            .iter()
            .find(|t| display_name(t.schema.as_deref(), &t.name) == name)
    }

    /// Every table in one namespace, in introspection order. `None` selects the
    /// tables that carry no namespace (i.e. all of them, on MySQL).
    pub fn tables_in(&self, schema: Option<&str>) -> impl Iterator<Item = &TableInfo> {
        self.tables
            .iter()
            .filter(move |t| t.schema.as_deref() == schema)
    }

    /// A `CREATE` script for every table in one namespace, blank-line separated.
    ///
    /// **Base tables first, then views** — a view's body references the tables it
    /// selects from, so the script only replays cleanly in that order. Foreign
    /// keys aren't emitted by [`TableInfo::create_ddl`] at all, so ordering
    /// *between* base tables doesn't affect validity.
    ///
    /// Empty when the namespace holds nothing.
    pub fn create_ddl_script(
        &self,
        schema: Option<&str>,
        dialect: crate::intel::SqlDialect,
    ) -> String {
        let (views, tables): (Vec<&TableInfo>, Vec<&TableInfo>) =
            self.tables_in(schema).partition(|t| t.is_view);
        tables
            .into_iter()
            .chain(views)
            .map(|t| t.create_ddl(dialect))
            .collect::<Vec<_>>()
            .join("\n\n")
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
///
/// The loaded schema is an `Arc` because reading it out of that signal is on the
/// typing path: `SignalGet::get` clones, and a by-value `DbSchema` meant every
/// completion, diagnostic and JOIN-target lookup deep-copied every `TableInfo`
/// and every `ColumnInfo` (with its ten heap fields) of **every** loaded
/// database — 1.8 ms per read on a 500-table schema, 7.7 ms at 1500, several
/// times per keystroke. With the `Arc` it is a refcount bump.
#[derive(Clone, Debug)]
pub enum SchemaState {
    /// Introspection query is in flight.
    Loading,
    Loaded(std::sync::Arc<DbSchema>),
    Failed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MySQL treats `\` as an escape inside a single-quoted literal, so a
    /// comment of `C:\temp` was stored as `C:<TAB>emp` — silently different from
    /// what the designer showed and the preview displayed. A value *ending* in a
    /// backslash escaped the closing quote and malformed the statement outright.
    /// PostgreSQL takes it literally, so doubling there would corrupt instead.
    #[test]
    fn ddl_string_escapes_backslashes_on_mysql_only() {
        assert_eq!(ddl_string(r"C:\temp", SqlDialect::MySql), r"'C:\\temp'");
        assert_eq!(ddl_string(r"C:\temp", SqlDialect::Postgres), r"'C:\temp'");
        // A trailing backslash is the case that breaks the statement, not just
        // the value.
        assert_eq!(ddl_string(r"ends\", SqlDialect::MySql), r"'ends\\'");
    }

    /// The injection guard, which applies on both engines.
    #[test]
    fn ddl_string_doubles_single_quotes_on_both_dialects() {
        for d in [SqlDialect::MySql, SqlDialect::Postgres] {
            assert_eq!(ddl_string("it's", d), "'it''s'");
        }
    }

    /// `ddl_string` and `export::sql_literal` quote for the same purpose, and
    /// having two implementations is what let one of them miss backslashes.
    #[test]
    fn ddl_string_agrees_with_the_export_literal() {
        for d in [SqlDialect::MySql, SqlDialect::Postgres] {
            for s in [r"C:\temp", "it's", "plain", r"both\'here", ""] {
                assert_eq!(
                    ddl_string(s, d),
                    crate::export::sql_literal(&crate::model::Value::Str(s.to_string()), d),
                    "{s:?} in {d:?}"
                );
            }
        }
    }

    /// The whole reason the column model was widened: MySQL's `MODIFY COLUMN`
    /// replaces a column outright, so anything this doesn't emit is destroyed by
    /// an ordinary type change. Each attribute is pinned individually.
    #[test]
    fn a_column_definition_restates_every_attribute() {
        let c = ColumnInfo {
            name: "status".into(),
            type_name: "varchar(20)".into(),
            nullable: false,
            default: Some("'draft'".into()),
            comment: Some("workflow state".into()),
            collation: Some("utf8mb4_bin".into()),
            ..Default::default()
        };
        assert_eq!(
            c.definition_sql(crate::intel::SqlDialect::MySql),
            "`status` varchar(20) COLLATE utf8mb4_bin NOT NULL DEFAULT 'draft' \
             COMMENT 'workflow state'"
        );
    }

    /// Auto-increment is spelled differently enough that a shared emitter has to
    /// branch — and a PostgreSQL comment isn't inline at all.
    #[test]
    fn auto_increment_and_comments_follow_the_dialect() {
        let c = ColumnInfo {
            name: "id".into(),
            type_name: "bigint".into(),
            primary_key: true,
            auto_increment: true,
            comment: Some("pk".into()),
            ..Default::default()
        };
        assert_eq!(
            c.definition_sql(crate::intel::SqlDialect::MySql),
            "`id` bigint NOT NULL AUTO_INCREMENT COMMENT 'pk'"
        );
        // PostgreSQL: identity syntax, and the comment is a separate statement.
        assert_eq!(
            c.definition_sql(crate::intel::SqlDialect::Postgres),
            "\"id\" bigint NOT NULL GENERATED BY DEFAULT AS IDENTITY"
        );
    }

    /// A generated column carries an expression *instead of* a default — emitting
    /// both is a syntax error.
    #[test]
    fn a_generated_column_emits_its_expression_and_no_default() {
        let c = ColumnInfo {
            name: "total".into(),
            type_name: "int".into(),
            nullable: true,
            generated: Some("qty * price".into()),
            default: Some("0".into()),
            ..Default::default()
        };
        let sql = c.definition_sql(crate::intel::SqlDialect::MySql);
        assert_eq!(sql, "`total` int GENERATED ALWAYS AS (qty * price)");
        assert!(!sql.contains("DEFAULT"));
    }

    /// A prefix index recreated without its length fails outright on a TEXT
    /// column, and a dropped DESC silently changes what the index is good for.
    #[test]
    fn an_index_key_keeps_prefixes_and_sort_order() {
        let ix = IndexInfo {
            name: "ix".into(),
            columns: vec![
                IndexColumn {
                    name: "bio".into(),
                    prefix: Some(20),
                    ..Default::default()
                },
                IndexColumn {
                    name: "age".into(),
                    descending: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            ix.key_sql(crate::intel::SqlDialect::MySql),
            "`bio`(20), `age` DESC"
        );
    }

    /// An expression key is SQL, not a name: quoting it would make the whole
    /// expression one identifier, and PostgreSQL needs the parentheses back.
    #[test]
    fn an_expression_key_is_emitted_parenthesised_and_unquoted() {
        let ix = IndexInfo {
            name: "ix".into(),
            columns: vec![
                IndexColumn::plain("last_name"),
                IndexColumn {
                    descending: true,
                    ..IndexColumn::expr("lower(email)")
                },
            ],
            ..Default::default()
        };
        // The column beside it is still quoted as an identifier — the difference
        // between the two halves is the whole point.
        assert_eq!(
            ix.key_sql(crate::intel::SqlDialect::Postgres),
            r#""last_name", (lower(email)) DESC"#
        );
    }

    /// An expression is not a row key: nothing in a result carries its value, and
    /// a caller that took it for a column name would build a `WHERE` on a column
    /// that doesn't exist. The columns beside it are still keys.
    #[test]
    fn column_names_skips_an_expression_key() {
        let ix = IndexInfo {
            name: "ix".into(),
            columns: vec![
                IndexColumn::plain("last_name"),
                IndexColumn::expr("lower(email)"),
            ],
            ..Default::default()
        };
        assert_eq!(ix.column_names().collect::<Vec<_>>(), vec!["last_name"]);
    }

    fn col(name: &str, ty: &str, nullable: bool, pk: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.to_string(),
            type_name: ty.to_string(),
            nullable,
            primary_key: pk,
            ..Default::default()
        }
    }

    fn fk(cols: &[&str], schema: Option<&str>, table: &str, ref_cols: &[&str]) -> ForeignKeyInfo {
        ForeignKeyInfo {
            name: format!("fk_{}", cols.join("_")),
            columns: cols.iter().map(|s| s.to_string()).collect(),
            ref_schema: schema.map(|s| s.to_string()),
            ref_table: table.to_string(),
            ref_columns: ref_cols.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
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
            ..Default::default()
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
                IndexInfo::plain("PRIMARY", vec!["id"], true),
                IndexInfo::plain("email_uq", vec!["email"], true),
            ],
            foreign_keys: Vec::new(),
            ..Default::default()
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
                IndexInfo::plain("PRIMARY", vec!["id"], true),
                IndexInfo::plain("email_uq", vec!["email"], true),
            ],
            foreign_keys: Vec::new(),
            ..Default::default()
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
            is_view: true,
            view_definition: Some("SELECT 1".to_string()),
            ..Default::default()
        };
        // Plain `CREATE VIEW`: a copied skeleton recreates the object elsewhere,
        // and failing on a name collision beats silently replacing a view.
        assert_eq!(
            t.create_ddl(crate::intel::SqlDialect::MySql),
            "CREATE VIEW `v` AS\nSELECT 1;"
        );
    }

    /// The two halves of a MySQL account are separate identifiers, split on the
    /// **last** `@` — a user name may hold one, a host name may not.
    #[test]
    fn definer_splits_the_account_and_quotes_both_halves() {
        assert_eq!(
            definer_sql("root@localhost"),
            "DEFINER = `root`@`localhost`"
        );
        assert_eq!(
            definer_sql("app@user@10.0.0.1"),
            "DEFINER = `app@user`@`10.0.0.1`"
        );
        assert_eq!(definer_sql("we`ird@host"), "DEFINER = `we``ird`@`host`");
        // No host part: still an identifier, still quoted.
        assert_eq!(definer_sql("root"), "DEFINER = `root`");
    }

    #[test]
    fn create_ddl_escapes_backticks() {
        let t = TableInfo {
            schema: None,
            name: "we`ird".to_string(),
            columns: vec![col("a`b", "int", true, false)],
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            ..Default::default()
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
            is_view: true,
            ..Default::default()
        };
        let ddl = t.create_ddl(crate::intel::SqlDialect::MySql);
        assert!(ddl.contains("-- View definition for `v` was not available."));
        assert!(ddl.contains("CREATE VIEW `v` AS\nSELECT ...;"));
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
            indexes: vec![IndexInfo::plain("orders_ts", vec!["id"], false)],
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
            "CREATE VIEW \"analytics\".\"daily\" AS\nSELECT 1;"
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
    fn create_ddl_script_emits_base_tables_before_views() {
        use crate::intel::SqlDialect::Postgres;
        let base = |ns: &str, name: &str| TableInfo {
            name: name.into(),
            schema: Some(ns.into()),
            columns: vec![col("id", "integer", false, true)],
            ..Default::default()
        };
        let s = DbSchema {
            tables: vec![
                // The view comes FIRST in introspection order, so a naive fold
                // would emit it before the table it selects from.
                TableInfo {
                    name: "big_orders".into(),
                    schema: Some("sales".into()),
                    is_view: true,
                    view_definition: Some("SELECT id FROM orders".into()),
                    ..Default::default()
                },
                base("sales", "orders"),
                base("public", "elsewhere"),
            ],
        };
        let out = s.create_ddl_script(Some("sales"), Postgres);
        let table_at = out.find("CREATE TABLE").expect("table emitted");
        let view_at = out.find("CREATE VIEW").expect("view emitted");
        assert!(table_at < view_at, "base tables must precede views:\n{out}");
        // Only this namespace's tables, blank-line separated.
        assert!(!out.contains("elsewhere"), "{out}");
        assert!(out.contains("\n\n"), "{out}");
    }

    #[test]
    fn find_by_display_round_trips_the_naming() {
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
                    columns: vec![col("legacy", "text", true, false)],
                    ..Default::default()
                },
            ],
        };
        // Every table round-trips through its own display name.
        for t in &s.tables {
            let found = s
                .find_by_display(&display_name(t.schema.as_deref(), &t.name))
                .expect("round-trips");
            assert_eq!(found.schema, t.schema);
        }
        // And the two are told apart.
        assert_eq!(
            s.find_by_display("sales.orders")
                .map(|t| t.schema.as_deref()),
            Some(Some("sales"))
        );
        assert_eq!(
            s.find_by_display("orders").map(|t| t.schema.as_deref()),
            Some(Some("public"))
        );
        // A stub / unknown id resolves to nothing rather than guessing.
        assert!(s.find_by_display("other_db.orders").is_none());
    }

    #[test]
    fn create_ddl_script_is_empty_for_an_unknown_namespace() {
        use crate::intel::SqlDialect::Postgres;
        let s = DbSchema {
            tables: vec![TableInfo {
                name: "orders".into(),
                schema: Some("sales".into()),
                ..Default::default()
            }],
        };
        assert_eq!(s.create_ddl_script(Some("ghosts"), Postgres), "");
        assert_eq!(s.tables_in(Some("ghosts")).count(), 0);
        assert_eq!(s.tables_in(Some("sales")).count(), 1);
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
        let ix = |name: &str| IndexInfo::plain(name, vec!["id"], true);
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
                    ..Default::default()
                },
                TableInfo {
                    name: "b".to_string(),
                    is_view: true,
                    ..Default::default()
                },
            ],
        };
        assert_eq!(s.table_count(), 2);
    }

    fn table_with_fks(fks: Vec<ForeignKeyInfo>) -> TableInfo {
        TableInfo {
            schema: None,
            name: "orders".to_string(),
            foreign_keys: fks,
            ..Default::default()
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
    fn follow_target_postgres_leaves_backslashes_alone() {
        use crate::intel::SqlDialect;
        // Postgres takes a backslash literally, so doubling it (MySQL's rule)
        // would follow the FK to a value that doesn't exist.
        let f = fk(&["p"], Some("public"), "files", &["path"]);
        let ft = follow_target(
            &f,
            &[Value::Str(r"C:\tmp".into())],
            "db",
            SqlDialect::Postgres,
        )
        .unwrap();
        assert_eq!(ft.sql, r#"SELECT * FROM "files" WHERE "path" = 'C:\tmp'"#);

        // MySQL still doubles it, because there `\` escapes.
        let m =
            follow_target(&f, &[Value::Str(r"C:\tmp".into())], "db", SqlDialect::MySql).unwrap();
        assert!(m.sql.ends_with(r"= 'C:\\tmp'"), "{}", m.sql);
    }

    #[test]
    fn follow_target_renders_a_nonfinite_float_as_null() {
        use crate::intel::SqlDialect;
        // Not reachable from a real key column (floats are refused as WHERE keys),
        // but the shared literal must never emit a bare `NaN` — that's a parse
        // error on both engines. Both dialects agree on NULL.
        let f = fk(&["m"], Some("public"), "m", &["v"]);
        for d in [SqlDialect::MySql, SqlDialect::Postgres] {
            let ft = follow_target(&f, &[Value::Float(f64::NAN)], "db", d).unwrap();
            assert!(ft.sql.ends_with("= NULL"), "{d:?}: {}", ft.sql);
        }
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
