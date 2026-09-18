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
    /// **SQLite's `AUTOINCREMENT` keyword**, which is a narrower claim than
    /// [`ColumnInfo::auto_increment`]. `false` on every other engine.
    ///
    /// Every `INTEGER PRIMARY KEY` in a rowid table is the rowid and is assigned
    /// by the engine, so `auto_increment` is true for all of them. The keyword
    /// adds one promise on top: the engine will never hand out an id it has used
    /// before, at the cost of a `sqlite_sequence` row it maintains per table.
    /// Reading the first as the second is how a rebuild came to add
    /// `AUTOINCREMENT` — and a `sqlite_sequence` entry — to every plain key it
    /// touched.
    pub sqlite_autoincrement: bool,
    /// The generated column is materialised (`STORED`) rather than recomputed on
    /// every read (`VIRTUAL`). Meaningless without [`ColumnInfo::generated`].
    ///
    /// **SQLite's is the only one that can be either.** PostgreSQL has no virtual
    /// form and MySQL reports its own, but SQLite defaults to `VIRTUAL` — so a
    /// `STORED` column re-emitted without the word stops being materialised, and
    /// the storage-versus-read trade the user chose is reversed silently. The
    /// distinction is in the `pragma_table_xinfo.hidden` value the reader already
    /// has (2 = VIRTUAL, 3 = STORED).
    pub generated_stored: bool,
    /// MySQL's `ON UPDATE CURRENT_TIMESTAMP` (the expression, not the keyword).
    pub on_update: Option<String>,
    pub comment: Option<String>,
    /// Explicit collation, when the server reports one for this column.
    pub collation: Option<String>,
    /// MySQL 8.0.23+ / MariaDB 10.3+ `INVISIBLE`: the column exists and is
    /// selectable by name, but `SELECT *` leaves it out.
    ///
    /// **Modelled because restating it wrong is a silent behaviour change.**
    /// Marking a column invisible is the standard way to retire one without
    /// breaking an application, and it is a decision a DBA *stages*. With no
    /// field here, every emitter restated the column visible: dump the database
    /// and restore it, or copy the DDL, and `SELECT *` starts returning the
    /// retired column again — every query written against the visible column set
    /// gets an extra one, and the retirement is undone with nothing said.
    ///
    /// [`index_disabled_sql`]'s sibling, one object down: the same catalogue,
    /// the same gesture, the same silent re-enable on a recreate. A *switched-off
    /// index* is answered by marking it [`IndexInfo::lossy`], because the model
    /// cannot restate it; a column can be restated, so this is a real field and
    /// the emitter writes the keyword. `false` on the two engines that have no
    /// such concept.
    pub invisible: bool,
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
    /// This key column's **own** collation, when the index states one that isn't
    /// the column's — `CREATE UNIQUE INDEX ix ON t (email COLLATE NOCASE)`.
    ///
    /// SQLite only. It is not decoration: the collation is what the uniqueness is
    /// *measured in*, so an index recreated without it accepts `'a@X'` beside
    /// `'A@x'` where the original refused the pair. `None` means "whatever the
    /// column collates as", which is what an ordinary index says.
    pub collation: Option<String>,
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
    /// The engine's **own** `CREATE INDEX` text for this index, terminated.
    ///
    /// Two of the three engines publish one: SQLite keeps the statement the user
    /// wrote in `sqlite_master.sql` (`None` for an index the engine created
    /// itself to back a `UNIQUE` or `PRIMARY KEY` constraint, which has a NULL
    /// `sql` because it is part of the table's declaration), and PostgreSQL
    /// renders one on demand with `pg_get_indexdef`. MySQL has no such accessor
    /// and leaves this `None`.
    ///
    /// It exists for the one job [`IndexInfo::lossy`] otherwise makes impossible.
    /// SQLite's twelve-step rebuild drops the table, so every index has to be
    /// created again — and an index re-emitted from a partial reading is a
    /// *different* index. Replaying this text puts back exactly what was there,
    /// the same fidelity argument [`TableInfo::dependent_ddl`] makes for
    /// triggers, and it is what lets a table with a partial or expression index
    /// be edited at all (`ddl::sqlite_rebuild_sql`).
    ///
    /// **On PostgreSQL it is what stops a structure dump rewriting an index
    /// nobody edited.** [`TableInfo::create_ddl`] emits from the model, and the
    /// model has no field for an `INCLUDE` list, `NULLS NOT DISTINCT` or a
    /// storage parameter — so a dump of `CREATE INDEX ix ON t (a, b) INCLUDE
    /// (c, d)` restored an index that no longer covers, with no edit and no
    /// warning. A lossy index there is emitted from this text instead.
    ///
    /// **Only ever replayed for an index the plan leaves alone.** The text is a
    /// snapshot of the index as it was; an edited one has to come from the model,
    /// and if the model can't carry it the plan is refused instead
    /// (`ddl::ChangeSet::unsupported`).
    pub create_sql: Option<String>,
}

/// What a [`TableInfo`] actually **is** — the three answers
/// `information_schema.TABLE_TYPE` gives.
///
/// `is_view` alone was the question every menu asked, and it has only two
/// answers: a MariaDB sequence fell on the "not a view, therefore an ordinary
/// table" side and was offered every table action there is. Passing `is_view =
/// true` for one would be no better — it is not a view either, and the
/// view-only entries (triggers, refresh, *Edit view*) would light up instead.
/// Three states, so a caller has to say which of the three it means.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableShape {
    #[default]
    Table,
    View,
    /// MariaDB's `CREATE SEQUENCE`, which the catalogue lists as a table. See
    /// [`TableInfo::is_sequence`].
    Sequence,
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

    /// **Does this index identify a row?** — the one question three different
    /// parts of the app ask of an index, and used to answer three ways.
    ///
    /// Not "is it unique". A unique index identifies a row only when all six
    /// hold:
    ///
    /// - **`unique`**, and **not `foreign`** — a foreign key constrains where a
    ///   value may point, not how many rows may hold it.
    /// - **not partial** ([`IndexInfo::predicate`]) — the uniqueness is promised
    ///   only over the rows the `WHERE` admits. `CREATE UNIQUE INDEX ux ON t
    ///   (email) WHERE deleted_at IS NULL` lets any number of soft-deleted rows
    ///   share an email.
    /// - **at least one key**, and **every key a plain column** — an expression
    ///   key is not a column, and no result column carries its value.
    ///   [`IndexInfo::column_names`] drops expressions, so `(a, lower(b))`
    ///   reduces to `[a]`, which is not unique: measured on PostgreSQL 16.15,
    ///   two rows with `a = 1` coexist happily under that index.
    /// - **not [`lossy`](IndexInfo::lossy)** — an index whose keys could not all
    ///   be read back, so what is in `columns` is not the whole index and none
    ///   of the checks above was made against the whole of it.
    /// - **no key carrying its own [`collation`](IndexColumn::collation)** —
    ///   the collation is what the uniqueness is *measured in*, and a
    ///   `WHERE col = ?` built from the key is measured in the **column's**
    ///   instead. SQLite is where the two can differ: given
    ///   `email TEXT COLLATE NOCASE` and `CREATE UNIQUE INDEX ux ON t (email
    ///   COLLATE BINARY)`, the index accepts `'A@x'` beside `'a@x'` and the
    ///   `UPDATE` then matches both, so the 1-row net rolls the batch back and
    ///   tells the user the edit failed for a reason that is not the reason —
    ///   the outcome this predicate exists to end. The safe direction (column
    ///   `BINARY`, index `NOCASE`) is refused with it, which is the
    ///   conservatism the paragraph below argues for. MySQL reports no
    ///   per-key collation and PostgreSQL reports a non-default operator class
    ///   as `lossy`, so this conjunct only ever narrows SQLite.
    ///
    /// **It says nothing about NULL.** A unique index over a nullable column
    /// identifies nothing (SQL lets any number of rows share a NULL), but that
    /// is a fact about the *table's* columns, which this cannot see. The two key
    /// resolvers add it; [`crate::ddl::supports_concurrent_refresh`] does not
    /// need to, because PostgreSQL checks it itself.
    ///
    /// This existed as `supports_concurrent_refresh`'s inner predicate — with a
    /// doc spelling out why uncertainty must refuse — while the two resolvers
    /// that decide whether a user may *edit a row* each filtered on `unique &&
    /// !foreign` alone. A partial unique index made a keyless table look
    /// editable; the `UPDATE` then matched two rows and the 1-row safety net
    /// rolled the whole batch back, telling the user the edit failed for a
    /// reason that was not the reason.
    pub fn identifies_a_row(&self) -> bool {
        self.unique
            && !self.foreign
            && self.predicate.is_none()
            && !self.lossy
            && !self.columns.is_empty()
            && self.columns.iter().all(|c| !c.expression)
            && self.columns.iter().all(|c| c.collation.is_none())
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
                // Before `DESC`, which is the order SQLite's grammar takes them
                // in — and this is what stops a recreate measuring uniqueness in
                // a different collation from the index it replaces.
                if let Some(col) = c.collation.as_deref().filter(|c| !c.is_empty()) {
                    s.push_str(&format!(" COLLATE {col}"));
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
    /// PostgreSQL's `MATCH FULL` / `MATCH PARTIAL`. `None` is `MATCH SIMPLE`,
    /// the unwritten default, so an untouched key round-trips exactly.
    ///
    /// The difference between refusing and accepting a **partially NULL**
    /// composite key: under `SIMPLE` a row with any NULL in the key passes the
    /// constraint, under `FULL` only an all-NULL one does. Recreating a `FULL`
    /// key as `SIMPLE` widens what the table accepts, and says nothing.
    ///
    /// MySQL parses `MATCH` and ignores it; SQLite has no such clause. `None` on
    /// both.
    pub match_type: Option<String>,
    /// PostgreSQL's `DEFERRABLE INITIALLY DEFERRED` / `DEFERRABLE INITIALLY
    /// IMMEDIATE`, written whole. `None` is `NOT DEFERRABLE`, the default.
    ///
    /// **This one changes what an application can do.** `DEFERRABLE INITIALLY
    /// DEFERRED` is what lets a transaction insert children before parents and
    /// have the key checked at commit; a copy restored without it refuses those
    /// inserts at statement time, so the application fails against a database
    /// that "restored fine".
    ///
    /// That it matters is already settled here: `ddl::unrestatable_sqlite_clauses`
    /// names *"a foreign key's DEFERRABLE clause"* and withholds the SQLite
    /// rebuild rather than drop it. The same attribute was a refusal on one
    /// engine and a silent drop on another, because the model had no field for
    /// either side to read.
    pub deferrable: Option<String>,
}

/// Backtick-quote a SQL identifier, doubling any embedded backtick.
/// Backtick-quote for the MySQL-only corners of generated DDL (a `DEFINER`
/// account, which no other engine has).
fn ddl_ident(name: &str) -> String {
    ddl_ident_in(name, crate::intel::SqlDialect::MySql)
}

/// Quote an identifier for generated DDL in `dialect`.
///
/// Delegates to [`crate::export::ident_sql`] — the one identifier-quoting rule.
/// The literal half of this module already went that way when `ddl_string`
/// turned out to be missing MySQL's backslash escaping while `sql_literal` had
/// it; this is the same consolidation for identifiers, done before rather than
/// after the divergence.
pub fn ddl_ident_in(name: &str, dialect: crate::intel::SqlDialect) -> String {
    crate::export::ident_sql(name, dialect)
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
        // SQLite shares MySQL's shape for the parts it has — `COLLATE`, the
        // generated expression, `NOT NULL`, `DEFAULT` — and has none of the
        // rest. Its key counter is `AUTOINCREMENT`, which is *only* legal
        // spelled inline as `INTEGER PRIMARY KEY AUTOINCREMENT`, so it belongs
        // to the table builder rather than to a column definition; `ON UPDATE`
        // is a MySQL timestamp attribute; and there are no comments at all, on
        // a column or anywhere else.
        let sqlite = dialect == crate::intel::SqlDialect::Sqlite;
        let mut out = format!("{} {}", ddl_ident_in(&self.name, dialect), self.type_name);
        if let Some(col) = &self.collation
            && !pg
        {
            out.push_str(&format!(" COLLATE {col}"));
        }
        // A generated column carries an expression instead of a default.
        if let Some(expr) = &self.generated {
            out.push_str(&format!(" GENERATED ALWAYS AS ({expr})"));
            // PostgreSQL only has the stored form and requires the keyword.
            // SQLite and MySQL both have both, both default to `VIRTUAL`, and
            // the difference is the storage/read trade the user chose — so a
            // `STORED` column that came back without the word has been silently
            // un-materialised.
            //
            // **MySQL was missing from that sentence while its reader set the
            // flag** (`extra_lc.contains("stored generated")`), so a dump, Copy
            // DDL, Duplicate table or a designer `ADD COLUMN` wrote the
            // keywordless form and the restored column came back `VIRTUAL` —
            // computed on every read instead of materialised, with nothing said.
            // Where the column is in the primary key it does not merely differ:
            // the restore fails, after the file's earlier statements have run.
            // Measured on MySQL 8.4.11 and MariaDB 10.11.14: the keywordless
            // form reports `VIRTUAL GENERATED` on both, both accept an explicit
            // `STORED` on `CREATE` and on `ADD COLUMN`, and a virtual generated
            // column in the primary key is refused (*ERROR 3106* / *ERROR
            // 1903*).
            //
            // The `false` case stays unwritten because `VIRTUAL` is every
            // engine's default, so omitting it round-trips and adding it would
            // rewrite the DDL of every existing virtual column.
            if pg || self.generated_stored {
                out.push_str(" STORED");
            }
        }
        if !self.nullable {
            out.push_str(" NOT NULL");
        }
        if self.generated.is_none() {
            // A server-assigned column carries its sequence *instead of* a
            // default, the same rule the generated branch above follows. A PG
            // `serial` reports as both — the catalogue renders its sequence
            // binding as a `nextval(...)` default — and naming both is an error
            // on either engine ("both default and identity specified" on
            // PostgreSQL, an invalid default on MySQL). The identity is the half
            // that stands alone: the default names a sequence that a fresh
            // `CREATE TABLE` has not created.
            if let Some(d) = &self.default
                && !self.auto_increment
            {
                // **Two engines want an expression default parenthesised**, and
                // neither hands the parentheses back with the value, so an
                // expression read and re-emitted verbatim is refused:
                //
                // - **SQLite** reports `pragma_table_xinfo.dflt_value` with the
                //   pair already stripped, so `DEFAULT (datetime('now'))` comes
                //   back bare and re-emits as `near "(": syntax error` — the
                //   table then uneditable for as long as the default exists.
                // - **MySQL 8** prints them in `SHOW CREATE TABLE` (the runnable
                //   form) but not in `information_schema.COLUMNS.COLUMN_DEFAULT`,
                //   so `b varchar(30) DEFAULT (CONCAT('a','c'))` restates as
                //   `DEFAULT concat(…)` and is `ERROR 1064` — measured on
                //   8.4.11, where the parenthesised form is accepted. MariaDB
                //   10.11.14 accepts both and normalises the pair away, so one
                //   answer serves the family.
                //
                // Which values may go bare is the *grammar's* question and is
                // asked of one predicate, per dialect: over-wrapping is not free
                // either, since MySQL reads `DEFAULT (7)` as an expression
                // default rather than a literal one.
                if is_bare_default(d, dialect) {
                    out.push_str(&format!(" DEFAULT {d}"));
                } else {
                    out.push_str(&format!(" DEFAULT ({d})"));
                }
            }
            if self.auto_increment && !sqlite {
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
            && !sqlite
        {
            out.push_str(&format!(" ON UPDATE {u}"));
        }
        // PostgreSQL has no inline column comment — it's a separate `COMMENT ON`
        // statement, which the DDL emitter adds alongside.
        if let Some(c) = &self.comment
            && !pg
            && !sqlite
            && !c.is_empty()
        {
            out.push_str(&format!(" COMMENT {}", ddl_string(c, dialect)));
        }
        // **Last, and MySQL-family only.** Both servers print `INVISIBLE` at the
        // end of the column definition, and neither PostgreSQL nor SQLite has
        // the concept — so a column the DBA had staged for retirement came back
        // visible from every recreate path, and `SELECT *` silently started
        // returning it again.
        if self.invisible && !pg && !sqlite {
            out.push_str(" INVISIBLE");
        }
        out
    }
}

/// The columns that identify a row of `t` for **browsing**, in key order —
/// empty when the table has none of its own.
///
/// **It has to answer the same question `edit::resolve_key` answers**, or the
/// grid projects a key the write path then ignores. `resolve_key` has three
/// sources: the primary key, then a unique non-foreign index whose columns are
/// all present and all `NOT NULL`, then the implicit key. `filter::BrowseKey`'s
/// caller used to supply only the first, so a `CREATE TABLE u (email TEXT NOT
/// NULL UNIQUE, name TEXT)` — a perfectly keyed table — was opened as
/// `SELECT rowid, * FROM u ORDER BY rowid`, carrying a rowid column into the
/// grid, every export and every copy, while the write keyed on `email` and never
/// looked at it. That is the outcome `BrowseKey::pick`'s own doc forbids.
///
/// The middle arm's `NOT NULL` requirement is the whole reason it is a *unique*
/// index and not merely a unique one: SQL lets any number of rows share a NULL
/// in a unique column, so a nullable one identifies nothing.
pub fn browse_key_columns(t: &TableInfo) -> Vec<String> {
    let pk: Vec<String> = t
        .columns
        .iter()
        .filter(|c| c.primary_key)
        .map(|c| c.name.clone())
        .collect();
    if !pk.is_empty() {
        return pk;
    }
    t.indexes
        .iter()
        // **The whole question, in one place** — see
        // [`IndexInfo::identifies_a_row`]. It subsumes the "no *column* keys"
        // guard this used to make on its own (PostgreSQL models
        // `CREATE UNIQUE INDEX ON u (lower(email))` as a real index over one
        // expression, and `column_names()` filters expressions out, so the
        // `all(…)` below is vacuously true for it), and adds the partial and
        // lossy cases that were missing here and in `edit::resolve_key` alike.
        .filter(|ix| ix.identifies_a_row())
        .find(|ix| {
            ix.column_names().all(|c| {
                t.columns
                    .iter()
                    .find(|tc| tc.name == c)
                    .map(|tc| !tc.nullable)
                    .unwrap_or(false)
            })
        })
        .map(|ix| ix.column_names().map(str::to_string).collect())
        .unwrap_or_default()
}

/// A trigger's `WHEN` clause, with the guard wrapped in parentheses that close
/// where a parenthesis will actually close.
///
/// **The user's guard is arbitrary SQL and may end in a line comment.**
/// `WHEN ({w})` then reads `WHEN (NEW.a > 0 -- only positives)` and the closing
/// paren is inside the comment: the engine fails on whatever comes next
/// (`near "BEGIN": syntax error`), which is not where the problem is, and the
/// text the user typed looks fine. So the group closes on its own line, always
/// — the same guard `ddl::create_view_sql` applies to its terminator, and
/// unconditional because a guard is multi-line as often as not and the shape
/// costs nothing.
fn when_group(guard: &str) -> String {
    format!("\nWHEN (\n{guard}\n)")
}

/// May this default text stand in a `DEFAULT` clause **without** parentheses in
/// `dialect`'s grammar?
///
/// One predicate for the whole question, because two engines ask it and both
/// hand the value back without the parentheses they require:
///
/// - **SQLite**'s grammar is narrow — a signed number, a string or blob literal,
///   `NULL`, `TRUE`/`FALSE`, and the three `CURRENT_*` keywords. Everything else
///   (a function call, an operator expression, a parenthesised anything) must be
///   wrapped, and `pragma_table_xinfo` strips exactly that pair.
/// - **MySQL and MariaDB** admit the same literal forms plus MySQL's `b'…'` bit
///   literal and a character-set introducer (`_utf8mb3'draft'`, which is how
///   MySQL 8 reports a literal it recorded as an expression default), and the
///   `CURRENT_TIMESTAMP` family with its optional precision. Nothing else:
///   `now()` is an expression there, and `SHOW CREATE TABLE` prints it
///   `DEFAULT (now())`.
/// - **PostgreSQL** takes an arbitrary expression bare — `nextval('s'::regclass)`
///   and `now()` are what `pg_get_expr` returns and what the grammar accepts —
///   so everything answers `true` and nothing is ever wrapped.
///
/// An already-parenthesised value is left alone on every engine, so a model
/// built from a designer edit rather than from a catalogue doesn't get a second
/// pair.
///
/// `pub(crate)` because `ddl::sqlite_constant_default` asks the same grammar
/// question for `ADD COLUMN` — the two used to answer it separately, and the one
/// that guessed sent statements the engine refuses down a path with no
/// transaction around it.
/// Which `DEFAULT` grammar an engine accepts **unparenthesised** — the
/// capability [`is_bare_default`] asks, instead of asking which engine it is.
///
/// **A fourth engine has to answer.** The three rule blocks below used to be
/// gated on `dialect == Postgres` and `dialect == MySql`, with everything else
/// falling through to SQLite's — the *narrowest* of the three — so a new variant
/// would inherit it with no comparison to grep for, and in the wrong direction:
/// SQLite wraps the most, and `ColumnInfo::definition_sql` turns a wrong `false`
/// into `DEFAULT (7)`, which MySQL reads as a different column. Over-wrapping is
/// not free.
///
/// The shape is `d4ff6bd`'s, one commit earlier in the same range, which removed
/// four instances of exactly this and whose subject is the rule's own words.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DefaultGrammar {
    /// Any expression may stand bare — PostgreSQL parses the whole clause as one.
    AnyExpression,
    /// A fixed keyword set, plus `CURRENT_TIMESTAMP(n)` and a character-set
    /// introducer before a literal. MySQL and MariaDB.
    MysqlKeywords,
    /// The narrowest: a literal, or one of a short keyword list. SQLite.
    LiteralsOnly,
}

/// The grammar `dialect` accepts, exhaustively, so a fourth engine is a compile
/// error rather than a silent fall-through. See [`DefaultGrammar`].
pub(crate) fn default_grammar(dialect: crate::intel::SqlDialect) -> DefaultGrammar {
    use crate::intel::SqlDialect;
    match dialect {
        SqlDialect::Postgres => DefaultGrammar::AnyExpression,
        SqlDialect::MySql => DefaultGrammar::MysqlKeywords,
        SqlDialect::Sqlite => DefaultGrammar::LiteralsOnly,
    }
}

pub(crate) fn is_bare_default(d: &str, dialect: crate::intel::SqlDialect) -> bool {
    let t = d.trim();
    if t.is_empty() {
        return true;
    }
    let grammar = default_grammar(dialect);
    if grammar == DefaultGrammar::AnyExpression {
        return true;
    }
    // **One group, closed by the final `)`** — not merely a `(` at each end.
    // `pragma_table_xinfo.dflt_value` strips the outer pair, so
    // `DEFAULT ((1+2)*(3+4))` arrives as `(1+2)*(3+4)`: two parens at the ends,
    // and an expression. A `starts_with`/`ends_with` pair called that bare,
    // `definition_sql` re-emitted it without the wrapper, and SQLite refuses the
    // result — which aborted the whole twelve-step rebuild on any edit to any
    // column of the table. The shared boundary lexer is what answers this, and
    // it is the same one twelve lines below (a paren inside a literal is not a
    // paren).
    // `balanced_paren_span` returns the index *of* the closing paren, so the
    // last byte is `len - 1`.
    if crate::sql::balanced_paren_span(t.as_bytes(), 0, dialect) == t.len().checked_sub(1) {
        return true;
    }
    let upper = t.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "NULL" | "TRUE" | "FALSE" | "CURRENT_TIME" | "CURRENT_DATE" | "CURRENT_TIMESTAMP"
    ) {
        return true;
    }
    // MySQL's `DEFAULT CURRENT_TIMESTAMP(6)` and its two synonyms, which take a
    // fractional-seconds precision the three bare keywords above do not.
    if grammar == DefaultGrammar::MysqlKeywords
        && let Some(rest) = ["CURRENT_TIMESTAMP", "LOCALTIMESTAMP", "LOCALTIME"]
            .iter()
            .find_map(|k| upper.strip_prefix(k))
        && let Some(n) = rest
            .trim()
            .strip_prefix('(')
            .and_then(|n| n.strip_suffix(')'))
        && n.trim().bytes().all(|c| c.is_ascii_digit())
    {
        return true;
    }
    // A string or blob literal — **the whole value**, which is what the shared
    // boundary lexer answers: it returns the offset just past the literal that
    // starts here, so `'a' || 'b'` correctly is *not* one (the literal ends at
    // 3, the value doesn't).
    //
    // MySQL adds two prefixes SQLite has no form of: `b'1010'` for a bit
    // literal, and a character-set introducer, which is what MySQL 8 puts in
    // front of a string it recorded as an expression default
    // (`_utf8mb3'draft'`). The literal itself is then the same question.
    let intro = if grammar == DefaultGrammar::MysqlKeywords {
        mysql_literal_introducer(t)
    } else {
        0
    };
    let (t, upper) = (&t[intro..], &upper[intro..]);
    let start = if upper.starts_with("X'") || (upper.starts_with("B'") && intro == 0) {
        1
    } else {
        0
    };
    if t.as_bytes().get(start) == Some(&b'\'')
        // Terminated, which the lexer alone can't say: an unclosed literal runs
        // to the end of the input and so also "ends" at `t.len()`.
        && t.len() > start + 1
        && t.ends_with('\'')
        && crate::sql::skip_noncode(t.as_bytes(), start, dialect) == Some(t.len())
    {
        return true;
    }
    // A signed number, decimal or hex — and *only* a number. `1+2` is an
    // expression, which SQLite's grammar admits nowhere a bare default can go,
    // and a permissive character-set test called it one.
    is_numeric_literal(t.strip_prefix(['+', '-']).unwrap_or(t))
}

/// How many bytes of `t` are a MySQL **character-set introducer** in front of a
/// string literal, or 0 when there is none.
///
/// `_utf8mb3'draft'` is what MySQL 8's `COLUMN_DEFAULT` holds for a string
/// recorded as an expression default, and `SHOW CREATE TABLE` prints it back
/// with the introducer intact — so it is part of the literal, not a reason to
/// call the value an expression. Shape only (`_` plus word bytes plus a quote);
/// the set of real charset names is the server's business and a name it doesn't
/// know is its error to give, not a reason to re-parenthesise.
fn mysql_literal_introducer(t: &str) -> usize {
    let b = t.as_bytes();
    if b.first() != Some(&b'_') {
        return 0;
    }
    let end = b[1..]
        .iter()
        .position(|c| !crate::sql::is_word_byte(*c))
        .map(|i| i + 1)
        .unwrap_or(b.len());
    if end > 1 && b.get(end) == Some(&b'\'') {
        end
    } else {
        0
    }
}

/// An unsigned SQLite numeric literal: `12`, `1.5`, `.5`, `1e-3`, `0xFF`.
fn is_numeric_literal(n: &str) -> bool {
    if let Some(hex) = n.strip_prefix("0x").or_else(|| n.strip_prefix("0X")) {
        return !hex.is_empty() && hex.bytes().all(|c| c.is_ascii_hexdigit());
    }
    // `<digits>[.<digits>][(e|E)[+|-]<digits>]`, with at least one digit in the
    // mantissa.
    let (mantissa, exponent) = match n.find(['e', 'E']) {
        Some(i) => (&n[..i], Some(&n[i + 1..])),
        None => (n, None),
    };
    let mut parts = mantissa.splitn(2, '.');
    let whole = parts.next().unwrap_or("");
    let frac = parts.next().unwrap_or("");
    let digits = |s: &str| s.bytes().all(|c| c.is_ascii_digit());
    if !digits(whole) || !digits(frac) || (whole.is_empty() && frac.is_empty()) {
        return false;
    }
    match exponent {
        None => true,
        Some(e) => {
            let e = e.strip_prefix(['+', '-']).unwrap_or(e);
            !e.is_empty() && digits(e)
        }
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
/// **Case-sensitively** `public`, and only that. PostgreSQL identifiers are
/// case-sensitive once quoted, so a schema literally named `"PUBLIC"` is a
/// different schema from `public` — and folding it away made every statement
/// generated for its objects address `public`'s same-named object instead,
/// including `recreate_type_sql`'s drop-and-rebuild. Reproduced live.
pub fn sql_qualifier(schema: Option<&str>) -> Option<&str> {
    match schema {
        Some(s) if s != PG_DEFAULT_SCHEMA => Some(s),
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

/// Is this database one a **list** may show and a **picker** may offer?
///
/// The SCHEMA panel's eye hides a database from *sight*, and sight is every
/// surface that lists databases rather than only the tree the eye is attached
/// to. The QUERY toolbar's selector went on offering hidden ones, so a database
/// the user had deliberately put away was still one click from being switched
/// to — the same failure Find-Anywhere would have if the keyboard walk didn't
/// ask this.
///
/// Everything that lists databases asks it: the schema tree's own stack, the
/// tree's `nav_rows` walk (which has to stay bug-for-bug identical to the
/// render), the active-database menu, and the trigger that opens it. The one
/// exception is the **eye's own menu**, which must list a hidden database for it
/// to be unhidden.
///
/// [`db_contributes`] is the same question asked by a surface that *describes*
/// the schema rather than listing it.
pub fn db_visible(hidden: &std::collections::HashSet<String>, database: &str) -> bool {
    !hidden.contains(database)
}

/// Does this database contribute its tables and columns to a surface that
/// **describes** the schema — autocomplete's suggestion pool, the AI's context?
///
/// [`db_visible`] plus one exception: the database being worked in. That one is
/// context rather than an offer — hiding the database you are inside would
/// otherwise take its own tables out of your completion and out of what the
/// assistant knows, while your queries went on running against it. It is the
/// same exception [`shown_database`] makes for the toolbar's label, for the same
/// reason: hiding governs what is *offered*, and a tab's own database is not an
/// offer.
///
/// Deliberately **not** asked by anything that validates. The completion
/// catalog behind the editor's diagnostics is built unfiltered, because a hidden
/// database's tables have not stopped existing and squiggling `archive.orders`
/// as unknown over a view preference would be a false error. Hiding governs what
/// is offered; never what is true.
pub fn db_contributes(
    hidden: &std::collections::HashSet<String>,
    database: &str,
    active_db: Option<&str>,
) -> bool {
    db_visible(hidden, database) || active_db.is_some_and(|a| a.eq_ignore_ascii_case(database))
}

/// The database to bind a tab to when nothing has chosen one — the first the
/// SCHEMA panel would actually *show*.
///
/// **The two sites that choose a tab's database are the ones [`db_visible`] has
/// to reach most.** They took `names.first()` unfiltered, so hiding the
/// alphabetically-first database and then switching to a connection with no open
/// tab (or pressing Ctrl+T before touching the selector) put the tab *inside*
/// it. [`db_contributes`]' active-database exception then fired for a database
/// the user had deliberately put away — it exists because "hiding it doesn't
/// move the tab", and here the app moved the tab into it — carrying it into the
/// toolbar's label, autocomplete's pool, both AI prompts with full column detail
/// and the MCP overview, from a state the user never asked for.
///
/// Falls back to the first name when **every** database is hidden: a connection
/// whose databases are all put away should still open a usable tab, and the
/// exception is then honest — the user is inside the one thing they can see.
/// **The connection's own database comes first, and that is what the form
/// promises.** The **Database** field says the connection opens there, and this
/// function ignored it entirely: every unbound tab landed in the
/// alphabetically-first *listed* database instead, so a connection configured
/// for `world` opened in `actdemo`. Every client the importer reads from opens
/// in the named database, and `conn_import` now fills the field from those
/// files, so the promise arrives pre-made and was being broken on the first
/// run.
///
/// It must still be a database this connection actually has and has not put
/// away: a stale name would bind every tab to something the tree does not show,
/// which is the failure the visibility filter below exists to prevent.
pub fn first_bindable<'a>(
    configured: Option<&str>,
    names: &'a [String],
    hidden: &std::collections::HashSet<String>,
) -> Option<&'a str> {
    configured
        .filter(|c| !c.trim().is_empty())
        .and_then(|c| names.iter().find(|n| n.as_str() == c))
        .filter(|n| db_visible(hidden, n))
        .or_else(|| names.iter().find(|n| db_visible(hidden, n)))
        .or_else(|| names.first())
        .map(String::as_str)
}

/// The database a **new tab** on this connection should bind to: the one the
/// user last switched to here, if it is still there and still visible, else the
/// connection's configured one, else
/// [`first_bindable`].
///
/// **The whole decision, not half of it.** The remembered branch used to test
/// only that the name still *existed* — so putting the current database away
/// with the SCHEMA eye and pressing Ctrl+T bound the new tab straight back into
/// it, and [`db_contributes`]' active-database exception then carried its every
/// table and column into autocomplete's pool, both AI prompts and the MCP
/// overview. That exception exists because "hiding it doesn't move the tab";
/// this is the app moving the tab *into* it. The fallback had been fixed and
/// this branch, one line above, had not.
///
/// `last` is **this connection's** remembered database. It used to be one global
/// signal with no connection dimension: picking `world` on MariaDB, where it is
/// visible, and then switching to a PostgreSQL connection where a `world` exists
/// and *is* hidden, bound the new tab to PostgreSQL's hidden one — the
/// per-connection guarantee `crate::db_hidden` exists to give, defeated a layer
/// above it. Keying the caller's map by connection is what makes that
/// impossible; this function only has to be told the right one.
pub fn tab_target<'a>(
    last: Option<&'a str>,
    configured: Option<&str>,
    names: &'a [String],
    hidden: &std::collections::HashSet<String>,
) -> Option<&'a str> {
    last.filter(|name| names.iter().any(|n| n == name) && db_visible(hidden, name))
        .or_else(|| first_bindable(configured, names, hidden))
}

/// The database name the QUERY toolbar's selector may show, given the databases
/// the active connection actually loaded.
///
/// A tab's `database` is *saved state* — it outlives the connection being
/// reachable, and it has to, or a server coming back would leave every tab
/// pointing somewhere new. What it must not do is get **drawn** as though it
/// were confirmed: a connection that failed to load shows an empty schema tree
/// and a "Disconnected" header, and a toolbar still naming a database is the one
/// surface claiming otherwise. The selector's own list would be empty, and
/// picking that name back is already refused (`set_active_db` checks the same
/// membership), so what it names cannot be chosen, listed or read.
///
/// Membership, not "is the list empty", so the rule holds on a live connection
/// too: a database dropped or renamed server-side stops being shown the moment a
/// reload no longer carries it.
///
/// The caller keeps the binding either way — this decides the *label*, and the
/// tab is left bound so a recovered connection restores it untouched.
pub fn shown_database<'a>(database: Option<&'a str>, loaded: &[String]) -> Option<&'a str> {
    let db = database?;
    loaded.iter().any(|name| name == db).then_some(db)
}

/// The SQL form of the same thing: a quoted, namespace-qualified object name.
///
/// The counterpart to [`display_name`] — one is what a person reads, this is what
/// a statement addresses — and the single builder for it, since every standalone
/// object (table, view, type, domain, sequence, function) needs the identical
/// "qualify unless it's `public`, then quote both halves" rule. It had been
/// written out inline in three places before the object emitters would have made
/// it six.
pub fn qualified_ident(
    name: &str,
    schema: Option<&str>,
    dialect: crate::intel::SqlDialect,
) -> String {
    match sql_qualifier(schema) {
        Some(s) => format!(
            "{}.{}",
            ddl_ident_in(s, dialect),
            ddl_ident_in(name, dialect)
        ),
        None => ddl_ident_in(name, dialect),
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
    /// True if this is a **MariaDB sequence** (`TABLE_TYPE = 'SEQUENCE'`).
    ///
    /// A separate flag rather than a third state of [`TableInfo::is_view`],
    /// because a sequence is not a view and answers differently to every
    /// view-shaped question: it takes no trigger, has no definition to edit and
    /// nothing to refresh. [`TableInfo::shape`] is the question with all three
    /// answers, and what the menus ask.
    ///
    /// **MariaDB stores a sequence as a one-row table**, and
    /// `information_schema.TABLES` lists it beside the real ones. Read as a base
    /// table it arrived with eight internal counter columns
    /// (`next_not_cached_value`, `minimum_value`, …), sat in the Tables folder,
    /// opened in the table designer offering `ALTER TABLE`, and dumped as
    /// `CREATE TABLE` **without** the `SEQUENCE=1` option that is what makes it
    /// one — so restoring the dump gave a plain table and every `NEXTVAL(sq1)`
    /// in the restored schema failed. Measured on MariaDB 10.11.14.
    ///
    /// Always `false` on MySQL, which has no sequences, and on PostgreSQL and
    /// SQLite, where a sequence is not a table at all — PostgreSQL's are
    /// modelled properly, in [`DbSchema::sequences`].
    pub is_sequence: bool,
    /// The name of a row identity this table has that is **not one of its
    /// columns** — SQLite's `rowid`, and nothing on MySQL or PostgreSQL, where
    /// every way of naming a row is a column. `None` for a table that has none,
    /// including a SQLite `WITHOUT ROWID` table.
    ///
    /// It exists so a table with no primary key and no usable unique index can
    /// still be edited: [`crate::filter::table_query`] projects it (a `SELECT *`
    /// would not return it), and the backend marks the resulting column
    /// [`crate::model::ColumnOrigin::implicit_key`] so the key resolver can fall
    /// back to it. This is a **capability**, not an engine tag: read it, don't
    /// ask which database this is.
    ///
    /// It states only that the table *has* one and how to spell it, never that it
    /// should be used — a real key still wins, and the projection is gated on the
    /// table having no key of its own.
    pub implicit_key: Option<String>,
    /// For views, the stored SELECT (`information_schema.VIEWS.VIEW_DEFINITION`),
    /// used to emit `CREATE VIEW`. `None` for base tables (and views whose
    /// definition couldn't be read).
    pub view_definition: Option<String>,
    /// The engine's **own** `CREATE` text for this table, when it keeps one —
    /// which of the three only SQLite does (`sqlite_master.sql`), including the
    /// separate `CREATE INDEX` statements a table's DDL is incomplete without.
    ///
    /// [`TableInfo::create_ddl`] returns it verbatim when present, and that is a
    /// fidelity decision rather than a shortcut. Reconstructing a SQLite table
    /// from this model emits MySQL's shape and gets it wrong in three ways at
    /// once: `AUTO_INCREMENT` instead of `AUTOINCREMENT` — which SQLite
    /// *accepts*, silently, by reading it as part of the type name — MySQL's
    /// inline `KEY name (cols)`, which SQLite has no syntax for at all, and an
    /// empty column list for an index whose keys are [`IndexInfo::lossy`]. It
    /// would also drop what the model doesn't carry: `WITHOUT ROWID`, CHECK
    /// constraints, column-level collations.
    ///
    /// `None` on MySQL and PostgreSQL, where the shared emitter is the answer and
    /// the model is complete enough to be. It is deliberately **not** used for a
    /// *view* even on SQLite: there the model genuinely is complete (a name and a
    /// body), so the emitter's output is both correct and consistent with the
    /// other engines'.
    pub create_sql: Option<String>,
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
    /// `CHECK` constraints declared on this table.
    ///
    /// Table-level on PostgreSQL and MySQL, and on MariaDB either that or
    /// **column-level** ([`CheckInfo::column_level`]) — which is the one that
    /// does *not* survive a `MODIFY COLUMN`, and so is the reason the flag
    /// exists at all.
    pub check_constraints: Vec<CheckInfo>,
    /// Triggers declared on this table. Table-owned on both engines, so they
    /// hang here rather than off [`DbSchema`] — a trigger has no independent
    /// existence to hang anywhere else.
    pub triggers: Vec<TriggerInfo>,
    /// The `CREATE` text of objects that **go down with this table** and have to
    /// be put back verbatim — SQLite's triggers, across the twelve-step rebuild
    /// (`ddl::sqlite_rebuild_sql`). Empty on every other engine, which alters in
    /// place and so never destroys the table its triggers hang off.
    ///
    /// Deliberately the server's own statement rather than a parsed model.
    /// Re-emitting a trigger from [`TriggerInfo`] would put it through a
    /// round-trip Schemaic doesn't yet do faithfully for SQLite, and the failure
    /// mode is the one [`IndexInfo::lossy`] exists to prevent: the part that
    /// didn't survive the parse is gone from a trigger that still looks armed.
    /// Replaying the text SQLite stored cannot lose anything.
    pub dependent_ddl: Vec<String>,
    /// **The `CREATE` text of triggers on _other_ tables whose SQL names this
    /// one** — SQLite only, and empty everywhere else.
    ///
    /// The sibling of [`TableInfo::dependent_ddl`] and its opposite in both
    /// directions: these are not dropped by a rebuild and are never replayed,
    /// but they are also never *rewritten* by one. SQLite's own
    /// `ALTER TABLE … RENAME COLUMN` rewrites every trigger in the database that
    /// names the column; the twelve-step rebuild renames the table underneath
    /// them and leaves their text exactly as it was.
    ///
    /// So a trigger on `other` that reads `UPDATE t SET b = 'hit'` survives a
    /// rebuild of `t` that renamed `b` — and SQLite does not validate a
    /// trigger's column references when it is created, so the plan *succeeds*,
    /// the report says it applied, and the next `INSERT INTO other` fails
    /// *no such column: b*. `other` then rejects every insert and nothing said
    /// so. That is word for word the failure
    /// [`crate::ddl::sqlite_rebuild_sql`]'s refusal exists to prevent, one table
    /// over, and it is what this field lets the refusal see.
    ///
    /// Kept as the server's verbatim text for the reason `dependent_ddl` is: the
    /// refusal reads it, and reading it through a parse Schemaic does not
    /// round-trip faithfully for triggers would be the same argument against.
    pub referring_ddl: Vec<String>,
    /// **SQLite `WITHOUT ROWID`.** `false` everywhere else, which has no such
    /// thing.
    ///
    /// Modelled because the rebuild writes the table back from this model, and a
    /// clause the model doesn't carry is a clause the rebuild drops: the table
    /// comes back as an ordinary rowid table, with a different storage layout,
    /// different `INTEGER PRIMARY KEY` semantics, and — the part that changes
    /// what the data is allowed to be — without the implicit `NOT NULL` a
    /// `WITHOUT ROWID` table's primary-key columns carry. The reader has it
    /// already: it is `pragma_table_list.wr`, the same row the implicit key asks.
    pub without_rowid: bool,
    /// **SQLite `STRICT`.** `false` everywhere else.
    ///
    /// Here for the same reason as [`TableInfo::without_rowid`]: a rebuild that
    /// drops it turns a table whose types the engine *enforces* into one whose
    /// declared types are advisory, and nothing in the plan or the result says
    /// so. It is the `strict` column of the same `pragma_table_list` row.
    pub strict: bool,
}

/// One `CHECK` constraint: a name and the predicate it enforces.
///
/// The expression is the server's own rendering (`pg_get_constraintdef` /
/// `CHECK_CLAUSE`), not the text the author typed — both engines re-print it
/// from the parse tree, adding their own quoting and parentheses. That is the
/// form to restate verbatim, and the reason [`crate::ddl::checks_equal`] exists:
/// a user who retypes an equivalent predicate must not produce a phantom change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckInfo {
    pub name: String,
    /// The predicate, without the wrapping `CHECK (…)`.
    pub expression: String,
    /// MySQL's `NOT ENFORCED`: the server records the constraint and does not
    /// apply it. Modelled — rather than assumed — for the same reason
    /// [`ViewOptions`]'s security type is: a constraint restated without it
    /// starts **rejecting writes** the table accepted a moment ago, and nothing
    /// in the statement says so.
    ///
    /// MySQL-only, like [`ViewOptions::algorithm`]. PostgreSQL's nearest thing is
    /// `NOT VALID`, which exempts only *existing* rows and so can't silently
    /// change what a write does — re-adding such a constraint as valid fails
    /// loudly against the rows that violate it, which is a report, not a trap.
    pub enforced: bool,
    /// **PostgreSQL**: `false` when the constraint was added `NOT VALID`, so the
    /// rows already in the table were never checked against it.
    ///
    /// Carried and restated rather than dropped. Dropping it is *safe* — the
    /// re-add fails loudly against violating rows rather than silently letting
    /// them through — but it is still a change to what the table promises, made
    /// by an edit that asked for something else, and it would turn a working
    /// Copy DDL script into one that fails on data the server itself accepts.
    pub validated: bool,
    /// **PostgreSQL**: `false` when the constraint was added `NO INHERIT`, so
    /// child tables don't get it. Restated for the same reason.
    pub inherited: bool,
    /// **MariaDB**: the constraint was written *inside* its column's definition
    /// (`information_schema.CHECK_CONSTRAINTS.LEVEL = 'Column'`), which is what
    /// the ordinary `q INT CHECK (q > 0)` syntax produces.
    ///
    /// Modelled because such a constraint is **part of the column**, exactly as
    /// its default or collation is: `ALTER TABLE … MODIFY COLUMN` replaces the
    /// whole definition, so a `MODIFY`/`CHANGE` that doesn't restate the check
    /// destroys it — silently, on the next introspection, with rows the table
    /// refused a moment ago accepted from then on. Measured on 10.11.14; MySQL 8
    /// rewrites the same syntax into a table constraint at `CREATE` time and so
    /// never has one.
    ///
    /// MariaDB gives it no name of its own — the syntax refuses a `CONSTRAINT`
    /// label at column level (1064) — so it is always named after its column and
    /// is renamed with it. `DROP CONSTRAINT` cannot find one (1091); the only way
    /// to change or remove it is to restate the column without it.
    pub column_level: bool,
}

impl Default for CheckInfo {
    /// A constraint is enforced unless the server says otherwise — the opposite
    /// default would quietly emit `NOT ENFORCED` on every check.
    fn default() -> Self {
        Self {
            name: String::new(),
            expression: String::new(),
            enforced: true,
            // Same rule: the opposite defaults would emit `NOT VALID` /
            // `NO INHERIT` on every check nobody asked to weaken.
            validated: true,
            inherited: true,
            // A check the model didn't read off MariaDB is a table constraint:
            // that is what every other producer makes, and what the emitter
            // writes.
            column_level: false,
        }
    }
}

impl CheckInfo {
    /// The `CONSTRAINT … CHECK (…)` clause, for a `CREATE TABLE` line or an
    /// `ADD CONSTRAINT`. Both engines spell it the same; only `NOT ENFORCED` is
    /// MySQL's alone.
    pub fn clause_sql(&self, dialect: crate::intel::SqlDialect) -> String {
        // **An unnamed check stays unnamed.** SQLite doesn't require a name and
        // most of its constraints don't have one; `CONSTRAINT "" CHECK (…)` is
        // not a nameless constraint but a syntax error, and inventing a name
        // would make a rebuild read as though it renamed something.
        let mut out = if self.name.is_empty() {
            format!("CHECK ({})", self.expression)
        } else {
            format!(
                "CONSTRAINT {} CHECK ({})",
                ddl_ident_in(&self.name, dialect),
                self.expression
            )
        };
        if dialect == crate::intel::SqlDialect::Postgres {
            // PostgreSQL's own order, as `pg_get_constraintdef` prints it:
            // `CHECK (…) NO INHERIT NOT VALID`.
            if !self.inherited {
                out.push_str(" NO INHERIT");
            }
            if !self.validated {
                out.push_str(" NOT VALID");
            }
        } else if !self.enforced {
            out.push_str(" NOT ENFORCED");
        }
        out
    }

    /// The same constraint written *inside* a column definition, as MariaDB
    /// accepts it there: bare `CHECK (…)`, with no name.
    ///
    /// See [`CheckInfo::column_level`] for why this spelling exists at all —
    /// MariaDB refuses a `CONSTRAINT` label at column level, and a `MODIFY`
    /// that omits the clause deletes the constraint. `NOT ENFORCED` is not
    /// emitted here: MariaDB, the only server with column-level checks, has no
    /// such clause.
    pub fn inline_sql(&self) -> String {
        format!("CHECK ({})", self.expression)
    }
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
/// Most fields belong to one engine (as [`TableInfo::engine`] does): MySQL has
/// the definer and the security type, PostgreSQL the storage parameters and
/// materialization, SQLite the explicit column list. `check_option` is the one
/// two of them spell the same way — SQLite has no form of it.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// **PostgreSQL `pg_class.relispopulated`.** False for a materialized view
    /// created `WITH NO DATA` and never refreshed since.
    ///
    /// Here because the server refuses `REFRESH MATERIALIZED VIEW
    /// CONCURRENTLY` on an unpopulated view, and
    /// [`crate::ddl::supports_concurrent_refresh`] had no way to see it: a view
    /// created `WITH NO DATA` that happens to carry a unique index got the
    /// concurrent form, PG refused it, and the single menu entry offered no
    /// plain fallback — so *Refresh view* was inoperable for that view, every
    /// time.
    ///
    /// **Defaults to `true`**, which is the right answer for every object that
    /// is not a PostgreSQL materialized view and keeps a `ViewOptions` built
    /// anywhere else behaving as it did.
    pub populated: bool,
    /// **SQLite.** The explicit column list of `CREATE VIEW v (x, y) AS …`,
    /// verbatim and without its parentheses — `None` for the usual view, which
    /// takes its column names from the body.
    ///
    /// Carried because on SQLite it is the one part of a view that is neither
    /// the body nor recoverable from it, and *every* edit there is a drop and a
    /// re-create ([`crate::ddl::supports_or_replace_view`]). Left out, an edit to
    /// the `WHERE` of `CREATE VIEW v (x, y) AS SELECT a, b …` would silently
    /// rename the view's columns to `a` and `b`.
    ///
    /// Verbatim rather than a `Vec<String>` because it round-trips exactly:
    /// SQLite hands back whatever quoting the list was written with, and
    /// re-quoting a parsed list is a way to change it. The other two engines
    /// bake the names into the body they report, so this stays `None` there.
    pub column_list: Option<String>,
}

/// **Hand-written for one field.** Every other default here is "absent", which
/// `derive` gets right; `populated` defaults to *true*, because "we do not know
/// whether this view holds rows" must not read as "it holds none" — that is the
/// answer that withdraws the concurrent refresh from every view on the two
/// engines that have no such flag at all.
impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            check_option: None,
            definer: None,
            security: None,
            algorithm: None,
            storage: Vec::new(),
            materialized: false,
            populated: true,
            column_list: None,
        }
    }
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

// ── Triggers ────────────────────────────────────────────────────────────────

/// When a trigger fires relative to the statement that set it off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TriggerTiming {
    #[default]
    Before,
    After,
    /// **PostgreSQL only**, and only on a view: it *replaces* the write rather
    /// than running alongside it.
    InsteadOf,
}

impl TriggerTiming {
    pub fn sql(self) -> &'static str {
        match self {
            TriggerTiming::Before => "BEFORE",
            TriggerTiming::After => "AFTER",
            TriggerTiming::InsteadOf => "INSTEAD OF",
        }
    }

    /// Read a server's spelling. MySQL's `ACTION_TIMING` says `BEFORE`/`AFTER`;
    /// PostgreSQL's `tgtype` is decoded in `schemaic-db` and arrives here as one
    /// of these words. Unknown ⇒ `None`, so a server that grows a new timing
    /// surfaces rather than being silently filed as `BEFORE`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        [
            TriggerTiming::Before,
            TriggerTiming::After,
            TriggerTiming::InsteadOf,
        ]
        .into_iter()
        .find(|t| {
            s.eq_ignore_ascii_case(t.sql())
                // PostgreSQL's catalogues spell it with an underscore.
                || (*t == TriggerTiming::InsteadOf && s.eq_ignore_ascii_case("INSTEAD_OF"))
        })
    }
}

/// What a trigger fires on.
///
/// **The declaration order is load-bearing**: the derived `Ord` is the order the
/// UI sorts a trigger's events into, and it must be the order PostgreSQL prints
/// them in — which is `pg_trigger.tgtype`'s bit order, `INSERT`(4),
/// `DELETE`(8), `UPDATE`(16), `TRUNCATE`(32), *not* the DML order a person would
/// write down. When this read `Insert, Update, Delete`, an introspected
/// `AFTER DELETE OR UPDATE` trigger came back as `[Delete, Update]`, one tick of
/// any checkbox renormalised it to `[Update, Delete]`, and `diff_triggers`'
/// element-wise compare reported a change on a trigger nothing had touched — so
/// Apply emitted a `DROP` + `CREATE` of its own accord. `db::pg_trigger_type`
/// pins the two together from the side that can see both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TriggerEvent {
    #[default]
    Insert,
    Delete,
    Update,
    /// **PostgreSQL only**, and only `FOR EACH STATEMENT`.
    Truncate,
}

impl TriggerEvent {
    pub fn sql(self) -> &'static str {
        match self {
            TriggerEvent::Insert => "INSERT",
            TriggerEvent::Update => "UPDATE",
            TriggerEvent::Delete => "DELETE",
            TriggerEvent::Truncate => "TRUNCATE",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        [
            TriggerEvent::Insert,
            TriggerEvent::Delete,
            TriggerEvent::Update,
            TriggerEvent::Truncate,
        ]
        .into_iter()
        .find(|e| s.eq_ignore_ascii_case(e.sql()))
    }
}

/// Once per affected row, or once per statement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TriggerLevel {
    #[default]
    Row,
    Statement,
}

impl TriggerLevel {
    pub fn sql(self) -> &'static str {
        match self {
            TriggerLevel::Row => "FOR EACH ROW",
            TriggerLevel::Statement => "FOR EACH STATEMENT",
        }
    }
}

/// MySQL's `FOLLOWS`/`PRECEDES`: where this trigger sits among the others on the
/// same table and event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TriggerOrder {
    Follows(String),
    Precedes(String),
}

impl TriggerOrder {
    /// The trigger this clause names — the one thing both arms have, and the
    /// one thing a reader deciding whether the clause can be *resolved* needs.
    /// See [`TriggerInfo::with_resolvable_order`].
    pub fn target(&self) -> &str {
        match self {
            TriggerOrder::Follows(n) | TriggerOrder::Precedes(n) => n.as_str(),
        }
    }
}

/// What the trigger runs when it fires — the one place the two engines differ in
/// *kind* rather than in spelling, which is why this is an enum and not a
/// `String` both sides pretend to understand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TriggerAction {
    /// MySQL/MariaDB: the statement body — one statement or a `BEGIN … END`
    /// block, no trailing `;`.
    ///
    /// **As `SHOW CREATE TRIGGER` reports it, not `information_schema`.** See
    /// [`TriggerSource`]: on MySQL 8 that column resolves the body's escapes,
    /// and a recreate from it writes a different trigger or destroys the one it
    /// replaces.
    Body(String),
    /// PostgreSQL: the function to call. A PG trigger holds no body of its own,
    /// so the function is a separate object with its own lifetime — dropping the
    /// trigger leaves it behind, and dropping it out from under the trigger
    /// breaks every write to the table.
    ///
    /// **`name` is emittable SQL** — already quoted, and qualified when it isn't
    /// in `public` — never a bare identifier. Both producers must write that
    /// shape: introspection's `tgfoid::regproc::text` does so natively, and the
    /// editor's picker builds it with [`qualified_ident`]. This is written down
    /// because the field once meant *both* things depending on who wrote it,
    /// which is how a trigger came to be bound to `public`'s copy of a function
    /// the user had picked from another schema — and how two review passes
    /// reached opposite conclusions about which side was wrong. Do not route
    /// this through a quoter on the way out; it is already quoted.
    Function { name: String, args: Vec<String> },
}

impl Default for TriggerAction {
    fn default() -> Self {
        TriggerAction::Body(String::new())
    }
}

/// One trigger on a table.
///
/// Carries its whole definition for the reason [`ColumnInfo`] does: **none of
/// the three engines can alter a trigger in place.** MySQL and SQLite have no
/// `CREATE OR REPLACE TRIGGER` at all, and PostgreSQL's replaces the entire
/// object — so every edit is a drop-and-create, and anything this model doesn't
/// hold is destroyed the first time a user changes the timing.
///
/// On SQLite that cuts deeper than on the other two, because the model is the
/// only structured account of the trigger there is: the server publishes no
/// catalogue of a trigger's parts, so this is filled by *parsing* the stored
/// statement ([`crate::ddl::sqlite_trigger_info`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TriggerInfo {
    pub name: String,
    /// The namespace of the table it hangs off — PostgreSQL's schema, `None` on
    /// MySQL. A trigger has no namespace of its own on either engine: its name
    /// is unique per table (PG) or per database (MySQL).
    pub schema: Option<String>,
    pub table: String,
    pub timing: TriggerTiming,
    /// The events it fires on. PostgreSQL allows several on one trigger
    /// (`BEFORE INSERT OR UPDATE`); MySQL and SQLite allow exactly one. The
    /// model holds both shapes and `TriggerDraft::validate` is what refuses the
    /// impossible one, so introspection never has to lie about what the server
    /// reported.
    pub events: Vec<TriggerEvent>,
    /// `UPDATE OF a, b` — narrows an `Update` event to named columns. Empty
    /// means every column.
    ///
    /// **PostgreSQL and SQLite**, which is not what this said when only two
    /// engines were wired: MySQL is the one with no such clause. Emitted for
    /// both ([`TriggerInfo::create_sql`]), and reading it as PostgreSQL's alone
    /// would drop it from a SQLite trigger on the re-create every edit there
    /// performs — leaving one that fires on every column instead.
    pub update_columns: Vec<String>,
    pub level: TriggerLevel,
    /// The `WHEN (…)` guard, held **bare** — without the parens the server
    /// prints around it and the emitter adds back. Same rule as
    /// [`crate::ddl::check_predicate`]: normalize on the way in, wrap exactly
    /// once on the way out, so a round trip doesn't grow a layer per edit.
    ///
    /// **PostgreSQL and SQLite**, for the reason [`TriggerInfo::update_columns`]
    /// spells out — MySQL is again the engine without one, and its
    /// `TriggerDraft::validate` arm is what says so.
    pub condition: Option<String>,
    pub action: TriggerAction,
    /// MySQL's `DEFINER`, unquoted (`root@localhost`). Modelled for the reason
    /// [`ViewOptions::definer`] is: a trigger recreated without it runs as
    /// whoever recreated it, and nothing in the statement says a privilege
    /// changed.
    pub definer: Option<String>,
    /// MySQL's ordering clause. Dropping it on a recreate silently reorders the
    /// triggers on that event — and order is the entire point when two of them
    /// write the same row.
    pub order: Option<TriggerOrder>,
    /// **MySQL/MariaDB**: the session state the trigger was *created* under,
    /// which is part of what it does and is not restated by `CREATE TRIGGER`.
    ///
    /// A trigger written under `sql_mode = ''` and recreated under a strict mode
    /// starts failing every parent `INSERT`; reversed, it stops raising and
    /// silently truncates. `character_set_client` and `collation_connection`
    /// decide how string literals *in the body* compare. None of the three is
    /// readable from `information_schema.TRIGGERS`, so they arrive with the body
    /// from `SHOW CREATE TRIGGER` — see [`TriggerSource`].
    ///
    /// `None` means "not known", which is what an unfetched trigger and every
    /// PostgreSQL one both are, and nothing is emitted for it.
    pub sql_mode: Option<String>,
    pub charset_client: Option<String>,
    pub collation_connection: Option<String>,
    /// **PostgreSQL**: `REFERENCING OLD TABLE AS …` / `NEW TABLE AS …` — the
    /// transition relations a statement-level trigger's function reads.
    ///
    /// Modelled for the same reason [`ViewOptions::definer`] is, only louder:
    /// `CREATE TRIGGER` without the clause succeeds, and *then* every write to
    /// the table fails with `relation "o" does not exist`, because the function
    /// body still references a table that no longer exists for it. The failure
    /// surfaces on a write, not in the preview.
    pub old_table: Option<String>,
    pub new_table: Option<String>,
    /// **PostgreSQL**: which sessions the trigger fires in — `tgenabled`'s four
    /// states, not two. Recreating any of them as the default starts (or stops)
    /// firing it against writes it was deliberately configured for, so
    /// [`TriggerInfo::create_sql`] restates it.
    pub enabled: TriggerEnabled,
    /// **PostgreSQL**: a `CREATE CONSTRAINT TRIGGER`. Schemaic doesn't model the
    /// deferral options one carries, so these are shown and droppable but not
    /// editable — the same call [`ViewOptions::materialized`] gets.
    pub constraint: bool,
}

/// What one `SHOW CREATE TRIGGER` round trip yields for a MySQL trigger: the
/// body **as written**, and the session state it was written under.
///
/// It exists because `information_schema.TRIGGERS.ACTION_STATEMENT` cannot be
/// used to recreate a trigger on MySQL 8. That column returns the body with its
/// escapes **already resolved**, and the damage is not recoverable by
/// re-escaping — measured on 8.4.11, a body holding `'C:\temp'` comes back
/// carrying a literal tab (`…27433A09656D7027`), which is indistinguishable
/// from a trigger that really was written with one; a body holding `'it''s'`
/// comes back as `'it's'`, which is a 1064 syntax error on restate, *after* the
/// `DROP` has committed and taken the only copy with it. MariaDB returns both
/// verbatim.
///
/// The same statement is also the only place the three session values live, so
/// one round trip answers both. Fetched **lazily**, when the editor opens — the
/// call [`ViewOptions::algorithm`] already makes, and for the same reason.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TriggerSource {
    /// Everything after `FOR EACH ROW` and any ordering clause.
    pub body: String,
    pub sql_mode: Option<String>,
    pub charset_client: Option<String>,
    pub collation_connection: Option<String>,
}

impl TriggerSource {
    /// Copy this onto a [`TriggerInfo`] — the body **and** the session state.
    ///
    /// One method because both sides of the diff have to be patched with it
    /// (`current` and the draft), exactly as `view_editor::fetch_algorithm`
    /// does: patching only the draft would make every MySQL trigger open
    /// already-changed against a `current` that still held the corrupt body.
    pub fn apply_to(&self, t: &mut TriggerInfo) {
        t.action = TriggerAction::Body(self.body.clone());
        t.sql_mode = self.sql_mode.clone();
        t.charset_client = self.charset_client.clone();
        t.collation_connection = self.collation_connection.clone();
    }
}

/// Which sessions a PostgreSQL trigger fires in — `pg_trigger.tgenabled`.
///
/// Four states, and only the first two are interchangeable with a bool. `A` and
/// `R` exist for logical replication: an `ALWAYS` trigger fires even while the
/// replication apply worker is writing, a `REPLICA` one fires *only* then. Both
/// used to fold into `true` and be recreated as [`TriggerEnabled::Origin`],
/// which changes what fires during replication with nothing to say so.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TriggerEnabled {
    /// `O` — fires in ordinary sessions. What `CREATE TRIGGER` produces.
    #[default]
    Origin,
    /// `D` — disabled; does not fire at all.
    Disabled,
    /// `A` — fires in ordinary sessions *and* during replication apply.
    Always,
    /// `R` — fires **only** during replication apply.
    Replica,
}

impl TriggerEnabled {
    /// Read `pg_trigger.tgenabled`. An unknown letter is [`Self::Origin`], the
    /// same "a server that grows a state surfaces as the ordinary one rather
    /// than as disabled" call [`TriggerTiming::parse`] makes.
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "D" => TriggerEnabled::Disabled,
            "A" => TriggerEnabled::Always,
            "R" => TriggerEnabled::Replica,
            _ => TriggerEnabled::Origin,
        }
    }

    /// The `ALTER TABLE … <clause> TRIGGER n` a recreate must follow the
    /// `CREATE` with, or `None` when the create already produced this state.
    pub fn alter_clause(self) -> Option<&'static str> {
        match self {
            TriggerEnabled::Origin => None,
            TriggerEnabled::Disabled => Some("DISABLE TRIGGER"),
            TriggerEnabled::Always => Some("ENABLE ALWAYS TRIGGER"),
            TriggerEnabled::Replica => Some("ENABLE REPLICA TRIGGER"),
        }
    }

    /// Does it fire in an ordinary session? What the UI's list shows.
    pub fn fires_normally(self) -> bool {
        matches!(self, TriggerEnabled::Origin | TriggerEnabled::Always)
    }
}

impl Default for TriggerInfo {
    /// A trigger fires unless the server says otherwise; defaulting to
    /// `Disabled` would emit a `DISABLE TRIGGER` after every create.
    fn default() -> Self {
        Self {
            name: String::new(),
            schema: None,
            table: String::new(),
            timing: TriggerTiming::default(),
            events: Vec::new(),
            update_columns: Vec::new(),
            level: TriggerLevel::default(),
            condition: None,
            action: TriggerAction::default(),
            definer: None,
            order: None,
            sql_mode: None,
            charset_client: None,
            collation_connection: None,
            old_table: None,
            new_table: None,
            enabled: TriggerEnabled::default(),
            constraint: false,
        }
    }
}

impl TriggerInfo {
    /// This trigger with an ordering clause **the server could not resolve**
    /// taken off, judged by `exists`.
    ///
    /// **`FOLLOWS`/`PRECEDES` is a statement about the group as it stands when
    /// the statement runs, not a property of the trigger.** MySQL and MariaDB
    /// both refuse a clause naming a trigger that is not there yet
    /// (`ERROR 3011` / `ERROR 4031`, *"Referenced trigger … for the given action
    /// time and event type does not exist"*), and the catalogue always gives the
    /// **leader** of a group `PRECEDES <successor>` — which is right for the
    /// caller that replaces one trigger inside a group that already exists, and
    /// impossible for the caller that creates the whole group from nothing.
    /// Emitted in catalogue order that way, a dump of any table with two
    /// triggers in one timing/event group died on its *first* `CREATE TRIGGER`,
    /// on both servers, after the `DROP TABLE` above it had already run.
    ///
    /// Dropping the leader's clause is sufficient rather than approximate: every
    /// non-leader carries `FOLLOWS <predecessor>`, so a group created in order
    /// reconstructs its own chain, and a trigger created alone into an empty
    /// group is its leader whatever it says.
    ///
    /// `exists` is the caller's, because only the caller knows what "yet"
    /// means. [`TriggerInfo::create_set_sql`] is the one that asks it today:
    /// whether the name comes earlier in the same set. A comparison between two
    /// databases would ask whether the other one already holds it — the reason
    /// the predicate is a parameter and not a field — but nothing does yet, and
    /// this said `crate::compare::SchemaComparison` did.
    ///
    /// Borrows when there is nothing to take off, which is every trigger on the
    /// other two engines — neither has the clause.
    pub fn with_resolvable_order(
        &self,
        exists: impl Fn(&str) -> bool,
    ) -> std::borrow::Cow<'_, TriggerInfo> {
        let Some(named) = self.order.as_ref().map(TriggerOrder::target) else {
            return std::borrow::Cow::Borrowed(self);
        };
        if exists(named) {
            return std::borrow::Cow::Borrowed(self);
        }
        let mut out = self.clone();
        out.order = None;
        std::borrow::Cow::Owned(out)
    }

    /// The `CREATE TRIGGER`s that rebuild `triggers` as a **set**, in the order
    /// given — each one's ordering clause kept only where the trigger it names
    /// is created earlier in the same set.
    ///
    /// The whole-set counterpart of [`TriggerInfo::create_sql`]; see
    /// [`TriggerInfo::with_resolvable_order`] for what the difference costs.
    /// Comparison is case-insensitive because a trigger name is the server's.
    pub fn create_set_sql(
        triggers: &[TriggerInfo],
        dialect: crate::intel::SqlDialect,
    ) -> Vec<String> {
        triggers
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let before = &triggers[..i];
                t.with_resolvable_order(|n| before.iter().any(|e| e.name.eq_ignore_ascii_case(n)))
                    .create_sql(dialect)
            })
            .collect()
    }

    /// The `CREATE TRIGGER` that recreates this trigger exactly — the **one**
    /// trigger emitter, shared by Copy DDL, the round-trip gate and the apply
    /// path, for the same reason [`crate::ddl::view_ddl`] is one.
    ///
    /// A disabled PostgreSQL trigger emits its `ALTER TABLE … DISABLE TRIGGER`
    /// too: `CREATE TRIGGER` always produces an enabled one, so a plan that
    /// stopped at the create would quietly switch it back on.
    pub fn create_sql(&self, dialect: crate::intel::SqlDialect) -> String {
        let pg = dialect == crate::intel::SqlDialect::Postgres;
        // SQLite's shape is neither of the other two: it has PostgreSQL's
        // `UPDATE OF` and `WHEN` but MySQL's inline body, so it is asked for by
        // name rather than reached by falling off the end of a `!pg`.
        let sqlite = dialect == crate::intel::SqlDialect::Sqlite;
        let q = |s: &str| ddl_ident_in(s, dialect);
        let qtable = match sql_qualifier(self.schema.as_deref()) {
            Some(s) => format!("{}.{}", q(s), q(&self.table)),
            None => q(&self.table),
        };
        // `UPDATE OF a, b` is part of the event, not a clause after it.
        let events = self
            .events
            .iter()
            .map(|e| {
                if (pg || sqlite) && *e == TriggerEvent::Update && !self.update_columns.is_empty() {
                    let cols = self
                        .update_columns
                        .iter()
                        .map(|c| q(c))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("UPDATE OF {cols}")
                } else {
                    e.sql().to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" OR ");

        if sqlite {
            // `CREATE TRIGGER name timing event ON table [FOR EACH ROW]
            //  [WHEN expr] BEGIN … END`. No definer, no ordering clause and no
            // session state to restate — SQLite has none of them — and the level
            // is always row: `FOR EACH STATEMENT` is a syntax error there.
            let mut out = format!(
                "CREATE TRIGGER {} {} {} ON {}\nFOR EACH ROW",
                q(&self.name),
                self.timing.sql(),
                events,
                qtable,
            );
            if let Some(w) = self
                .condition
                .as_deref()
                .map(str::trim)
                .filter(|w| !w.is_empty())
            {
                // Wrapped exactly once, the guard being held bare in the model —
                // and closed on a line of its own, since a guard ending in a
                // line comment would otherwise swallow the `)`.
                out.push_str(&when_group(w));
            }
            let body = match &self.action {
                TriggerAction::Body(b) => b.trim().to_string(),
                // Symmetric to the other two branches: say so rather than emit a
                // statement that looks fine and isn't.
                // Comment-safed: the name is server-supplied and this is a `--`
                // line in a file that gets run. See `TableInfo::create_ddl`.
                TriggerAction::Function { name, .. } => {
                    let name = crate::export::comment_text(name);
                    format!("-- Schemaic can't call the function {name} from a SQLite trigger.")
                }
            };
            out.push('\n');
            out.push_str(&body);
            out.push(';');
            return out;
        }

        if !pg {
            let mut out = String::from("CREATE ");
            if let Some(d) = &self.definer {
                out.push_str(&definer_sql(d));
                out.push(' ');
            }
            out.push_str(&format!(
                "TRIGGER {} {} {} ON {}\nFOR EACH ROW",
                q(&self.name),
                self.timing.sql(),
                events,
                qtable,
            ));
            // MySQL puts the ordering between FOR EACH ROW and the body.
            match &self.order {
                Some(TriggerOrder::Follows(n)) => out.push_str(&format!(" FOLLOWS {}", q(n))),
                Some(TriggerOrder::Precedes(n)) => out.push_str(&format!(" PRECEDES {}", q(n))),
                None => {}
            }
            let body = match &self.action {
                TriggerAction::Body(b) => b.trim().to_string(),
                // A PG-shaped action on MySQL can't be spelled; say so rather
                // than emit a statement that looks fine and isn't.
                // Comment-safed, for the same reason as the SQLite arm above.
                TriggerAction::Function { name, .. } => {
                    let name = crate::export::comment_text(name);
                    format!("-- Schemaic can't call the function {name} from a MySQL trigger.")
                }
            };
            out.push('\n');
            out.push_str(&body);
            out.push(';');
            return out;
        }

        let mut out = String::from("CREATE ");
        if self.constraint {
            out.push_str("CONSTRAINT ");
        }
        out.push_str(&format!(
            "TRIGGER {} {} {} ON {}",
            q(&self.name),
            self.timing.sql(),
            events,
            qtable,
        ));
        // `REFERENCING` sits between the table and the level, per PostgreSQL's
        // grammar — and a trigger that had one and is recreated without it
        // leaves every write to the table failing.
        let transitions = [
            ("OLD TABLE AS", &self.old_table),
            ("NEW TABLE AS", &self.new_table),
        ]
        .into_iter()
        .filter_map(|(kw, name)| name.as_deref().map(|n| format!("{kw} {}", q(n))))
        .collect::<Vec<_>>();
        if !transitions.is_empty() {
            out.push_str(&format!("\nREFERENCING {}", transitions.join(" ")));
        }
        out.push('\n');
        out.push_str(self.level.sql());
        if let Some(w) = self
            .condition
            .as_deref()
            .map(str::trim)
            .filter(|w| !w.is_empty())
        {
            out.push_str(&when_group(w));
        }
        let call = match &self.action {
            TriggerAction::Function { name, args } => {
                let args = args
                    .iter()
                    .map(|a| ddl_string(a, dialect))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{name}({args})")
            }
            // Symmetric to the MySQL branch: PostgreSQL has nowhere to put a body.
            //
            // **Comment-safed, not merely quoted** — the fifth site of the
            // family, and the only one that interpolated a server-supplied name
            // with no treatment at all. A trigger named `t1` + newline +
            // `DROP DATABASE prod; --` turns the second line of this note into a
            // top-level statement in whatever script carries it, and
            // `Db::run_script`'s guard deliberately never reads the file. Same
            // rule as the two arms above and `create_ddl`'s: nothing
            // server-supplied reaches a `--` line without `export::comment_text`.
            TriggerAction::Body(_) => {
                return format!(
                    "-- Schemaic can't emit a PostgreSQL trigger without a function to call.\n\
                     -- Trigger {} on {} has an inline body, which PostgreSQL has no place for.",
                    crate::export::comment_text(&self.name),
                    crate::export::comment_text(&qtable),
                );
            }
        };
        out.push_str(&format!("\nEXECUTE FUNCTION {call};"));
        if let Some(clause) = self.enabled.alter_clause() {
            out.push_str(&format!(
                "\nALTER TABLE {qtable} {clause} {};",
                q(&self.name)
            ));
        }
        out
    }
}

// ── Routines (stored functions and procedures) ──────────────────────────────

/// Which kind of stored routine this is.
///
/// A separate tag rather than "does it have a return type", because the two are
/// different objects to every statement that addresses one: `DROP FUNCTION` will
/// not drop a procedure, `COMMENT ON PROCEDURE` is required for one, and MySQL's
/// `CREATE PROCEDURE` has no `RETURNS` clause at all. PostgreSQL grew procedures
/// in 11 (`pg_proc.prokind = 'p'`); MySQL has had both since 5.0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RoutineKind {
    #[default]
    Function,
    Procedure,
}

impl RoutineKind {
    /// The keyword that addresses it in `CREATE`/`ALTER`/`DROP`/`COMMENT ON`.
    pub fn sql_keyword(self) -> &'static str {
        match self {
            RoutineKind::Function => "FUNCTION",
            RoutineKind::Procedure => "PROCEDURE",
        }
    }

    /// What it is called in a sentence a person reads.
    pub fn label(self) -> &'static str {
        match self {
            RoutineKind::Function => "function",
            RoutineKind::Procedure => "procedure",
        }
    }

    /// Read a server's spelling — MySQL's `information_schema.ROUTINES.
    /// ROUTINE_TYPE` (`FUNCTION`/`PROCEDURE`) and PostgreSQL's `pg_proc.prokind`
    /// (`f`/`p`), which `schemaic-db` hands over as the same two words.
    ///
    /// An unrecognised answer is [`RoutineKind::Function`], which is what an
    /// aggregate or a window function is closest to — and both of those are
    /// filtered out before they reach here, so this is the honest default rather
    /// than a silent mis-file of something editable.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "procedure" | "p" => RoutineKind::Procedure,
            _ => RoutineKind::Function,
        }
    }
}

/// MySQL's `SQL_DATA_ACCESS` characteristic — what the routine does with data.
///
/// Modelled for the reason [`ViewOptions::definer`] is: MySQL has no statement
/// that alters a routine's body, so **every** edit here is a `DROP` plus a
/// `CREATE`, and anything the new `CREATE` doesn't restate reverts to the
/// server's default. `CONTAINS SQL` is that default, so a `READS SQL DATA`
/// routine recreated without the clause is silently re-declared as something it
/// isn't — which changes what the server will let it do under
/// `--log-bin-trust-function-creators = 0`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SqlDataAccess {
    /// The server's default.
    #[default]
    ContainsSql,
    NoSql,
    ReadsSqlData,
    ModifiesSqlData,
}

impl SqlDataAccess {
    pub fn sql(self) -> &'static str {
        match self {
            SqlDataAccess::ContainsSql => "CONTAINS SQL",
            SqlDataAccess::NoSql => "NO SQL",
            SqlDataAccess::ReadsSqlData => "READS SQL DATA",
            SqlDataAccess::ModifiesSqlData => "MODIFIES SQL DATA",
        }
    }

    /// Read `information_schema.ROUTINES.SQL_DATA_ACCESS`, which spells the
    /// four with an underscore where the clause has a space.
    pub fn parse(s: &str) -> Self {
        match s
            .trim()
            .to_ascii_uppercase()
            .replace(['_', ' '], "")
            .as_str()
        {
            "NOSQL" => SqlDataAccess::NoSql,
            "READSSQLDATA" => SqlDataAccess::ReadsSqlData,
            "MODIFIESSQLDATA" => SqlDataAccess::ModifiesSqlData,
            _ => SqlDataAccess::ContainsSql,
        }
    }
}

/// How often PostgreSQL may assume a function returns the same answer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Volatility {
    #[default]
    Volatile,
    Stable,
    Immutable,
}

impl Volatility {
    pub fn sql(self) -> &'static str {
        match self {
            Volatility::Volatile => "VOLATILE",
            Volatility::Stable => "STABLE",
            Volatility::Immutable => "IMMUTABLE",
        }
    }

    /// From `pg_proc.provolatile`, which is a single char.
    pub fn parse_code(c: &str) -> Volatility {
        match c.trim() {
            "i" => Volatility::Immutable,
            "s" => Volatility::Stable,
            _ => Volatility::Volatile,
        }
    }
}

/// Whether PostgreSQL may run a function inside a parallel plan.
///
/// Modelled as an enum for the reason [`Volatility`] is: the server stores a
/// single char and there are exactly three answers, so a `String` would let a
/// fourth through to the emitter and into a `CREATE` the server then refuses.
///
/// **`Unsafe` is the default**, and it is the *restrictive* one — a function
/// that says nothing is one no parallel plan may use. That is why the loss this
/// models was silent: a `PARALLEL SAFE` function dropped back to `UNSAFE` on
/// every redefinition, and nothing failed, queries just stopped parallelising.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Parallel {
    #[default]
    Unsafe,
    Restricted,
    Safe,
}

impl Parallel {
    pub fn sql(self) -> &'static str {
        match self {
            Parallel::Unsafe => "PARALLEL UNSAFE",
            Parallel::Restricted => "PARALLEL RESTRICTED",
            Parallel::Safe => "PARALLEL SAFE",
        }
    }

    /// From `pg_proc.proparallel`, which is a single char.
    pub fn parse_code(c: &str) -> Parallel {
        match c.trim() {
            "s" => Parallel::Safe,
            "r" => Parallel::Restricted,
            _ => Parallel::Unsafe,
        }
    }
}

/// A stored routine — a function or a procedure.
///
/// Modelled first because a PostgreSQL trigger holds no body of its own — it is
/// a binding to one of these — so triggers there were only half a feature
/// without it, and widened afterwards to the routines a database has for their
/// own sake. The fields past `body` exist for the reason [`ViewOptions`]'s
/// security type does: **a redefinition replaces the whole routine**, so
/// anything the statement doesn't restate reverts to the server's default. That
/// is true of PostgreSQL's `CREATE OR REPLACE FUNCTION` and doubly true of
/// MySQL, which has no replace at all and reaches every edit through a `DROP`
/// plus a `CREATE` ([`crate::ddl::supports_or_replace_routine`]).
///
/// `settings` is the sharpest of those. A `SECURITY DEFINER` function runs with
/// its owner's rights, and the `SET search_path` pinned to it is what stops a
/// caller from resolving an unqualified name inside the body to a table of their
/// own. A replace that drops the `SET` leaves the function running as its owner
/// with the caller's `search_path` — a privilege-escalation hole opened by an
/// edit that said nothing about privileges.
///
/// Most fields belong to one engine, as [`ViewOptions`]'s do: `volatility`,
/// `strict` and `settings` are PostgreSQL's, `deterministic`, `data_access`,
/// `definer`, `comment` and the three session values are MySQL's. The emitter
/// asks the dialect and writes only the clauses that engine has, so a field the
/// other server never reported stays at its default and says nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoutineInfo {
    pub name: String,
    /// PostgreSQL namespace. `None` means unqualified — see [`sql_qualifier`].
    /// Always `None` on MySQL, where a database *is* the namespace.
    pub schema: Option<String>,
    /// Function or procedure. See [`RoutineKind`] for why it is a tag rather
    /// than "is `returns` empty".
    pub kind: RoutineKind,
    /// The parameter list as the server renders it —
    /// `pg_get_function_arguments` on PostgreSQL, and the `IN a INT, OUT b
    /// TEXT` form rebuilt from `information_schema.PARAMETERS` on MySQL. Empty
    /// for a trigger function, which receives its arguments through `TG_ARGV`
    /// instead.
    pub arguments: String,
    /// **PostgreSQL.** The same list in the form that *identifies* the routine —
    /// `pg_get_function_identity_arguments`. Empty on MySQL, whose identity is
    /// the bare name.
    ///
    /// A second field rather than a reformatting of `arguments`, because the two
    /// strings genuinely differ and the difference is a syntax error either way
    /// round: `CREATE FUNCTION` needs the defaults (`b boolean DEFAULT false`),
    /// while `DROP`/`ALTER … RENAME` take `[argmode] [argname] argtype` and
    /// answer `syntax error at or near "DEFAULT"` to anything more. Neither form
    /// can be derived from the other without re-parsing the expression.
    pub identity_arguments: String,
    /// The return type as `pg_get_function_result` (or MySQL's `DTD_IDENTIFIER`)
    /// renders it — `trigger` for the ones a trigger can bind to. Empty for a
    /// procedure, which has no `RETURNS` clause on either engine.
    pub returns: String,
    /// `plpgsql`, `sql`, `c`, … MySQL reports `SQL` for everything it stores.
    pub language: String,
    pub body: String,
    /// **PostgreSQL.** How often the planner may assume the same answer.
    pub volatility: Volatility,
    /// **PostgreSQL.** `RETURNS NULL ON NULL INPUT`. A replace that omits it
    /// makes the function start running on NULL arguments it used to
    /// short-circuit.
    pub strict: bool,
    /// PostgreSQL's `SECURITY DEFINER` and MySQL's `SQL SECURITY DEFINER` — the
    /// same question, and the reason the two share a field.
    ///
    /// **Their defaults are opposite**, which is why the MySQL arm of
    /// [`RoutineInfo::create_sql`] states the clause either way instead of
    /// leaving the default unwritten as the PostgreSQL arm does: omitting it
    /// there does not mean `INVOKER`, it means `DEFINER`.
    pub security_definer: bool,
    /// **PostgreSQL.** Whether a parallel plan may use this function. See
    /// [`Parallel`]: the default is the restrictive answer, so a redefinition
    /// that says nothing takes the function *out* of every parallel plan.
    pub parallel: Parallel,
    /// **PostgreSQL.** `LEAKPROOF` — the promise that the function reveals
    /// nothing about its arguments beyond its return value, which is what lets
    /// the planner push it below a security barrier or a row-level-security
    /// policy. Losing it does not break a query; it makes some queries slower
    /// and is a privilege the owner had to be superuser to grant, so it is not
    /// something an edit should quietly hand back.
    pub leakproof: bool,
    /// **PostgreSQL.** `COST`, as the server renders it (`pg_proc.procost` is a
    /// `real`).
    ///
    /// **A string, not an `f32`**, so [`RoutineInfo`] keeps its `Eq` — the diff
    /// compares whole routines — and so the number goes back exactly as it came:
    /// re-printing a parsed float is how `0.5` becomes `0.5000000074505806`.
    /// `None` means unknown (every MySQL routine, and any draft assembled by
    /// hand); the emitter writes nothing for it, and nothing for a value that
    /// equals the language's default either — see [`RoutineInfo::pg_default_cost`].
    pub cost: Option<String>,
    /// **PostgreSQL.** `ROWS`, the estimated result size of a **set-returning**
    /// function, as the server renders it.
    ///
    /// `None` on anything that returns one row, where the clause is not merely
    /// unnecessary but refused: `ERROR: ROWS is not applicable when function
    /// does not return a set`. The read side sets it only when `proretset`, so
    /// the emitter never has to guess from the return type's spelling.
    pub rows: Option<String>,
    /// **PostgreSQL.** Per-function `SET` clauses, already rendered as
    /// `key=value`.
    pub settings: Vec<String>,
    /// **MySQL.** `DETERMINISTIC` / `NOT DETERMINISTIC`. Not cosmetic: a
    /// non-deterministic routine is refused outright on a server with binary
    /// logging and `log_bin_trust_function_creators = 0`, so a recreate that
    /// declared the wrong one either fails or quietly relaxes the promise the
    /// replication setup relies on.
    pub deterministic: bool,
    /// **MySQL.** See [`SqlDataAccess`].
    pub data_access: SqlDataAccess,
    /// **MySQL.** `DEFINER`, as the catalogue reports it — `root@localhost`,
    /// unquoted. Modelled for the reason [`ViewOptions::definer`] is: with
    /// `SQL SECURITY DEFINER` (the default here) the account in this clause is
    /// whose rights the body runs with, and a recreate that dropped it would
    /// silently hand the routine to whoever applied the edit.
    pub definer: Option<String>,
    /// **MySQL.** The `COMMENT` clause. A recreate that omits it loses it.
    pub comment: Option<String>,
    /// **MySQL.** The session state the routine was created under, from
    /// `SHOW CREATE`. `None` means "not known" — an unfetched routine and every
    /// PostgreSQL one both are — and nothing is emitted for it. Carried and
    /// restored around the `CREATE` for the reason [`TriggerSource`] gives.
    pub sql_mode: Option<String>,
    pub charset_client: Option<String>,
    pub collation_connection: Option<String>,
    /// **MariaDB.** `CREATE AGGREGATE FUNCTION` — a function called once per
    /// group, whose body drives the rows with `FETCH GROUP NEXT ROW`.
    ///
    /// **Known only from `SHOW CREATE`.** MariaDB's
    /// `information_schema.ROUTINES` has no column that distinguishes one
    /// (verified live: no column of that table names it), so the eager read
    /// that fills the schema tree always leaves this `false` and the lazy
    /// [`RoutineSource`] fetch is what sets it. Dropping the keyword destroyed
    /// the function — the recreate answered `ERROR 4105` after its `DROP` had
    /// committed — which is why it is modelled rather than inferred.
    ///
    /// Not user-editable: it rides with the session state in
    /// [`RoutineSource::apply_session_to`], not with the body.
    pub aggregate: bool,
}

impl RoutineInfo {
    /// Whether a trigger can bind to this function.
    pub fn is_trigger_function(&self) -> bool {
        self.kind == RoutineKind::Function
            && (self.returns.trim().eq_ignore_ascii_case("trigger")
                || self.returns.trim().eq_ignore_ascii_case("event_trigger"))
    }

    /// Whether this routine is one Schemaic can **edit** — the entry point's
    /// gate, the call [`crate::ddl::supports_view_editing`] makes for a
    /// materialized view and `is_editable_trigger` for a constraint trigger.
    ///
    /// The emitter writes the body as the routine's *source*: dollar-quoted on
    /// PostgreSQL, verbatim on MySQL. That is right for every language whose
    /// body really is source text — `sql`, `plpgsql`, `plpython3u`, `plperl` —
    /// and wrong for the two where it isn't. A `LANGUAGE c` function's `prosrc`
    /// is a **link symbol**, and recreating it needs
    /// `AS 'obj_file', 'link_symbol'`, a form this emitter has no arm for; a
    /// `LANGUAGE internal` one names a built-in. Re-emitting either as
    /// `AS $$symbol$$` produces a routine that is not the one that was there.
    ///
    /// Such a routine is still **listed and droppable**, exactly as an identity
    /// column's sequence is: the tree row is context, and hiding it would be a
    /// worse lie than showing it with Edit greyed out.
    pub fn is_editable(&self) -> bool {
        !matches!(
            self.language.trim().to_ascii_lowercase().as_str(),
            "c" | "internal"
        )
    }

    /// The routine's identity in SQL, as `DROP`/`ALTER`/`COMMENT ON` need it.
    ///
    /// **PostgreSQL identifies a routine by its argument types**, not its name —
    /// overloads share one name — so all three statements need the parameter
    /// list there and none of them accept the bare name. MySQL is the other way
    /// round: a name is unique within a database and `DROP PROCEDURE p(...)` is
    /// a syntax error, so the list must *not* be written.
    ///
    /// An exhaustive `match` rather than a `== Postgres`, so a fourth engine has
    /// to answer for itself instead of inheriting whichever side it fell on.
    ///
    /// The list is [`RoutineInfo::identity_arguments`], **not** `arguments`:
    /// only the identity form is grammatical here, and a routine with a
    /// defaulted parameter could not be dropped or renamed from the app at all
    /// while this spliced in the `CREATE` form. It falls back to `arguments` for
    /// a routine assembled by hand rather than read from a catalogue — a draft's
    /// own, where the two are the same string.
    pub fn signature_sql(&self, dialect: crate::intel::SqlDialect) -> String {
        let name = qualified_ident(&self.name, self.schema.as_deref(), dialect);
        match dialect {
            crate::intel::SqlDialect::Postgres => {
                let args = match self.identity_arguments.trim() {
                    "" => self.arguments.trim(),
                    ident => ident,
                };
                format!("{name}({args})")
            }
            crate::intel::SqlDialect::MySql | crate::intel::SqlDialect::Sqlite => name,
        }
    }

    /// The parameter list to show **beside a name in prose**, or empty where the
    /// engine doesn't identify a routine by it.
    ///
    /// Unquoted and unqualified, unlike [`RoutineInfo::signature_sql`]: this
    /// goes in a modal title, not in SQL. It exists because a remembered
    /// Find-Anywhere hit carries only `(namespace, kind, name)` and resolves a
    /// PostgreSQL overload to the *first* of them — so the editor could open on
    /// `add(text, text)` when `add(integer, integer)` was searched for, with
    /// nothing on screen distinguishing the two.
    pub fn identity_suffix(&self, dialect: crate::intel::SqlDialect) -> String {
        match dialect {
            crate::intel::SqlDialect::Postgres => {
                let args = match self.identity_arguments.trim() {
                    "" => self.arguments.trim(),
                    ident => ident,
                };
                format!("({args})")
            }
            crate::intel::SqlDialect::MySql | crate::intel::SqlDialect::Sqlite => String::new(),
        }
    }

    /// `CREATE [OR REPLACE] {FUNCTION|PROCEDURE}`, with every option restated —
    /// the single routine emitter, on the same rule as [`crate::ddl::view_ddl`].
    ///
    /// `replace` is honoured only where the engine has the form. MySQL has no
    /// `CREATE OR REPLACE` for a routine, so the flag is ignored there and
    /// [`crate::ddl::diff_routine`] emits the `DROP` that has to come first
    /// instead — the same resolution [`diff_view`] performs for SQLite.
    ///
    /// [`diff_view`]: crate::ddl::diff_view
    pub fn create_sql(&self, dialect: crate::intel::SqlDialect, replace: bool) -> String {
        match dialect {
            crate::intel::SqlDialect::Postgres => self.pg_create_sql(replace),
            // SQLite has no stored routines at all; it never reaches here
            // (`supports_routine_editing` is false for it), and MySQL's shape is
            // the closer of the two if it somehow does.
            crate::intel::SqlDialect::MySql | crate::intel::SqlDialect::Sqlite => {
                self.mysql_create_sql(dialect)
            }
        }
    }

    /// What PostgreSQL assumes a function costs when the `CREATE` says nothing:
    /// 1 for a C or internal function, 100 for every other language.
    ///
    /// Public because it is the *reason* a `COST` clause is or is not emitted,
    /// and a test that hard-coded 100 would agree with the emitter about the one
    /// case they are both wrong on.
    pub fn pg_default_cost(&self) -> f64 {
        match self.language.trim().to_ascii_lowercase().as_str() {
            "c" | "internal" => 1.0,
            _ => 100.0,
        }
    }

    /// `COST n`, or nothing when the routine is at its language's default (or
    /// the value was never read).
    fn pg_cost_clause(&self) -> Option<String> {
        let raw = self
            .cost
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())?;
        // An unparsable value is still restated rather than dropped: this
        // module's job is not to lose what the server said, and a bad number
        // fails loudly at the server instead of silently here.
        match raw.parse::<f64>() {
            Ok(v) if (v - self.pg_default_cost()).abs() < f64::EPSILON => None,
            _ => Some(format!("COST {raw}")),
        }
    }

    /// `ROWS n`, or nothing for a function that returns one row or is at the
    /// server's 1000-row default.
    fn pg_rows_clause(&self) -> Option<String> {
        let raw = self
            .rows
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())?;
        match raw.parse::<f64>() {
            Ok(v) if (v - 1000.0).abs() < f64::EPSILON => None,
            _ => Some(format!("ROWS {raw}")),
        }
    }

    fn pg_create_sql(&self, replace: bool) -> String {
        let d = crate::intel::SqlDialect::Postgres;
        let tag = dollar_tag(&self.body);
        let mut out = String::from("CREATE ");
        if replace {
            out.push_str("OR REPLACE ");
        }
        // **`arguments`, not `signature_sql`** — the two differ here. A `CREATE`
        // wants the declaration form, defaults and all; `signature_sql` answers
        // the *identity* form, which omits them because `DROP`/`ALTER` have no
        // grammar for them.
        out.push_str(&format!(
            "{} {}({})\n",
            self.kind.sql_keyword(),
            qualified_ident(&self.name, self.schema.as_deref(), d),
            self.arguments.trim()
        ));
        // **`CREATE PROCEDURE` takes a strict subset of a function's
        // attributes**, and the ones it rejects are rejected outright:
        // PostgreSQL's grammar allows only `LANGUAGE`, `TRANSFORM`, `SECURITY`,
        // `SET` and `AS` on one, and anything else is
        // `ERROR: invalid attribute in procedure definition`. So the return
        // type, the volatility and the strictness are all function-only here —
        // this guarded `RETURNS` alone at first, which left an edit that merely
        // *touched* a procedure's Volatility control emitting a statement the
        // server would refuse.
        let is_function = self.kind == RoutineKind::Function;
        if is_function {
            out.push_str(&format!("RETURNS {}\n", self.returns.trim()));
        }
        out.push_str(&format!("LANGUAGE {}\n", self.language.trim()));
        // VOLATILE is the default and says nothing, so it isn't restated — the
        // same call `create_view_sql` makes about `ALGORITHM = UNDEFINED`.
        if is_function && self.volatility != Volatility::Volatile {
            out.push_str(self.volatility.sql());
            out.push('\n');
        }
        if is_function && self.leakproof {
            out.push_str("LEAKPROOF\n");
        }
        if is_function && self.strict {
            out.push_str("STRICT\n");
        }
        if self.security_definer {
            out.push_str("SECURITY DEFINER\n");
        }
        // **The three the redefinition used to drop.** Each has a default the
        // server falls back to, and each default is the one that costs
        // something: `PARALLEL UNSAFE` takes the function out of every parallel
        // plan, `COST 100` is 20x off a function declared at 5, and a lost
        // `ROWS` sends the planner a 1000-row estimate for a function that
        // returns three. Nothing failed, which is why it went unnoticed.
        //
        // Restated only where it differs from that default, the way
        // `SequenceInfo::clauses` does — a `CREATE` that spells out every
        // server default is noise in the preview and in Copy DDL.
        if is_function && self.parallel != Parallel::Unsafe {
            out.push_str(self.parallel.sql());
            out.push('\n');
        }
        if is_function && let Some(c) = self.pg_cost_clause() {
            out.push_str(&c);
            out.push('\n');
        }
        if is_function && let Some(r) = self.pg_rows_clause() {
            out.push_str(&r);
            out.push('\n');
        }
        for s in &self.settings {
            out.push_str(&format!("SET {s}\n"));
        }
        out.push_str(&format!(
            "AS {tag}\n{}\n{tag};",
            self.body.trim_matches('\n')
        ));
        out
    }

    /// MySQL's shape: `DEFINER`, the name and parameters, `RETURNS`, then the
    /// characteristics. The first three are positional and the characteristics
    /// are not — MySQL accepts those in any order — so their sequence here is
    /// the order `SHOW CREATE` prints them in, chosen so a re-emitted routine
    /// reads like the one the server would hand back.
    ///
    /// **No trailing semicolon.** A routine body is a compound statement whose
    /// own statements end in `;`, so what is emitted is one statement that
    /// happens to contain several. `Db::run_ddl` sends each plan step whole, on
    /// one connection, which is why no `DELIMITER` dance is needed — that is a
    /// client-side convention of the `mysql` CLI, not part of the protocol.
    fn mysql_create_sql(&self, d: crate::intel::SqlDialect) -> String {
        let mut out = String::from("CREATE ");
        if let Some(def) = self.definer.as_deref().filter(|s| !s.trim().is_empty()) {
            out.push_str(&definer_sql(def));
            out.push(' ');
        }
        // MariaDB's, and it appears only where the server printed it — see
        // [`RoutineInfo::aggregate`]. Losing it is `ERROR 4105` after the
        // recreate's `DROP` has already committed.
        if self.aggregate {
            out.push_str("AGGREGATE ");
        }
        out.push_str(&format!(
            "{} {}({})\n",
            self.kind.sql_keyword(),
            qualified_ident(&self.name, self.schema.as_deref(), d),
            self.arguments.trim()
        ));
        if self.kind == RoutineKind::Function && !self.returns.trim().is_empty() {
            out.push_str(&format!("RETURNS {}\n", self.returns.trim()));
        }
        if self.deterministic {
            out.push_str("DETERMINISTIC\n");
        } else {
            // Stated rather than left to the default for the same reason the
            // security clause below is: a stored function on a binary-logging
            // server is judged on this word, and the two engines' silence means
            // different things.
            out.push_str("NOT DETERMINISTIC\n");
        }
        out.push_str(self.data_access.sql());
        out.push('\n');
        out.push_str(if self.security_definer {
            "SQL SECURITY DEFINER\n"
        } else {
            "SQL SECURITY INVOKER\n"
        });
        if let Some(c) = self.comment.as_deref().filter(|s| !s.is_empty()) {
            out.push_str(&format!("COMMENT {}\n", ddl_string(c, d)));
        }
        out.push_str(self.body.trim_matches('\n'));
        out
    }
}

/// What one `SHOW CREATE {PROCEDURE|FUNCTION}` round trip yields for a MySQL
/// routine: the body **as written**, and the session state it was written under.
///
/// It exists for the reason [`TriggerSource`] does, and against the same column
/// family: `information_schema.ROUTINES.ROUTINE_DEFINITION` returns the body
/// with its escapes already resolved on MySQL 8, so a routine holding `'it''s'`
/// comes back as `'it's'` — a syntax error on restate, *after* the `DROP` this
/// engine's every edit begins with has committed and taken the only copy with
/// it. The same statement is also the only place the three session values live.
///
/// Fetched **lazily**, when the editor opens. The eager
/// `information_schema.ROUTINES` read that fills the schema tree keeps the
/// resolved body, which is fine for reading and for `Generate DDL` and is not
/// what an edit is emitted from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoutineSource {
    /// The body as written, or `None` when the `CREATE` text couldn't be
    /// understood (or the account may not read it).
    ///
    /// **Optional so a body this can't parse doesn't take the session state
    /// with it.** The three values below come from their own columns of the
    /// same row and are always trustworthy; folding them into the body's
    /// success meant a routine with an unfamiliar header was later recreated
    /// under whatever `sql_mode` the applying session happened to have — the
    /// silent re-filing [`crate::ddl`]'s session wrapper exists to prevent.
    pub body: Option<String>,
    pub sql_mode: Option<String>,
    pub charset_client: Option<String>,
    pub collation_connection: Option<String>,
    /// **MariaDB.** The `AGGREGATE` keyword, read off this statement's header —
    /// the only place it exists. See [`RoutineInfo::aggregate`].
    pub aggregate: bool,
}

impl RoutineSource {
    /// Copy this onto a [`RoutineInfo`] — the body **and** the session state.
    ///
    /// One method because both sides of the diff have to be patched with it
    /// (`current` and the draft), exactly as [`TriggerSource::apply_to`] does:
    /// patching only the draft would make every MySQL routine open
    /// already-changed against a `current` that still held the resolved body.
    ///
    /// A `None` body leaves whatever was there — which is `information_schema`'s
    /// copy, the state every build before this shipped, and better than blanking
    /// the field.
    pub fn apply_to(&self, r: &mut RoutineInfo) {
        self.apply_body_to(r);
        self.apply_session_to(r);
    }

    /// The body alone.
    ///
    /// Split from the session state because the two answer different questions,
    /// and the caller has to gate them differently: the body is the user's to
    /// overwrite and must not be patched over an edit they have already made,
    /// while the session state is not editable anywhere in the app and is only
    /// ever *more* correct than what was there.
    pub fn apply_body_to(&self, r: &mut RoutineInfo) {
        if let Some(body) = &self.body {
            r.body = body.clone();
        }
    }

    /// The session state alone, and **only where this read has a value**.
    ///
    /// MySQL's `information_schema.ROUTINES` carries the same three columns
    /// `SHOW CREATE` prints, so a routine arrives with them already set; a
    /// `SHOW CREATE` that came back short must not blank them and leave the
    /// recreate unwrapped. Applied unconditionally — with the body-equality
    /// guard on the body alone — because a keystroke landing before this reply
    /// is no reason to strip the wrapper off a `CREATE` whose `DROP` has
    /// already committed.
    pub fn apply_session_to(&self, r: &mut RoutineInfo) {
        if self.sql_mode.is_some() {
            r.sql_mode = self.sql_mode.clone();
        }
        if self.charset_client.is_some() {
            r.charset_client = self.charset_client.clone();
        }
        if self.collation_connection.is_some() {
            r.collation_connection = self.collation_connection.clone();
        }
        // **Only ever raised here, never lowered.** This read is the sole source
        // of the flag ([`RoutineInfo::aggregate`]), so `false` means "this
        // statement didn't say so", which for a failed or unparsed `SHOW CREATE`
        // is not the same as "it isn't one" — and clearing a flag that is
        // already true would put the `ERROR 4105` back.
        if self.aggregate {
            r.aggregate = true;
        }
    }
}

/// A dollar-quote delimiter that cannot appear inside `body`.
///
/// A function body is arbitrary user text and is quoted by wrapping, so the
/// delimiter has to be one the body doesn't contain — otherwise the statement
/// terminates in the middle of the body and the rest is parsed as SQL. `$$` is
/// the common case; a body that already uses it (a nested function definition,
/// or `$$` inside a string) walks up through tagged forms until one is free.
///
/// Deliberately not "escape the body": PostgreSQL has no escape inside a
/// dollar-quoted string, which is the entire point of the construct.
pub fn dollar_tag(body: &str) -> String {
    if !body.contains("$$") {
        return "$$".to_string();
    }
    for tag in ["$fn$", "$body$", "$function$"] {
        if !body.contains(tag) {
            return tag.to_string();
        }
    }
    // Numbered fallback. A body can only contain finitely many tags, so this
    // terminates; the loop is bounded anyway so a pathological body degrades to
    // a wrong quote rather than a hang.
    for i in 1..1000 {
        let tag = format!("$fn{i}$");
        if !body.contains(&tag) {
            return tag;
        }
    }
    "$schemaic$".to_string()
}

// ── Scheduled events (MySQL) ────────────────────────────────────────────────

/// Whether a scheduled event fires once or on a repeating interval.
///
/// Two shapes rather than a nullable everything, because MySQL's grammar has
/// two: `ON SCHEDULE AT …` takes one timestamp and nothing else, and
/// `ON SCHEDULE EVERY …` takes an interval and optional bounds. A single struct
/// holding all four would let a draft describe `AT` *and* `ENDS`, which is a
/// syntax error the form would then have had to refuse separately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventSchedule {
    /// `AT <expr>` — a one-shot. Unless the event also says
    /// `ON COMPLETION PRESERVE`, the server **drops it** once it has run; see
    /// [`EventInfo::preserve`].
    At(String),
    /// `EVERY <value> <unit>`, optionally bounded by `STARTS`/`ENDS`.
    Every {
        /// The quantity, **as SQL** — see [`event_interval_expr`] for why every
        /// timestamp and quantity here is an expression rather than a value.
        /// `6` for `EVERY 6 HOUR`, `'1:30'` for the compound units whose
        /// quantity MySQL takes as a string.
        value: String,
        /// The interval keyword, uppercased — `DAY`, `HOUR`, `HOUR_MINUTE`. A
        /// raw string rather than an enum, on the rule
        /// [`EVENT_INTERVAL_UNITS`] states.
        unit: String,
        starts: Option<String>,
        ends: Option<String>,
    },
}

impl Default for EventSchedule {
    /// `EVERY 1 DAY`. A new event has to start from *something* legal, and a
    /// recurring daily job is what the feature is overwhelmingly used for; the
    /// one-shot form is one control away.
    fn default() -> Self {
        EventSchedule::Every {
            value: "1".to_string(),
            unit: "DAY".to_string(),
            starts: None,
            ends: None,
        }
    }
}

impl EventSchedule {
    /// Is this the one-shot form? What the editor's schedule control reads, and
    /// what [`EventInfo::preserve`]'s warning turns on.
    pub fn is_one_shot(&self) -> bool {
        matches!(self, EventSchedule::At(_))
    }

    /// The `ON SCHEDULE` clause's payload, without the two keywords that
    /// introduce it — `AT '2026-01-01 03:00:00'`, or `EVERY 1 DAY STARTS …`.
    ///
    /// The bounds go on their own lines because a schedule carrying both is the
    /// one that genuinely needs reading before it is applied.
    pub fn sql(&self) -> String {
        match self {
            EventSchedule::At(at) => format!("AT {}", at.trim()),
            EventSchedule::Every {
                value,
                unit,
                starts,
                ends,
            } => {
                let mut out = format!("EVERY {} {}", value.trim(), unit.trim());
                if let Some(s) = starts.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    out.push_str(&format!("\n  STARTS {s}"));
                }
                if let Some(e) = ends.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
                    out.push_str(&format!("\n  ENDS {e}"));
                }
                out
            }
        }
    }
}

/// The interval keywords `EVERY` accepts, **smallest unit first, then the
/// compound ones** — a reading order for a dropdown, not the manual's.
///
/// The set is exactly MySQL's fifteen; the *order* is not. The manual lists them
/// `YEAR | QUARTER | MONTH | DAY | HOUR | MINUTE | WEEK | SECOND | YEAR_MONTH |
/// DAY_HOUR | …`, which puts `WEEK` between `MINUTE` and `SECOND` and is no order
/// to scan a list in. This said "in MySQL's own documented order" and was simply
/// wrong about it — noticed by the `S3.1` review pass, left unraised there for
/// want of a server, and checked since.
///
/// A vocabulary for the editor's dropdown, **not** a parse: the unit stays a
/// `String` on [`EventSchedule::Every`], so a server carrying a keyword this
/// list doesn't know is still browsed, edited and re-emitted with the unit it
/// really has. The same call [`RoutineInfo::language`]'s dropdown makes — offer
/// what we know, and append whatever this object already is.
pub const EVENT_INTERVAL_UNITS: [&str; 15] = [
    "SECOND",
    "MINUTE",
    "HOUR",
    "DAY",
    "WEEK",
    "MONTH",
    "QUARTER",
    "YEAR",
    "MINUTE_SECOND",
    "HOUR_MINUTE",
    "HOUR_SECOND",
    "DAY_MINUTE",
    "DAY_HOUR",
    "DAY_SECOND",
    "YEAR_MONTH",
];

/// Whether the scheduler will run this event, as `information_schema` reports
/// it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EventStatus {
    #[default]
    Enabled,
    Disabled,
    /// `DISABLE ON SLAVE` — set by the server itself on a replica, so the event
    /// runs on the source and the rows it writes arrive through replication
    /// rather than being generated twice.
    ///
    /// Modelled rather than folded into `Disabled`, because the two are
    /// different states and restating the wrong one is a real change: turning a
    /// replica-disabled event into a plainly disabled one is the same edit as
    /// enabling it on the source, applied backwards.
    /// [`crate::ddl::diff_event`] restates the status **only when it changed**,
    /// so an event in this state that nobody touched keeps it.
    SlavesideDisabled,
    /// The same state on **MySQL 8.4**, which renamed both the column value
    /// (`REPLICA_SIDE_DISABLED`) and the keyword (`DISABLE ON REPLICA`) and
    /// removed the old spelling of the latter.
    ///
    /// Two variants for one state, because the *server's own answer is the only
    /// version signal there is here*: an event read as `SLAVESIDE_DISABLED` came
    /// from a server that takes `DISABLE ON SLAVE`, one read as
    /// `REPLICA_SIDE_DISABLED` from a server that rejects it. Folding them
    /// together — which this did — threw that away and left every generated
    /// statement using the pre-8.4 keyword. They are separate variants rather
    /// than one carrying a word so that equality still means "the same state":
    /// the differ compares statuses, and a status that differed from itself in
    /// spelling would emit an `ALTER` for an event nobody edited.
    ReplicaDisabled,
}

impl EventStatus {
    /// Read `information_schema.EVENTS.STATUS`.
    ///
    /// MySQL 8.0.26 renamed the replica state's keyword and 8.4 renamed this
    /// column's value with it, so the two spellings are kept **apart**: which one
    /// the server used is the only thing that says which keyword it will accept
    /// back (see [`EventStatus::ReplicaDisabled`]). An unrecognised answer is
    /// `Enabled`, which is the state the scheduler treats an event as being in.
    pub fn parse(s: &str) -> EventStatus {
        match s.trim().to_ascii_uppercase().as_str() {
            "DISABLED" => EventStatus::Disabled,
            "SLAVESIDE_DISABLED" => EventStatus::SlavesideDisabled,
            "REPLICA_SIDE_DISABLED" => EventStatus::ReplicaDisabled,
            _ => EventStatus::Enabled,
        }
    }

    /// The clause that sets this state on `CREATE`/`ALTER EVENT`.
    ///
    /// **The replica state has two spellings and they are not interchangeable:**
    /// `DISABLE ON SLAVE` is what MySQL 8.0 and every MariaDB accept, and MySQL
    /// 8.4 removed it in favour of `DISABLE ON REPLICA`. Which one an event gets
    /// is decided by the word its own server reported, which is why `parse` keeps
    /// them apart — every statement here used the pre-8.4 keyword while it did
    /// not, so `create_sql` handed an 8.4 server a Copy-CREATE it rejects, and a
    /// whole-database script aborted at that event.
    pub fn sql_keyword(self) -> &'static str {
        match self {
            EventStatus::Enabled => "ENABLE",
            EventStatus::Disabled => "DISABLE",
            EventStatus::SlavesideDisabled => "DISABLE ON SLAVE",
            EventStatus::ReplicaDisabled => "DISABLE ON REPLICA",
        }
    }

    /// What the state is called in a sentence a person reads.
    ///
    /// The two replica spellings read the same, deliberately: the *state* is one
    /// thing, and which keyword sets it is the server's business, not the
    /// reader's. (Nothing shows both at once — the editor's list carries whichever
    /// one the event is in.)
    pub fn label(self) -> &'static str {
        match self {
            EventStatus::Enabled => "Enabled",
            EventStatus::Disabled => "Disabled",
            EventStatus::SlavesideDisabled | EventStatus::ReplicaDisabled => "Disabled on replica",
        }
    }
}

/// A scheduled event — MySQL's `CREATE EVENT`, a statement the server runs on a
/// clock.
///
/// **MySQL and MariaDB have these and no other engine here does.** PostgreSQL's
/// nearest equivalent is `pg_cron`, an extension with its own catalogue and no
/// `CREATE EVENT` grammar, and SQLite has no scheduler at all — so this is the
/// one object modelled for a single engine's sake, and
/// [`crate::ddl::supports_event_editing`] is what every surface asks before
/// offering it.
///
/// The fields are what `CREATE EVENT` states, with one addition: `time_zone`.
/// An event's schedule is interpreted in the time zone the session held when it
/// was created, the server records it, and the statement has **no clause** for
/// it — so it rides in the session wrapper the body's `sql_mode` already needs,
/// exactly as [`RoutineInfo`]'s three session values do. Dropping it moves every
/// future firing of a nightly job by the offset between the creator's zone and
/// the applier's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventInfo {
    pub name: String,
    /// Always `None`: MySQL is the only engine with events, and there a database
    /// *is* the namespace. Carried so an event is the same shape as every other
    /// [`ObjectItem`], whose tree folder, search predicate and palette row all
    /// ask for one.
    pub schema: Option<String>,
    /// `DEFINER`, as the catalogue reports it — `root@localhost`, unquoted.
    ///
    /// Load-bearing for the reason [`RoutineInfo::definer`] is, and more so: an
    /// event has no caller, so the definer's rights are the **only** rights its
    /// body ever runs with.
    pub definer: Option<String>,
    pub schedule: EventSchedule,
    /// `ON COMPLETION PRESERVE`. False — MySQL's default — means the server
    /// **drops the event** once its last firing is past, which for a one-shot is
    /// after the first one.
    pub preserve: bool,
    pub status: EventStatus,
    pub comment: Option<String>,
    /// The statement the event runs, as it follows `DO`. One statement, or a
    /// `BEGIN … END` compound.
    ///
    /// **From `information_schema` this is not trustworthy for an edit** — the
    /// same escape-resolving column family [`RoutineSource`] exists for.
    /// [`EventSource`] is the correct source, and is read lazily when the editor
    /// opens.
    pub body: String,
    /// The session `time_zone` the schedule is interpreted in — `SYSTEM`, or an
    /// offset like `+00:00`. See this struct's own note for why it is modelled.
    pub time_zone: Option<String>,
    /// The session state the body was written under, as [`RoutineInfo`] carries
    /// it and for the same reason. `None` means "not known", and nothing is
    /// emitted for it.
    pub sql_mode: Option<String>,
    pub charset_client: Option<String>,
    pub collation_connection: Option<String>,
}

impl EventInfo {
    /// `CREATE EVENT`, with every clause restated — the single event emitter, on
    /// the same rule as [`RoutineInfo::create_sql`].
    ///
    /// **No trailing semicolon**, for the reason the routine emitter gives: the
    /// body is a statement (or a compound of several, each ending in `;`) and
    /// this is one statement that contains it.
    /// The `ALTER EVENT` [`crate::ddl::diff_event`] builds follows the same rule
    /// whenever it restates the body.
    ///
    /// The clause order is MySQL's grammar rather than a preference: `ALTER
    /// EVENT` accepts these in exactly one sequence, and writing `CREATE` in the
    /// same order keeps the two readable against each other.
    pub fn create_sql(&self, dialect: crate::intel::SqlDialect) -> String {
        let mut out = String::from("CREATE ");
        if let Some(def) = self.definer.as_deref().filter(|s| !s.trim().is_empty()) {
            out.push_str(&definer_sql(def));
            out.push(' ');
        }
        out.push_str(&format!(
            "EVENT {}\n",
            qualified_ident(&self.name, self.schema.as_deref(), dialect)
        ));
        out.push_str(&format!("ON SCHEDULE {}\n", self.schedule.sql()));
        // Stated either way. MySQL's default is `NOT PRESERVE`, which for a
        // one-shot means the event deletes itself once it has run — silence here
        // is the difference between an event that survives and one that does
        // not, left to whatever the server assumes.
        out.push_str(if self.preserve {
            "ON COMPLETION PRESERVE\n"
        } else {
            "ON COMPLETION NOT PRESERVE\n"
        });
        out.push_str(self.status.sql_keyword());
        out.push('\n');
        if let Some(c) = self.comment.as_deref().filter(|s| !s.is_empty()) {
            out.push_str(&format!("COMMENT {}\n", ddl_string(c, dialect)));
        }
        out.push_str("DO\n");
        out.push_str(self.body.trim_matches('\n'));
        out
    }

    /// The one-line summary the tree shows beside the name: the schedule, and
    /// the fact that it is switched off when it is.
    ///
    /// The schedule is what tells two events apart at a glance; the body is too
    /// long for a row and the definer says nothing about what it does. A
    /// disabled event is called out because it is otherwise indistinguishable
    /// from one that runs, which is the single most confusing thing about
    /// browsing these.
    pub fn detail(&self) -> String {
        let sched = self
            .schedule
            .sql()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        match self.status {
            EventStatus::Enabled => sched,
            s => format!("{sched} — {}", s.label().to_lowercase()),
        }
    }
}

/// What one `SHOW CREATE EVENT` round trip yields: the body **as written**, and
/// the session state it was written under.
///
/// It exists for the reason [`RoutineSource`] does and against the same column
/// family — `information_schema.EVENTS.EVENT_DEFINITION` resolves the body's
/// escapes, so an event holding `'it''s'` comes back as `'it's'`. The
/// consequence is milder here than for a routine, because `ALTER EVENT` edits in
/// place and a body the server refuses leaves the event standing; it is still
/// wrong, and the failure is the user's edit rejected over a quote they never
/// typed.
///
/// Fetched **lazily**, when the editor opens, exactly as [`RoutineSource`] is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventSource {
    /// The body as written, or `None` when the `CREATE` text couldn't be
    /// understood (or the account may not read it).
    ///
    /// Optional so a body this can't parse doesn't take the session state with
    /// it — [`RoutineSource::body`] records the bug that rule comes from.
    pub body: Option<String>,
    pub time_zone: Option<String>,
    pub sql_mode: Option<String>,
    pub charset_client: Option<String>,
    pub collation_connection: Option<String>,
}

impl EventSource {
    /// Copy this onto an [`EventInfo`] — the body **and** the session state.
    ///
    /// One method because both sides of the diff have to be patched with it, as
    /// [`RoutineSource::apply_to`] is: patching only the draft would make every
    /// event open already-changed against a `current` that still held the
    /// resolved body.
    pub fn apply_to(&self, e: &mut EventInfo) {
        self.apply_body_to(e);
        self.apply_session_to(e);
    }

    /// The body alone — the half the user may already have overwritten before
    /// this reply landed.
    pub fn apply_body_to(&self, e: &mut EventInfo) {
        if let Some(body) = &self.body {
            e.body = body.clone();
        }
    }

    /// The session state alone, and **only where this read has a value**: a
    /// `SHOW CREATE` that came back short must not blank what the catalogue read
    /// already filled in.
    pub fn apply_session_to(&self, e: &mut EventInfo) {
        if self.time_zone.is_some() {
            e.time_zone = self.time_zone.clone();
        }
        if self.sql_mode.is_some() {
            e.sql_mode = self.sql_mode.clone();
        }
        if self.charset_client.is_some() {
            e.charset_client = self.charset_client.clone();
        }
        if self.collation_connection.is_some() {
            e.collation_connection = self.collation_connection.clone();
        }
    }
}

/// Quote a catalogue timestamp into the expression [`EventSchedule`] holds.
///
/// `information_schema.EVENTS` reports `EXECUTE_AT`, `STARTS` and `ENDS` as bare
/// datetimes — `2026-01-01 03:00:00` — and the model holds **SQL**, because a
/// schedule field that can only hold a literal cannot express
/// `CURRENT_TIMESTAMP + INTERVAL 1 HOUR`, which is how most events are started.
/// So the reader is what quotes, once, here; everything downstream emits the
/// string verbatim, the same contract a column default already has.
///
/// An empty column is `None`: the schedule simply has no such bound.
pub fn event_time_expr(raw: &str, dialect: crate::intel::SqlDialect) -> Option<String> {
    let raw = raw.trim();
    (!raw.is_empty()).then(|| ddl_string(raw, dialect))
}

/// Quote a catalogue interval quantity into the expression
/// [`EventSchedule::Every`] holds.
///
/// `INTERVAL_VALUE` is a plain number for the single-part units (`EVERY 6
/// HOUR`) and a punctuated string for the compound ones (`EVERY '1:30'
/// HOUR_MINUTE`), and MySQL's grammar wants the second quoted. The test is what
/// the value *is*, not which unit it arrived with: a digits-only quantity stays
/// bare, anything else becomes a literal — so a compound unit is quoted whether
/// or not [`EVENT_INTERVAL_UNITS`] has heard of it.
pub fn event_interval_expr(raw: &str, dialect: crate::intel::SqlDialect) -> String {
    let raw = raw.trim();
    if !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()) {
        return raw.to_string();
    }
    ddl_string(raw, dialect)
}

// ── Standalone objects (PostgreSQL) ─────────────────────────────────────────

/// A user-defined enum type — `CREATE TYPE mood AS ENUM ('sad', 'ok')`.
///
/// PostgreSQL-only as a *type*. MySQL spells `ENUM` as a column type, which is
/// already carried by [`ColumnInfo::type_name`] and has no independent existence
/// to model, so nothing here has a MySQL arm.
///
/// The values are stored in **sort order** (`pg_enum.enumsortorder`), not
/// creation order, because that is the order comparisons and `ORDER BY` use —
/// and it is what `ALTER TYPE … ADD VALUE … BEFORE/AFTER` manipulates. A list in
/// any other order would show one thing and mean another.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnumInfo {
    pub name: String,
    /// PostgreSQL namespace. `None` means unqualified — see [`sql_qualifier`].
    pub schema: Option<String>,
    pub values: Vec<String>,
    pub comment: Option<String>,
}

impl EnumInfo {
    /// `CREATE TYPE … AS ENUM (…)`, plus a `COMMENT ON TYPE` when there is one.
    pub fn create_sql(&self, dialect: crate::intel::SqlDialect) -> String {
        let qname = qualified_ident(&self.name, self.schema.as_deref(), dialect);
        let values = self
            .values
            .iter()
            .map(|v| ddl_string(v, dialect))
            .collect::<Vec<_>>()
            .join(", ");
        let mut out = format!("CREATE TYPE {qname} AS ENUM ({values});");
        if let Some(c) = &self.comment
            && !c.is_empty()
        {
            out.push_str(&format!(
                "\nCOMMENT ON TYPE {qname} IS {};",
                ddl_string(c, dialect)
            ));
        }
        out
    }
}

/// A domain: a base type with a default and constraints attached, reusable as a
/// column type.
///
/// The constraints are [`CheckInfo`]s — the same type a table's are — because
/// they are the same thing: a named predicate the server re-prints from its own
/// parse tree. Sharing it means [`crate::ddl::checks_equal`] governs both, so a
/// retyped-but-equivalent predicate can't produce a phantom change here either.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DomainInfo {
    pub name: String,
    pub schema: Option<String>,
    /// The underlying type as `format_type` renders it — `character varying(45)`,
    /// `numeric(10,2)`, `text[]`.
    pub base_type: String,
    /// The collation the domain was declared with, bare — reported only when it
    /// differs from the base type's, so an ordinary `text` domain carries none.
    pub collation: Option<String>,
    /// The namespace [`DomainInfo::collation`] lives in, when it needs one.
    ///
    /// Carried because a collation is an object like any other and the clause
    /// resolves through `search_path`: emitting a bare `COLLATE "mycoll"` for a
    /// collation in another schema either fails (`collation "mycoll" for
    /// encoding "UTF8" does not exist`) or — worse, and measured on 16.14 —
    /// silently binds a *different*, same-named collation that is on the path,
    /// so the recreated domain sorts and compares under another locale and every
    /// index over it is rebuilt with a different ordering.
    ///
    /// `None` for a built-in (`pg_catalog` is searched first and can't be
    /// shadowed) and, following [`qualified_ident`]'s rule, `Some("public")`
    /// still emits bare.
    pub collation_schema: Option<String>,
    /// Ready-to-emit SQL text, as [`ColumnInfo::default`] is.
    pub default_value: Option<String>,
    pub not_null: bool,
    pub checks: Vec<CheckInfo>,
    pub comment: Option<String>,
}

impl DomainInfo {
    /// `CREATE DOMAIN … AS …`, with every constraint inline, plus a
    /// `COMMENT ON DOMAIN` when there is one.
    pub fn create_sql(&self, dialect: crate::intel::SqlDialect) -> String {
        let qname = qualified_ident(&self.name, self.schema.as_deref(), dialect);
        let mut out = format!("CREATE DOMAIN {qname} AS {}", self.base_type.trim());
        if let Some(c) = &self.collation
            && !c.is_empty()
        {
            out.push_str(&format!(
                "\n  COLLATE {}",
                qualified_ident(c, self.collation_schema.as_deref(), dialect)
            ));
        }
        if let Some(d) = &self.default_value
            && !d.is_empty()
        {
            out.push_str(&format!("\n  DEFAULT {d}"));
        }
        if self.not_null {
            out.push_str("\n  NOT NULL");
        }
        for ck in &self.checks {
            out.push_str(&format!("\n  {}", ck.clause_sql(dialect)));
        }
        out.push(';');
        if let Some(c) = &self.comment
            && !c.is_empty()
        {
            out.push_str(&format!(
                "\nCOMMENT ON DOMAIN {qname} IS {};",
                ddl_string(c, dialect)
            ));
        }
        out
    }
}

/// What a sequence is attached to, when it is attached to anything.
///
/// `internal` is the distinction that decides whether the sequence is the user's
/// object at all. A `serial` column *owns* its sequence (`pg_depend` deptype
/// `a`): the sequence is a real object, droppable and alterable on its own. An
/// identity column's counter (deptype `i`) is **part of the column** — PostgreSQL
/// refuses `DROP SEQUENCE` on it and tells you to drop the column instead — so
/// Schemaic shows it and lets it be altered, and never offers the drop.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SequenceOwner {
    pub table: String,
    pub column: String,
    pub internal: bool,
}

/// A sequence — the counter behind `serial`/identity columns, and an object in
/// its own right.
///
/// The bounds are stored as the server reports them rather than as
/// `Option<i64>`, because PostgreSQL has no "unset": `NO MAXVALUE` *is* the
/// type's maximum, and a sequence read back always names concrete numbers. What
/// varies is whether those numbers are the implicit ones — [`implicit_bounds`]
/// answers that, and it is why [`SequenceInfo::create_sql`] can emit a clean
/// three-line statement instead of restating six clauses that say nothing.
///
/// [`implicit_bounds`]: SequenceInfo::implicit_bounds
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequenceInfo {
    pub name: String,
    pub schema: Option<String>,
    /// `smallint`, `integer` or `bigint`. Bounds are clamped to this type's range.
    pub data_type: String,
    pub start: i64,
    pub increment: i64,
    pub min_value: i64,
    pub max_value: i64,
    pub cache: i64,
    pub cycle: bool,
    pub owned_by: Option<SequenceOwner>,
    /// The counter's current position, or `None` when the sequence has never been
    /// used (or the connected role can't read it). Display-only: it is a *live*
    /// value, not part of the definition, so it takes no part in any diff.
    pub last_value: Option<i64>,
    pub comment: Option<String>,
}

impl Default for SequenceInfo {
    /// PostgreSQL's own defaults for a bare `CREATE SEQUENCE`: an ascending
    /// `bigint` from 1. Zeroes would be wrong in a way that emits a statement the
    /// server rejects (`INCREMENT BY 0`).
    fn default() -> Self {
        Self {
            name: String::new(),
            schema: None,
            data_type: "bigint".to_string(),
            start: 1,
            increment: 1,
            min_value: 1,
            max_value: i64::MAX,
            cache: 1,
            cycle: false,
            owned_by: None,
            last_value: None,
            comment: None,
        }
    }
}

impl SequenceInfo {
    /// The inclusive range of the sequence's storage type.
    pub fn type_bounds(data_type: &str) -> (i64, i64) {
        match data_type.trim().to_ascii_lowercase().as_str() {
            "smallint" | "int2" => (i16::MIN as i64, i16::MAX as i64),
            "integer" | "int" | "int4" => (i32::MIN as i64, i32::MAX as i64),
            _ => (i64::MIN, i64::MAX),
        }
    }

    /// The bounds PostgreSQL would apply if the statement named none, which
    /// depend on the direction: an ascending sequence runs `1 ..= type_max`, a
    /// descending one `type_min ..= -1`.
    pub fn implicit_bounds(&self) -> (i64, i64) {
        let (tmin, tmax) = Self::type_bounds(&self.data_type);
        if self.increment < 0 {
            (tmin, -1)
        } else {
            (1, tmax)
        }
    }

    /// Where the counter starts when the statement doesn't say: the low end for
    /// an ascending sequence, the high end for a descending one.
    pub fn implicit_start(&self) -> i64 {
        if self.increment < 0 {
            self.max_value
        } else {
            self.min_value
        }
    }

    /// The clauses that differ from what PostgreSQL would assume, in the order
    /// `CREATE`/`ALTER SEQUENCE` takes them. Empty when the sequence is entirely
    /// default — which is what lets an `ALTER` that changes nothing emit nothing.
    fn clauses(&self) -> Vec<String> {
        let (imin, imax) = self.implicit_bounds();
        let mut out = Vec::new();
        if !self.data_type.trim().eq_ignore_ascii_case("bigint") {
            out.push(format!("AS {}", self.data_type.trim()));
        }
        if self.increment != 1 {
            out.push(format!("INCREMENT BY {}", self.increment));
        }
        // `NO MINVALUE` and an explicit implicit bound mean the same thing to the
        // server; saying nothing is the honest rendering of "the default".
        if self.min_value != imin {
            out.push(format!("MINVALUE {}", self.min_value));
        }
        if self.max_value != imax {
            out.push(format!("MAXVALUE {}", self.max_value));
        }
        if self.start != self.implicit_start() {
            out.push(format!("START WITH {}", self.start));
        }
        if self.cache != 1 {
            out.push(format!("CACHE {}", self.cache));
        }
        if self.cycle {
            out.push("CYCLE".to_string());
        }
        out
    }

    /// `CREATE SEQUENCE …`, naming only what isn't the server's default, plus the
    /// `OWNED BY` and `COMMENT ON` that follow it.
    ///
    /// `OWNED BY` is restated because it is not cosmetic: it is what makes the
    /// sequence get dropped with its column, and a copy of the DDL that omits it
    /// recreates the sequence as an orphan that outlives the table.
    ///
    /// **The owner is qualified with the *sequence's* namespace, and that is
    /// correct** — the one place in this file where a schema is borrowed from a
    /// different object, so it is worth saying why rather than leaving it to be
    /// re-raised. [`SequenceOwner`] carries no namespace of its own, and does
    /// not need one: PostgreSQL 16 refuses
    /// `ALTER SEQUENCE sales.s OWNED BY public.orders.id` outright with
    /// *"sequence must be in same schema as table it is linked to"*, measured
    /// live. There is no state in which the two differ, so there is no
    /// namespace to lose. `db/tests/live/namespaces.rs`'s
    /// `a_sequence_cannot_be_owned_across_namespaces` pins the server's
    /// refusal; if a future release relaxes it, that test goes red and this
    /// line needs a namespace the model would then have to read.
    pub fn create_sql(&self, dialect: crate::intel::SqlDialect) -> String {
        let qname = qualified_ident(&self.name, self.schema.as_deref(), dialect);
        let mut out = format!("CREATE SEQUENCE {qname}");
        for c in self.clauses() {
            out.push_str(&format!("\n  {c}"));
        }
        if let Some(o) = &self.owned_by {
            out.push_str(&format!(
                "\n  OWNED BY {}.{}",
                qualified_ident(&o.table, self.schema.as_deref(), dialect),
                ddl_ident_in(&o.column, dialect)
            ));
        }
        out.push(';');
        if let Some(c) = &self.comment
            && !c.is_empty()
        {
            out.push_str(&format!(
                "\nCOMMENT ON SEQUENCE {qname} IS {};",
                ddl_string(c, dialect)
            ));
        }
        out
    }
}

/// Does a **name** match a schema-search term? `needle_lower` must already be
/// lower-cased; an empty needle matches nothing, since every caller answers "no
/// filter" separately.
///
/// This is the single name-versus-term rule for the whole schema-search family —
/// standalone objects, table names ([`TableInfo::matches_search`]), column names
/// ([`TableInfo::any_column_matches`]) and the ER diagram's find bar
/// ([`crate::erd::search`]) all ask it rather than spelling
/// `to_lowercase().contains` again. Three of those did spell it themselves, which
/// is how the empty-needle case came to be handled in some of them and not others.
///
/// The rule lives here as a free function, not only as
/// [`ObjectItem::matches_search`], so a caller can ask it of a *borrowed*
/// `EnumInfo`/`DomainInfo`/`SequenceInfo` without building an owned `ObjectItem`
/// first — see [`DbSchema::objects_matching`], which is on a per-keystroke path
/// and so must not clone the objects it rejects.
/// **It allocates nothing for an ASCII name.** The body was
/// `name.to_lowercase().contains(needle_lower)` — one heap allocation per name
/// asked — and the Find-Anywhere palette is deliberately undebounced, so a term
/// that has not narrowed to anything yet (`custz` on the way to `customers`)
/// walked every table name and every column name of every loaded database,
/// allocating and freeing a `String` for each. Measured in release on a
/// synthetic schema with a needle matching nothing — the worst case, and the
/// one that happens while you are still typing — that was 0.66 ms per database
/// at 500 tables × 25 columns and 1.57 ms at 1000 × 30; ten middling databases
/// is ~6.6 ms on the UI thread per character. The early exits do not help on
/// that input: nothing fills a bucket, so nothing breaks out.
///
/// The fallback is kept rather than replaced because `to_lowercase` is not
/// `to_ascii_lowercase`: `İ` lowercases to two chars and `ẞ` to `ß`, so a
/// byte-wise walk would answer differently for the names that motivated
/// `ghost_suffix`. The fast path is taken only when *both* sides are ASCII,
/// where the two are identical by construction, and
/// `the_allocation_free_path_agrees_with_lowercasing_the_name` is what says so.
pub fn object_name_matches(name: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return false;
    }
    if name.is_ascii() && needle_lower.is_ascii() {
        let (hay, needle) = (name.as_bytes(), needle_lower.as_bytes());
        return hay.len() >= needle.len()
            && hay.windows(needle.len()).any(|w| {
                w.iter()
                    .zip(needle)
                    .all(|(a, b)| a.to_ascii_lowercase() == *b)
            });
    }
    name.to_lowercase().contains(needle_lower)
}

/// [`object_name_matches`] with the **opposite** empty-needle answer: an empty
/// filter leaves every name standing.
///
/// The two rules are both real and they are not interchangeable. A *search*
/// asks "does this name match what I typed", and nothing matches nothing —
/// which is why `object_name_matches` refuses an empty needle and every caller
/// answers "no filter" separately. A *filter* on the schema tree asks "does
/// this row survive", and with the box empty every row does.
///
/// It exists so the tree can stop hand-spelling
/// `filt.is_empty() || name.to_lowercase().contains(filt)`, which it did at
/// eight sites — the database and namespace rows, in the rendered tree and
/// again in the keyboard walk that has to stay bug-for-bug identical to it.
/// Folding those into `object_name_matches` directly would have inverted the
/// tree on an empty filter, which is why this is a second spelling that owns
/// the empty case rather than a rewrite of the first.
pub fn name_survives(name: &str, filt: &str) -> bool {
    filt.is_empty() || object_name_matches(name, filt)
}

/// The order to introspect a connection's databases in — indices into `names`,
/// most-wanted first.
///
/// **The load used to be one unbounded fan-out in catalogue order.** Opening a
/// connection ran a full `fetch_schema` for *every* database the server
/// returned, each opening a connection of its own and reading every column,
/// index, key, view, check and trigger of a whole database. On a shared host
/// with 200 user databases — the ordinary shape of a hosting account — that is
/// 200 simultaneous handshakes against a server whose `max_connections` is 151
/// out of the box, so the 152nd onward failed ERROR 1040 and rendered as
/// `Failed` rows with no retry but a manual Refresh, which repeats the storm.
/// Below the cap it is still N connections and N catalogue reads per connect.
///
/// Bounding the concurrency is the caller's half. This is the other: **which
/// database's answer arrives first** once only a handful are in flight. The
/// active one is what the user is looking at; the ones they have already
/// expanded are what is on screen; a hidden one is on nobody's screen by
/// definition, so it goes last rather than being dropped — the completion
/// index and `intel`'s catalogue read the same models, and skipping it
/// entirely would be a laziness change rather than an ordering one.
///
/// Stable within each rank, so a server's own order is preserved among equals.
pub fn introspection_order(
    names: &[String],
    active: Option<&str>,
    expanded: &std::collections::HashSet<String>,
    hidden: &std::collections::HashSet<String>,
) -> Vec<usize> {
    let rank = |n: &str| -> u8 {
        if active.is_some_and(|a| a.eq_ignore_ascii_case(n)) {
            0
        } else if !db_visible(hidden, n) {
            3
        } else if expanded.contains(n) {
            1
        } else {
            2
        }
    };
    let mut order: Vec<usize> = (0..names.len()).collect();
    order.sort_by_key(|&i| rank(&names[i]));
    order
}

/// One standalone object, whichever kind it is.
///
/// The tree renders a mixed list of these and the editor holds exactly one, so
/// both need a single type that can answer "what are you, what are you called,
/// and what does your `CREATE` look like" without a three-way match at every
/// site. The kind tag itself lives in [`crate::ddl::ObjectKind`], next to the
/// changes that are shared across the first three.
///
/// `Routine` is behind an [`Arc`](std::sync::Arc) and the others aren't, for two reasons that
/// point the same way. A [`RoutineInfo`] carries a whole body, so an inline arm
/// would widen every `ObjectItem` — including the enums and sequences a tree
/// holds far more of — to the size of the largest. And **`objects_all` is on the
/// keyboard-walk path**: `visible_nav_rows` rebuilds the whole row list on every
/// arrow key, through `object_groups` → `objects_all`, so a `Box` there meant
/// deep-copying every routine body in the database per keypress. An `Arc` makes
/// that clone a refcount bump and leaves every consumer's code identical, which
/// is why this is the fix rather than a borrowed second path — `nav_rows` has to
/// stay bug-for-bug identical to the render, and giving it its own cheaper view
/// of the objects is exactly how the two drifted before.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectItem {
    Enum(EnumInfo),
    Domain(DomainInfo),
    Sequence(SequenceInfo),
    Routine(std::sync::Arc<RoutineInfo>),
    /// Behind an [`Arc`](std::sync::Arc) for the reason `Routine` is: it carries
    /// a body, and the keyboard walk rebuilds this list on every arrow key.
    Event(std::sync::Arc<EventInfo>),
}

impl ObjectItem {
    pub fn kind(&self) -> crate::ddl::ObjectKind {
        match self {
            ObjectItem::Enum(_) => crate::ddl::ObjectKind::Enum,
            ObjectItem::Domain(_) => crate::ddl::ObjectKind::Domain,
            ObjectItem::Sequence(_) => crate::ddl::ObjectKind::Sequence,
            ObjectItem::Routine(r) => crate::ddl::ObjectKind::of_routine(r.kind),
            ObjectItem::Event(_) => crate::ddl::ObjectKind::Event,
        }
    }

    /// The routine behind this item, when it is one — what the tree's menu and
    /// the editor both need, and the reason neither has to look it up by name.
    pub fn routine(&self) -> Option<&RoutineInfo> {
        match self {
            ObjectItem::Routine(r) => Some(r),
            _ => None,
        }
    }

    /// The event behind this item, when it is one — the counterpart of
    /// [`ObjectItem::routine`], and what the tree's menu and the editor both
    /// need so neither has to look it up by name.
    pub fn event(&self) -> Option<&EventInfo> {
        match self {
            ObjectItem::Event(e) => Some(e),
            _ => None,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            ObjectItem::Enum(e) => &e.name,
            ObjectItem::Domain(d) => &d.name,
            ObjectItem::Sequence(s) => &s.name,
            ObjectItem::Routine(r) => &r.name,
            ObjectItem::Event(e) => &e.name,
        }
    }

    pub fn schema(&self) -> Option<&str> {
        match self {
            ObjectItem::Enum(e) => e.schema.as_deref(),
            ObjectItem::Domain(d) => d.schema.as_deref(),
            ObjectItem::Sequence(s) => s.schema.as_deref(),
            ObjectItem::Routine(r) => r.schema.as_deref(),
            ObjectItem::Event(e) => e.schema.as_deref(),
        }
    }

    /// The one-line summary shown beside the name — what the object *is*, in the
    /// space a column row gives its type.
    ///
    /// An enum shows its values, because that list is the entire content of the
    /// type and a name alone says nothing. It is clipped rather than wrapped: a
    /// tree row is one line, and past a few values the useful information is
    /// that there are more. A sequence shows what owns it, which is the fact that
    /// decides whether it is an object anyone should touch.
    /// Collapse every whitespace run to a single space, so arbitrary text fits
    /// a one-line row. Leading and trailing whitespace is *kept* as a single
    /// space, because in an enum label it is data and dropping it would show
    /// two different labels identically.
    fn one_line(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut in_ws = false;
        for c in s.chars() {
            if c.is_whitespace() {
                in_ws = true;
                continue;
            }
            if in_ws {
                out.push(' ');
                in_ws = false;
            }
            out.push(c);
        }
        if in_ws {
            out.push(' ');
        }
        out
    }

    pub fn detail(&self) -> String {
        const VALUES: usize = 4;
        match self {
            ObjectItem::Enum(e) => {
                // Whitespace runs collapse to one space **before** joining: an
                // enum label is arbitrary text and may hold a newline or tab —
                // the same fact `pg_types` reads its labels one row at a time
                // for — while this string goes into a tree row of fixed height,
                // which a raw newline overflows.
                let head: Vec<String> = e
                    .values
                    .iter()
                    .take(VALUES)
                    .map(|v| Self::one_line(v))
                    .collect();
                let mut out = head.join(", ");
                if e.values.len() > VALUES {
                    out.push_str(&format!(", +{}", e.values.len() - VALUES));
                }
                out
            }
            ObjectItem::Domain(d) => d.base_type.clone(),
            ObjectItem::Sequence(s) => match &s.owned_by {
                Some(o) => format!("{}.{}", o.table, o.column),
                None => s.data_type.clone(),
            },
            // A routine's signature is what tells two overloads apart, so it is
            // the detail even when it's empty — `()` is the honest rendering of
            // a routine that takes nothing, and reads as a signature rather than
            // as a missing summary. The return type follows it for a function,
            // which is the other half of what picking one out needs.
            ObjectItem::Routine(r) => {
                let args = Self::one_line(r.arguments.trim());
                let returns = r.returns.trim();
                if r.kind == RoutineKind::Function && !returns.is_empty() {
                    format!("({args}) → {}", Self::one_line(returns))
                } else {
                    format!("({args})")
                }
            }
            // The schedule, which is what tells two events apart in a list —
            // and, when it is switched off, that it is. See
            // [`EventInfo::detail`].
            ObjectItem::Event(e) => e.detail(),
        }
    }

    /// Does this object match a schema-search term? **By name only**, and
    /// `needle_lower` must already be lower-cased — the counterpart of
    /// [`TableInfo::matches_search`], and an empty needle matches nothing for the
    /// same reason (every caller answers "no filter" separately).
    ///
    /// Name-only because [`ObjectItem::detail`] is a summary the row happens to
    /// show: matching it would surface a sequence because some unrelated table's
    /// name appeared in its owner, and an enum because one of its values spelled
    /// the term.
    ///
    /// This is the **one** predicate behind both search surfaces — the schema
    /// tree's filter box and the Find-Anywhere palette. They were two, and the
    /// palette's simply had no object arm at all, so on a PostgreSQL connection
    /// Ctrl+P for a type you were looking at in the sidebar returned nothing.
    pub fn matches_search(&self, needle_lower: &str) -> bool {
        object_name_matches(self.name(), needle_lower)
    }

    /// Whether this object exists only as part of a column, and so can be
    /// inspected and altered but never dropped on its own — an identity column's
    /// counter, and nothing else today. The same call `is_editable_view` makes
    /// for a materialized view.
    pub fn is_internal(&self) -> bool {
        match self {
            ObjectItem::Sequence(s) => s.owned_by.as_ref().is_some_and(|o| o.internal),
            _ => false,
        }
    }

    pub fn create_sql(&self, dialect: crate::intel::SqlDialect) -> String {
        match self {
            ObjectItem::Enum(e) => e.create_sql(dialect),
            ObjectItem::Domain(d) => d.create_sql(dialect),
            ObjectItem::Sequence(s) => s.create_sql(dialect),
            // Not a replace: this is the `CREATE` a reader copies, and one that
            // said `OR REPLACE` would silently overwrite whatever the target
            // database already had under that name.
            //
            // Through [`crate::ddl::client_script`], which is what makes it
            // *runnable*: a MySQL routine's `CREATE` carries no terminator (the
            // apply path sends it whole) and its body is full of `;`, so pasted
            // into a query tab it was cut mid-body, and two of them joined in a
            // folder's script ran together. Every other object here already
            // ends in its own `;` and the call leaves those untouched.
            //
            // **And through the session wrapper the apply path uses**, which is
            // the difference between a script that recreates the object and one
            // that recreates something adjacent to it. A routine's body was
            // parsed under the `sql_mode` and character set of the session that
            // wrote it and re-parses under the applying one's: `"col"` under
            // `ANSI_QUOTES` is an identifier there and a string literal here. The
            // emitter is deliberately shared with the apply path; the wrapper has
            // to be too, or four of the twelve fields the model carries reach the
            // server on one caller and are dropped on the other.
            ObjectItem::Routine(r) => crate::ddl::client_script(
                &crate::ddl::session_wrapped(None, r.create_sql(dialect, false), r, dialect),
                dialect,
            ),
            // Through `client_script` for the same reason a routine is: this
            // `CREATE` carries no terminator of its own (the apply path sends it
            // whole) and its body may be a `BEGIN … END` full of `;`.
            //
            // The wrapper carries a fourth value here, and it is the one with no
            // other carrier: an event's `AT`/`STARTS`/`ENDS` are stored against
            // the session `time_zone` that wrote them and `CREATE EVENT` has no
            // clause for it, so a script run from another zone moves every future
            // firing by the offset between the two — a nightly 03:00 job that
            // starts running at 01:00, for ever, with nothing saying so.
            ObjectItem::Event(e) => crate::ddl::client_script(
                &crate::ddl::event_session_wrapped(e.create_sql(dialect), e, dialect),
                dialect,
            ),
        }
    }
}

impl TableInfo {
    /// Which of the three things in [`TableShape`] this is — the question the
    /// menus, the designer and the dump ask, in place of the two-answer
    /// `is_view`.
    pub fn shape(&self) -> TableShape {
        if self.is_view {
            TableShape::View
        } else if self.is_sequence {
            TableShape::Sequence
        } else {
            TableShape::Table
        }
    }

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
        // Delegated, not inlined. This was a fifth copy of the identifier
        // quoter — byte-identical to `ddl_ident_in`, so it produced no wrong
        // output, but the range that added the check loop below put the
        // divergence *inside one function body*: the checks emit through
        // `ddl_ident_in` while the columns three lines up used this closure.
        // **Invariant:** one identifier quoter.
        let q = |s: &str| ddl_ident_in(s, dialect);
        // The table's own name, schema-qualified when it isn't in `public`.
        let qname = match sql_qualifier(self.schema.as_deref()) {
            Some(s) => format!("{}.{}", q(s), q(&self.name)),
            None => q(&self.name),
        };
        // **A sequence's columns are its counter, not its definition.** MariaDB
        // stores one as an eight-column table whose *row* holds the start,
        // increment, bounds and cache — none of which is in the catalogue this
        // model was built from. Emitting the columns produces a `CREATE TABLE`
        // that restores as a plain table, and every `NEXTVAL` against it then
        // fails; emitting a `CREATE SEQUENCE` with invented parameters would be
        // worse. So the script names the object and says what it could not
        // restate, the way the unreadable-view arm below does — the one thing
        // that leaves the reader able to fix it.
        // **Comment-safed, not merely quoted.** Every arm below that puts this
        // name on a `--` line emits `cname`, never `qname`: a `--` comment ends
        // at the first newline and a quoted identifier may hold one, so a
        // sequence named `sq\nDROP DATABASE prod; --` otherwise turned the line
        // the reader takes for an explanation into a top-level statement in a
        // file they take for a backup — and `Db::run_script`'s guard
        // deliberately never reads the file. `ident_sql`/`ddl_ident_in` are not
        // the fix: they double a quote character and say nothing about `\n`.
        // **Invariant:** quoting is not comment-safety.
        let cname = crate::export::comment_text(&qname);
        if self.is_sequence {
            return format!(
                "-- {cname} is a sequence. Schemaic reads its definition from the row, not the\n\
                 -- catalogue, so this script cannot restate it. Copy it from\n\
                 -- `SHOW CREATE SEQUENCE {cname}` on the source server."
            );
        }
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
                    "-- View definition for {cname} was not available.\nCREATE VIEW {qname} AS\nSELECT ...;"
                ),
            };
        }
        // The engine's own text wins where it has one — see `create_sql` for why
        // reconstructing a SQLite table is not merely different but wrong.
        if let Some(sql) = self
            .create_sql
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return sql.to_string();
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
        // Table constraints, inline on both engines — unlike an index, which
        // PostgreSQL can only create in a statement of its own.
        for ck in &self.check_constraints {
            lines.push(format!("  {}", ck.clause_sql(dialect)));
        }
        let non_pk = self.indexes.iter().filter(|ix| !ix.is_primary());
        if pg {
            // Postgres: indexes are separate statements after the table.
            let mut out = format!("CREATE TABLE {qname} (\n{}\n);", lines.join(",\n"));
            for ix in non_pk {
                // **An index the model only partly read is emitted from the
                // server's own statement**, because emitting it from the model
                // is not merely different but wrong: `INCLUDE`, `NULLS NOT
                // DISTINCT` and a storage parameter have no field here, so the
                // line below would restore an index that no longer covers, no
                // longer forbids two NULLs, or is back at the default
                // fillfactor — on a dump nobody edited, reported as a success.
                // See `IndexInfo::create_sql`; a fully-read index keeps the
                // model's emission, which is the one the designer's preview and
                // the compare pane are written against.
                if ix.lossy
                    && let Some(sql) = ix
                        .create_sql
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                {
                    out.push('\n');
                    out.push_str(sql);
                    continue;
                }
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
            // MySQL: inline KEY / UNIQUE KEY / FULLTEXT KEY, through the one
            // clause builder the two emitters in `ddl` use. This branch was the
            // third site hand-building it, and the third to lose `ix.method` —
            // so the DDL a user copied out of the schema tree and ran elsewhere
            // built a table whose `MATCH … AGAINST` fails. The PostgreSQL branch
            // six lines above restates the method, which is what makes it a
            // MySQL-only loss; the view arm of this same function was routed
            // through `ddl::view_ddl` for exactly this reason.
            for ix in non_pk {
                lines.push(format!(
                    "  {}",
                    crate::ddl::mysql_index_clause(ix, "KEY", dialect)
                ));
            }
            format!("CREATE TABLE {qname} (\n{}\n);", lines.join(",\n"))
        }
    }

    /// Does any of this table's column names contain `needle_lower`
    /// (case-insensitive)? `needle_lower` must already be lower-cased by the caller.
    pub fn any_column_matches(&self, needle_lower: &str) -> bool {
        self.columns
            .iter()
            .any(|c| object_name_matches(&c.name, needle_lower))
    }

    /// Does this table match a schema-search term — by its own name OR by any of
    /// its column names? `needle_lower` must already be lower-cased. An empty
    /// needle matches nothing (callers treat "no filter" separately).
    pub fn matches_search(&self, needle_lower: &str) -> bool {
        object_name_matches(&self.name, needle_lower) || self.any_column_matches(needle_lower)
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
    // **A capability, not a comparison.** The question is whether
    // `fk.ref_schema` names the *database* the reference points into or a
    // namespace within the current one — which is exactly
    // `ddl::ref_schema_is_database`, the predicate CLAUDE.md names for it and
    // which the schema comparison already asks. Spelled `== Postgres` here, it
    // sorted SQLite onto the same side by accident, and a fourth engine would
    // have joined it with no comparison to grep for.
    let same_database = !crate::ddl::ref_schema_is_database(dialect);
    // Same-database: open the current DB, not the schema.
    let database = if same_database {
        default_schema.to_string()
    } else {
        fk.ref_schema
            .clone()
            .unwrap_or_else(|| default_schema.to_string())
    };
    // On Postgres the FK's `ref_schema` *is* the namespace, so it becomes the
    // target's schema. On MySQL it was already consumed as the database above.
    let schema = same_database.then(|| fk.ref_schema.clone()).flatten();
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
    let sql = if same_database {
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

/// Look one object up by `(namespace, name)` — the rule every `find_*` on
/// [`DbSchema`] follows, written once.
///
/// An exact namespace match wins. When the caller has no namespace to offer —
/// MySQL, or a tab restored from a session file written before multi-schema
/// browsing — it falls back to the name alone, preferring `public` so the common
/// case resolves the way it always did rather than to whichever same-named
/// object happens to come first.
fn find_by_ns<'a, T>(
    items: &'a [T],
    schema: Option<&str>,
    name: &str,
    key: impl Fn(&'a T) -> (Option<&'a str>, &'a str) + Copy,
) -> Option<&'a T> {
    if schema.is_some() {
        return items
            .iter()
            .find(|i| key(i).1 == name && key(i).0 == schema);
    }
    let by_name = || items.iter().filter(|i| key(i).1 == name);
    by_name()
        .find(|i| key(i).0 == Some(PG_DEFAULT_SCHEMA))
        .or_else(|| by_name().next())
}

/// The introspected schema of one database.
///
/// The lists past `tables` are the database's standalone objects — the first
/// three are PostgreSQL's and stay empty on MySQL, the fourth is on both. They
/// live *here* because they are browsable: the schema tree lists them beside
/// the tables, and a column's type is one of them. A second, separately
/// refreshed cache keyed the same way would be a second answer to "what is in
/// this database", and the two would diverge on the first refresh that only
/// updated one.
///
/// `routines` used to be the exception, fetched lazily for the trigger editor's
/// dropdown alone on the grounds that nothing rendered a body until an editor
/// asked. Browsing them is exactly the reader that argument said didn't exist —
/// and completion wants their names as much as it wants a table's — so they
/// arrive with everything else now. The bodies are no heavier here than the view
/// definitions and trigger bodies this struct has always carried.
///
/// **A MySQL routine's `body` is the one field here that is not trustworthy for
/// an edit.** It comes from `information_schema`, which resolves the escapes;
/// [`RoutineSource`] is the correct source and is read lazily when the editor
/// opens, exactly as [`TriggerSource`] is.
#[derive(Clone, Debug, Default)]
pub struct DbSchema {
    pub tables: Vec<TableInfo>,
    pub enums: Vec<EnumInfo>,
    pub domains: Vec<DomainInfo>,
    pub sequences: Vec<SequenceInfo>,
    /// Stored functions and procedures. Both engines that have them report them;
    /// SQLite has none and leaves this empty.
    ///
    /// `Arc`, not a plain `RoutineInfo`, for the reason [`ObjectItem::Routine`]
    /// gives: the schema-tree walk turns these into `ObjectItem`s on every arrow
    /// key, and a body copied per keypress is the cost that buys nothing.
    pub routines: Vec<std::sync::Arc<RoutineInfo>>,
    /// Scheduled events. **MySQL and MariaDB only** — the other two engines have
    /// no such object and leave this empty; see [`EventInfo`].
    ///
    /// `Arc` for the reason `routines` is: an event carries a body, and the
    /// schema-tree walk turns these into [`ObjectItem`]s on every arrow key.
    pub events: Vec<std::sync::Arc<EventInfo>>,
    /// Which MySQL-family server this came from, when it came from one.
    ///
    /// `SqlDialect` deliberately has no MariaDB arm — the two speak one dialect
    /// as far as parsing, quoting and completion are concerned, and giving them
    /// separate arms would fork every `match` in `sql`/`intel`/`filter` for a
    /// difference none of them care about. But they **diverge at the emitter**,
    /// and each divergence is a data-loss bug rather than a syntax preference:
    /// MariaDB has no `NOT ENFORCED`, and its `MODIFY COLUMN` silently destroys
    /// the column's own `CHECK`.
    ///
    /// So the flavour rides on the introspected schema instead, where
    /// `collect_schema` already computes it from `SELECT VERSION()` and used to
    /// throw it away. `Unknown` is the honest default for PostgreSQL, for a
    /// hand-built schema, and for a server that hasn't been asked — and the
    /// emitter treats it as "don't assume MariaDB", so a missing answer costs a
    /// feature rather than a table's constraints.
    pub flavour: ServerFlavour,
    /// **Which database this was read from** — the schema's own address.
    ///
    /// Not part of the schema, and deliberately not stamped onto the objects in
    /// it: [`TableInfo::schema`] is `None` on MySQL precisely because a database
    /// *is* its namespace there. It rides here because one reader needs to
    /// subtract an object's own address before comparing it against another
    /// database's, and the model records that address nowhere else.
    ///
    /// The reader is [`crate::compare::SchemaComparison::of`] and the case is
    /// MySQL's, where a foreign key's [`ForeignKeyInfo::ref_schema`] and a
    /// view's rewritten [`TableInfo::view_definition`] both come back qualified
    /// with the reading database. Two structurally identical databases then
    /// differ in every object holding either, and the migration re-points the
    /// left database's key — and its view's body — at the *right* one.
    ///
    /// `None` means the reader did not record it, which is the honest answer for
    /// a hand-built schema. A side with no address is compared exactly as it
    /// arrived rather than guessed at.
    pub database: Option<String>,
}

/// Which MySQL-family server a schema was introspected from. See
/// [`DbSchema::flavour`] for why this is not a [`crate::intel::SqlDialect`] arm.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ServerFlavour {
    /// PostgreSQL, a hand-built schema, or a MySQL-family server not yet asked.
    #[default]
    Unknown,
    MySql,
    MariaDb,
}

impl ServerFlavour {
    /// Read `SELECT VERSION()`. MariaDB puts its name in the string; MySQL does
    /// not, which is the same test `schemaic-db` has always used.
    pub fn parse_version(v: &str) -> Self {
        if v.to_ascii_lowercase().contains("mariadb") {
            ServerFlavour::MariaDb
        } else {
            ServerFlavour::MySql
        }
    }

    /// Is this **known** to be MariaDB? Unknown answers `false`, so a feature is
    /// withheld rather than a destructive assumption made.
    pub fn is_mariadb(self) -> bool {
        self == ServerFlavour::MariaDb
    }
}

/// The `information_schema.STATISTICS` expression answering **is this index
/// switched off** as `1`/`0`, for the MySQL-family server that reported
/// `version`.
///
/// A DBA hides an index to test a plan change — MySQL 8's
/// `ALTER TABLE … ALTER INDEX … INVISIBLE`, MariaDB 10.6's `… IGNORED`. Both
/// servers publish the state and both print it in `SHOW CREATE TABLE`, and
/// neither was read: a hidden index came back as an ordinary one with
/// [`IndexInfo::lossy`] `false`, so any edit to it took `ddl::diff`'s
/// `DROP INDEX` + `ADD INDEX` arm and brought it back **live**, with the
/// optimizer using it again and the preview saying nothing.
///
/// **An expression rather than a column name**, because the two columns are
/// named differently *and* carry opposite polarity (`IGNORED = 'YES'` versus
/// `IS_VISIBLE = 'NO'`) — one place for that, not two — and because a server
/// too old for either has neither column, where naming one fails the whole
/// query rather than the one value. Same reason the `EXPRESSION` and
/// `ALGORITHM` columns beside it hold the row shape steady with a `NULL`.
///
/// A version this cannot parse falls to the constant `0`: an unread flag is
/// reported as "not switched off", which is what the model said before this
/// existed and is the reading that offers an edit rather than refusing one on
/// a guess.
pub fn index_disabled_sql(version: &str) -> &'static str {
    let (major, minor) = major_minor(version);
    match ServerFlavour::parse_version(version) {
        // MariaDB 10.6.0 added `IGNORED`.
        ServerFlavour::MariaDb if (major, minor) >= (10, 6) => {
            "CASE WHEN IGNORED = 'YES' THEN 1 ELSE 0 END"
        }
        // MySQL 8.0.0 added invisible indexes and `IS_VISIBLE`.
        ServerFlavour::MySql if major >= 8 => "CASE WHEN IS_VISIBLE = 'NO' THEN 1 ELSE 0 END",
        _ => "0",
    }
}

/// The leading `major.minor` of a `SELECT VERSION()` string, or `(0, 0)`.
///
/// Both families lead with the numbers and then diverge —
/// `10.11.14-MariaDB-1:10.11.14+maria~ubu2204`, `8.4.11`, `5.7.44-log` — so the
/// digits before the second `.` are the whole of what is portable here.
fn major_minor(version: &str) -> (u32, u32) {
    let mut parts = version.split('.').map(|p| {
        p.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse::<u32>()
            .unwrap_or(0)
    });
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

impl DbSchema {
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }

    /// The enum type with this `(namespace, name)` identity, on the same
    /// name-falls-back-to-`public` rule as [`DbSchema::find_table`].
    pub fn find_enum(&self, schema: Option<&str>, name: &str) -> Option<&EnumInfo> {
        find_by_ns(&self.enums, schema, name, |e| {
            (e.schema.as_deref(), e.name.as_str())
        })
    }

    pub fn find_domain(&self, schema: Option<&str>, name: &str) -> Option<&DomainInfo> {
        find_by_ns(&self.domains, schema, name, |d| {
            (d.schema.as_deref(), d.name.as_str())
        })
    }

    pub fn find_sequence(&self, schema: Option<&str>, name: &str) -> Option<&SequenceInfo> {
        find_by_ns(&self.sequences, schema, name, |s| {
            (s.schema.as_deref(), s.name.as_str())
        })
    }

    /// One routine by namespace, kind and name, on the same `find_by_ns`
    /// namespace rule.
    ///
    /// **Narrowed by kind first**, because a function and a procedure may share a
    /// name and are different objects. It still answers with the *first* of a set
    /// of PostgreSQL overloads: they share a name, a kind and a namespace, so
    /// nothing short of the argument types tells them apart — which is exactly
    /// what a remembered palette hit or a search-history entry cannot carry. The
    /// tree passes the whole [`RoutineInfo`] instead and never comes here.
    pub fn find_routine(
        &self,
        schema: Option<&str>,
        kind: RoutineKind,
        name: &str,
    ) -> Option<&std::sync::Arc<RoutineInfo>> {
        let of_kind: Vec<&std::sync::Arc<RoutineInfo>> =
            self.routines.iter().filter(|r| r.kind == kind).collect();
        find_by_ns(&of_kind, schema, name, |r| {
            (r.schema.as_deref(), r.name.as_str())
        })
        .copied()
    }

    /// Every standalone object in one namespace, of one kind, in introspection
    /// order — what a schema-tree group renders.
    ///
    /// `schema` is matched exactly, so a `None` selects the objects that carry no
    /// namespace (i.e. none, on PostgreSQL). The tree's *flat* case passes
    /// [`DbSchema::objects_all`] instead, since flat means "this database has no
    /// schema level", not "these objects have no namespace" — the distinction
    /// that once made keyboard navigation reach no table at all.
    /// **Filtered before it is cloned**, which is the whole reason this does
    /// not read `objects_all(kind).into_iter().filter(…)`. That spelling
    /// materialised every enum, domain and sequence in the *database* — names,
    /// comments, an enum's whole value list — and then discarded the ones
    /// outside the namespace. `ui::schema_tree`'s `object_groups` calls this
    /// once per `ObjectKind::ALL` per namespace, and `nav_rows` rebuilds the
    /// whole visible row list on **every arrow key**, so on a PostgreSQL
    /// database with 20 schemas it was tens of thousands of `String`
    /// allocations per keypress, ≥95% of them thrown away by the very next
    /// `filter`. It scaled with the number of namespaces, not with what was on
    /// screen.
    ///
    /// Same rule, and the same wording, as [`DbSchema::objects_matching`],
    /// which was given the cheap path when the palette hit it — this one and
    /// the tree's two predicates were the sites that did not follow.
    pub fn objects_in(
        &self,
        schema: Option<&str>,
        kind: crate::ddl::ObjectKind,
    ) -> Vec<ObjectItem> {
        self.objects_where(kind, &|s, _| s == schema)
    }

    /// Does **any** object of `kind` satisfy `keep`?
    ///
    /// The half of [`DbSchema::objects_where`] that clones nothing at all and
    /// stops at the first hit. The schema tree asks it per database and per
    /// namespace on every filter keystroke, and used to answer by building the
    /// whole owned list and calling `.iter().any(…)` on it.
    fn objects_any(
        &self,
        kind: crate::ddl::ObjectKind,
        keep: &dyn Fn(Option<&str>, &str) -> bool,
    ) -> bool {
        use crate::ddl::ObjectKind as K;
        match kind {
            K::Enum => self
                .enums
                .iter()
                .any(|e| keep(e.schema.as_deref(), &e.name)),
            K::Domain => self
                .domains
                .iter()
                .any(|d| keep(d.schema.as_deref(), &d.name)),
            K::Sequence => self
                .sequences
                .iter()
                .any(|s| keep(s.schema.as_deref(), &s.name)),
            k @ (K::Function | K::Procedure) => self
                .routines
                .iter()
                .any(|r| Some(r.kind) == k.routine_kind() && keep(r.schema.as_deref(), &r.name)),
            K::Event => self
                .events
                .iter()
                .any(|e| keep(e.schema.as_deref(), &e.name)),
        }
    }

    /// The objects of `kind` that `keep` accepts, **cloning only those** — the
    /// one dispatch every list-returning accessor here goes through, so a
    /// caller cannot accidentally take the expensive route.
    ///
    /// `keep` sees a namespace and a name, which is everything the three
    /// questions asked of it need (which namespace, which name matches a
    /// needle, all of them).
    fn objects_where(
        &self,
        kind: crate::ddl::ObjectKind,
        keep: &dyn Fn(Option<&str>, &str) -> bool,
    ) -> Vec<ObjectItem> {
        use crate::ddl::ObjectKind as K;
        match kind {
            K::Enum => self
                .enums
                .iter()
                .filter(|e| keep(e.schema.as_deref(), &e.name))
                .cloned()
                .map(ObjectItem::Enum)
                .collect(),
            K::Domain => self
                .domains
                .iter()
                .filter(|d| keep(d.schema.as_deref(), &d.name))
                .cloned()
                .map(ObjectItem::Domain)
                .collect(),
            K::Sequence => self
                .sequences
                .iter()
                .filter(|s| keep(s.schema.as_deref(), &s.name))
                .cloned()
                .map(ObjectItem::Sequence)
                .collect(),
            k @ (K::Function | K::Procedure) => self
                .routines
                .iter()
                .filter(|r| Some(r.kind) == k.routine_kind() && keep(r.schema.as_deref(), &r.name))
                .cloned()
                .map(ObjectItem::Routine)
                .collect(),
            K::Event => self
                .events
                .iter()
                .filter(|e| keep(e.schema.as_deref(), &e.name))
                .cloned()
                .map(ObjectItem::Event)
                .collect(),
        }
    }

    /// Does anything in this database match by object name? The schema tree's
    /// database-row filter, which would otherwise hide the very database that
    /// defines the type being searched for.
    ///
    /// In core, and answering without cloning: it runs per database on every
    /// keystroke, and the view's version built six owned lists to return a
    /// `bool`.
    pub fn any_object_matches(&self, needle_lower: &str) -> bool {
        crate::ddl::ObjectKind::ALL
            .into_iter()
            .any(|k| self.objects_any(k, &|_, n| object_name_matches(n, needle_lower)))
    }

    /// [`DbSchema::any_object_matches`], narrowed to one namespace — the
    /// namespace-row filter, asked once per namespace per keystroke.
    pub fn any_object_in_matches(&self, schema: Option<&str>, needle_lower: &str) -> bool {
        crate::ddl::ObjectKind::ALL.into_iter().any(|k| {
            self.objects_any(k, &|s, n| {
                s == schema && object_name_matches(n, needle_lower)
            })
        })
    }

    /// Does this namespace hold any standalone object at all?
    ///
    /// The question behind the tree's "No tables" hint, which asked it by
    /// building every group's owned list and testing `.is_empty()`. `None`
    /// means the flat case — any object anywhere in the database.
    pub fn has_objects_in(&self, schema: Option<Option<&str>>) -> bool {
        crate::ddl::ObjectKind::ALL
            .into_iter()
            .any(|k| match schema {
                Some(ns) => self.objects_any(k, &|s, _| s == ns),
                None => self.objects_any(k, &|_, _| true),
            })
    }

    /// One standalone object by namespace, kind and name — the kind-agnostic
    /// counterpart of [`DbSchema::find_enum`] and friends, on the same
    /// `find_by_ns` namespace rule tables use.
    ///
    /// Owned rather than borrowed, matching [`DbSchema::objects_all`]: the
    /// callers are the ones that resolve a *remembered* target (a Find-Anywhere
    /// hit, a search-history entry) against whatever the schema now holds, and
    /// they hand the result straight to an editor that wants it by value.
    pub fn find_object(
        &self,
        schema: Option<&str>,
        kind: crate::ddl::ObjectKind,
        name: &str,
    ) -> Option<ObjectItem> {
        match kind {
            crate::ddl::ObjectKind::Enum => {
                self.find_enum(schema, name).cloned().map(ObjectItem::Enum)
            }
            crate::ddl::ObjectKind::Domain => self
                .find_domain(schema, name)
                .cloned()
                .map(ObjectItem::Domain),
            crate::ddl::ObjectKind::Sequence => self
                .find_sequence(schema, name)
                .cloned()
                .map(ObjectItem::Sequence),
            // Both routine kinds go through one lookup narrowed by kind, so a
            // procedure and a function of the same name resolve to the one that
            // was asked for. PostgreSQL's overloads share a name *and* a kind, so
            // this still answers with the first — a remembered palette hit names
            // only `(namespace, kind, name)`, which is all a search-history entry
            // can carry.
            // Spelled out rather than caught by a `_`, so a sixth `ObjectKind`
            // has to answer for itself instead of silently resolving to
            // nothing here and in the two lookups below.
            k @ (crate::ddl::ObjectKind::Function | crate::ddl::ObjectKind::Procedure) => k
                .routine_kind()
                .and_then(|rk| self.find_routine(schema, rk, name))
                .cloned()
                .map(ObjectItem::Routine),
            crate::ddl::ObjectKind::Event => self
                .find_event(schema, name)
                .cloned()
                .map(ObjectItem::Event),
        }
    }

    /// One scheduled event by name. The namespace argument is carried for the
    /// shape every other `find_*` has and is always `None` in practice — MySQL
    /// is the only engine with events, and a database *is* the namespace there.
    pub fn find_event(
        &self,
        schema: Option<&str>,
        name: &str,
    ) -> Option<&std::sync::Arc<EventInfo>> {
        find_by_ns(&self.events, schema, name, |e| {
            (e.schema.as_deref(), e.name.as_str())
        })
    }

    /// The objects of one kind whose **name** matches, in any namespace —
    /// [`object_name_matches`] applied to the borrowed catalogue, so an object
    /// that doesn't match is never cloned.
    ///
    /// This exists rather than `objects_all(kind).retain(…)` because the caller
    /// is the Find-Anywhere palette, whose query signal is **not** debounced: it
    /// re-runs on every keystroke, over every loaded database, three times. Going
    /// through `objects_all` there cloned every `EnumInfo`/`DomainInfo`/
    /// `SequenceInfo` in the database — names, comments, an enum's whole value
    /// list — to answer a substring test, thousands of allocations per character
    /// on the UI thread. Same rule as `SignalGet::with` over `get`.
    ///
    /// An empty needle matches **nothing**, following `object_name_matches`; a
    /// caller that means "no filter" wants [`DbSchema::objects_all`].
    pub fn objects_matching(
        &self,
        kind: crate::ddl::ObjectKind,
        needle_lower: &str,
    ) -> Vec<ObjectItem> {
        self.objects_where(kind, &|_, n| object_name_matches(n, needle_lower))
    }

    /// Every standalone object of one kind, whatever namespace it is in.
    pub fn objects_all(&self, kind: crate::ddl::ObjectKind) -> Vec<ObjectItem> {
        self.objects_where(kind, &|_, _| true)
    }

    /// Every enum and domain in one namespace, as names a column's type could be.
    ///
    /// What the designer's type dropdown appends to [`crate::ddl::common_types`]:
    /// a user-defined type is as usable in a column definition as `integer` is,
    /// and a type list that omits the ones this database actually defines makes
    /// the dropdown a worse answer than typing.
    pub fn user_types_in(&self, schema: Option<&str>) -> Vec<String> {
        let mut out: Vec<String> = self
            .enums
            .iter()
            .filter(|e| e.schema.as_deref() == schema)
            .map(|e| e.name.clone())
            .chain(
                self.domains
                    .iter()
                    .filter(|d| d.schema.as_deref() == schema)
                    .map(|d| d.name.clone()),
            )
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// The introspected table with this `(namespace, name)` identity.
    ///
    /// An exact namespace match wins. When the caller has no namespace to offer —
    /// MySQL, or a tab restored from a session file written before multi-schema
    /// browsing — it falls back to the name alone, preferring `public` so the
    /// common case resolves the way it always did rather than to whichever
    /// same-named table happens to come first.
    pub fn find_table(&self, schema: Option<&str>, name: &str) -> Option<&TableInfo> {
        find_by_ns(&self.tables, schema, name, |t| {
            (t.schema.as_deref(), t.name.as_str())
        })
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

    /// A `CREATE` script for everything in one namespace, blank-line separated.
    ///
    /// **In dependency order**: the standalone types first, then base tables,
    /// then views, then the sequences that stand on their own. A view's body
    /// references the tables it selects from, and a column's type may *be* one
    /// of the namespace's enums or domains — `format_type` prints that
    /// qualified, so the script names it — which is the ordering that matters
    /// most: an omitted foreign key leaves a script that still runs, an omitted
    /// type leaves one that fails on its first `CREATE TABLE`. Foreign keys
    /// aren't emitted by [`TableInfo::create_ddl`] at all, so ordering *between*
    /// base tables doesn't affect validity.
    ///
    /// A sequence created by a `serial` or an identity column is skipped
    /// ([`ObjectItem::is_internal`], plus the `serial`'s own owner): the
    /// column's definition creates it, and restating it makes the script fail
    /// on a name that already exists.
    ///
    /// Empty when the namespace holds nothing.
    pub fn create_ddl_script(
        &self,
        schema: Option<&str>,
        dialect: crate::intel::SqlDialect,
    ) -> String {
        use crate::ddl::ObjectKind;
        // **Three ways, through `shape()`.** A two-way `partition(|t| t.is_view)`
        // put a MariaDB sequence in the base-table half — harmless only because
        // `create_ddl` then withholds it, which is a second function's accident
        // rather than this one's decision. A sequence in this script is the
        // `objects_in(Sequence)` list's business, below.
        let (views, tables): (Vec<&TableInfo>, Vec<&TableInfo>) = self
            .tables_in(schema)
            .filter(|t| t.shape() != TableShape::Sequence)
            .partition(|t| t.shape() == TableShape::View);
        let types: Vec<String> = [ObjectKind::Enum, ObjectKind::Domain]
            .into_iter()
            .flat_map(|k| self.objects_in(schema, k))
            .map(|o| o.create_sql(dialect))
            .collect();
        // A sequence a table in this script already owns is created by that
        // table's column, whether or not the catalogue calls the link internal.
        let owned_here: std::collections::HashSet<&str> =
            tables.iter().map(|t| t.name.as_str()).collect();
        let seqs: Vec<String> = self
            .objects_in(schema, ObjectKind::Sequence)
            .into_iter()
            .filter(|o| !o.is_internal())
            .filter(|o| match o {
                ObjectItem::Sequence(s) => s
                    .owned_by
                    .as_ref()
                    .is_none_or(|w| !owned_here.contains(w.table.as_str())),
                _ => true,
            })
            .map(|o| o.create_sql(dialect))
            .collect();
        // **Last**, because a routine body names the tables and views above it.
        // The order is what makes the script runnable end to end rather than a
        // set of statements that happen to be here. Events come after the
        // routines within that group, on the same rule: an event's body may call
        // one. They are MySQL's alone, so they contribute nothing on the other
        // two engines — and leaving them off this list was a script that
        // restored every table and silently dropped every scheduled job.
        let routines: Vec<String> = [
            ObjectKind::Function,
            ObjectKind::Procedure,
            ObjectKind::Event,
        ]
        .into_iter()
        .flat_map(|k| self.objects_in(schema, k))
        .map(|o| o.create_sql(dialect))
        .collect();
        types
            .into_iter()
            .chain(tables.into_iter().map(|t| t.create_ddl(dialect)))
            .chain(views.into_iter().map(|t| t.create_ddl(dialect)))
            .chain(seqs)
            .chain(routines)
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// A `CREATE` script for the **whole database** — the database node's
    /// analogue of [`DbSchema::create_ddl_script`], which covers one namespace.
    ///
    /// Namespaces are walked in [`DbSchema::schemas`] order, which is the order
    /// the tree shows them (`public` first, then alphabetical), so the script
    /// reads down the tree it was raised from. Where an engine has no
    /// namespaces at all — MySQL and SQLite, whose tables all carry `None` —
    /// there is nothing to walk and this *is* the flat script.
    ///
    /// Ordering **between** namespaces is display order rather than dependency
    /// order: a type in one namespace used by a table in another is emitted
    /// after it if the alphabet says so. That is the same class of gap
    /// `create_ddl_script` already documents for foreign keys — the script is
    /// read and edited before it is run, and the DDL preview is what runs
    /// anything.
    ///
    /// Empty when the database holds nothing.
    pub fn create_ddl_script_all(&self, dialect: crate::intel::SqlDialect) -> String {
        let namespaces = self.schemas();
        if namespaces.is_empty() {
            return self.create_ddl_script(None, dialect);
        }
        namespaces
            .iter()
            .map(|ns| self.create_ddl_script(Some(ns), dialect))
            // An empty namespace contributes nothing rather than a blank run:
            // `join` over the parts that exist, not over every namespace.
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Every namespace present, in display order (`public` first, then
    /// alphabetical). Empty on MySQL, where tables carry no namespace — which is
    /// how the schema tree decides whether to render a schema level at all.
    pub fn schemas(&self) -> Vec<String> {
        // Every kind of object contributes, not just tables: a namespace holding
        // only types or sequences is still a namespace, and leaving it out would
        // make its contents unreachable in the tree.
        let mut out: Vec<String> = self
            .tables
            .iter()
            .filter_map(|t| t.schema.clone())
            .chain(self.enums.iter().filter_map(|e| e.schema.clone()))
            .chain(self.domains.iter().filter_map(|d| d.schema.clone()))
            .chain(self.sequences.iter().filter_map(|s| s.schema.clone()))
            .chain(self.routines.iter().filter_map(|r| r.schema.clone()))
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
/// `int(11) unsigned`, `character varying(45)`, `text[]`) by the type it names.
/// Case-, modifier- and array-insensitive; note MySQL `bool`/`boolean` is a
/// `tinyint(1)` alias, so only the literal `bool`/`boolean` spelling maps to
/// [`ColumnTypeClass::Boolean`] (a bare `tinyint` is
/// [`ColumnTypeClass::Numeric`]).
///
/// **Two passes, because a type name is not one word.** This took the leading
/// run of `[A-Za-z0-9_]` and matched that alone, which is right for every MySQL
/// spelling and wrong for PostgreSQL's, where `format_type` writes
/// `character varying(45)`, `double precision` and `timestamp with time zone`.
/// The leading word of the first is `character`, which matched nothing — so
/// [`ColumnTypeClass::Other`] and its "unrecognised type" glyph were what every
/// PostgreSQL connection showed for its most common column type, in the schema
/// tree, the ER diagram's cards and tooltips, the completion popup and
/// Find-Anywhere at once. The whole phrase is tried first and the leading word
/// second, so a name the second pass already handled (`int unsigned`,
/// `timestamp with time zone`) still answers exactly as it did.
pub fn classify_column_type(type_name: &str) -> ColumnTypeClass {
    // Anything from the first `(` is a modifier — a width, a precision, an
    // enum's members — and says nothing about the family. `timestamp(3) with
    // time zone` loses its tail with it, which is harmless: the leading word
    // answers that one.
    let head = type_name.split('(').next().unwrap_or_default();
    // **An array is its element type.** The icon is about what a cell holds, and
    // a `text[]` cell holds text; `format_type` writes any number of dimensions
    // as repeated brackets.
    let head = head.trim().trim_end_matches("[]").trim();
    let phrase = head.split_whitespace().collect::<Vec<_>>().join(" ");
    let phrase = phrase.to_ascii_lowercase();
    if let Some(c) = type_keyword_class(&phrase) {
        return c;
    }
    type_keyword_class(phrase.split(' ').next().unwrap_or_default())
        .unwrap_or(ColumnTypeClass::Other)
}

/// One type name — a whole phrase or a single leading word — to its family, or
/// `None` for a name this doesn't know.
///
/// `None` rather than [`ColumnTypeClass::Other`] because
/// [`classify_column_type`] asks twice and has to tell "not recognised" from
/// "recognised as unclassifiable" to know whether the second ask is worth
/// making.
fn type_keyword_class(name: &str) -> Option<ColumnTypeClass> {
    Some(match name {
        "bool" | "boolean" => ColumnTypeClass::Boolean,
        "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint" | "decimal" | "dec"
        | "numeric" | "fixed" | "float" | "double" | "real" | "bit" => ColumnTypeClass::Numeric,
        // PostgreSQL's own numerics. `money` is a fixed-point currency value,
        // `oid` an object identifier, and the three `serial`s are the notation a
        // draft uses for an integer with a sequence behind it.
        "double precision" | "money" | "oid" | "smallserial" | "serial" | "bigserial"
        | "bit varying" => ColumnTypeClass::Numeric,
        // **SQLite's own documented spellings**, and the engine where they
        // matter most: SQLite stores the declared type text verbatim, so an
        // unusual spelling is normal there rather than exotic, and every one of
        // these is an example in its type-affinity documentation. `int2`/`int8`
        // and `float4`/`float8` are also PostgreSQL's internal aliases for
        // `smallint`/`bigint` and `real`/`double precision`.
        "unsigned big int" | "unsigned" | "int2" | "int4" | "int8" | "float4" | "float8" => {
            ColumnTypeClass::Numeric
        }
        "char" | "varchar" | "tinytext" | "text" | "mediumtext" | "longtext" | "enum" | "set" => {
            ColumnTypeClass::Text
        }
        // PostgreSQL's string family, as `format_type` spells it: `character
        // varying` and `character` are what `varchar`/`char` come back as,
        // `bpchar` is the catalogue's internal name for the padded one, and
        // `name` is the type every identifier column in the catalogue has.
        // `uuid`, `xml` and the network types are grouped here because a cell of
        // one is text on screen and text in a filter — the icon says "a string
        // you can read", which is true of all of them.
        "character varying" | "character" | "bpchar" | "name" | "uuid" | "xml" | "inet"
        | "cidr" | "macaddr" | "macaddr8" => ColumnTypeClass::Text,
        // SQLite's, from the same documentation as the numerics above.
        // `varying character` and `native character` are listed as whole
        // phrases as well as by their leading word, because the second pass
        // tries `varying`/`native` alone and neither means anything else here.
        "varying character" | "varying" | "native character" | "native" | "nvarchar" | "nchar"
        | "clob" => ColumnTypeClass::Text,
        "date" | "datetime" | "time" | "timestamp" | "year" => ColumnTypeClass::DateTime,
        // PostgreSQL's, and the reason `interval` is here rather than under
        // numerics: it is a span of time, and it renders as one.
        "interval" | "timestamptz" | "timetz" => ColumnTypeClass::DateTime,
        "json" | "geometry" | "geomcollection" | "geometrycollection" | "point" | "linestring"
        | "polygon" | "multipoint" | "multilinestring" | "multipolygon" => ColumnTypeClass::Json,
        // `jsonb` is the one PostgreSQL actually stores, and the geometric types
        // beside it are its own rather than the OpenGIS set above.
        "jsonb" | "line" | "lseg" | "box" | "path" | "circle" => ColumnTypeClass::Json,
        "blob" | "tinyblob" | "mediumblob" | "longblob" | "binary" | "varbinary" => {
            ColumnTypeClass::Binary
        }
        // PostgreSQL's one binary type.
        "bytea" => ColumnTypeClass::Binary,
        _ => return None,
    })
}

/// The five affinities SQLite assigns a column from its **declared type text**.
///
/// SQLite has no column types — a cell of any storage class can go in any
/// column — but the declared text still decides which storage class the engine
/// *prefers*, and that is the only thing a reader can ask about the column
/// itself. Unlike MySQL and PostgreSQL, the text is arbitrary: `MEDIUMBLOB`,
/// `VARBINARY(16)` and a column declared with **no type at all** are all things
/// a SQLite table can say, and only the affinity rule sorts them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqliteAffinity {
    Integer,
    Text,
    /// Raw bytes — also what a column with no declared type gets.
    Blob,
    Real,
    Numeric,
}

/// A PostgreSQL type name as `format_type` writes it, split into `(namespace,
/// name)` **and unquoted** — the form the catalogue holds.
///
/// `format_type` quotes what needs quoting, and the naive `rsplit_once('.')` this
/// replaced kept the quotes: a type named `MyMood` arrived as `"MyMood"` and was
/// looked up under that spelling, which no catalogue row has, so every
/// mixed-case enum type silently kept the plain text field instead of its
/// dropdown. A quoted name may also *contain* the separator (`"a.b"`), where
/// splitting on the last dot picks a namespace out of the middle of one name.
///
/// `None` when the text isn't one or two identifiers — an unterminated quote, a
/// third part, trailing text. A name this cannot read is a name to leave alone,
/// not to guess at.
///
/// **It lives here because two readers of that same string need it**, and the
/// second one was still splitting on the last dot: `celledit::editor_for_column`
/// decides whether a column gets an enum dropdown, and `ddl::type_dependents`
/// decides which columns an enum recreate has to re-cast — and *that* one
/// answering "none" makes the preview say nothing uses the type and the plan
/// emit no `ALTER COLUMN`, so the `DROP TYPE` fails on a dependent the plan
/// never mentioned. One reader for the catalogue's own spelling.
pub(crate) fn split_type_name(declared: &str) -> Option<(Option<String>, String)> {
    let (first, rest) = type_ident(declared)?;
    if rest.is_empty() {
        return Some((None, first));
    }
    let (second, rest) = type_ident(rest.strip_prefix('.')?)?;
    rest.is_empty().then_some((Some(first), second))
}

/// One identifier off the front: a `"…"` run (in which `""` is one literal quote
/// and a `.` is an ordinary character), or everything up to the next `.`.
///
/// The bare arm keeps spaces, because a bare multi-word type name (`character
/// varying`) is one name — it will simply match no enum.
fn type_ident(s: &str) -> Option<(String, &str)> {
    let Some(body) = s.strip_prefix('"') else {
        let end = s.find('.').unwrap_or(s.len());
        return (!s.is_empty()).then(|| (s[..end].to_string(), &s[end..]));
    };
    let (mut out, mut rest) = (String::new(), body);
    loop {
        let q = rest.find('"')?;
        out.push_str(&rest[..q]);
        rest = &rest[q + 1..];
        match rest.strip_prefix('"') {
            // A doubled quote inside the run is one quote of the name itself.
            Some(r) => {
                out.push('"');
                rest = r;
            }
            None => return Some((out, rest)),
        }
    }
}

/// Which affinity SQLite gives a column declared `declared`.
///
/// This is [the five rules of *Determination of Column
/// Affinity*](https://sqlite.org/datatype3.html#determination_of_column_affinity),
/// in order and case-insensitively, and the order is the whole algorithm: it is
/// why `VARCHAR` is TEXT despite containing `CHAR` *and* nothing else, and why
/// `POINT` — which contains neither `INT` at the start nor any other keyword —
/// is INTEGER, not NUMERIC.
///
/// It lives here rather than in the backend because two separate readings of a
/// declared type depend on it: whether a column holds bytes the grid must not
/// let anyone type over, and what an imported CSV value should be coerced to.
/// Both used to spell their own narrower test inline.
pub fn sqlite_affinity(declared: &str) -> SqliteAffinity {
    let t = declared.trim().to_ascii_uppercase();
    if t.contains("INT") {
        SqliteAffinity::Integer
    } else if t.contains("CHAR") || t.contains("CLOB") || t.contains("TEXT") {
        SqliteAffinity::Text
    } else if t.is_empty() || t.contains("BLOB") {
        SqliteAffinity::Blob
    } else if t.contains("REAL") || t.contains("FLOA") || t.contains("DOUB") {
        SqliteAffinity::Real
    } else {
        SqliteAffinity::Numeric
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

impl SchemaState {
    /// What to write to the state signal when a re-introspection of this
    /// database starts — or `None` to leave it exactly as it is, which is the
    /// answer whenever there is already something on screen.
    ///
    /// A refresh re-fetches the whole database (10 catalogue round-trips on
    /// MySQL, 8 on PostgreSQL), and dropping to [`SchemaState::Loading`] for
    /// its duration replaces **every** table and column row under that database
    /// with one "Loading" row. That is a flash locally (measured: 48 ms for 600
    /// tables / 12.6k columns on MySQL, 134 ms on PostgreSQL) and most of a
    /// second over a tunnel, where the round-trips dominate — and it happens
    /// after every schema edit, since applying DDL refreshes too. The rows are
    /// still accurate for as long as it takes; showing them beats blanking.
    ///
    /// It is `Option`, rather than a state to write unconditionally, because a
    /// floem signal **never dedups**: writing an equal `Loaded` back would
    /// notify all the same, disposing and rebuilding the subtree the refresh is
    /// meant to leave alone — the blanking's cost without even the blank. Not
    /// writing is the only way to keep it.
    ///
    /// Nothing marks the row as busy meanwhile, deliberately: at these durations
    /// an indicator is a flicker of a glyph for a frame or two, which reads as a
    /// rendering fault rather than as progress.
    ///
    /// A database with nothing to show — never loaded, or last seen failed —
    /// loads as before.
    pub fn begin_refresh(&self) -> Option<SchemaState> {
        match self {
            // Already showing rows, or already showing a load in progress.
            SchemaState::Loaded(_) | SchemaState::Loading => None,
            SchemaState::Failed(_) => Some(SchemaState::Loading),
        }
    }
}

/// How long a catalogue read must have been running before the schema panel's
/// header says so.
///
/// **Not zero, and that is the whole of the design.** [`SchemaState::
/// begin_refresh`] measures a local refresh at 48 ms on MySQL and 134 ms on
/// PostgreSQL, and states the consequence for the *rows*: a glyph that appears
/// and vanishes inside that reads as a rendering fault rather than as progress,
/// so the rows deliberately show nothing. A header indicator that appeared
/// instantly would inherit exactly that problem — and worse, because applying
/// any DDL refreshes, so it would strobe after every schema edit.
///
/// The header is still the right place to report a read that is genuinely
/// *long*: the rows of an already-loaded database stay on screen through a
/// refresh (that is what `begin_refresh` preserves), so without this nothing
/// anywhere says a multi-second re-introspection is in flight. 400 ms is past
/// every measured local refresh and roughly where a wait stops feeling
/// instantaneous.
pub const READ_NOTICE_DELAY: std::time::Duration = std::time::Duration::from_millis(400);

/// Should the schema panel's header report a catalogue read in progress?
///
/// Both terms matter: a read that has finished says nothing however long it
/// ran, and a read still running says nothing until it has outlasted
/// [`READ_NOTICE_DELAY`].
pub fn report_read(reading: bool, for_how_long: std::time::Duration) -> bool {
    reading && for_how_long >= READ_NOTICE_DELAY
}

#[cfg(test)]
mod trigger_tests {
    use super::*;
    use crate::intel::SqlDialect;

    fn mysql_trigger() -> TriggerInfo {
        TriggerInfo {
            name: "audit_ins".into(),
            table: "orders".into(),
            timing: TriggerTiming::Before,
            events: vec![TriggerEvent::Insert],
            action: TriggerAction::Body("SET NEW.created = NOW()".into()),
            definer: Some("root@localhost".into()),
            ..Default::default()
        }
    }

    fn pg_trigger() -> TriggerInfo {
        TriggerInfo {
            name: "audit_upd".into(),
            schema: Some("public".into()),
            table: "orders".into(),
            timing: TriggerTiming::After,
            events: vec![TriggerEvent::Insert, TriggerEvent::Update],
            level: TriggerLevel::Row,
            action: TriggerAction::Function {
                name: "audit_fn".into(),
                args: vec![],
            },
            ..Default::default()
        }
    }

    #[test]
    fn default_trigger_is_enabled() {
        // The opposite default would append a DISABLE TRIGGER to every create.
        assert_eq!(TriggerInfo::default().enabled, TriggerEnabled::Origin);
        assert!(TriggerInfo::default().enabled.fires_normally());
    }

    #[test]
    fn mysql_create_carries_definer_and_body() {
        let sql = mysql_trigger().create_sql(SqlDialect::MySql);
        assert!(sql.starts_with("CREATE DEFINER = `root`@`localhost` TRIGGER `audit_ins` "));
        assert!(sql.contains("BEFORE INSERT ON `orders`"));
        assert!(sql.contains("FOR EACH ROW"));
        assert!(sql.trim_end().ends_with("SET NEW.created = NOW();"));
    }

    #[test]
    fn mysql_ordering_sits_between_for_each_row_and_the_body() {
        let mut t = mysql_trigger();
        t.order = Some(TriggerOrder::Follows("other".into()));
        let sql = t.create_sql(SqlDialect::MySql);
        assert!(
            sql.contains("FOR EACH ROW FOLLOWS `other`\nSET NEW.created"),
            "{sql}"
        );
    }

    #[test]
    fn pg_joins_events_with_or_and_omits_public() {
        let sql = pg_trigger().create_sql(SqlDialect::Postgres);
        assert!(
            sql.contains("AFTER INSERT OR UPDATE ON \"orders\""),
            "{sql}"
        );
        // `public` is on the default search_path — same rule as sql_qualifier.
        assert!(!sql.contains("\"public\""), "{sql}");
        assert!(sql.contains("EXECUTE FUNCTION audit_fn();"), "{sql}");
    }

    #[test]
    fn pg_update_of_columns_rides_inside_the_event() {
        let mut t = pg_trigger();
        t.events = vec![TriggerEvent::Update];
        t.update_columns = vec!["total".into(), "status".into()];
        let sql = t.create_sql(SqlDialect::Postgres);
        assert!(
            sql.contains("AFTER UPDATE OF \"total\", \"status\" ON"),
            "{sql}"
        );
    }

    #[test]
    fn pg_when_is_wrapped_exactly_once() {
        let mut t = pg_trigger();
        // Held bare in the model; the emitter is the only thing that parenthesises.
        t.condition = Some("new.total > 0".into());
        let sql = t.create_sql(SqlDialect::Postgres);
        assert!(sql.contains("\nWHEN (\nnew.total > 0\n)\n"), "{sql}");
        assert!(!sql.contains("((new.total > 0))"), "{sql}");
    }

    /// **A guard is arbitrary SQL and may end in a line comment**, which is why
    /// the group closes on a line of its own: `WHEN (NEW.a > 0 -- why)` puts the
    /// closing paren inside the comment, and the engine then fails on whatever
    /// follows — `near "BEGIN": syntax error`, which is not where the problem is.
    #[test]
    fn a_when_guard_ending_in_a_comment_still_closes_its_group() {
        for dialect in [SqlDialect::Postgres, SqlDialect::Sqlite] {
            let mut t = pg_trigger();
            if dialect == SqlDialect::Sqlite {
                t.schema = None;
                t.action = TriggerAction::Body("BEGIN UPDATE t SET b = 1; END".into());
            }
            t.condition = Some("NEW.a > 0 -- only positives".into());
            let sql = t.create_sql(dialect);
            let at = sql.find("WHEN (").expect("a WHEN group") + "WHEN ".len();
            assert_eq!(
                crate::sql::balanced_paren_span(sql.as_bytes(), at, dialect),
                sql[at..].find("\n)").map(|i| at + i + 1),
                "the group must close at a code position:\n{sql}"
            );
        }
    }

    #[test]
    fn pg_disabled_trigger_restates_the_disable() {
        let mut t = pg_trigger();
        t.enabled = TriggerEnabled::Disabled;
        let sql = t.create_sql(SqlDialect::Postgres);
        // CREATE TRIGGER always makes an enabled one, so stopping at the create
        // would silently switch it back on.
        assert!(
            sql.contains("ALTER TABLE \"orders\" DISABLE TRIGGER \"audit_upd\";"),
            "{sql}"
        );
    }

    /// `tgenabled` has four states, not two. Folding `A`/`R` into "enabled"
    /// recreates them as plain `O`, and a trigger the DBA set to fire on a
    /// replica *stops firing during replication apply* — silently, on a plan
    /// the user asked for something else entirely.
    #[test]
    fn pg_always_and_replica_triggers_restate_their_firing_mode() {
        for (state, clause) in [
            (TriggerEnabled::Always, "ENABLE ALWAYS TRIGGER"),
            (TriggerEnabled::Replica, "ENABLE REPLICA TRIGGER"),
            (TriggerEnabled::Disabled, "DISABLE TRIGGER"),
        ] {
            let mut t = pg_trigger();
            t.enabled = state;
            let sql = t.create_sql(SqlDialect::Postgres);
            assert!(
                sql.contains(&format!("ALTER TABLE \"orders\" {clause} \"audit_upd\";")),
                "{state:?}: {sql}"
            );
        }
        // The ordinary state says nothing — `CREATE TRIGGER` already made one.
        let sql = pg_trigger().create_sql(SqlDialect::Postgres);
        assert!(!sql.contains("ALTER TABLE"), "{sql}");
    }

    /// Regression: `REFERENCING OLD/NEW TABLE` was not modelled, so recreating
    /// such a trigger succeeded and dropped the clause — after which **every
    /// write to the table fails** with `relation "o" does not exist`, because
    /// the function body still references the transition table.
    #[test]
    fn pg_transition_tables_survive_a_recreate() {
        let mut t = pg_trigger();
        t.level = TriggerLevel::Statement;
        t.old_table = Some("o".into());
        t.new_table = Some("n".into());
        let sql = t.create_sql(SqlDialect::Postgres);
        assert!(
            sql.contains("REFERENCING OLD TABLE AS \"o\" NEW TABLE AS \"n\""),
            "{sql}"
        );
        // PostgreSQL wants the clause between `ON table` and `FOR EACH`.
        let refs = sql.find("REFERENCING").expect("clause");
        let each = sql.find("FOR EACH").expect("level");
        assert!(refs < each, "{sql}");
        assert!(sql.find("ON \"orders\"").expect("table") < refs, "{sql}");
    }

    #[test]
    fn pg_function_args_are_quoted_as_literals() {
        let mut t = pg_trigger();
        t.action = TriggerAction::Function {
            name: "audit_fn".into(),
            args: vec!["orders".into(), "it's".into()],
        };
        let sql = t.create_sql(SqlDialect::Postgres);
        assert!(
            sql.contains("EXECUTE FUNCTION audit_fn('orders', 'it''s');"),
            "{sql}"
        );
    }

    #[test]
    fn constraint_trigger_keeps_its_keyword() {
        let mut t = pg_trigger();
        t.constraint = true;
        assert!(
            t.create_sql(SqlDialect::Postgres)
                .starts_with("CREATE CONSTRAINT TRIGGER ")
        );
    }

    /// The engines can't hold each other's action shape. Emitting something that
    /// looks like SQL and isn't is worse than saying so.
    #[test]
    fn a_mismatched_action_reports_instead_of_emitting_nonsense() {
        let mut my = mysql_trigger();
        my.action = TriggerAction::Function {
            name: "f".into(),
            args: vec![],
        };
        assert!(
            my.create_sql(SqlDialect::MySql)
                .contains("can't call the function f")
        );

        let mut pg = pg_trigger();
        pg.action = TriggerAction::Body("SET x = 1".into());
        let sql = pg.create_sql(SqlDialect::Postgres);
        assert!(sql.starts_with("-- Schemaic can't emit"), "{sql}");
        assert!(!sql.contains("EXECUTE FUNCTION"), "{sql}");
    }

    #[test]
    fn timing_and_event_parse_round_trip_and_reject_the_unknown() {
        for t in [
            TriggerTiming::Before,
            TriggerTiming::After,
            TriggerTiming::InsteadOf,
        ] {
            assert_eq!(TriggerTiming::parse(t.sql()), Some(t));
            assert_eq!(TriggerTiming::parse(&t.sql().to_ascii_lowercase()), Some(t));
        }
        assert_eq!(
            TriggerTiming::parse("INSTEAD_OF"),
            Some(TriggerTiming::InsteadOf)
        );
        assert_eq!(TriggerTiming::parse("SIDEWAYS"), None);

        for e in [
            TriggerEvent::Insert,
            TriggerEvent::Update,
            TriggerEvent::Delete,
            TriggerEvent::Truncate,
        ] {
            assert_eq!(TriggerEvent::parse(e.sql()), Some(e));
        }
        assert_eq!(TriggerEvent::parse("MERGE"), None);
    }
}

#[cfg(test)]
mod schema_state_tests {
    use super::*;

    fn schema() -> std::sync::Arc<DbSchema> {
        std::sync::Arc::new(DbSchema {
            tables: vec![TableInfo {
                name: "orders".into(),
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    /// The point of the method: a re-introspection of a database already on
    /// screen writes nothing, so the tree neither blanks nor rebuilds.
    #[test]
    fn a_loaded_schema_is_left_alone_by_the_start_of_a_refresh() {
        assert!(SchemaState::Loaded(schema()).begin_refresh().is_none());
    }

    /// A refresh landing on a load already in flight is the same answer, for the
    /// same reason: `Loading` written over `Loading` still notifies.
    #[test]
    fn a_load_already_in_flight_is_left_alone() {
        assert!(SchemaState::Loading.begin_refresh().is_none());
    }

    /// A failed database has no rows to keep, so it shows the retry rather than
    /// a stale error.
    #[test]
    fn a_failed_database_goes_back_to_loading() {
        assert!(matches!(
            SchemaState::Failed("gone".into()).begin_refresh(),
            Some(SchemaState::Loading)
        ));
    }
}

#[cfg(test)]
mod browse_key_tests {
    use super::*;

    fn col(name: &str, nullable: bool, pk: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            type_name: "TEXT".into(),
            nullable,
            primary_key: pk,
            ..Default::default()
        }
    }

    fn table(columns: Vec<ColumnInfo>, indexes: Vec<IndexInfo>) -> TableInfo {
        TableInfo {
            name: "u".into(),
            columns,
            indexes,
            implicit_key: Some("rowid".into()),
            ..Default::default()
        }
    }

    /// **A partial unique index is not a key.** `CREATE UNIQUE INDEX ux ON t
    /// (email) WHERE deleted_at IS NULL` promises uniqueness over the rows the
    /// `WHERE` admits and nothing about the rest, so any number of soft-deleted
    /// rows may share an email. The table has no key of its own and must be
    /// browsed by its implicit one.
    #[test]
    fn a_partial_unique_index_is_not_a_key() {
        let mut ix = IndexInfo::plain("ux", vec!["email"], true);
        ix.predicate = Some("deleted_at IS NULL".into());
        let t = table(
            vec![col("email", false, false), col("deleted_at", true, false)],
            vec![ix],
        );
        assert!(
            browse_key_columns(&t).is_empty(),
            "{:?}",
            browse_key_columns(&t)
        );
    }

    /// **A lossy one is not a key either**, and this is the case a partial index
    /// takes on SQLite: `db::sqlite` leaves the predicate unread on purpose and
    /// sets `lossy` instead, so `predicate.is_none()` is true and `lossy` is the
    /// only thing saying the index is not what `columns` says it is.
    #[test]
    fn a_lossy_unique_index_is_not_a_key() {
        let mut ix = IndexInfo::plain("ux", vec!["email"], true);
        ix.lossy = true;
        let t = table(
            vec![col("email", false, false), col("name", true, false)],
            vec![ix],
        );
        assert!(
            browse_key_columns(&t).is_empty(),
            "{:?}",
            browse_key_columns(&t)
        );
    }

    /// **And one collated differently from its column is not**, which is the
    /// SQLite case:
    ///
    /// ```sql
    /// CREATE TABLE t (email TEXT NOT NULL COLLATE NOCASE, note TEXT);
    /// CREATE UNIQUE INDEX ux ON t (email COLLATE BINARY);
    /// INSERT INTO t VALUES ('A@x','one'), ('a@x','two');   -- both accepted
    /// ```
    ///
    /// The index is unique in `BINARY`, so both rows fit under it; the `WHERE
    /// email = 'A@x'` the write builds from that key is measured in the
    /// *column's* `NOCASE` and matches both. Offering it as a key sends the
    /// edit into the 1-row net and reports a failure whose stated reason is not
    /// the reason — the outcome `identifies_a_row` exists to end.
    #[test]
    fn a_unique_index_collated_differently_from_its_column_is_not_a_key() {
        let mut ix = IndexInfo::plain("ux", vec!["email"], true);
        ix.columns[0].collation = Some("BINARY".into());
        let t = table(
            vec![col("email", false, false), col("note", true, false)],
            vec![ix.clone()],
        );
        assert!(!ix.identifies_a_row());
        assert!(
            browse_key_columns(&t).is_empty(),
            "{:?}",
            browse_key_columns(&t)
        );
        // The counterweight: with no per-key collation — which is what MySQL
        // always reports and what SQLite reports when the index agrees with the
        // column — the same index is still a key.
        let plain = IndexInfo::plain("ux", vec!["email"], true);
        assert!(plain.identifies_a_row());
        let t = table(
            vec![col("email", false, false), col("note", true, false)],
            vec![plain],
        );
        assert_eq!(browse_key_columns(&t), ["email"]);
    }

    /// **And a *mixed* one is not**, which the all-expression case already
    /// covered by accident. `CREATE UNIQUE INDEX ux ON t (a, lower(b))`
    /// constrains the pair; `column_names()` drops the expression and leaves
    /// `[a]`, which is not unique — measured on PostgreSQL 16.15, two rows with
    /// `a = 1` coexist under it.
    #[test]
    fn a_unique_index_over_a_column_and_an_expression_is_not_a_key() {
        let ix = IndexInfo {
            name: "ux".into(),
            unique: true,
            columns: vec![
                IndexColumn::plain("a"),
                IndexColumn {
                    name: "lower(b)".into(),
                    expression: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let t = table(
            vec![col("a", false, false), col("b", false, false)],
            vec![ix],
        );
        assert!(
            browse_key_columns(&t).is_empty(),
            "{:?}",
            browse_key_columns(&t)
        );
    }

    /// The three refusals above must not have cost the arm they sit in: an
    /// ordinary unique index over a `NOT NULL` column is still a key, and one
    /// that sorts *after* a refused index is still reached.
    #[test]
    fn a_sound_unique_index_after_a_refused_one_is_still_the_key() {
        let mut partial = IndexInfo::plain("ux_partial", vec!["email"], true);
        partial.predicate = Some("deleted_at IS NULL".into());
        let t = table(
            vec![
                col("email", false, false),
                col("code", false, false),
                col("deleted_at", true, false),
            ],
            vec![partial, IndexInfo::plain("ux_code", vec!["code"], true)],
        );
        assert_eq!(browse_key_columns(&t), vec!["code".to_string()]);
    }

    #[test]
    fn a_primary_key_wins() {
        let t = table(
            vec![col("id", false, true), col("email", false, false)],
            vec![IndexInfo::plain("uq", vec!["email"], true)],
        );
        assert_eq!(browse_key_columns(&t), vec!["id".to_string()]);
    }

    /// **The arm that was missing.** `CREATE TABLE u (email TEXT NOT NULL UNIQUE,
    /// name TEXT)` has a perfectly good row key, and the browse gate asked only
    /// about the primary key — so the tab opened `SELECT rowid, * … ORDER BY
    /// rowid`, carrying a rowid column into the grid, every export and every
    /// copy, while the write path keyed on `email` and never looked at it.
    #[test]
    fn a_unique_not_null_index_is_a_key() {
        let t = table(
            vec![col("email", false, false), col("name", true, false)],
            vec![IndexInfo::plain("uq", vec!["email"], true)],
        );
        assert_eq!(browse_key_columns(&t), vec!["email".to_string()]);
    }

    /// A *nullable* unique column identifies nothing: SQL lets any number of
    /// rows share a NULL there. This is the same rule `edit::resolve_key`
    /// applies, and the reason the two have to be one function.
    #[test]
    fn a_nullable_unique_index_is_not_a_key() {
        let t = table(
            vec![col("email", true, false), col("name", true, false)],
            vec![IndexInfo::plain("uq", vec!["email"], true)],
        );
        assert!(browse_key_columns(&t).is_empty());
    }

    #[test]
    fn a_non_unique_or_foreign_index_is_not_a_key() {
        let plain = table(
            vec![col("email", false, false)],
            vec![IndexInfo::plain("ix", vec!["email"], false)],
        );
        assert!(browse_key_columns(&plain).is_empty());

        let mut fk = IndexInfo::plain("fk", vec!["email"], true);
        fk.foreign = true;
        assert!(browse_key_columns(&table(vec![col("email", false, false)], vec![fk])).is_empty());
    }

    #[test]
    fn a_table_with_neither_has_no_key_of_its_own() {
        let t = table(vec![col("a", true, false), col("b", true, false)], vec![]);
        assert!(browse_key_columns(&t).is_empty());
    }

    /// **An expression-only unique index keys nothing, and must not end the
    /// search.** PostgreSQL models `CREATE UNIQUE INDEX ON u (lower(email))` as
    /// a real index over one expression; `column_names()` skips expressions, so
    /// the NOT NULL predicate was vacuously true for it and `find` stopped
    /// there and answered with an *empty* key — hiding the plain unique index
    /// that sorted after it, and opening a perfectly keyed table read-only.
    #[test]
    fn an_expression_only_unique_index_does_not_hide_a_real_one() {
        let expr = IndexInfo {
            name: "a_expr".into(),
            unique: true,
            columns: vec![IndexColumn::expr("lower(email)")],
            ..Default::default()
        };
        let t = table(
            vec![col("email", false, false), col("code", false, false)],
            vec![expr.clone(), IndexInfo::plain("b_code", vec!["code"], true)],
        );
        assert_eq!(browse_key_columns(&t), vec!["code".to_string()]);

        // On its own it is simply not a key.
        let alone = table(vec![col("email", false, false)], vec![expr]);
        assert!(browse_key_columns(&alone).is_empty());
    }

    /// A composite unique index comes back whole and in key order.
    #[test]
    fn a_composite_unique_index_keeps_its_order() {
        let t = table(
            vec![col("a", false, false), col("b", false, false)],
            vec![IndexInfo::plain("uq", vec!["b", "a"], true)],
        );
        assert_eq!(
            browse_key_columns(&t),
            vec!["b".to_string(), "a".to_string()]
        );
    }
}

/// Which values may stand in a `DEFAULT` clause bare, and which the emitter has
/// to parenthesise. Two engines require the parentheses and neither hands them
/// back with the value, so both halves are pinned here.
#[cfg(test)]
mod default_clause_tests {
    use super::*;

    /// Everything SQLite's grammar lets stand without parentheses.
    #[test]
    fn the_literals_stand_bare() {
        for d in [
            "NULL",
            "null",
            "TRUE",
            "FALSE",
            "CURRENT_TIME",
            "CURRENT_DATE",
            "CURRENT_TIMESTAMP",
            "'hi'",
            "''",
            "'it''s'",
            "X'00FF'",
            "x'00ff'",
            "0",
            "3",
            "-1",
            "+7",
            "1.5",
            ".5",
            "1e-3",
            "1E+10",
            "0xFF",
            "0X1a",
        ] {
            assert!(is_bare_default(d, crate::intel::SqlDialect::Sqlite), "{d}");
        }
        // Already parenthesised, so nothing to add.
        assert!(is_bare_default(
            "(datetime('now'))",
            crate::intel::SqlDialect::Sqlite
        ));
        // Nothing at all is nothing to wrap.
        assert!(is_bare_default("", crate::intel::SqlDialect::Sqlite));
    }

    /// **And everything that is an expression, which is where this went wrong.**
    /// A character-set test called `1+2` a number and a `starts_with('\'')` test
    /// called `'a' || 'b'` a string, so both took the `ADD COLUMN` fast path —
    /// on which SQLite refuses the statement, and Copy / "Open in editor" then
    /// half-applies a two-column add.
    #[test]
    fn an_expression_is_not_a_literal() {
        for d in [
            "1+2",
            "-1*2",
            "'a'||'b'",
            "'a' || 'b'",
            "datetime('now')",
            "upper('a')",
            "a+1",
            "1e",
            "1e+",
            "0x",
            "1.2.3",
            "1 2",
            "'unterminated",
        ] {
            assert!(!is_bare_default(d, crate::intel::SqlDialect::Sqlite), "{d}");
        }
    }

    /// A column carrying just a type and a default, which is all the
    /// parenthesising gate reads.
    fn with_default(ty: &str, d: &str) -> ColumnInfo {
        ColumnInfo {
            name: "b".to_string(),
            type_name: ty.to_string(),
            nullable: true,
            default: Some(d.to_string()),
            ..Default::default()
        }
    }

    /// **MySQL 8 requires the parentheses and does not hand them back.**
    /// `information_schema.COLUMNS.COLUMN_DEFAULT` for
    /// `b varchar(30) DEFAULT (CONCAT('a','c'))` is `concat(…)` with no outer
    /// pair, while `SHOW CREATE TABLE` prints `DEFAULT (concat(…))` — and
    /// measured on 8.4.11 the bare form is `ERROR 1064` and the wrapped form is
    /// accepted. `create_sql` is `None` on MySQL, so `create_ddl` reconstructs
    /// the column and the bare form reached Copy DDL, the structure dump, MCP
    /// `describe`, `MODIFY COLUMN`, `ADD COLUMN` and `CREATE TABLE` alike.
    #[test]
    fn a_mysql_expression_default_is_parenthesised() {
        let c = with_default("varchar(30)", "concat(_utf8mb3'a',_utf8mb3'c')");
        assert!(
            c.definition_sql(crate::intel::SqlDialect::MySql)
                .ends_with(" DEFAULT (concat(_utf8mb3'a',_utf8mb3'c'))"),
            "{}",
            c.definition_sql(crate::intel::SqlDialect::MySql)
        );
    }

    /// **Over-wrapping is not free either**, which is why the gate asks the
    /// grammar rather than wrapping everything: MySQL reads `DEFAULT (7)` as an
    /// *expression* default rather than a literal one, and a `TEXT` column's
    /// defaults are legal only in one of those two forms. Every literal shape
    /// the two servers hand back stays bare.
    #[test]
    fn a_mysql_literal_default_stays_bare() {
        let bare = [
            "'draft'",
            "7",
            "-1",
            "1.5",
            "0xFF",
            "b'1010'",
            "NULL",
            "TRUE",
            "CURRENT_TIMESTAMP",
            "CURRENT_TIMESTAMP(6)",
            "localtimestamp(3)",
            // MySQL 8's spelling of a string it recorded as an expression
            // default: the introducer is part of the literal, and
            // `SHOW CREATE TABLE` prints it back intact.
            "_utf8mb3'draft'",
        ];
        for d in bare {
            let c = with_default("varchar(30)", d);
            let sql = c.definition_sql(crate::intel::SqlDialect::MySql);
            assert!(sql.ends_with(&format!(" DEFAULT {d}")), "{sql}");
        }
    }

    /// And the expression shapes, all of which MySQL prints parenthesised.
    #[test]
    fn a_mysql_expression_default_is_wrapped_whatever_shape_it_is() {
        for d in ["now()", "uuid()", "1 + 2", "concat('a','b')", "'a' + 'b'"] {
            let c = with_default("varchar(30)", d);
            let sql = c.definition_sql(crate::intel::SqlDialect::MySql);
            assert!(sql.ends_with(&format!(" DEFAULT ({d})")), "{sql}");
        }
    }

    /// An already-parenthesised value does not get a second pair — the model may
    /// have come from a designer edit rather than a catalogue.
    #[test]
    fn a_mysql_default_that_already_carries_its_parens_keeps_one_pair() {
        let c = with_default("varchar(36)", "(uuid())");
        assert!(
            c.definition_sql(crate::intel::SqlDialect::MySql)
                .ends_with(" DEFAULT (uuid())")
        );
    }

    /// **PostgreSQL takes an arbitrary expression bare**, and everything
    /// `pg_get_expr` returns is one — so nothing there is ever wrapped, and
    /// widening the gate must not have started.
    #[test]
    fn a_postgres_default_is_never_parenthesised() {
        for d in ["now()", "nextval('s'::regclass)", "'draft'::text", "7"] {
            let c = with_default("text", d);
            let sql = c.definition_sql(crate::intel::SqlDialect::Postgres);
            assert!(sql.ends_with(&format!(" DEFAULT {d}")), "{sql}");
        }
    }

    /// **Two parentheses at the ends are not one group**, and the test here was
    /// a two-ended character check rather than a balanced one.
    ///
    /// `pragma_table_xinfo.dflt_value` strips the *outer* pair, so
    /// `DEFAULT ((1+2)*(3+4))` comes back as `(1+2)*(3+4)` and
    /// `DEFAULT (('x')||('y'))` as `('x')||('y')` — both of which start with `(`
    /// and end with `)` while being expressions. So `definition_sql` re-emitted
    /// them bare, and SQLite refuses the result
    /// (`near "||": syntax error`). Since `create_table_sql` builds the
    /// rebuild's new table from *every* column's `definition_sql`, editing any
    /// column of such a table aborted the twelve-step rebuild with a message
    /// about a column the user had not touched — permanently uneditable through
    /// the designer, and via Run all it landed with the table already dropped.
    ///
    /// Measured on SQLite 3.45.1.
    #[test]
    fn a_parenthesised_pair_that_is_not_one_group_still_needs_wrapping() {
        for d in [
            "(1+2)*(3+4)",
            "('x')||('y')",
            "(1)+(2)",
            ")1+2(",
            "(unbalanced",
        ] {
            assert!(!is_bare_default(d, crate::intel::SqlDialect::Sqlite), "{d}");
        }
        // One group, closed by the final paren — still bare, including when the
        // group contains a literal holding a paren of its own, which is the case
        // only the boundary lexer can answer.
        for d in ["(datetime('now'))", "((1+2)*(3+4))", "('a)b')", "(1)"] {
            assert!(is_bare_default(d, crate::intel::SqlDialect::Sqlite), "{d}");
        }
    }
}

#[cfg(test)]
mod sqlite_affinity_tests {
    use super::*;

    /// The rules SQLite documents, each with the example the documentation
    /// itself uses, plus the spellings the exact-match test this replaced let
    /// through.
    #[test]
    fn the_five_rules_in_order() {
        use SqliteAffinity::*;
        for (declared, want) in [
            ("INT", Integer),
            ("INTEGER", Integer),
            ("BIGINT", Integer),
            ("UNSIGNED BIG INT", Integer),
            // Rule 1 wins over rule 2 even though the text also says CHAR.
            ("INT CHAR", Integer),
            ("CHARACTER(20)", Text),
            ("VARCHAR(255)", Text),
            ("NCHAR(55)", Text),
            ("CLOB", Text),
            ("TEXT", Text),
            ("BLOB", Blob),
            ("MEDIUMBLOB", Blob),
            ("longblob", Blob),
            ("REAL", Real),
            ("DOUBLE PRECISION", Real),
            ("FLOAT", Real),
            ("NUMERIC", Numeric),
            ("DECIMAL(10,5)", Numeric),
            ("BOOLEAN", Numeric),
            ("DATE", Numeric),
            ("DATETIME", Numeric),
        ] {
            assert_eq!(sqlite_affinity(declared), want, "{declared}");
        }
    }

    /// **The case the grid gets wrong if this is an exact match.** A column
    /// declared with no type at all is idiomatic SQLite — `CREATE TABLE t (id
    /// INTEGER PRIMARY KEY, thumb)` — and it has BLOB affinity, so it is exactly
    /// where raw bytes end up.
    #[test]
    fn no_declared_type_is_blob() {
        assert_eq!(sqlite_affinity(""), SqliteAffinity::Blob);
        assert_eq!(sqlite_affinity("   "), SqliteAffinity::Blob);
    }

    /// The declared text is arbitrary, and case is not part of it.
    #[test]
    fn the_rules_are_case_insensitive_and_ignore_the_padding() {
        assert_eq!(sqlite_affinity(" varbinary(16) "), SqliteAffinity::Numeric);
        assert_eq!(sqlite_affinity("VarBinary(16)"), SqliteAffinity::Numeric);
        assert_eq!(sqlite_affinity("tinyblob"), SqliteAffinity::Blob);
    }
}

#[cfg(test)]
mod read_notice_tests {
    use super::*;
    use std::time::Duration;

    /// The ordinary local refresh, which is what the delay exists to stay out
    /// of: `begin_refresh` measures 48 ms on MySQL and 134 ms on PostgreSQL,
    /// and every applied DDL causes one. Reporting any of those would put a
    /// glyph on screen for two frames after every schema edit.
    #[test]
    fn a_refresh_that_finishes_quickly_is_never_announced() {
        for ms in [0, 48, 134, 250, 399] {
            assert!(
                !report_read(true, Duration::from_millis(ms)),
                "{ms} ms was announced"
            );
        }
    }

    /// The case the header exists for: a re-introspection long enough that the
    /// rows on screen — which a refresh deliberately leaves in place — are the
    /// only thing saying anything, and they say nothing.
    #[test]
    fn a_read_that_outlasts_the_delay_is_announced() {
        assert!(report_read(true, READ_NOTICE_DELAY));
        assert!(report_read(true, Duration::from_secs(3)));
    }

    /// The other term, and not redundant: a read that has *ended* says nothing
    /// however long it ran, so the notice cannot outlive the work.
    #[test]
    fn a_finished_read_is_never_announced_however_long_it_took() {
        for ms in [0, 400, 30_000] {
            assert!(!report_read(false, Duration::from_millis(ms)));
        }
    }

    /// **The delay is longer than the refreshes it must not fire on**, which is
    /// the property the constant is chosen for rather than an arbitrary round
    /// number. Pinned here so lowering it has to argue with the measurement in
    /// `begin_refresh`'s doc rather than quietly reintroducing the flicker.
    #[test]
    fn the_delay_clears_every_measured_local_refresh() {
        // The two figures `SchemaState::begin_refresh` records.
        for measured in [Duration::from_millis(48), Duration::from_millis(134)] {
            assert!(
                READ_NOTICE_DELAY > measured,
                "a {measured:?} refresh would flicker"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two servers that can hide an index name the column differently and
    /// answer with **opposite polarity**, and a server too old for either has
    /// no column at all — where naming one fails the whole schema fetch rather
    /// than one value.
    #[test]
    fn index_disabled_sql_asks_each_server_the_column_it_has() {
        // Measured servers: MariaDB 10.11.14 (`IGNORED = 'YES'`) and MySQL
        // 8.4.11 (`IS_VISIBLE = 'NO'`).
        assert_eq!(
            index_disabled_sql("10.11.14-MariaDB-1:10.11.14+maria~ubu2204"),
            "CASE WHEN IGNORED = 'YES' THEN 1 ELSE 0 END"
        );
        assert_eq!(
            index_disabled_sql("8.4.11"),
            "CASE WHEN IS_VISIBLE = 'NO' THEN 1 ELSE 0 END"
        );
        // Too old for the feature, so too old for the column.
        assert_eq!(index_disabled_sql("10.5.23-MariaDB"), "0");
        assert_eq!(index_disabled_sql("5.7.44-log"), "0");
        // MariaDB's own numbering passed 8 long ago and it has never had
        // `IS_VISIBLE`, which is why the flavour is asked before the number.
        assert_eq!(
            index_disabled_sql("11.4.2-MariaDB"),
            "CASE WHEN IGNORED = 'YES' THEN 1 ELSE 0 END"
        );
        // An unparseable version withholds the read rather than guessing.
        assert_eq!(index_disabled_sql(""), "0");
        assert_eq!(index_disabled_sql("unknown"), "0");
    }

    #[test]
    fn major_minor_reads_both_families_leading_numbers() {
        assert_eq!(major_minor("10.11.14-MariaDB-1:10.11.14"), (10, 11));
        assert_eq!(major_minor("8.4.11"), (8, 4));
        assert_eq!(major_minor("5.7.44-log"), (5, 7));
        assert_eq!(major_minor("8"), (8, 0));
        assert_eq!(major_minor(""), (0, 0));
        assert_eq!(major_minor("not-a-version"), (0, 0));
    }

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

    /// **`STORED` is a choice the user made, and every engine's default is
    /// `VIRTUAL`, so a column that comes back without the word has been
    /// silently un-materialised.**
    ///
    /// The keyword was written for PostgreSQL and SQLite only, while MySQL's
    /// reader has always set the flag — so a dump, Copy DDL, Duplicate table or
    /// a designer `ADD COLUMN` on the busiest engine emitted the keywordless
    /// form and the restored column was computed on every read instead of
    /// materialised, with nothing said. Where the column is in the primary key
    /// the restore does not differ, it **fails**, after the file's earlier
    /// statements have run.
    ///
    /// Measured on MySQL 8.4.11 and MariaDB 10.11.14: the keywordless form
    /// reports `VIRTUAL GENERATED` on both; both accept the explicit `STORED`
    /// on `CREATE TABLE` and on `ADD COLUMN`; and a virtual generated column in
    /// the primary key is refused (*ERROR 3106* / *ERROR 1903*).
    #[test]
    fn a_stored_generated_column_keeps_its_keyword_on_every_engine() {
        let c = |stored: bool| ColumnInfo {
            name: "total".into(),
            type_name: "int".into(),
            nullable: true,
            generated: Some("qty * price".into()),
            generated_stored: stored,
            ..Default::default()
        };
        for d in [
            crate::intel::SqlDialect::MySql,
            crate::intel::SqlDialect::Sqlite,
            crate::intel::SqlDialect::Postgres,
        ] {
            assert!(
                c(true).definition_sql(d).contains(" STORED"),
                "{d:?} dropped the keyword: {}",
                c(true).definition_sql(d)
            );
        }
        // `VIRTUAL` is every engine's default, so the false case stays
        // unwritten — except on PostgreSQL, which has only the stored form and
        // requires the word.
        for d in [
            crate::intel::SqlDialect::MySql,
            crate::intel::SqlDialect::Sqlite,
        ] {
            assert!(
                !c(false).definition_sql(d).contains("STORED"),
                "{d:?} invented a keyword: {}",
                c(false).definition_sql(d)
            );
        }
        assert!(
            c(false)
                .definition_sql(crate::intel::SqlDialect::Postgres)
                .contains(" STORED")
        );
    }

    /// The same rule as a generated column, for the other server-assigned form:
    /// PostgreSQL rejects a column that names both a default and an identity
    /// ("both default and identity specified"), so a `serial` — which the
    /// catalogue reports as a `nextval` default *and* as auto-increment — must
    /// emit one of them. The identity is the half that stands on its own; the
    /// default names a sequence a fresh `CREATE TABLE` has not created.
    #[test]
    fn a_server_assigned_column_emits_no_default_beside_its_identity() {
        let c = ColumnInfo {
            name: "id".into(),
            type_name: "integer".into(),
            primary_key: true,
            auto_increment: true,
            default: Some("nextval('t_id_seq'::regclass)".into()),
            ..Default::default()
        };
        let pg = c.definition_sql(crate::intel::SqlDialect::Postgres);
        assert_eq!(
            pg,
            "\"id\" integer NOT NULL GENERATED BY DEFAULT AS IDENTITY"
        );
        assert!(!pg.contains("DEFAULT nextval"));
        // MySQL rejects the pairing too — `AUTO_INCREMENT` and `DEFAULT` on one
        // column is an error there ("Invalid default value").
        let my = c.definition_sql(crate::intel::SqlDialect::MySql);
        assert_eq!(my, "`id` integer NOT NULL AUTO_INCREMENT");
    }

    /// **A column the DBA retired must come back retired.** `INVISIBLE` is how
    /// a column is taken out of `SELECT *` without breaking anything that names
    /// it — the staged first half of dropping it. The model carried no such
    /// field, so a dump-and-restore, a Copy DDL and `create_ddl_script` all
    /// restated the column visible, and every query written against the visible
    /// column set silently got an extra one back.
    #[test]
    fn an_invisible_column_is_restated_invisible() {
        let c = ColumnInfo {
            name: "secret".into(),
            type_name: "varchar(64)".into(),
            nullable: true,
            invisible: true,
            ..Default::default()
        };
        assert_eq!(
            c.definition_sql(crate::intel::SqlDialect::MySql),
            "`secret` varchar(64) INVISIBLE"
        );
        // It goes last, after everything else the column carries — which is
        // where both servers print it.
        let full = ColumnInfo {
            default: Some("'x'".into()),
            comment: Some("retiring".into()),
            nullable: false,
            ..c.clone()
        };
        let sql = full.definition_sql(crate::intel::SqlDialect::MySql);
        assert!(sql.ends_with(" INVISIBLE"), "{sql}");
        assert!(sql.contains("COMMENT"), "{sql}");
        // Neither other engine has the concept, and emitting the word is a
        // syntax error on both.
        for d in [
            crate::intel::SqlDialect::Postgres,
            crate::intel::SqlDialect::Sqlite,
        ] {
            assert!(!c.definition_sql(d).contains("INVISIBLE"), "{d:?}");
        }
        // And an ordinary column is untouched.
        let plain = ColumnInfo {
            invisible: false,
            ..c
        };
        assert_eq!(
            plain.definition_sql(crate::intel::SqlDialect::MySql),
            "`secret` varchar(64)"
        );
    }

    /// A `CREATE TABLE` that drops the table's checks recreates something that
    /// accepts data the original refused, and says nothing about it.
    #[test]
    fn create_table_restates_its_check_constraints() {
        let t = TableInfo {
            name: "orders".into(),
            columns: vec![ColumnInfo {
                name: "qty".into(),
                type_name: "int".into(),
                ..Default::default()
            }],
            check_constraints: vec![CheckInfo {
                name: "qty_positive".into(),
                expression: "`qty` > 0".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let sql = t.create_ddl(crate::intel::SqlDialect::MySql);
        assert!(
            sql.contains("CONSTRAINT `qty_positive` CHECK (`qty` > 0)"),
            "{sql}"
        );
    }

    /// `NOT ENFORCED` is the half that changes what a write does, so it has to
    /// survive — and it is MySQL's alone, so PostgreSQL must not see it.
    #[test]
    fn an_unenforced_check_stays_unenforced_on_mysql_only() {
        let c = CheckInfo {
            name: "soft".into(),
            expression: "qty > 0".into(),
            enforced: false,
            ..Default::default()
        };
        assert_eq!(
            c.clause_sql(crate::intel::SqlDialect::MySql),
            "CONSTRAINT `soft` CHECK (qty > 0) NOT ENFORCED"
        );
        assert_eq!(
            c.clause_sql(crate::intel::SqlDialect::Postgres),
            "CONSTRAINT \"soft\" CHECK (qty > 0)"
        );
        // The default is enforced — the opposite would emit the clause on every
        // constraint the server never marked.
        assert!(CheckInfo::default().enforced);
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

    /// **A MariaDB sequence is not a table**, and `create_ddl` must not write
    /// one as if it were.
    ///
    /// The catalogue lists it as `TABLE_TYPE = 'SEQUENCE'` with eight internal
    /// counter columns, and its actual definition — start, increment, bounds,
    /// cache — lives in the **row**, which this model does not hold. Emitting
    /// the columns produced a `CREATE TABLE` that restores as a plain table,
    /// after which every `NEXTVAL(sq1)` in the restored schema fails; the
    /// `SEQUENCE=1` option that would have made it a sequence is not read.
    #[test]
    fn a_sequence_is_not_dumped_as_a_table() {
        let t = TableInfo {
            name: "sq1".into(),
            is_sequence: true,
            columns: vec![ColumnInfo {
                name: "next_not_cached_value".into(),
                type_name: "bigint(21)".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let ddl = t.create_ddl(crate::intel::SqlDialect::MySql);
        assert!(
            !ddl.contains("CREATE TABLE"),
            "a sequence was emitted as a table:\n{ddl}"
        );
        assert!(
            !ddl.contains("next_not_cached_value"),
            "the counter column reached the script:\n{ddl}"
        );
        // It names the object and says where the real statement is, rather than
        // vanishing from the script: an object silently missing from a restore
        // is the failure this is replacing, not an improvement on it.
        assert!(ddl.contains("sq1"), "{ddl}");
        assert!(ddl.contains("SHOW CREATE SEQUENCE"), "{ddl}");
        // Every line is a comment, so a script holding it still runs.
        assert!(
            ddl.lines().all(|l| l.trim_start().starts_with("--")),
            "{ddl}"
        );
    }

    /// The three-answer question, and that it agrees with the two flags. A
    /// `TableInfo` cannot be both, and `is_view` wins if one ever is — a view is
    /// the shape with the most restrictive editor.
    #[test]
    fn a_tables_shape_is_one_of_three() {
        let of = |v: bool, s: bool| TableInfo {
            name: "t".into(),
            is_view: v,
            is_sequence: s,
            ..Default::default()
        };
        assert_eq!(of(false, false).shape(), TableShape::Table);
        assert_eq!(of(true, false).shape(), TableShape::View);
        assert_eq!(of(false, true).shape(), TableShape::Sequence);
        assert_eq!(of(true, true).shape(), TableShape::View);
        // A default `TableInfo` — what every hand-built draft is — is a table.
        assert_eq!(TableInfo::default().shape(), TableShape::Table);
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

    /// **Every assertion above is a MySQL spelling**, which is why a whole
    /// engine's worth of columns rendering as "unrecognised type" was invisible
    /// to the suite. These four are `format_type`'s output, measured on
    /// PostgreSQL 16.15, for the four most common column types a PostgreSQL
    /// database has.
    #[test]
    fn classify_column_type_reads_postgresqls_spellings() {
        use ColumnTypeClass::*;
        assert_eq!(classify_column_type("character varying(45)"), Text);
        assert_eq!(classify_column_type("character(2)"), Text);
        assert_eq!(classify_column_type("bytea"), Binary);
        assert_eq!(classify_column_type("jsonb"), Json);
        // The rest of what a PostgreSQL schema routinely holds.
        assert_eq!(classify_column_type("double precision"), Numeric);
        assert_eq!(classify_column_type("timestamp with time zone"), DateTime);
        assert_eq!(
            classify_column_type("timestamp(3) without time zone"),
            DateTime
        );
        assert_eq!(classify_column_type("interval"), DateTime);
        assert_eq!(classify_column_type("uuid"), Text);
        assert_eq!(classify_column_type("bpchar"), Text);
        assert_eq!(classify_column_type("numeric(10,2)"), Numeric);
        assert_eq!(classify_column_type("money"), Numeric);
        assert_eq!(classify_column_type("inet"), Text);
    }

    /// **And the third engine's.** The two tests above are MySQL's spellings
    /// and PostgreSQL's, which is how the same gap the PostgreSQL one closed
    /// survived for SQLite: every name below is an example in SQLite's *own*
    /// type-affinity documentation, and SQLite is the engine where they matter
    /// most, because it stores the declared type text verbatim — an unusual
    /// spelling there is normal rather than exotic. All eight drew the
    /// "unrecognised type" glyph in the schema tree, the ER diagram's cards and
    /// tooltips, the completion popup and Find Anywhere.
    #[test]
    fn classify_column_type_reads_sqlites_own_documented_spellings() {
        use ColumnTypeClass::*;
        assert_eq!(classify_column_type("VARYING CHARACTER(255)"), Text);
        assert_eq!(classify_column_type("NATIVE CHARACTER(70)"), Text);
        assert_eq!(classify_column_type("NVARCHAR(100)"), Text);
        assert_eq!(classify_column_type("NCHAR(55)"), Text);
        assert_eq!(classify_column_type("CLOB"), Text);
        assert_eq!(classify_column_type("UNSIGNED BIG INT"), Numeric);
        assert_eq!(classify_column_type("INT2"), Numeric);
        assert_eq!(classify_column_type("INT8"), Numeric);
        // The affinity rule's other examples, which already worked, so the set
        // above is the whole of what was missing rather than a sample of it.
        for already in ["INTEGER", "TEXT", "BLOB", "REAL", "DOUBLE", "DECIMAL(10,5)"] {
            assert_ne!(classify_column_type(already), Other, "{already}");
        }
        // `float4`/`float8` come with the same edit: PostgreSQL's internal
        // aliases for `real` and `double precision`, which `format_type` does
        // not emit but a hand-written draft does.
        assert_eq!(classify_column_type("float8"), Numeric);
        assert_eq!(classify_column_type("int4"), Numeric);
    }

    /// An array column is its element type — the icon is about what a cell
    /// holds. `format_type` writes any number of dimensions as repeated
    /// brackets, and it writes them after the modifier, so the two have to be
    /// stripped in that order.
    #[test]
    fn an_array_column_is_classified_as_its_element_type() {
        use ColumnTypeClass::*;
        assert_eq!(classify_column_type("text[]"), Text);
        assert_eq!(classify_column_type("integer[]"), Numeric);
        assert_eq!(classify_column_type("character varying(45)[]"), Text);
        assert_eq!(classify_column_type("bytea[][]"), Binary);
    }

    /// **The second pass must not shadow the first.** Matching only the whole
    /// phrase would lose every MySQL type that carries a trailing modifier, and
    /// matching only the leading word is the bug being fixed — so the two orders
    /// are asserted against each other here.
    #[test]
    fn a_trailing_modifier_still_classifies_by_its_leading_word() {
        use ColumnTypeClass::*;
        assert_eq!(classify_column_type("int(11) unsigned"), Numeric);
        assert_eq!(classify_column_type("bigint unsigned zerofill"), Numeric);
        assert_eq!(
            classify_column_type("varchar(20) CHARACTER SET utf8mb4"),
            Text
        );
        assert_eq!(classify_column_type("timestamp with time zone"), DateTime);
        // And an unknown leading word is still unknown, however many words
        // follow it: the fallback widens the match, it must not invent one.
        assert_eq!(classify_column_type("hstore"), Other);
        assert_eq!(classify_column_type("my_type with trimmings"), Other);
    }

    /// **The gate.** Every question about standalone objects that is *not*
    /// "give me all of them" has to filter before it clones.
    ///
    /// `objects_in` was `objects_all(kind).into_iter().filter(…)`, and the
    /// schema tree's two match predicates built six owned lists apiece to
    /// return a `bool` — on a path (`nav_rows`) that rebuilds the whole visible
    /// row list on every arrow key, per database and per namespace. On a
    /// PostgreSQL database with 20 schemas that is tens of thousands of
    /// `EnumInfo`/`DomainInfo`/`SequenceInfo` clones per keypress, ≥95% of them
    /// discarded by the next `filter`. `objects_matching` had already been
    /// given the cheap path with a paragraph saying why; these were the sites
    /// that did not follow.
    ///
    /// Scoped to the bodies of the five methods, not to the file: `objects_all`
    /// is the legitimate "all of them" answer and several callers want it.
    #[test]
    fn no_narrowing_object_query_goes_through_the_whole_list() {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/schema.rs"))
            .expect("this module's own source");
        for name in [
            "objects_in",
            "objects_matching",
            "objects_any",
            "any_object_matches",
            "any_object_in_matches",
            "has_objects_in",
        ] {
            let at = src
                .find(&format!("pub fn {name}("))
                .or_else(|| src.find(&format!("fn {name}(")))
                .unwrap_or_else(|| panic!("{name} is gone — this gate is stale"));
            let end = src[at..].find("\n    }\n").expect("the end of the method") + at;
            let body = &src[at..end];
            assert!(
                !body.contains("objects_all("),
                "{name} routes through `objects_all`, which clones every object \
                 in the database before the filter runs"
            );
        }
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

    /// **What the user is looking at is introspected first**, and nothing is
    /// dropped: with the fan-out bounded, order is what decides whether the
    /// tree fills in from the top or from wherever the catalogue happened to
    /// list things.
    #[test]
    fn the_active_and_expanded_databases_are_introspected_first() {
        let names: Vec<String> = ["archive", "analytics", "shop", "world", "scratch"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let expanded: std::collections::HashSet<String> = ["world".to_string()].into();
        let hidden: std::collections::HashSet<String> = ["archive".to_string()].into();

        let got: Vec<&str> = introspection_order(&names, Some("shop"), &expanded, &hidden)
            .into_iter()
            .map(|i| names[i].as_str())
            .collect();
        assert_eq!(
            got,
            ["shop", "world", "analytics", "scratch", "archive"],
            "active, then expanded, then visible in catalogue order, hidden last"
        );
        // Nothing is dropped — the completion index and `intel`'s catalogue
        // read the same models, so this is an ordering and not a laziness.
        assert_eq!(got.len(), names.len());
    }

    /// The degenerate inputs: no active database, nothing expanded, nothing
    /// hidden — the order the server gave, unchanged.
    #[test]
    fn introspection_order_without_a_preference_is_the_catalogue_order() {
        let names: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let none = std::collections::HashSet::new();
        assert_eq!(introspection_order(&names, None, &none, &none), [0, 1, 2]);
        assert!(introspection_order(&[], Some("a"), &none, &none).is_empty());
        // The active database wins over hidden — it is the one being queried,
        // which is the same exception `schema::tab_target` makes.
        let hidden: std::collections::HashSet<String> = ["a".to_string()].into();
        assert_eq!(introspection_order(&names, Some("a"), &none, &hidden)[0], 0);
    }

    /// **The two empty-filter answers are opposite, and both are right.**
    ///
    /// The schema tree hand-spelled `filt.is_empty() || name.to_lowercase()
    /// .contains(filt)` at eight sites — the database and namespace rows, in
    /// the rendered tree and again in the keyboard walk that must stay
    /// bug-for-bug identical to it. Folding those into `object_name_matches`,
    /// which refuses an empty needle, would have inverted the tree: an empty
    /// search box would hide every row instead of showing them all. That is
    /// what `name_survives` exists to make impossible to get wrong by
    /// accident, and this is what says the two have not been conflated.
    #[test]
    fn an_empty_filter_keeps_every_row_and_matches_no_search() {
        assert!(name_survives("orders", ""));
        assert!(name_survives("", ""));
        assert!(!object_name_matches("orders", ""));

        // With a term, they agree exactly — including the case-insensitive
        // substring rule and the misses.
        for (name, filt) in [
            ("Orders", "ord"),
            ("Orders", "zzz"),
            ("sales", "ale"),
            ("İzmir", "izmir"),
        ] {
            assert_eq!(
                name_survives(name, filt),
                object_name_matches(name, filt),
                "{name:?} vs {filt:?}"
            );
        }
    }

    /// **The allocation-free path and the lowercasing one answer the same.**
    ///
    /// `object_name_matches` is the single name-versus-term rule for every
    /// schema-search surface — fourteen callers — and its body stopped being
    /// `name.to_lowercase().contains(needle)` so the undebounced palette would
    /// stop allocating a `String` per table and per column per keystroke. The
    /// fast path is only sound where both sides are ASCII, because
    /// `to_lowercase` is not `to_ascii_lowercase`: `İ` becomes two chars and
    /// `ẞ` becomes `ß`. This walks both against a corpus that crosses that
    /// line in every direction.
    #[test]
    fn the_allocation_free_path_agrees_with_lowercasing_the_name() {
        let slow = |name: &str, needle_lower: &str| {
            !needle_lower.is_empty() && name.to_lowercase().contains(needle_lower)
        };
        let names = [
            "",
            "o",
            "Orders",
            "ORDERS",
            "order_lines",
            "customer_email",
            // Non-ASCII names, including the two whose lowercase changes
            // length — the reason the slow path is kept rather than replaced.
            "İzmir",
            "STRAẞE",
            "café",
            "ÎLE_DE_FRANCE",
            "Ünterlagen",
        ];
        let needles = [
            "o", "ord", "orders", "orderz", "s", "_", "email", "zzz",
            // Lower-cased the way every caller lower-cases its term.
            "i̇zmir", "izmir", "straße", "café", "î", "ü", "ẞ",
        ];
        for name in names {
            for needle in needles {
                assert_eq!(
                    object_name_matches(name, needle),
                    slow(name, needle),
                    "{name:?} vs {needle:?}"
                );
            }
            // The empty needle is nobody's match, on either path.
            assert!(!object_name_matches(name, ""));
        }
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

    /// **An index the model only partly read is emitted from the server's own
    /// statement**, because emitting it from the model is not merely different
    /// but wrong. Measured on PostgreSQL 16.15: `CREATE INDEX ix ON inc (a, b)
    /// INCLUDE (c, d)` is reported as two key rows and nothing else, so the
    /// model's emission restored an index that no longer covers — on a
    /// structure dump with no edit anywhere, reported as a success. `NULLS NOT
    /// DISTINCT` and a storage parameter are the same shape.
    #[test]
    fn create_ddl_postgres_emits_a_lossy_index_from_the_servers_own_text() {
        let mut t = TableInfo {
            name: "inc".into(),
            columns: vec![col("a", "integer", true, false)],
            indexes: vec![IndexInfo {
                name: "ix_inc".into(),
                columns: vec![IndexColumn::plain("a")],
                lossy: true,
                create_sql: Some(
                    "CREATE INDEX ix_inc ON public.inc USING btree (a, b) INCLUDE (c, d);".into(),
                ),
                ..Default::default()
            }],
            ..Default::default()
        };
        let sql = t.create_ddl(crate::intel::SqlDialect::Postgres);
        assert!(sql.contains("INCLUDE (c, d)"), "{sql}");

        // And a fully-read one keeps the model's emission, which is the one the
        // designer's preview and the compare pane are written against — so the
        // fix does not quietly change every PostgreSQL table's DDL.
        t.indexes[0].lossy = false;
        let sql = t.create_ddl(crate::intel::SqlDialect::Postgres);
        assert!(!sql.contains("INCLUDE"), "{sql}");
        assert!(
            sql.contains("CREATE INDEX \"ix_inc\" ON \"inc\" (\"a\");"),
            "{sql}"
        );
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
        // …but a schema literally *named* `PUBLIC` is a different schema, and
        // `nspname` is what it is really called. This asserted `None` — folding
        // case here made every statement generated for its objects address
        // `public`'s same-named object instead, `recreate_type_sql`'s
        // drop-and-rebuild included. Reproduced on PG 16.14.
        assert_eq!(sql_qualifier(Some("PUBLIC")), Some("PUBLIC"));
        assert_eq!(display_name(Some("PUBLIC"), "orders"), "PUBLIC.orders");
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

    /// The polarity, which is the whole of what this predicate can get wrong —
    /// and getting it wrong inverts every list of databases in the app at once.
    #[test]
    fn a_hidden_database_is_listed_by_nothing_and_an_unhidden_one_by_everything() {
        let hidden: std::collections::HashSet<String> =
            ["archive".to_string()].into_iter().collect();
        assert!(!db_visible(&hidden, "archive"));
        assert!(db_visible(&hidden, "sakila"));
        assert!(db_visible(&Default::default(), "archive"));
    }

    /// **The picker that binds a tab has to ask the same question every list
    /// asks.** It took the raw first name, so hiding the alphabetically-first
    /// database put the next new tab inside it — and `db_contributes`' active
    /// exception then handed it to autocomplete, both prompts and the MCP
    /// overview, from a state the user never chose.
    #[test]
    fn a_new_tab_is_never_bound_to_a_hidden_database() {
        let names: Vec<String> = ["actdemo", "bigschema", "sakila"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let hidden = |v: &[&str]| -> std::collections::HashSet<String> {
            v.iter().map(|s| s.to_string()).collect()
        };
        assert_eq!(first_bindable(None, &names, &hidden(&[])), Some("actdemo"));
        assert_eq!(
            first_bindable(None, &names, &hidden(&["actdemo"])),
            Some("bigschema")
        );
        assert_eq!(
            first_bindable(None, &names, &hidden(&["actdemo", "bigschema"])),
            Some("sakila")
        );
        // Everything hidden: a usable tab beats no tab, and the exception is
        // then honest — the user is inside the one thing they can see.
        assert_eq!(
            first_bindable(None, &names, &hidden(&["actdemo", "bigschema", "sakila"])),
            Some("actdemo")
        );
        assert_eq!(first_bindable(None, &[], &hidden(&[])), None);

        // The composed property, over the whole decision rather than the
        // fallback alone. `tab_target` is what a new tab actually asks — the
        // remembered branch used to test existence and not visibility, so this
        // property held for every input except the one the user creates by
        // hiding the database they are in and pressing Ctrl+T.
        let h = hidden(&["actdemo"]);
        for last in [None, Some("actdemo"), Some("sakila"), Some("gone")] {
            let bound = tab_target(last, None, &names, &h).unwrap();
            assert!(db_contributes(&h, bound, Some(bound)), "{last:?}");
            assert!(!db_contributes(&h, "actdemo", Some(bound)), "{last:?}");
        }
    }

    /// **The connection's own Database field is what the form promises**, and
    /// it was ignored entirely: an unbound tab landed in the alphabetically
    /// first *listed* database, so a connection configured for `sakila` opened
    /// in `actdemo`. Every client the importer reads from opens in the named
    /// database, and `conn_import` now fills the field from those files — so
    /// the promise arrives pre-made and was broken on the first run.
    #[test]
    fn a_tab_opens_in_the_connections_configured_database() {
        let names: Vec<String> = ["actdemo", "bigschema", "sakila"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let hidden = |v: &[&str]| -> std::collections::HashSet<String> {
            v.iter().map(|s| s.to_string()).collect()
        };
        assert_eq!(
            first_bindable(Some("sakila"), &names, &hidden(&[])),
            Some("sakila")
        );
        // Blank means "no preference", which is the pre-range behaviour and
        // most saved connections.
        assert_eq!(
            first_bindable(Some(""), &names, &hidden(&[])),
            Some("actdemo")
        );
        // A stale name would otherwise bind every tab to something the tree
        // does not show.
        assert_eq!(
            first_bindable(Some("gone"), &names, &hidden(&[])),
            Some("actdemo")
        );
        // And it does not override the eye. Putting a database away is as
        // explicit as typing it in the form, and it is the more recent of the
        // two; the app must never move a tab *into* a hidden database.
        assert_eq!(
            first_bindable(Some("sakila"), &names, &hidden(&["sakila"])),
            Some("actdemo")
        );
        // What the user last switched to on this connection still wins — the
        // field says where the connection *opens*, not where it stays.
        assert_eq!(
            tab_target(Some("bigschema"), Some("sakila"), &names, &hidden(&[])),
            Some("bigschema")
        );
        assert_eq!(
            tab_target(None, Some("sakila"), &names, &hidden(&[])),
            Some("sakila")
        );
    }

    /// The remembered database wins **when it is still bindable**, and the three
    /// ways it stops being so all fall through to the same fallback a fresh
    /// connection takes.
    #[test]
    fn a_remembered_database_is_taken_only_while_it_is_still_bindable() {
        let names: Vec<String> = ["actdemo", "bigschema", "sakila"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let hidden = |v: &[&str]| -> std::collections::HashSet<String> {
            v.iter().map(|s| s.to_string()).collect()
        };
        // Remembered, present, visible → it.
        assert_eq!(
            tab_target(Some("sakila"), None, &names, &hidden(&[])),
            Some("sakila")
        );
        // Remembered but since hidden → the first visible one, not back into it.
        assert_eq!(
            tab_target(Some("sakila"), None, &names, &hidden(&["sakila"])),
            Some("actdemo")
        );
        // Remembered but gone from this connection → the same fallback. (Before
        // the map was keyed by connection this was the *cross-connection* case:
        // a name picked on one server that happens to exist, hidden, on
        // another.)
        assert_eq!(
            tab_target(Some("gone"), None, &names, &hidden(&[])),
            Some("actdemo")
        );
        // Nothing remembered → `first_bindable`, unchanged.
        assert_eq!(
            tab_target(None, None, &names, &hidden(&["actdemo"])),
            Some("bigschema")
        );
        // Everything hidden: the remembered one is as good as any, and both
        // arms answer the same way rather than one of them answering `None`.
        assert_eq!(
            tab_target(
                Some("sakila"),
                None,
                &names,
                &hidden(&["actdemo", "bigschema", "sakila"])
            ),
            Some("actdemo")
        );
        // No databases loaded yet.
        assert_eq!(tab_target(Some("sakila"), None, &[], &hidden(&[])), None);
        assert_eq!(tab_target(None, None, &[], &hidden(&[])), None);
    }

    #[test]
    fn a_hidden_database_describes_nothing_either() {
        let hidden: std::collections::HashSet<String> =
            ["archive".to_string()].into_iter().collect();
        assert!(!db_contributes(&hidden, "archive", Some("sakila")));
        assert!(db_contributes(&hidden, "sakila", Some("sakila")));
        assert!(db_contributes(
            &Default::default(),
            "archive",
            Some("sakila")
        ));
    }

    #[test]
    fn the_database_being_worked_in_contributes_even_when_hidden() {
        // Hiding is about what is *offered*; the database the tab is bound to is
        // context. Dropping it would take that tab's own tables out of its
        // completion, and out of what the assistant is told, while its queries
        // still ran there.
        let hidden: std::collections::HashSet<String> =
            ["archive".to_string()].into_iter().collect();
        assert!(db_contributes(&hidden, "archive", Some("archive")));
        // Case-insensitively, like every other name comparison around it.
        assert!(db_contributes(&hidden, "archive", Some("ARCHIVE")));
        // …and only that one: a *different* hidden database stays out.
        assert!(!db_contributes(&hidden, "archive", Some("sakila")));
    }

    #[test]
    fn with_no_active_database_hiding_is_the_whole_rule() {
        let hidden: std::collections::HashSet<String> =
            ["archive".to_string()].into_iter().collect();
        assert!(!db_contributes(&hidden, "archive", None));
        assert!(db_contributes(&hidden, "sakila", None));
    }

    #[test]
    fn a_database_the_connection_loaded_is_shown() {
        let loaded = vec!["main".to_string(), "sales".to_string()];
        assert_eq!(shown_database(Some("sales"), &loaded), Some("sales"));
    }

    #[test]
    fn a_database_nothing_loaded_is_not_shown() {
        // The reported case: a SQLite connection whose file is missing loads no
        // databases at all, and the tab's saved `main` is a name nothing can
        // confirm. The tree is empty beside it and the header says Disconnected.
        assert_eq!(shown_database(Some("main"), &[]), None);
        // Same rule when the connection is up but that particular database is
        // gone (dropped or renamed server-side) — it isn't selectable either.
        let loaded = vec!["sales".to_string()];
        assert_eq!(shown_database(Some("archive"), &loaded), None);
    }

    #[test]
    fn a_tab_bound_to_nothing_shows_nothing() {
        let loaded = vec!["sales".to_string()];
        assert_eq!(shown_database(None, &loaded), None);
        assert_eq!(shown_database(None, &[]), None);
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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

    /// A column's type may be one of the namespace's own enums or domains —
    /// `format_type` prints those qualified, so the script names them — and a
    /// script that creates the table first fails on its very first statement
    /// (`ERROR: type "s31a.weird" does not exist`, measured on 16.14).
    #[test]
    fn create_ddl_script_emits_types_before_the_tables_that_use_them() {
        use crate::intel::SqlDialect::Postgres;
        let s = DbSchema {
            tables: vec![TableInfo {
                name: "usest".into(),
                schema: Some("s31a".into()),
                columns: vec![col("m", "s31a.weird", true, false)],
                ..Default::default()
            }],
            enums: vec![EnumInfo {
                name: "weird".into(),
                schema: Some("s31a".into()),
                values: vec!["a,b".into()],
                comment: None,
            }],
            domains: vec![DomainInfo {
                name: "d_nn".into(),
                schema: Some("s31a".into()),
                base_type: "integer".into(),
                not_null: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let out = s.create_ddl_script(Some("s31a"), Postgres);
        let ty = out.find("CREATE TYPE").expect("enum emitted");
        let dom = out.find("CREATE DOMAIN").expect("domain emitted");
        let tbl = out.find("CREATE TABLE").expect("table emitted");
        assert!(ty < tbl && dom < tbl, "{out}");
    }

    /// **The script carries the routines the tree lists under the same node.**
    /// Last, because a body names the tables above it — and through
    /// `ObjectItem::create_sql`, so MySQL's `DELIMITER` wrapper is the one that
    /// already works when the script is pasted into a query tab. A database
    /// recreated from a script that had quietly dropped them would answer
    /// "function does not exist" to its own triggers.
    #[test]
    fn create_ddl_script_emits_the_namespaces_routines() {
        use crate::intel::SqlDialect::{MySql, Postgres};
        let s = DbSchema {
            tables: vec![TableInfo {
                name: "payment".into(),
                columns: vec![col("id", "int", false, true)],
                ..Default::default()
            }],
            routines: vec![std::sync::Arc::new(RoutineInfo {
                name: "rewards_report".into(),
                kind: RoutineKind::Procedure,
                language: "SQL".into(),
                body: "BEGIN SELECT 1 FROM payment; END".into(),
                ..Default::default()
            })],
            ..Default::default()
        };
        let out = s.create_ddl_script(None, MySql);
        let tbl = out.find("CREATE TABLE").expect("table emitted");
        let proc = out.find("rewards_report").expect("routine emitted");
        assert!(tbl < proc, "{out}");
        // Runnable when pasted, not bare: the client script sets a delimiter
        // around a body that contains `;`.
        assert!(out.contains("DELIMITER"), "{out}");

        // The whole-database script is the per-namespace one where there are no
        // namespaces, so it carries them too.
        assert!(s.create_ddl_script_all(MySql).contains("rewards_report"));

        // PostgreSQL dollar-quotes instead, and has no delimiter directive.
        let pg = DbSchema {
            routines: vec![std::sync::Arc::new(RoutineInfo {
                name: "audit".into(),
                schema: Some("public".into()),
                returns: "trigger".into(),
                language: "plpgsql".into(),
                body: "BEGIN RETURN NEW; END;".into(),
                ..Default::default()
            })],
            ..Default::default()
        };
        let out = pg.create_ddl_script(Some("public"), Postgres);
        assert!(out.contains("CREATE FUNCTION") || out.contains("CREATE OR REPLACE FUNCTION"));
        assert!(!out.contains("DELIMITER"), "{out}");
    }

    /// A `serial`'s counter is created by the column, so restating it would make
    /// the script fail on a name that already exists. A standalone sequence is
    /// the user's own object and belongs in the script.
    #[test]
    fn create_ddl_script_skips_a_sequence_its_own_table_creates() {
        use crate::intel::SqlDialect::Postgres;
        let seq = |name: &str, owner: Option<SequenceOwner>| SequenceInfo {
            name: name.into(),
            schema: Some("s31a".into()),
            owned_by: owner,
            ..Default::default()
        };
        let s = DbSchema {
            tables: vec![TableInfo {
                name: "usest".into(),
                schema: Some("s31a".into()),
                columns: vec![col("id", "integer", false, true)],
                ..Default::default()
            }],
            sequences: vec![
                seq(
                    "usest_id_seq",
                    Some(SequenceOwner {
                        table: "usest".into(),
                        column: "id".into(),
                        internal: false,
                    }),
                ),
                seq("ticket_no", None),
            ],
            ..Default::default()
        };
        let out = s.create_ddl_script(Some("s31a"), Postgres);
        assert!(!out.contains("usest_id_seq"), "{out}");
        assert!(
            out.contains("CREATE SEQUENCE \"s31a\".\"ticket_no\""),
            "{out}"
        );
    }

    /// The database-node script on an engine with no namespaces is exactly the
    /// one the namespace call already builds — MySQL and SQLite carry every
    /// table under `None`, so there is nothing to walk.
    #[test]
    fn create_ddl_script_all_is_the_flat_script_without_namespaces() {
        use crate::intel::SqlDialect::MySql;
        let s = DbSchema {
            tables: vec![
                TableInfo {
                    name: "users".into(),
                    columns: vec![col("id", "int", false, false)],
                    ..Default::default()
                },
                TableInfo {
                    name: "orders".into(),
                    columns: vec![col("id", "int", false, false)],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert!(s.schemas().is_empty(), "MySQL carries no namespace");
        assert_eq!(
            s.create_ddl_script_all(MySql),
            s.create_ddl_script(None, MySql)
        );
    }

    /// Every namespace, in the order the tree shows them — `public` first, then
    /// alphabetical — so the script reads down the tree it was raised from.
    #[test]
    fn create_ddl_script_all_walks_every_namespace_in_display_order() {
        use crate::intel::SqlDialect::Postgres;
        let tbl = |ns: &str, name: &str| TableInfo {
            name: name.into(),
            schema: Some(ns.into()),
            columns: vec![col("id", "integer", false, false)],
            ..Default::default()
        };
        let s = DbSchema {
            // Deliberately not in display order: `schemas()` sorts, and this is
            // what would pass by accident if it didn't.
            tables: vec![
                tbl("sales", "orders"),
                tbl("public", "users"),
                tbl("archive", "old_orders"),
            ],
            ..Default::default()
        };
        let out = s.create_ddl_script_all(Postgres);
        let at = |t: &str| out.find(t).unwrap_or_else(|| panic!("{t} missing: {out}"));
        assert!(at("users") < at("old_orders"), "public first: {out}");
        assert!(at("old_orders") < at("orders"), "then alphabetical: {out}");
    }

    /// A namespace that holds nothing contributes no blank run to the script —
    /// the join is over the non-empty parts, not over every namespace.
    #[test]
    fn create_ddl_script_all_is_empty_for_an_empty_database() {
        use crate::intel::SqlDialect::Postgres;
        assert_eq!(DbSchema::default().create_ddl_script_all(Postgres), "");
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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

    // ── Standalone objects ──────────────────────────────────────────────────

    use crate::intel::SqlDialect::Postgres;

    #[test]
    fn qualified_ident_drops_public_and_quotes_both_halves() {
        assert_eq!(
            qualified_ident("mood", Some("public"), Postgres),
            "\"mood\""
        );
        assert_eq!(qualified_ident("mood", None, Postgres), "\"mood\"");
        assert_eq!(
            qualified_ident("mood", Some("sales"), Postgres),
            "\"sales\".\"mood\""
        );
        // The quote character inside a name is doubled, not dropped.
        assert_eq!(
            qualified_ident("we\"ird", Some("od\"d"), Postgres),
            "\"od\"\"d\".\"we\"\"ird\""
        );
    }

    #[test]
    fn enum_create_sql_quotes_values_as_literals() {
        let e = EnumInfo {
            name: "mood".into(),
            schema: Some("public".into()),
            values: vec!["sad".into(), "it's ok".into()],
            comment: None,
        };
        assert_eq!(
            e.create_sql(Postgres),
            "CREATE TYPE \"mood\" AS ENUM ('sad', 'it''s ok');"
        );
    }

    #[test]
    fn enum_create_sql_appends_its_comment() {
        let e = EnumInfo {
            name: "mood".into(),
            schema: Some("sales".into()),
            values: vec!["ok".into()],
            comment: Some("how it went".into()),
        };
        let sql = e.create_sql(Postgres);
        assert!(
            sql.starts_with("CREATE TYPE \"sales\".\"mood\" AS ENUM ('ok');"),
            "{sql}"
        );
        assert!(
            sql.contains("COMMENT ON TYPE \"sales\".\"mood\" IS 'how it went';"),
            "{sql}"
        );
        // An empty comment is not a comment — emitting `IS ''` would *set* one.
        let blank = EnumInfo {
            comment: Some(String::new()),
            ..e
        };
        assert!(!blank.create_sql(Postgres).contains("COMMENT"));
    }

    #[test]
    fn domain_create_sql_carries_every_clause_in_order() {
        let d = DomainInfo {
            name: "email".into(),
            schema: Some("public".into()),
            base_type: "character varying(255)".into(),
            collation: None,
            collation_schema: None,
            default_value: Some("''::character varying".into()),
            not_null: true,
            checks: vec![CheckInfo {
                name: "email_shaped".into(),
                expression: "VALUE ~ '@'::text".into(),
                ..Default::default()
            }],
            comment: None,
        };
        let sql = d.create_sql(Postgres);
        assert_eq!(
            sql,
            "CREATE DOMAIN \"email\" AS character varying(255)\n  \
             DEFAULT ''::character varying\n  NOT NULL\n  \
             CONSTRAINT \"email_shaped\" CHECK (VALUE ~ '@'::text);"
        );
    }

    #[test]
    fn a_bare_domain_is_just_the_type() {
        let d = DomainInfo {
            name: "positive".into(),
            base_type: "integer".into(),
            ..Default::default()
        };
        assert_eq!(
            d.create_sql(Postgres),
            "CREATE DOMAIN \"positive\" AS integer;"
        );
    }

    #[test]
    fn sequence_bounds_follow_the_storage_type_and_direction() {
        let asc = SequenceInfo::default();
        assert_eq!(asc.implicit_bounds(), (1, i64::MAX));
        assert_eq!(asc.implicit_start(), 1);

        let desc = SequenceInfo {
            increment: -1,
            min_value: i32::MIN as i64,
            max_value: -1,
            data_type: "integer".into(),
            start: -1,
            ..Default::default()
        };
        assert_eq!(desc.implicit_bounds(), (i32::MIN as i64, -1));
        // A descending sequence starts at the *top* of its range.
        assert_eq!(desc.implicit_start(), -1);

        assert_eq!(
            SequenceInfo::type_bounds("smallint"),
            (i16::MIN as i64, i16::MAX as i64)
        );
        assert_eq!(
            SequenceInfo::type_bounds("integer"),
            (i32::MIN as i64, i32::MAX as i64)
        );
        // Anything unrecognised is bigint — the type PostgreSQL itself defaults to.
        assert_eq!(SequenceInfo::type_bounds("nonsense"), (i64::MIN, i64::MAX));
    }

    #[test]
    fn a_default_sequence_emits_no_clauses() {
        // Every value equals what the server would assume, so restating them
        // would be six lines saying nothing.
        let s = SequenceInfo {
            name: "counter".into(),
            schema: Some("public".into()),
            ..Default::default()
        };
        assert_eq!(s.create_sql(Postgres), "CREATE SEQUENCE \"counter\";");
    }

    #[test]
    fn a_sequence_names_only_what_differs() {
        let s = SequenceInfo {
            name: "odds".into(),
            schema: Some("public".into()),
            data_type: "integer".into(),
            increment: 2,
            min_value: 1,
            max_value: 99,
            start: 3,
            cache: 10,
            cycle: true,
            ..Default::default()
        };
        assert_eq!(
            s.create_sql(Postgres),
            "CREATE SEQUENCE \"odds\"\n  AS integer\n  INCREMENT BY 2\n  \
             MAXVALUE 99\n  START WITH 3\n  CACHE 10\n  CYCLE;"
        );
        // MINVALUE is absent because 1 *is* the implicit ascending minimum.
        assert!(!s.create_sql(Postgres).contains("MINVALUE"));
    }

    #[test]
    fn a_sequence_restates_its_owner() {
        // Not cosmetic: without `OWNED BY` the recreated sequence outlives the
        // column it belongs to instead of being dropped with it.
        let s = SequenceInfo {
            name: "orders_id_seq".into(),
            schema: Some("sales".into()),
            owned_by: Some(SequenceOwner {
                table: "orders".into(),
                column: "id".into(),
                internal: false,
            }),
            ..Default::default()
        };
        assert!(
            s.create_sql(Postgres)
                .contains("OWNED BY \"sales\".\"orders\".\"id\""),
            "{}",
            s.create_sql(Postgres)
        );
    }

    #[test]
    fn last_value_is_display_only_and_never_reaches_the_ddl() {
        let s = SequenceInfo {
            name: "counter".into(),
            last_value: Some(4171),
            ..Default::default()
        };
        assert!(!s.create_sql(Postgres).contains("4171"));
    }

    fn objects() -> DbSchema {
        DbSchema {
            enums: vec![
                EnumInfo {
                    name: "mood".into(),
                    schema: Some("public".into()),
                    values: vec!["ok".into()],
                    comment: None,
                },
                EnumInfo {
                    name: "mood".into(),
                    schema: Some("sales".into()),
                    values: vec!["great".into()],
                    comment: None,
                },
            ],
            domains: vec![DomainInfo {
                name: "email".into(),
                schema: Some("sales".into()),
                base_type: "text".into(),
                ..Default::default()
            }],
            sequences: vec![SequenceInfo {
                name: "counter".into(),
                schema: Some("public".into()),
                ..Default::default()
            }],
            // A function and a procedure that **share a name**, which is what
            // makes the kind part of the lookup rather than a label on the row.
            routines: vec![
                std::sync::Arc::new(RoutineInfo {
                    name: "settle".into(),
                    schema: Some("public".into()),
                    kind: RoutineKind::Function,
                    arguments: "a integer".into(),
                    returns: "integer".into(),
                    language: "sql".into(),
                    body: "SELECT $1".into(),
                    ..Default::default()
                }),
                std::sync::Arc::new(RoutineInfo {
                    name: "settle".into(),
                    schema: Some("sales".into()),
                    kind: RoutineKind::Procedure,
                    language: "plpgsql".into(),
                    body: "BEGIN END;".into(),
                    ..Default::default()
                }),
            ],
            ..Default::default()
        }
    }

    /// The tree, the palette and the Create menu all reach routines through the
    /// same kind-keyed machinery the types use — which is the whole reason they
    /// are [`ObjectItem`]s. A kind that answered with the wrong list would show
    /// procedures in the Functions folder.
    #[test]
    fn routines_are_listed_as_objects_of_their_own_kind() {
        use crate::ddl::ObjectKind;
        let s = objects();
        let fns = s.objects_all(ObjectKind::Function);
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].name(), "settle");
        assert_eq!(fns[0].kind(), ObjectKind::Function);
        assert_eq!(fns[0].schema(), Some("public"));

        let procs = s.objects_all(ObjectKind::Procedure);
        assert_eq!(procs.len(), 1);
        assert_eq!(procs[0].kind(), ObjectKind::Procedure);
        // …and each folder scopes to its namespace like every other one.
        assert!(s.objects_in(Some("sales"), ObjectKind::Function).is_empty());
        assert_eq!(s.objects_in(Some("sales"), ObjectKind::Procedure).len(), 1);
    }

    /// The three `bool` questions the schema tree asks per keystroke, which
    /// used to be answered by building the owned list and testing it. They are
    /// in core so they can be tested at all, and because the view keeps a thin
    /// wrapper over core rather than the logic.
    ///
    /// The namespace and the needle both have to bite, and separately: a
    /// predicate that dropped either term would answer `true` for the whole
    /// database and the filter would stop hiding anything.
    #[test]
    fn the_trees_object_filters_answer_without_building_a_list() {
        let s = objects();
        // A name that is there, in the namespace that has it. `email` is the
        // fixture's one object that lives in exactly one namespace — `mood`
        // and `settle` are in both, deliberately, and would answer `true`
        // either way.
        assert!(s.any_object_matches("email"));
        assert!(s.any_object_in_matches(Some("sales"), "email"));
        // The same name, asked of the wrong namespace.
        assert!(!s.any_object_in_matches(Some("public"), "email"));
        // …and the mirror, so the namespace term cannot be passing by being
        // wrong in one direction only.
        assert!(s.any_object_in_matches(Some("public"), "counter"));
        assert!(!s.any_object_in_matches(Some("sales"), "counter"));
        // A name that is nowhere.
        assert!(!s.any_object_matches("no_such_object"));
        assert!(!s.any_object_in_matches(Some("sales"), "no_such_object"));
        // An empty needle matches nothing, following `object_name_matches` —
        // a caller that means "no filter" is asking `has_objects_in`.
        assert!(!s.any_object_matches(""));

        // …which is the other question, and it is about presence, not names.
        assert!(s.has_objects_in(None), "the flat case: anything anywhere");
        assert!(s.has_objects_in(Some(Some("sales"))));
        assert!(!s.has_objects_in(Some(Some("nowhere"))));
        assert!(!DbSchema::default().has_objects_in(None));
    }

    /// A remembered palette hit carries `(namespace, kind, name)` and nothing
    /// else, so the kind is what keeps a same-named procedure from answering for
    /// a function.
    #[test]
    fn finding_a_routine_by_name_is_narrowed_by_kind_first() {
        use crate::ddl::ObjectKind;
        let s = objects();
        assert_eq!(
            s.find_object(Some("public"), ObjectKind::Function, "settle")
                .map(|o| o.kind()),
            Some(ObjectKind::Function)
        );
        assert_eq!(
            s.find_object(Some("sales"), ObjectKind::Procedure, "settle")
                .map(|o| o.kind()),
            Some(ObjectKind::Procedure)
        );
        // The kind that isn't there in that namespace is a miss, not a fallback
        // to the other one.
        assert!(
            s.find_object(Some("sales"), ObjectKind::Function, "settle")
                .is_none()
        );
    }

    /// A routine's row detail is its signature, because that is what tells two
    /// of them apart — and `()` is the honest rendering of one that takes
    /// nothing rather than a blank where a summary should be.
    #[test]
    fn a_routines_detail_is_its_signature() {
        let s = objects();
        assert_eq!(
            s.objects_all(crate::ddl::ObjectKind::Function)[0].detail(),
            "(a integer) → integer"
        );
        // A procedure returns nothing, so nothing is shown for it.
        assert_eq!(
            s.objects_all(crate::ddl::ObjectKind::Procedure)[0].detail(),
            "()"
        );
    }

    /// The three catalogue readers behind a routine. Each has an "unknown"
    /// answer that must land on the **server's own default**, because the
    /// emitter restates whatever this produced: a mis-read data access or
    /// volatility is silently re-declared on the next edit.
    #[test]
    fn a_routines_catalogue_codes_read_back_to_the_servers_defaults() {
        assert_eq!(RoutineKind::parse("PROCEDURE"), RoutineKind::Procedure);
        assert_eq!(RoutineKind::parse("p"), RoutineKind::Procedure);
        assert_eq!(RoutineKind::parse("FUNCTION"), RoutineKind::Function);
        assert_eq!(RoutineKind::parse("f"), RoutineKind::Function);
        assert_eq!(RoutineKind::parse("nonsense"), RoutineKind::Function);

        assert_eq!(Volatility::parse_code("i"), Volatility::Immutable);
        assert_eq!(Volatility::parse_code(" s "), Volatility::Stable);
        assert_eq!(Volatility::parse_code("v"), Volatility::Volatile);
        assert_eq!(Volatility::parse_code(""), Volatility::Volatile);

        // `information_schema` spells them with an underscore where the clause
        // has a space, so both forms have to read the same.
        assert_eq!(
            SqlDataAccess::parse("READS_SQL_DATA"),
            SqlDataAccess::ReadsSqlData
        );
        assert_eq!(
            SqlDataAccess::parse("reads sql data"),
            SqlDataAccess::ReadsSqlData
        );
        assert_eq!(SqlDataAccess::parse("NO_SQL"), SqlDataAccess::NoSql);
        assert_eq!(
            SqlDataAccess::parse("MODIFIES_SQL_DATA"),
            SqlDataAccess::ModifiesSqlData
        );
        assert_eq!(SqlDataAccess::parse(""), SqlDataAccess::ContainsSql);
    }

    /// Only a **function** returning `trigger` can be bound to one — a procedure
    /// cannot, and PostgreSQL has no way to make it so.
    #[test]
    fn only_a_function_counts_as_a_trigger_function() {
        let mut r = RoutineInfo {
            kind: RoutineKind::Function,
            returns: "trigger".into(),
            ..Default::default()
        };
        assert!(r.is_trigger_function());
        r.returns = "event_trigger".into();
        assert!(r.is_trigger_function());
        r.returns = "integer".into();
        assert!(!r.is_trigger_function());
        r.returns = "trigger".into();
        r.kind = RoutineKind::Procedure;
        assert!(!r.is_trigger_function());
    }

    /// **`Generate DDL` has to hand back something that runs.** A MySQL
    /// routine's `CREATE` carries no terminator — the apply path sends each
    /// statement whole — and its body is full of `;`, so the copied form has to
    /// go through `client_script` or a query tab cuts it mid-body.
    #[test]
    fn a_mysql_routines_copied_ddl_is_runnable() {
        let r = ObjectItem::Routine(std::sync::Arc::new(RoutineInfo {
            name: "restock".into(),
            kind: RoutineKind::Procedure,
            language: "SQL".into(),
            body: "BEGIN\n    SELECT 1;\nEND".into(),
            ..Default::default()
        }));
        let sql = r.create_sql(crate::intel::SqlDialect::MySql);
        assert!(sql.starts_with("DELIMITER $$"), "{sql}");
        assert!(sql.contains("CREATE PROCEDURE `restock`()"), "{sql}");
        assert!(sql.trim_end().ends_with("DELIMITER ;"), "{sql}");
        // Never `OR REPLACE`: this is the `CREATE` a reader copies, and it must
        // not silently overwrite whatever the target database already had.
        assert!(!sql.contains("OR REPLACE"), "{sql}");

        // PostgreSQL's already terminates itself and needs no wrapper.
        let pg = ObjectItem::Routine(std::sync::Arc::new(RoutineInfo {
            name: "audit".into(),
            schema: Some("public".into()),
            returns: "trigger".into(),
            language: "plpgsql".into(),
            body: "BEGIN RETURN NEW; END;".into(),
            ..Default::default()
        }))
        .create_sql(crate::intel::SqlDialect::Postgres);
        assert!(!pg.contains("DELIMITER"), "{pg}");
        assert!(pg.trim_end().ends_with(';'), "{pg}");
    }

    /// **The session state survives a body the reader couldn't parse.** All four
    /// values come from one `SHOW CREATE` row and only the body needs
    /// understanding, so a `None` body must leave what was already there rather
    /// than blanking it — and must not take the `sql_mode` with it, or the
    /// recreate runs under whatever mode the applying session had.
    #[test]
    fn an_unreadable_body_still_carries_the_session_state() {
        let mut r = RoutineInfo {
            body: "the resolved copy".into(),
            ..Default::default()
        };
        RoutineSource {
            body: None,
            sql_mode: Some("NO_ENGINE_SUBSTITUTION".into()),
            charset_client: Some("utf8mb4".into()),
            collation_connection: None,
            aggregate: false,
        }
        .apply_to(&mut r);
        assert_eq!(r.body, "the resolved copy");
        assert_eq!(r.sql_mode.as_deref(), Some("NO_ENGINE_SUBSTITUTION"));
        assert_eq!(r.charset_client.as_deref(), Some("utf8mb4"));

        // A body that *was* read replaces it.
        RoutineSource {
            body: Some("BEGIN SELECT 'it''s'; END".into()),
            ..Default::default()
        }
        .apply_to(&mut r);
        assert_eq!(r.body, "BEGIN SELECT 'it''s'; END");
        // …and leaves the session state alone, because it has none to offer.
        // The catalogue read is where these come from now, so a `SHOW CREATE`
        // that came back short must not blank them and leave the recreate
        // unwrapped.
        assert_eq!(r.sql_mode.as_deref(), Some("NO_ENGINE_SUBSTITUTION"));
    }

    /// **The two appliers touch different fields**, which is what lets the
    /// editor apply the session state unconditionally while holding the body
    /// patch back: the session state is not editable anywhere, and on MySQL the
    /// `CREATE` it wraps is preceded by a `DROP` that has already committed.
    ///
    /// It does **not** test *whether* the body patch is withheld. That decision
    /// is `ddl::routine_source_outcome`, and this test's docstring used to claim
    /// it — while nothing here computes the guard, and a live bug in exactly
    /// that guard passed unchanged. The guard's own test is
    /// `ddl::tests::a_keystroke_before_the_source_lands_leaves_the_draft_stale`,
    /// which has a case per outcome including the one that destroys a routine.
    #[test]
    fn the_two_source_appliers_touch_different_fields() {
        let mut r = RoutineInfo {
            body: "what the user typed".into(),
            ..Default::default()
        };
        let src = RoutineSource {
            body: Some("the source as written".into()),
            sql_mode: Some("TRADITIONAL".into()),
            charset_client: Some("utf8mb3".into()),
            collation_connection: Some("utf8mb3_general_ci".into()),
            aggregate: false,
        };
        src.apply_session_to(&mut r);
        assert_eq!(r.body, "what the user typed");
        assert_eq!(r.sql_mode.as_deref(), Some("TRADITIONAL"));
        assert_eq!(r.charset_client.as_deref(), Some("utf8mb3"));
        assert_eq!(
            r.collation_connection.as_deref(),
            Some("utf8mb3_general_ci")
        );

        src.apply_body_to(&mut r);
        assert_eq!(r.body, "the source as written");
    }

    /// **A redefinition restates the planner attributes it used to drop.**
    ///
    /// Measured on PostgreSQL 16.15: a function created `PARALLEL SAFE
    /// LEAKPROOF COST 5` came back `parallel=u cost=100 leakproof=false` after
    /// one body edit, because [`RoutineInfo`] had no field for any of them and
    /// the emitter wrote nothing. Nothing failed — the defaults are the
    /// conservative ones in every case, so the function merely stopped being
    /// usable in a parallel plan and started lying to the planner by 20x.
    ///
    /// The whole clause list is asserted rather than four `contains`, because
    /// the failure this guards against is a clause landing somewhere the
    /// grammar refuses (`COST` before `RETURNS`, say) — which a `contains`
    /// cannot see.
    #[test]
    fn a_pg_redefinition_restates_parallel_leakproof_and_a_non_default_cost() {
        let r = RoutineInfo {
            name: "f".into(),
            schema: Some("public".into()),
            kind: RoutineKind::Function,
            arguments: "x integer".into(),
            returns: "integer".into(),
            language: "sql".into(),
            body: "SELECT x".into(),
            volatility: Volatility::Immutable,
            parallel: Parallel::Safe,
            leakproof: true,
            cost: Some("5".into()),
            ..Default::default()
        };
        assert_eq!(
            r.create_sql(crate::intel::SqlDialect::Postgres, true),
            // Unqualified: `qualified_ident` leaves `public` off, as every
            // other emitter here does.
            "CREATE OR REPLACE FUNCTION \"f\"(x integer)\n\
             RETURNS integer\n\
             LANGUAGE sql\n\
             IMMUTABLE\n\
             LEAKPROOF\n\
             PARALLEL SAFE\n\
             COST 5\n\
             AS $$\nSELECT x\n$$;"
        );
    }

    /// The other direction, which is what keeps the preview and Copy DDL
    /// readable: a function sitting on every server default says none of them.
    /// Without this the fix could be "always emit all four", and every
    /// `CREATE` in the app would grow three lines of noise.
    #[test]
    fn a_pg_routine_at_its_defaults_states_none_of_the_four() {
        let r = RoutineInfo {
            name: "f".into(),
            kind: RoutineKind::Function,
            returns: "integer".into(),
            language: "plpgsql".into(),
            body: "BEGIN RETURN 1; END".into(),
            parallel: Parallel::Unsafe,
            leakproof: false,
            // What the server reports for a function that declared no `COST`,
            // and for a set-returning one that declared no `ROWS`.
            cost: Some("100".into()),
            rows: Some("1000".into()),
            ..Default::default()
        };
        let sql = r.create_sql(crate::intel::SqlDialect::Postgres, true);
        for word in ["PARALLEL", "LEAKPROOF", "COST", "ROWS"] {
            assert!(
                !sql.contains(word),
                "{word} was restated needlessly:\n{sql}"
            );
        }
    }

    /// `COST` is 1 for a C function and 100 for everything else, so a single
    /// hard-coded default would emit `COST 1` on every C function in the
    /// database — and suppress the one place `COST 100` is meaningful.
    #[test]
    fn the_cost_default_follows_the_language() {
        let of = |lang: &str, cost: &str| RoutineInfo {
            name: "f".into(),
            kind: RoutineKind::Function,
            returns: "integer".into(),
            language: lang.into(),
            body: "x".into(),
            cost: Some(cost.into()),
            ..Default::default()
        };
        assert!(
            !of("sql", "100")
                .create_sql(crate::intel::SqlDialect::Postgres, false)
                .contains("COST")
        );
        assert!(
            of("c", "100")
                .create_sql(crate::intel::SqlDialect::Postgres, false)
                .contains("COST 100")
        );
        // ...and 1 is that default, so it is the one value a C function does
        // not restate - the exact inverse of the `sql` case above.
        assert!(
            !of("c", "1")
                .create_sql(crate::intel::SqlDialect::Postgres, false)
                .contains("COST")
        );
        assert_eq!(of("c", "1").pg_default_cost(), 1.0);
        assert_eq!(of("internal", "1").pg_default_cost(), 1.0);
        assert_eq!(of("plpgsql", "1").pg_default_cost(), 100.0);
    }

    /// **A procedure takes none of them**, and PostgreSQL does not ignore the
    /// ones it rejects: `CREATE PROCEDURE … PARALLEL SAFE` is
    /// `ERROR: invalid attribute in procedure definition`. This is the same
    /// trap the `RETURNS` guard was added for, and the four new clauses walked
    /// straight into it — so they sit behind the same `is_function`.
    #[test]
    fn a_pg_procedure_states_none_of_the_function_only_attributes() {
        let r = RoutineInfo {
            name: "p".into(),
            kind: RoutineKind::Procedure,
            language: "sql".into(),
            body: "SELECT 1".into(),
            volatility: Volatility::Immutable,
            parallel: Parallel::Safe,
            leakproof: true,
            strict: true,
            cost: Some("5".into()),
            rows: Some("3".into()),
            // The one attribute a procedure *does* take, so the test proves the
            // gate is per-clause and not "emit nothing for a procedure".
            security_definer: true,
            ..Default::default()
        };
        let sql = r.create_sql(crate::intel::SqlDialect::Postgres, false);
        for word in [
            "PARALLEL",
            "LEAKPROOF",
            "COST",
            "ROWS",
            "STRICT",
            "IMMUTABLE",
            "RETURNS",
        ] {
            assert!(
                !sql.contains(word),
                "a procedure cannot take {word}:\n{sql}"
            );
        }
        assert!(sql.contains("SECURITY DEFINER"), "{sql}");
    }

    /// `ROWS` is refused on a function that returns one row —
    /// *"ROWS is not applicable when function does not return a set"* — so the
    /// read side sets the field only for a set-returning one and the emitter
    /// restates whatever it was given. A non-default estimate is the whole
    /// point of the clause: losing it hands the planner 1000 for a function
    /// that returns three.
    #[test]
    fn a_set_returning_functions_row_estimate_survives() {
        let r = RoutineInfo {
            name: "f".into(),
            kind: RoutineKind::Function,
            returns: "SETOF integer".into(),
            language: "sql".into(),
            body: "SELECT 1".into(),
            rows: Some("3".into()),
            ..Default::default()
        };
        assert!(
            r.create_sql(crate::intel::SqlDialect::Postgres, false)
                .contains("ROWS 3"),
            "{}",
            r.create_sql(crate::intel::SqlDialect::Postgres, false)
        );
        // And a routine assembled by hand, which knows none of this, says
        // nothing rather than guessing.
        let bare = RoutineInfo {
            rows: None,
            ..r.clone()
        };
        assert!(
            !bare
                .create_sql(crate::intel::SqlDialect::Postgres, false)
                .contains("ROWS")
        );
    }

    /// None of the four is MySQL's, and the MySQL emitter must not learn them:
    /// `PARALLEL`/`LEAKPROOF`/`COST`/`ROWS` are not in its grammar at all.
    #[test]
    fn the_mysql_emitter_says_nothing_about_any_of_the_four() {
        let r = RoutineInfo {
            name: "f".into(),
            kind: RoutineKind::Function,
            returns: "int".into(),
            language: "SQL".into(),
            body: "RETURN 1".into(),
            parallel: Parallel::Safe,
            leakproof: true,
            cost: Some("5".into()),
            rows: Some("3".into()),
            ..Default::default()
        };
        let sql = r.create_sql(crate::intel::SqlDialect::MySql, false);
        for word in ["PARALLEL", "LEAKPROOF", "COST", "ROWS"] {
            assert!(!sql.contains(word), "{word} reached MySQL:\n{sql}");
        }
    }

    /// A routine whose body is a **link symbol** rather than source can't be
    /// re-emitted by an emitter that dollar-quotes it, so it is listed and
    /// droppable but not editable — the call a materialized view gets.
    #[test]
    fn a_routine_whose_body_is_not_source_is_not_editable() {
        let of = |lang: &str| RoutineInfo {
            language: lang.into(),
            ..Default::default()
        };
        assert!(!of("c").is_editable());
        assert!(!of("C").is_editable());
        assert!(!of(" internal ").is_editable());
        // Every language whose body really is its source stays editable —
        // including the ones the form's dropdown doesn't propose.
        assert!(of("sql").is_editable());
        assert!(of("plpgsql").is_editable());
        assert!(of("plpython3u").is_editable());
        assert!(of("SQL").is_editable());
    }

    /// A namespace that holds only routines is still a namespace: leaving it out
    /// of `schemas()` would make its contents unreachable in the tree, which is
    /// the same fault the enums and sequences were added to fix.
    #[test]
    fn a_namespace_holding_only_a_routine_is_still_listed() {
        let s = DbSchema {
            routines: vec![std::sync::Arc::new(RoutineInfo {
                name: "lonely".into(),
                schema: Some("ops".into()),
                ..Default::default()
            })],
            ..Default::default()
        };
        assert_eq!(s.schemas(), vec!["ops".to_string()]);
    }

    #[test]
    fn objects_look_up_by_namespace_on_the_same_rule_tables_do() {
        let s = objects();
        assert_eq!(
            s.find_enum(Some("sales"), "mood").map(|e| e.values.clone()),
            Some(vec!["great".to_string()])
        );
        // No namespace offered → `public` wins over whichever came first.
        assert_eq!(
            s.find_enum(None, "mood").map(|e| e.schema.clone()),
            Some(Some("public".into()))
        );
        // A namespace we don't have is a miss, not a fallback.
        assert!(s.find_enum(Some("archive"), "mood").is_none());
        assert!(s.find_domain(Some("sales"), "email").is_some());
        assert!(s.find_sequence(None, "counter").is_some());
        assert!(s.find_sequence(None, "nope").is_none());
    }

    #[test]
    fn user_types_are_the_enums_and_domains_of_one_namespace() {
        let s = objects();
        assert_eq!(s.user_types_in(Some("public")), vec!["mood"]);
        // Sorted, and a domain counts as a type just as an enum does.
        assert_eq!(s.user_types_in(Some("sales")), vec!["email", "mood"]);
        assert!(s.user_types_in(Some("archive")).is_empty());
    }

    #[test]
    fn a_namespace_holding_only_objects_is_still_a_namespace() {
        // A table-less schema used to vanish from the tree, taking its types
        // with it.
        let s = objects();
        assert_eq!(s.schemas(), vec!["public", "sales"]);
    }

    #[test]
    fn an_object_matches_a_search_by_name_case_insensitively() {
        let s = objects();
        let mood = s
            .find_object(Some("public"), crate::ddl::ObjectKind::Enum, "mood")
            .unwrap();
        assert!(mood.matches_search("moo"));
        assert!(mood.matches_search("mood"));
        assert!(mood.matches_search("oo"), "substring, not prefix");
        assert!(!mood.matches_search("xyz"));
    }

    /// The caller lower-cases the needle; an upper-case name still matches.
    #[test]
    fn object_search_folds_the_name_it_is_matching() {
        let e = ObjectItem::Enum(EnumInfo {
            name: "OrderStatus".into(),
            schema: Some("public".into()),
            values: vec![],
            comment: None,
        });
        assert!(e.matches_search("orderstatus"));
        assert!(e.matches_search("status"));
    }

    /// An empty needle matches nothing, the same call [`TableInfo::matches_search`]
    /// makes — "no filter" is a separate question every caller answers first.
    #[test]
    fn an_empty_search_matches_no_object() {
        let s = objects();
        let mood = s
            .find_object(Some("public"), crate::ddl::ObjectKind::Enum, "mood")
            .unwrap();
        assert!(!mood.matches_search(""));
    }

    /// Matching the *detail* would surface a sequence because some unrelated
    /// table's name appears in its owner, and an enum because a value happens to
    /// spell the term. The name is the only thing anyone searches an object by.
    #[test]
    fn object_search_ignores_the_detail_line() {
        let e = ObjectItem::Enum(EnumInfo {
            name: "mood".into(),
            schema: None,
            values: vec!["shipped".into()],
            comment: None,
        });
        assert!(
            !e.matches_search("shipped"),
            "an enum value is not its name"
        );
        let seq = ObjectItem::Sequence(SequenceInfo {
            name: "counter".into(),
            schema: None,
            owned_by: Some(SequenceOwner {
                table: "invoices".into(),
                column: "id".into(),
                internal: false,
            }),
            ..Default::default()
        });
        assert!(
            !seq.matches_search("invoices"),
            "the owning table is not the sequence's name"
        );
    }

    /// `objects_matching` must agree with `objects_all` + the predicate — it is
    /// an optimisation, and the only thing it may change is what it allocates.
    #[test]
    fn objects_matching_agrees_with_filtering_the_whole_list() {
        use crate::ddl::ObjectKind;
        let s = objects();
        for q in ["mood", "moo", "email", "counter", "settle", "o", "zzz"] {
            for kind in ObjectKind::ALL {
                let expected: Vec<ObjectItem> = s
                    .objects_all(kind)
                    .into_iter()
                    .filter(|o| o.matches_search(q))
                    .collect();
                assert_eq!(s.objects_matching(kind, q), expected, "{kind:?} on {q:?}");
            }
        }
    }

    /// It keeps every namespace's copy, as `objects_all` does — the palette
    /// qualifies them on the row rather than collapsing them.
    #[test]
    fn objects_matching_keeps_a_name_that_exists_in_two_namespaces() {
        let s = objects();
        let hits = s.objects_matching(crate::ddl::ObjectKind::Enum, "mood");
        assert_eq!(hits.len(), 2);
        assert_eq!(
            hits.iter().map(|o| o.schema()).collect::<Vec<_>>(),
            vec![Some("public"), Some("sales")]
        );
    }

    /// An empty needle means "nothing", not "everything" — a caller wanting the
    /// whole list has `objects_all`. Getting this backwards would put every type
    /// in the database into the palette the moment the query box was cleared.
    #[test]
    fn objects_matching_returns_nothing_for_an_empty_needle() {
        let s = objects();
        for kind in [
            crate::ddl::ObjectKind::Enum,
            crate::ddl::ObjectKind::Domain,
            crate::ddl::ObjectKind::Sequence,
        ] {
            assert!(s.objects_matching(kind, "").is_empty(), "{kind:?}");
        }
    }

    #[test]
    fn find_object_resolves_every_kind_on_the_namespace_rule_tables_use() {
        use crate::ddl::ObjectKind;
        let s = objects();
        // Same name in two namespaces resolves independently.
        assert_eq!(
            s.find_object(Some("sales"), ObjectKind::Enum, "mood")
                .map(|o| o.detail()),
            Some("great".to_string())
        );
        // No namespace offered → `public` wins, as `find_enum` does.
        assert_eq!(
            s.find_object(None, ObjectKind::Enum, "mood")
                .and_then(|o| o.schema().map(str::to_string)),
            Some("public".into())
        );
        assert!(
            s.find_object(Some("sales"), ObjectKind::Domain, "email")
                .is_some()
        );
        assert!(
            s.find_object(None, ObjectKind::Sequence, "counter")
                .is_some()
        );
        // The kind is part of the identity: a domain is not an enum.
        assert!(
            s.find_object(Some("sales"), ObjectKind::Enum, "email")
                .is_none()
        );
        assert!(
            s.find_object(Some("archive"), ObjectKind::Enum, "mood")
                .is_none()
        );
    }
}

#[cfg(test)]
mod event_tests {
    use super::*;
    use crate::intel::SqlDialect::MySql;

    /// The session state is populated, and `time_zone` is the reason: it is the
    /// one field of an event that no clause of `CREATE EVENT` carries, so a script
    /// that drops it recreates the event on a different schedule. A fixture that
    /// left it `None` made the wrapper a no-op and could not tell a script that
    /// carries it from one that does not.
    fn ev() -> EventInfo {
        EventInfo {
            name: "nightly".into(),
            definer: Some("root@localhost".into()),
            body: "DELETE FROM sessions WHERE expires_at < NOW()".into(),
            time_zone: Some("+00:00".into()),
            sql_mode: Some("STRICT_TRANS_TABLES".into()),
            charset_client: Some("utf8mb4".into()),
            collation_connection: Some("utf8mb4_0900_ai_ci".into()),
            ..Default::default()
        }
    }

    /// The catalogue hands back a bare datetime and the model holds **SQL**, so
    /// the reader is where the quoting happens — once, and not again downstream.
    #[test]
    fn a_catalogue_timestamp_becomes_a_literal() {
        assert_eq!(
            event_time_expr("2026-01-01 03:00:00", MySql).as_deref(),
            Some("'2026-01-01 03:00:00'")
        );
        // An empty column is no bound at all, not an empty expression that
        // would emit `STARTS ''`.
        assert_eq!(event_time_expr("", MySql), None);
        assert_eq!(event_time_expr("   ", MySql), None);
    }

    /// A single-part unit's quantity is a number and stays bare; a compound
    /// unit's is punctuated and MySQL's grammar wants it quoted. The test is on
    /// the **value**, so a compound unit this build has never heard of is still
    /// quoted correctly.
    #[test]
    fn an_interval_quantity_is_quoted_only_when_it_is_not_a_number() {
        assert_eq!(event_interval_expr("6", MySql), "6");
        assert_eq!(event_interval_expr(" 1 ", MySql), "1");
        assert_eq!(event_interval_expr("1:30", MySql), "'1:30'");
        assert_eq!(event_interval_expr("2 3:04", MySql), "'2 3:04'");
        // Not a number and not empty either — quoted, so the statement fails
        // visibly rather than becoming `EVERY  DAY`.
        assert_eq!(event_interval_expr("", MySql), "''");
    }

    /// Every spelling `information_schema.EVENTS.STATUS` reports, including the
    /// one MySQL 8.0.26 renamed the keyword for. An unknown answer is `Enabled`,
    /// which is how the scheduler treats it.
    #[test]
    fn the_status_column_reads_every_spelling() {
        assert_eq!(EventStatus::parse("ENABLED"), EventStatus::Enabled);
        assert_eq!(EventStatus::parse("disabled"), EventStatus::Disabled);
        assert_eq!(
            EventStatus::parse("SLAVESIDE_DISABLED"),
            EventStatus::SlavesideDisabled
        );
        assert_eq!(
            EventStatus::parse("REPLICA_SIDE_DISABLED"),
            EventStatus::ReplicaDisabled,
            "8.4's own word, kept apart from 8.0's — it is the only thing that \
             says which keyword this server takes back"
        );
        assert_eq!(EventStatus::parse("something new"), EventStatus::Enabled);
        // One state, one sentence: the spelling is the server's business.
        assert_eq!(
            EventStatus::SlavesideDisabled.label(),
            EventStatus::ReplicaDisabled.label()
        );
    }

    /// The two schedule shapes, and the bounds that only one of them has.
    #[test]
    fn a_schedule_renders_both_of_its_shapes() {
        assert_eq!(
            EventSchedule::At("'2026-06-01 00:00:00'".into()).sql(),
            "AT '2026-06-01 00:00:00'"
        );
        assert_eq!(EventSchedule::default().sql(), "EVERY 1 DAY");
        assert_eq!(
            EventSchedule::Every {
                value: "1".into(),
                unit: "DAY".into(),
                starts: Some("'2026-01-01 03:00:00'".into()),
                ends: Some("'2027-01-01 03:00:00'".into()),
            }
            .sql(),
            "EVERY 1 DAY\n  STARTS '2026-01-01 03:00:00'\n  ENDS '2027-01-01 03:00:00'"
        );
        // An empty bound emits no clause: `STARTS ''` is a value MySQL refuses.
        assert_eq!(
            EventSchedule::Every {
                value: "1".into(),
                unit: "DAY".into(),
                starts: Some("  ".into()),
                ends: None,
            }
            .sql(),
            "EVERY 1 DAY"
        );
    }

    /// The whole `CREATE EVENT`, in MySQL's clause order — and the two clauses
    /// that are stated **even at their default**, because silence there means
    /// something different from what the model holds.
    #[test]
    fn create_event_restates_every_clause() {
        let mut e = ev();
        e.schedule = EventSchedule::Every {
            value: "1".into(),
            unit: "DAY".into(),
            starts: Some("'2026-01-01 03:00:00'".into()),
            ends: None,
        };
        e.comment = Some("nightly purge".into());
        let sql = e.create_sql(MySql);
        assert_eq!(
            sql,
            "CREATE DEFINER = `root`@`localhost` EVENT `nightly`\n\
             ON SCHEDULE EVERY 1 DAY\n  STARTS '2026-01-01 03:00:00'\n\
             ON COMPLETION NOT PRESERVE\n\
             ENABLE\n\
             COMMENT 'nightly purge'\n\
             DO\n\
             DELETE FROM sessions WHERE expires_at < NOW()"
        );
        // **No trailing semicolon.** The body is a statement of its own — a
        // compound one, here — and this is one statement that contains it.
        assert!(!sql.ends_with(';'));
    }

    /// The comment is a literal, and a literal is quoted by the one quoter.
    #[test]
    fn a_comment_with_a_quote_in_it_is_escaped() {
        let mut e = ev();
        e.comment = Some("it's nightly".into());
        assert!(e.create_sql(MySql).contains("COMMENT 'it''s nightly'"));
    }

    /// `DISABLE ON SLAVE` is the state a replica sets for itself, and it round
    /// -trips: an event read in it and re-emitted says so rather than coming
    /// back enabled.
    ///
    /// **In the server's own spelling**, which is the half that was wrong: an
    /// event read from MySQL 8.4 got a keyword 8.4 has removed, so Copy CREATE
    /// produced a statement it rejects and a whole-database script stopped there.
    #[test]
    fn a_replica_disabled_event_keeps_saying_so() {
        let mut e = ev();
        e.status = EventStatus::SlavesideDisabled;
        assert!(e.create_sql(MySql).contains("\nDISABLE ON SLAVE\n"));
        e.status = EventStatus::ReplicaDisabled;
        let sql = e.create_sql(MySql);
        assert!(sql.contains("\nDISABLE ON REPLICA\n"), "{sql}");
        assert!(
            !sql.contains("ON SLAVE"),
            "8.4 removed that keyword — emitting it is a statement that fails"
        );
    }

    /// The tree row: the schedule on one line, and the fact that it is switched
    /// off when it is.
    #[test]
    fn the_detail_line_is_the_schedule_and_whether_it_runs() {
        let mut e = ev();
        e.schedule = EventSchedule::Every {
            value: "1".into(),
            unit: "DAY".into(),
            starts: Some("'2026-01-01 03:00:00'".into()),
            ends: None,
        };
        // The clause is multi-line SQL and a tree row is one line, so the
        // newline and its indent collapse to a single space.
        assert_eq!(e.detail(), "EVERY 1 DAY STARTS '2026-01-01 03:00:00'");
        e.status = EventStatus::Disabled;
        assert_eq!(
            e.detail(),
            "EVERY 1 DAY STARTS '2026-01-01 03:00:00' — disabled"
        );
    }

    /// **Generate DDL must carry the events.** The kinds in `create_ddl_script`
    /// are enumerated by hand, and an object kind missing from that list is a
    /// script that restores every table and silently drops every scheduled job —
    /// the worst shape a backup can take, because nothing on screen says so.
    ///
    /// They come **after** the routines, on the rule that group already follows:
    /// an event's body may call one.
    #[test]
    fn the_generated_script_carries_the_events_after_the_routines() {
        let mut s = DbSchema {
            tables: vec![TableInfo {
                name: "sessions".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        s.events.push(std::sync::Arc::new(ev()));
        s.routines.push(std::sync::Arc::new(RoutineInfo {
            name: "purge".into(),
            kind: RoutineKind::Procedure,
            body: "BEGIN DELETE FROM sessions; END".into(),
            // The mode the body was parsed under — the routine's half of the same
            // wrapper the event needs.
            sql_mode: Some("ANSI_QUOTES".into()),
            ..Default::default()
        }));

        let script = s.create_ddl_script(None, MySql);
        let event_at = script
            .find("CREATE DEFINER = `root`@`localhost` EVENT `nightly`")
            .expect("the script names the event");
        let routine_at = script.find("PROCEDURE `purge`").expect("…and the routine");
        let table_at = script.find("`sessions`").expect("…and the table");
        assert!(table_at < routine_at, "tables before routines");
        assert!(routine_at < event_at, "routines before events");

        // Through `client_script`, so a body full of `;` is pasteable — the same
        // treatment a MySQL routine's `CREATE` gets.
        assert!(script.contains("DO\nDELETE FROM sessions WHERE expires_at < NOW()"));

        // **And through the session wrapper**, which is what makes the script a
        // backup rather than a lookalike. `time_zone` is the field with no other
        // carrier: `CREATE EVENT` has no clause for it, so a script run from
        // another zone moves every future firing by the offset between the two —
        // silently, and for ever. The apply path has always wrapped; the copy
        // path handed over the bare `CREATE`.
        assert!(
            script.contains("SESSION time_zone = '+00:00'"),
            "the event's own zone has to travel with it: {script}"
        );
        assert!(
            script.contains("@schemaic_time_zone = @@SESSION.time_zone"),
            "{script}"
        );
        assert!(
            script.contains("SESSION time_zone = @schemaic_time_zone"),
            "the restore: {script}"
        );
        // The routine's three travel for the same reason — a body parsed under
        // `ANSI_QUOTES` re-parses under the applying session's mode, where `"col"`
        // is a string literal.
        assert!(
            script.contains("SESSION sql_mode = 'ANSI_QUOTES'"),
            "the routine's mode: {script}"
        );

        // And the whole-database walk goes through the same builder.
        assert!(s.create_ddl_script_all(MySql).contains("EVENT `nightly`"));
    }

    /// **The `DELIMITER` wrapping is what that `client_script` call is for**, and
    /// the fixture above cannot see it: its body has no internal `;`, so
    /// `needs_delimiter` returns false and `client_script` takes its early
    /// return — which means the arm passes identically with the call deleted.
    /// A `BEGIN … END` body is what `EventDraft::blank` starts every new event
    /// from, so this is the ordinary shape, not an edge case.
    #[test]
    fn a_copied_event_with_a_compound_body_is_wrapped_in_a_delimiter() {
        let mut e = ev();
        e.body = "BEGIN\n  DELETE FROM sessions;\n  DELETE FROM tokens;\nEND".into();
        let script = ObjectItem::Event(std::sync::Arc::new(e)).create_sql(MySql);
        assert!(script.starts_with("DELIMITER $$\n"), "{script}");
        assert!(script.trim_end().ends_with("DELIMITER ;"), "{script}");
        // The `CREATE` is terminated with `$$`, not the `;` that would cut it
        // mid-body.
        assert!(script.contains("END$$"), "{script}");
        assert!(
            !script.contains("END;\nDELIMITER"),
            "the statement must not carry a bare `;` terminator: {script}"
        );
        // A one-line body needs none of it, which is the case the existing
        // fixture covers — and the reason it could not see this.
        let plain = ObjectItem::Event(std::sync::Arc::new(ev())).create_sql(MySql);
        assert!(!plain.contains("DELIMITER"), "{plain}");
    }

    /// The lazy read's two halves are applied separately, because the caller has
    /// to gate them differently — the body is the user's to overwrite, the
    /// session state is not editable anywhere.
    #[test]
    fn the_source_patches_the_body_and_the_session_apart() {
        let src = EventSource {
            body: Some("SELECT 'it''s'".into()),
            time_zone: Some("+00:00".into()),
            sql_mode: Some("STRICT_TRANS_TABLES".into()),
            charset_client: Some("utf8mb4".into()),
            collation_connection: None,
        };

        let mut only_session = ev();
        only_session.collation_connection = Some("utf8mb4_general_ci".into());
        src.apply_session_to(&mut only_session);
        assert_eq!(only_session.body, ev().body, "the body is left alone");
        assert_eq!(only_session.time_zone.as_deref(), Some("+00:00"));
        assert_eq!(
            only_session.sql_mode.as_deref(),
            Some("STRICT_TRANS_TABLES")
        );
        // A read that came back short must not blank what the catalogue read
        // already filled in.
        assert_eq!(
            only_session.collation_connection.as_deref(),
            Some("utf8mb4_general_ci")
        );

        let mut both = ev();
        src.apply_to(&mut both);
        assert_eq!(both.body, "SELECT 'it''s'");
        assert_eq!(both.time_zone.as_deref(), Some("+00:00"));
    }
}
