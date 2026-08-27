//! Schema + data dump: the plan for one `.sql` file that recreates a set of
//! tables and refills them.
//!
//! Both halves of a dump already existed and had never been joined:
//! [`crate::schema::TableInfo::create_ddl`] is the structure (Copy DDL's own
//! emitter) and [`crate::export::ExportFormat::Sql`] is the data (one `INSERT`
//! per row, streamed). This module decides **what goes in the file and in what
//! order**; it writes nothing and connects to nothing, so every decision in it
//! is unit-testable. `schemaic-app` executes the plan — a [`DumpStep::Text`] is
//! written straight out, a [`DumpStep::Rows`] is streamed through the export
//! renderer into the same writer.
//!
//! **A dump is written, never run.** The file is the user's to replay, which is
//! the same side of the "generated DDL is never run silently" invariant Copy DDL
//! stands on.
//!
//! **The UI calls this *Export*; the code calls it a dump, deliberately.** The
//! word `export` is already taken here by [`crate::export`], which renders *one
//! result set* to a file, and the two are different features with different
//! inputs — a reader who sees `export` in this crate should be able to assume
//! the result-grid one. The menu says Export because that is what a user calls
//! it, and this is where the two vocabularies are reconciled.
//!
//! ## What a *replayable* file needs beyond structure + data
//!
//! [`crate::schema::TableInfo::create_ddl`] deliberately emits no foreign keys —
//! for Copy DDL an omitted FK still leaves a script that runs, so the ordering
//! effort there went to types and views. A dump can't take that trade: a restore
//! that silently drops every constraint is not a restore. So the file ends with a
//! constraints section built from [`crate::ddl::ChangeSet::emit`], the emitter the
//! apply path uses — not a second one. Triggers ride along with their table for
//! the same reason.
//!
//! That section is skipped for a table whose model carries **verbatim** DDL
//! ([`crate::schema::TableInfo::create_sql`], which is SQLite's whole captured
//! statement): its `CREATE TABLE` already contains the constraints, and there is
//! no `ALTER TABLE … ADD CONSTRAINT` to add them with. The question is asked of
//! the *table*, not of the engine — the data answers it directly.

use crate::ddl::{Change, ChangeSet, ObjectKind};
use crate::export::{ident_sql, qualified_table};
use crate::intel::SqlDialect;
use crate::schema::{DbSchema, ServerFlavour, TableInfo, display_name};

/// What the file carries. [`Default`] is the mysqldump-shaped answer — the one
/// that replays onto a database that already holds these tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DumpOptions {
    /// `CREATE TABLE`, its triggers, and the closing foreign keys.
    pub structure: bool,
    /// The rows.
    pub data: bool,
    /// Types, sequences, routines and events belonging to the namespaces the
    /// chosen tables live in. Off leaves a file that recreates tables only —
    /// which fails on the first column typed as one of the database's enums, so
    /// it is on by default.
    pub other_objects: bool,
    /// `DROP TABLE IF EXISTS` before each `CREATE`.
    pub drop_if_exists: bool,
    /// Wrap the load in one transaction.
    ///
    /// Worth less than it looks on MySQL, where DDL commits implicitly and a
    /// structure dump therefore can't be one atomic unit — it is still what makes
    /// a *data-only* file all-or-nothing there, and it is honest on the two
    /// engines with transactional DDL.
    pub wrap_transaction: bool,
    /// Turn foreign-key enforcement off for the duration, where the engine has a
    /// session switch for it ([`fk_guard_sql`]).
    pub disable_fk_checks: bool,
}

impl Default for DumpOptions {
    fn default() -> Self {
        Self {
            structure: true,
            data: true,
            other_objects: true,
            drop_if_exists: true,
            wrap_transaction: true,
            disable_fk_checks: true,
        }
    }
}

impl DumpOptions {
    /// Nothing to write — neither half was asked for. What the modal's button
    /// gates on.
    pub fn is_empty(self) -> bool {
        !self.structure && !self.data
    }
}

/// One thing the driver does, in file order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DumpStep {
    /// SQL (or a comment) to write verbatim.
    Text(String),
    /// A table to stream: run `select`, render the rows as `INSERT`s naming
    /// `database`/`schema`/`table`, append them.
    Rows {
        database: String,
        schema: Option<String>,
        table: String,
        select: String,
    },
}

/// The file, decided.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DumpPlan {
    pub steps: Vec<DumpStep>,
    /// How many tables the file covers — what the progress line counts against.
    pub tables: usize,
    /// The selection's foreign keys form a cycle, so no creation order can
    /// satisfy them all. The header says so, and the FK guard is what carries the
    /// file.
    pub cycles: bool,
}

/// The session switch that turns foreign-key enforcement off and back on, when
/// the engine has one an ordinary user can throw.
///
/// **PostgreSQL has none** — `session_replication_role` is superuser-only, so
/// offering it would be a checkbox that fails the restore for most roles. There
/// the ordering plus the closing constraints section is the answer.
///
/// The SQLite pragma is the reason [`plan`] puts this **outside** the
/// transaction: `PRAGMA foreign_keys` is a silent no-op inside one, so a guard
/// emitted after `BEGIN` would look right in the file and do nothing at all.
pub fn fk_guard_sql(dialect: SqlDialect) -> Option<(&'static str, &'static str)> {
    match dialect {
        SqlDialect::MySql => Some(("SET FOREIGN_KEY_CHECKS = 0;", "SET FOREIGN_KEY_CHECKS = 1;")),
        SqlDialect::Sqlite => Some(("PRAGMA foreign_keys = OFF;", "PRAGMA foreign_keys = ON;")),
        SqlDialect::Postgres => None,
    }
}

/// How this dialect opens and closes the load's transaction.
pub fn transaction_sql(dialect: SqlDialect) -> (&'static str, &'static str) {
    match dialect {
        SqlDialect::MySql => ("START TRANSACTION;", "COMMIT;"),
        SqlDialect::Postgres | SqlDialect::Sqlite => ("BEGIN;", "COMMIT;"),
    }
}

/// Whether this table's foreign keys need restating after the data is in.
///
/// A table whose DDL is the engine's own captured text already carries them, and
/// is on the one engine with no `ADD CONSTRAINT` to restate them with.
pub fn needs_fk_section(t: &TableInfo) -> bool {
    !t.is_view
        && !t.foreign_keys.is_empty()
        && !t
            .create_sql
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
}

/// The statement that points the rest of the file at one database, where the
/// engine needs one.
///
/// **MySQL only, and not cosmetic.** `TableInfo::create_ddl` names a MySQL table
/// bare (a database is not a namespace there, so there is nothing to qualify
/// with), while the `INSERT`s come from the export renderer, which addresses a
/// table through [`qualified_table`] and *does* name the database. Without this
/// line the file would create `orders` wherever the client is pointed and then
/// insert into `shop.orders` — two different tables, and a failed restore if
/// `shop` isn't there. It is also the one line to edit to restore the dump
/// somewhere else, which is how a `mysqldump` is retargeted.
///
/// PostgreSQL needs none: both halves name the namespace. SQLite has no
/// qualifier at all.
pub fn target_database_sql(dialect: SqlDialect, database: &str) -> Option<String> {
    match dialect {
        SqlDialect::MySql => Some(format!("USE {};", ident_sql(database, dialect))),
        SqlDialect::Postgres | SqlDialect::Sqlite => None,
    }
}

/// The columns a dump reads out of a table and writes back into it: everything
/// the **server** does not assign for itself.
///
/// `SELECT *` is the wrong statement here, and wrong on all three engines at
/// once. The renderer names every column the result carries, and an `INSERT`
/// that names a generated column is an error rather than a value — MySQL 3105,
/// SQLite "cannot INSERT into generated column", PostgreSQL "cannot insert a
/// non-DEFAULT value". PostgreSQL's `GENERATED ALWAYS AS IDENTITY` refuses one
/// too, without an `OVERRIDING SYSTEM VALUE` clause the shared emitter has no
/// way to write.
///
/// [`crate::schema::ColumnInfo::is_server_assigned`] is the existing answer to exactly this
/// question — the import path asks it for the same reason, about the same
/// columns. **The cost is stated rather than hidden**: an identity column's
/// values are not carried, so the restored rows are renumbered, which is why
/// `plan` says so in the file.
pub fn exported_columns(t: &TableInfo) -> Vec<&str> {
    t.columns
        .iter()
        .filter(|c| !c.is_server_assigned())
        .map(|c| c.name.as_str())
        .collect()
}

/// Does `fk` point at `cand`?
///
/// A key with no namespace of its own means "in mine" — which is every key on
/// MySQL, where the server reports the *database* and the table carries no
/// namespace at all. Only two namespaces that both exist and differ are a miss.
fn fk_targets(fk: &crate::schema::ForeignKeyInfo, cand: &TableInfo) -> bool {
    fk.ref_table == cand.name
        && match (fk.ref_schema.as_deref(), cand.schema.as_deref()) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
}

/// The chosen tables in the order they can be created and filled: a referenced
/// table before the table referencing it, views after every base table, ties by
/// name so two dumps of one schema are the same file.
///
/// Returns the indices into `tables` plus whether a cycle had to be broken.
///
/// **A cycle is reported, never dropped.** No creation order satisfies a cycle,
/// so one edge is broken at the smallest name — the file still carries every
/// table, and [`DumpPlan::cycles`] is what tells the reader the order alone
/// can't be trusted.
pub fn order_tables(tables: &[TableInfo], chosen: &[String]) -> (Vec<usize>, bool) {
    let key = |i: usize| display_name(tables[i].schema.as_deref(), &tables[i].name);
    let mut picked: Vec<usize> = (0..tables.len())
        .filter(|&i| chosen.iter().any(|c| *c == key(i)))
        .collect();
    picked.sort_by_key(|&i| key(i));
    // Views last, whatever their name: a view's body selects from the tables
    // above it, and it holds no rows to order against anything.
    let (views, base): (Vec<usize>, Vec<usize>) =
        picked.into_iter().partition(|&i| tables[i].is_view);

    // Edges point *into* the table that has to wait. A self-reference is not an
    // edge — it is one table, and it can only ever be created before itself.
    let waits_for: Vec<Vec<usize>> = base
        .iter()
        .map(|&i| {
            base.iter()
                .enumerate()
                .filter(|&(_, &j)| {
                    j != i
                        && tables[i]
                            .foreign_keys
                            .iter()
                            .any(|fk| fk_targets(fk, &tables[j]))
                })
                .map(|(pos, _)| pos)
                .collect()
        })
        .collect();

    let mut done = vec![false; base.len()];
    let mut out: Vec<usize> = Vec::with_capacity(base.len() + views.len());
    let mut cycles = false;
    for _ in 0..base.len() {
        // The first *ready* one — `base` is already in name order, so "first"
        // is the name tie-break, and two dumps of one schema are byte-identical.
        let next = (0..base.len())
            .find(|&p| !done[p] && waits_for[p].iter().all(|&d| done[d]))
            .or_else(|| {
                // Nothing is ready and something is left: a cycle. Break it at
                // the smallest name and say so.
                cycles = true;
                (0..base.len()).find(|&p| !done[p])
            });
        let Some(p) = next else { break };
        done[p] = true;
        out.push(base[p]);
    }
    out.extend(views);
    (out, cycles)
}

/// The whole file, as steps.
///
/// The order is the feature: guard *outside* the transaction (SQLite's pragma is
/// a no-op inside one), types before the tables that are typed with them, every
/// table filled before any foreign key is put back.
pub fn plan(
    schema: &DbSchema,
    database: &str,
    chosen: &[String],
    opts: DumpOptions,
    dialect: SqlDialect,
) -> DumpPlan {
    if opts.is_empty() {
        return DumpPlan::default();
    }
    let (order, cycles) = order_tables(&schema.tables, chosen);
    if order.is_empty() {
        return DumpPlan::default();
    }
    let q = |s: &str| ident_sql(s, dialect);
    // The same qualification `TableInfo::create_ddl` uses, so a `DROP` names the
    // table its `CREATE` is about to make.
    let qname = |t: &TableInfo| match crate::schema::sql_qualifier(t.schema.as_deref()) {
        Some(s) => format!("{}.{}", q(s), q(&t.name)),
        None => q(&t.name),
    };

    // ── The closing constraints, decided *first* ─────────────────────────────
    //
    // They are written last, but the header has to be able to say what was left
    // out of them, so the decision happens before a line is emitted.
    //
    // **A key is only restated when the table it points at is in this file too.**
    // Exporting one table is now a first-class thing to do (a table's own Export
    // entry), and `ALTER TABLE orders ADD CONSTRAINT … REFERENCES customers` on a
    // file that never creates `customers` fails at restore — on PostgreSQL with
    // no guard to hide behind, and *after* the rows have landed. The constraint
    // is dropped and the header says so, which is the honest half of the trade:
    // silently emitting a statement that cannot succeed is not the alternative.
    let mut fks: Vec<String> = Vec::new();
    let mut dropped_fks = 0usize;
    if opts.structure {
        for &i in &order {
            let t = &schema.tables[i];
            if !needs_fk_section(t) {
                continue;
            }
            let (here, elsewhere): (Vec<_>, Vec<_>) = t
                .foreign_keys
                .iter()
                .cloned()
                .partition(|fk| order.iter().any(|&j| fk_targets(fk, &schema.tables[j])));
            dropped_fks += elsewhere.len();
            if here.is_empty() {
                continue;
            }
            let set = ChangeSet {
                table: t.name.clone(),
                schema: t.schema.clone(),
                dialect,
                flavour: ServerFlavour::Unknown,
                changes: here
                    .into_iter()
                    .map(|fk| Change::AddForeignKey(Box::new(fk)))
                    .collect(),
            };
            fks.extend(set.emit());
        }
    }

    let mut steps: Vec<DumpStep> = Vec::new();
    // A macro rather than a closure: a closure holding `&mut steps` is alive
    // across the whole body, and the row steps below have to push too.
    macro_rules! text {
        ($s:expr) => {
            steps.push(DumpStep::Text($s))
        };
    }

    // ── Header ───────────────────────────────────────────────────────────────
    let what = match (opts.structure, opts.data) {
        (true, true) => "structure and data",
        (true, false) => "structure only",
        _ => "data only",
    };
    let mut header = format!(
        "-- Schemaic dump of {database}\n-- {} {}, {what}.",
        order.len(),
        crate::text::plural(order.len(), "table", "tables"),
    );
    if cycles {
        header.push_str(
            "\n--\n-- The foreign keys among these tables form a cycle, so no creation order\n\
             -- satisfies every one of them. The constraints are added after the data for\n\
             -- exactly this reason; load the file whole.",
        );
    }
    // **The columns the file cannot carry, named in it.** `exported_columns`
    // leaves out what the server assigns for itself, because an `INSERT` that
    // names one is an error rather than a value — but for an identity column that
    // also means the values are gone and the restored rows are renumbered. The
    // person replaying the file is the one who needs to know, and the same
    // silence about a `NULL`ed blob is what the tally exists to break.
    if opts.data {
        let lost: Vec<String> = order
            .iter()
            .flat_map(|&i| {
                let t = &schema.tables[i];
                t.columns
                    .iter()
                    .filter(|c| c.is_server_assigned())
                    .map(|c| format!("{}.{}", t.name, c.name))
            })
            .collect();
        if !lost.is_empty() {
            header.push_str(&format!(
                "\n--\n-- The server assigns {} itself, so {} not in this file and the\n\
                 -- restored rows are renumbered: {}.",
                crate::text::plural(lost.len(), "this column", "these columns"),
                crate::text::plural(lost.len(), "its value is", "their values are"),
                lost.join(", "),
            ));
        }
    }
    // Said in the file, because the file is where it will be noticed: a restore
    // that comes back without a constraint it used to have is worth one line.
    if dropped_fks > 0 {
        header.push_str(&format!(
            "\n--\n-- {dropped_fks} foreign {} not restated: {} point at tables outside this\n\
             -- export. Add the missing tables to carry {}.",
            crate::text::plural(dropped_fks, "key is", "keys are"),
            crate::text::plural(dropped_fks, "it does", "they do"),
            crate::text::plural(dropped_fks, "it", "them"),
        ));
    }
    text!(header);

    if let Some(sql) = target_database_sql(dialect, database) {
        text!(sql);
    }

    // ── Scaffolding: the guard wraps the transaction, never the other way ────
    let guard = opts
        .disable_fk_checks
        .then(|| fk_guard_sql(dialect))
        .flatten();
    if let Some((open, _)) = guard {
        text!(open.to_string());
    }
    let tx = opts.wrap_transaction.then(|| transaction_sql(dialect));
    if let Some((open, _)) = tx {
        text!(open.to_string());
    }

    // ── Standalone objects the tables lean on ────────────────────────────────
    if opts.structure && opts.other_objects {
        // Only the namespaces the chosen tables live in: a dump of `sales` has no
        // business recreating `archive`'s types.
        let namespaces: Vec<Option<String>> = {
            let mut ns: Vec<Option<String>> = order
                .iter()
                .map(|&i| schema.tables[i].schema.clone())
                .collect();
            ns.sort();
            ns.dedup();
            ns
        };
        let kinds = [
            ObjectKind::Enum,
            ObjectKind::Domain,
            ObjectKind::Sequence,
            ObjectKind::Function,
            ObjectKind::Procedure,
            ObjectKind::Event,
        ];
        // A sequence one of *these* tables owns is created by that table's column,
        // so restating it fails the load on a name that already exists.
        // `is_internal` alone does not answer this: a catalogue can report the
        // link as external while the column still owns the counter, which is why
        // `DbSchema::create_ddl_script` asks about the owner as well. Same
        // question, same answer — the two scripts must not disagree about which
        // sequences a set of tables brings with it.
        // **`(namespace, name)`, not the name.** `create_ddl_script` compares
        // names because it works inside one namespace; a selection spans them, so
        // a `sales.orders_id_seq` owned by `sales.orders` would be dropped on the
        // strength of a chosen `public.orders` — and the column that defaults to
        // it would then have nothing behind it. A sequence carries its own
        // namespace, and its owner is in that namespace.
        let owned_here: Vec<(Option<&str>, &str)> = order
            .iter()
            .map(|&i| {
                (
                    schema.tables[i].schema.as_deref(),
                    schema.tables[i].name.as_str(),
                )
            })
            .collect();
        let mut objects: Vec<String> = Vec::new();
        for kind in kinds {
            for o in schema.objects_all(kind) {
                // `is_internal` is what keeps a `serial`'s own sequence out.
                if o.is_internal() {
                    continue;
                }
                if let crate::schema::ObjectItem::Sequence(s) = &o
                    && s.owned_by
                        .as_ref()
                        .is_some_and(|w| owned_here.contains(&(s.schema.as_deref(), &w.table)))
                {
                    continue;
                }
                if namespaces.iter().any(|ns| ns.as_deref() == o.schema()) {
                    objects.push(o.create_sql(dialect));
                }
            }
        }
        if !objects.is_empty() {
            text!("-- Types, sequences and routines".to_string());
            for o in objects {
                text!(o);
            }
        }
    }

    // ── Each table: structure, then its rows ─────────────────────────────────
    for &i in &order {
        let t = &schema.tables[i];
        text!(format!("-- {}", display_name(t.schema.as_deref(), &t.name)));
        if opts.structure {
            if opts.drop_if_exists {
                let kw = if t.is_view { "VIEW" } else { "TABLE" };
                text!(format!("DROP {kw} IF EXISTS {};", qname(t)));
            }
            text!(t.create_ddl(dialect));
            for tr in &t.triggers {
                text!(tr.create_sql(dialect));
            }
        }
        // Named columns, never `*` — see [`exported_columns`]. A table the server
        // fills entirely has nothing insertable and gets no data step at all;
        // `SELECT  FROM` would not even parse.
        let cols = exported_columns(t);
        if opts.data && !t.is_view && !cols.is_empty() {
            steps.push(DumpStep::Rows {
                database: database.to_string(),
                schema: t.schema.clone(),
                table: t.name.clone(),
                select: format!(
                    "SELECT {} FROM {}",
                    cols.iter().map(|c| q(c)).collect::<Vec<_>>().join(", "),
                    qualified_table(database, t.schema.as_deref(), &t.name, dialect)
                ),
            });
        }
    }

    // ── Foreign keys, once every table is filled ─────────────────────────────
    if opts.structure && !fks.is_empty() {
        steps.push(DumpStep::Text("-- Foreign keys".to_string()));
        steps.extend(fks.into_iter().map(DumpStep::Text));
    }

    if let Some((_, close)) = tx {
        steps.push(DumpStep::Text(close.to_string()));
    }
    if let Some((_, close)) = guard {
        steps.push(DumpStep::Text(close.to_string()));
    }

    DumpPlan {
        steps,
        tables: order.len(),
        cycles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{
        ColumnInfo, ForeignKeyInfo, TriggerAction, TriggerEvent, TriggerInfo, TriggerTiming,
    };

    fn table(name: &str) -> TableInfo {
        TableInfo {
            name: name.to_string(),
            columns: vec![ColumnInfo {
                name: "id".to_string(),
                type_name: "int".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn view(name: &str) -> TableInfo {
        TableInfo {
            is_view: true,
            view_definition: Some("SELECT 1".to_string()),
            ..table(name)
        }
    }

    /// `t` gains a foreign key onto `target`.
    fn refs(mut t: TableInfo, target: &str) -> TableInfo {
        t.foreign_keys.push(ForeignKeyInfo {
            name: format!("fk_{}_{}", t.name, target),
            columns: vec!["id".to_string()],
            ref_schema: None,
            ref_table: target.to_string(),
            ref_columns: vec!["id".to_string()],
            ..Default::default()
        });
        t
    }

    fn schema_of(tables: Vec<TableInfo>) -> DbSchema {
        DbSchema {
            tables,
            ..Default::default()
        }
    }

    fn names(schema: &DbSchema, order: &[usize]) -> Vec<String> {
        order
            .iter()
            .map(|&i| schema.tables[i].name.clone())
            .collect()
    }

    fn all(schema: &DbSchema) -> Vec<String> {
        schema
            .tables
            .iter()
            .map(|t| display_name(t.schema.as_deref(), &t.name))
            .collect()
    }

    /// Every `Text` step, joined — what the file's non-row half reads as.
    fn text_of(plan: &DumpPlan) -> String {
        plan.steps
            .iter()
            .filter_map(|s| match s {
                DumpStep::Text(t) => Some(t.as_str()),
                DumpStep::Rows { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The whole file as one string, rows included as a marker, so a test can
    /// assert on **ordering across the two kinds of step** — which is where this
    /// module's real bugs live.
    fn file_of(plan: &DumpPlan) -> String {
        plan.steps
            .iter()
            .map(|s| match s {
                DumpStep::Text(t) => t.clone(),
                DumpStep::Rows { table, select, .. } => {
                    format!("<<rows {table}: {select}>>")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The `SELECT` planned for one table — asserted on directly, because the
    /// column a data step must *not* name is one the `CREATE TABLE` above it
    /// still has to declare.
    fn select_of(plan: &DumpPlan, want: &str) -> String {
        plan.steps
            .iter()
            .find_map(|s| match s {
                DumpStep::Rows { table, select, .. } if table == want => Some(select.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no data step for {want}"))
    }

    fn pos(hay: &str, needle: &str) -> usize {
        hay.find(needle)
            .unwrap_or_else(|| panic!("{needle:?} missing from:\n{hay}"))
    }

    // ── order_tables ─────────────────────────────────────────────────────────

    #[test]
    fn a_referenced_table_is_created_before_the_table_referencing_it() {
        // `orders` → `customers`, declared in the wrong order on purpose.
        let s = schema_of(vec![refs(table("orders"), "customers"), table("customers")]);
        let (order, cycles) = order_tables(&s.tables, &all(&s));
        assert_eq!(names(&s, &order), vec!["customers", "orders"]);
        assert!(!cycles);
    }

    #[test]
    fn a_diamond_puts_the_root_first_and_the_join_last() {
        let s = schema_of(vec![
            refs(refs(table("order_items"), "orders"), "products"),
            refs(table("orders"), "customers"),
            refs(table("products"), "customers"),
            table("customers"),
        ]);
        let (order, cycles) = order_tables(&s.tables, &all(&s));
        let out = names(&s, &order);
        assert!(!cycles);
        assert_eq!(out[0], "customers");
        assert_eq!(out[3], "order_items");
        // The two middles are interchangeable, but must both sit between.
        assert!(out[1..3].contains(&"orders".to_string()));
        assert!(out[1..3].contains(&"products".to_string()));
    }

    #[test]
    fn a_self_reference_is_not_a_cycle() {
        // An employee's manager is an employee. One table, orderable.
        let s = schema_of(vec![refs(table("employees"), "employees")]);
        let (order, cycles) = order_tables(&s.tables, &all(&s));
        assert_eq!(names(&s, &order), vec!["employees"]);
        assert!(!cycles, "a self-reference orders fine — it is one table");
    }

    #[test]
    fn a_two_table_cycle_still_dumps_every_table_and_says_so() {
        let s = schema_of(vec![refs(table("a"), "b"), refs(table("b"), "a")]);
        let (order, cycles) = order_tables(&s.tables, &all(&s));
        assert!(
            cycles,
            "no order satisfies both keys — the caller must know"
        );
        assert_eq!(order.len(), 2, "a cycle must not drop a table");
    }

    #[test]
    fn an_fk_to_a_table_outside_the_selection_does_not_order_it_in() {
        let s = schema_of(vec![refs(table("orders"), "archive"), table("archive")]);
        let (order, cycles) = order_tables(&s.tables, &["orders".to_string()]);
        assert_eq!(names(&s, &order), vec!["orders"]);
        assert!(!cycles);
    }

    #[test]
    fn views_come_after_every_base_table() {
        let s = schema_of(vec![view("v_recent"), table("orders"), table("customers")]);
        let (order, _) = order_tables(&s.tables, &all(&s));
        assert_eq!(names(&s, &order).last().unwrap(), "v_recent");
    }

    #[test]
    fn ties_break_by_name_so_two_dumps_of_one_schema_match() {
        let s = schema_of(vec![table("zebra"), table("apple"), table("mango")]);
        let (order, _) = order_tables(&s.tables, &all(&s));
        assert_eq!(names(&s, &order), vec!["apple", "mango", "zebra"]);
    }

    // ── plan: what each option puts in the file ──────────────────────────────

    #[test]
    fn structure_only_plans_no_row_steps() {
        let s = schema_of(vec![table("orders")]);
        let opts = DumpOptions {
            data: false,
            ..Default::default()
        };
        let p = plan(&s, "shop", &all(&s), opts, SqlDialect::MySql);
        assert!(
            !p.steps.iter().any(|s| matches!(s, DumpStep::Rows { .. })),
            "data was not asked for"
        );
        assert!(text_of(&p).contains("CREATE TABLE"));
    }

    #[test]
    fn data_only_plans_no_create_and_no_drop() {
        let s = schema_of(vec![table("orders")]);
        let opts = DumpOptions {
            structure: false,
            ..Default::default()
        };
        let p = plan(&s, "shop", &all(&s), opts, SqlDialect::MySql);
        let text = text_of(&p);
        assert!(!text.contains("CREATE TABLE"));
        assert!(
            !text.contains("DROP TABLE"),
            "dropping a table a data-only file then can't recreate is destruction, not a dump"
        );
        assert_eq!(
            p.steps
                .iter()
                .filter(|s| matches!(s, DumpStep::Rows { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn an_empty_selection_plans_nothing() {
        let s = schema_of(vec![table("orders")]);
        let p = plan(&s, "shop", &[], DumpOptions::default(), SqlDialect::MySql);
        assert_eq!(p.tables, 0);
        assert!(
            !file_of(&p).contains("orders"),
            "nothing was chosen, so nothing is in the file"
        );
    }

    #[test]
    fn drop_precedes_its_create_and_is_absent_when_off() {
        let s = schema_of(vec![table("orders")]);
        let on = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        );
        let text = text_of(&on);
        assert!(pos(&text, "DROP TABLE IF EXISTS") < pos(&text, "CREATE TABLE"));

        let off = DumpOptions {
            drop_if_exists: false,
            ..Default::default()
        };
        let p = plan(&s, "shop", &all(&s), off, SqlDialect::MySql);
        assert!(!text_of(&p).contains("DROP TABLE"));
    }

    #[test]
    fn the_row_select_is_quoted_and_qualified_per_dialect() {
        let s = schema_of(vec![table("orders")]);
        let mysql = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        );
        assert_eq!(
            select_of(&mysql, "orders"),
            "SELECT `id` FROM `shop`.`orders`"
        );

        let sqlite = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Sqlite,
        );
        assert_eq!(
            select_of(&sqlite, "orders"),
            "SELECT \"id\" FROM \"orders\"",
            "SQLite has no database to qualify with"
        );
    }

    #[test]
    fn a_view_is_created_but_never_selected_from() {
        let s = schema_of(vec![table("orders"), view("v_recent")]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        );
        assert!(
            !p.steps.iter().any(|st| matches!(
                st,
                DumpStep::Rows { table, .. } if table == "v_recent"
            )),
            "a view holds no rows of its own"
        );
        assert!(
            text_of(&p).contains("v_recent"),
            "but it is still recreated"
        );
    }

    // ── plan: the sections that make the file replayable ─────────────────────

    #[test]
    fn foreign_keys_are_restated_after_every_table_is_filled() {
        // The whole point of the trailing section: `orders` can be filled before
        // `customers` exists, because the key that says otherwise isn't on yet.
        let s = schema_of(vec![refs(table("orders"), "customers"), table("customers")]);
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        ));
        assert!(pos(&file, "<<rows orders") < pos(&file, "ADD CONSTRAINT"));
        assert!(pos(&file, "<<rows customers") < pos(&file, "ADD CONSTRAINT"));
    }

    #[test]
    fn a_table_with_verbatim_ddl_gets_no_foreign_key_section() {
        // SQLite's captured `CREATE TABLE` already carries its keys, and there is
        // no `ADD CONSTRAINT` to restate them with.
        let mut t = refs(table("orders"), "customers");
        t.create_sql =
            Some("CREATE TABLE orders (id INTEGER REFERENCES customers(id))".to_string());
        let s = schema_of(vec![t, table("customers")]);
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Sqlite,
        ));
        assert!(!text.contains("ADD CONSTRAINT"));
        assert!(
            text.contains("REFERENCES customers"),
            "they are in the CREATE"
        );
    }

    #[test]
    fn triggers_follow_the_table_they_hang_off() {
        let mut t = table("orders");
        t.triggers.push(TriggerInfo {
            name: "orders_ai".to_string(),
            table: "orders".to_string(),
            timing: TriggerTiming::After,
            events: vec![TriggerEvent::Insert],
            action: TriggerAction::Body("BEGIN END".to_string()),
            ..Default::default()
        });
        let s = schema_of(vec![t]);
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        ));
        assert!(pos(&text, "CREATE TABLE") < pos(&text, "orders_ai"));
    }

    // ── plan: the scaffolding, and the one composition that can be wrong ─────

    #[test]
    fn the_sqlite_guard_sits_outside_the_transaction() {
        // `PRAGMA foreign_keys` is a **silent no-op inside a transaction**: a
        // guard emitted after `BEGIN` reads correctly and does nothing. This is
        // asserted on the file rather than on `fk_guard_sql`, because the bug is
        // the composition, not the string.
        let s = schema_of(vec![table("orders")]);
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Sqlite,
        ));
        assert!(pos(&file, "PRAGMA foreign_keys = OFF;") < pos(&file, "BEGIN;"));
        assert!(pos(&file, "COMMIT;") < pos(&file, "PRAGMA foreign_keys = ON;"));
    }

    #[test]
    fn mysql_opens_its_guard_before_the_transaction_too() {
        let s = schema_of(vec![table("orders")]);
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        ));
        assert!(pos(&file, "SET FOREIGN_KEY_CHECKS = 0;") < pos(&file, "START TRANSACTION;"));
        assert!(pos(&file, "COMMIT;") < pos(&file, "SET FOREIGN_KEY_CHECKS = 1;"));
    }

    #[test]
    fn postgres_gets_no_guard_but_still_opens_a_transaction() {
        let s = schema_of(vec![table("orders")]);
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        assert!(
            !file.contains("session_replication_role"),
            "superuser-only — a checkbox that fails the restore for most roles"
        );
        assert!(file.contains("BEGIN;") && file.contains("COMMIT;"));
    }

    #[test]
    fn the_scaffolding_is_absent_when_it_was_not_asked_for() {
        let s = schema_of(vec![table("orders")]);
        let opts = DumpOptions {
            wrap_transaction: false,
            disable_fk_checks: false,
            ..Default::default()
        };
        let file = file_of(&plan(&s, "shop", &all(&s), opts, SqlDialect::MySql));
        assert!(!file.contains("START TRANSACTION"));
        assert!(!file.contains("FOREIGN_KEY_CHECKS"));
    }

    #[test]
    fn a_column_the_file_cannot_carry_is_announced_in_it() {
        // The rows come back renumbered, and the person replaying the file is the
        // one who needs to know — silently dropping a column's values and saying
        // nothing is the same class of quiet loss the tally exists to prevent.
        let mut t = table("orders");
        t.columns.push(ColumnInfo {
            name: "seq".to_string(),
            type_name: "int".to_string(),
            identity_always: true,
            ..Default::default()
        });
        let s = schema_of(vec![t]);
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        assert!(text.contains("seq"), "the column is named");
        assert!(
            text.to_lowercase().contains("server"),
            "and why its values are not in the file"
        );
    }

    #[test]
    fn a_file_that_carries_every_column_says_nothing_about_it() {
        // The note is about a *loss*. On the ordinary table there is none, and a
        // caveat printed on every dump is one nobody reads.
        let s = schema_of(vec![table("orders")]);
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        ));
        assert!(!text.to_lowercase().contains("server assigns"));
    }

    #[test]
    fn a_cycle_is_announced_in_the_file_it_affects() {
        let s = schema_of(vec![refs(table("a"), "b"), refs(table("b"), "a")]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        );
        assert!(p.cycles);
        assert!(
            text_of(&p).to_lowercase().contains("cycle"),
            "someone reading the file has to know why the order can't be trusted"
        );
    }

    /// A column the **server** fills in — `GENERATED AS`, or PostgreSQL's
    /// `GENERATED ALWAYS AS IDENTITY`.
    fn server_assigned(name: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.to_string(),
            type_name: "int".to_string(),
            generated: Some("1 + 1".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn a_generated_column_is_never_selected_into_the_insert() {
        // Every engine refuses an `INSERT` that names a generated column, so a
        // `SELECT *` here is a file that dies on its first row.
        let mut t = table("orders");
        t.columns.push(server_assigned("total"));
        let s = schema_of(vec![t]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        );
        assert_eq!(
            select_of(&p, "orders"),
            "SELECT `id` FROM `shop`.`orders`",
            "the server computes `total`; naming it is an error, not a value"
        );
        assert!(
            text_of(&p).contains("total"),
            "the column is still declared — it is only the INSERT it must stay out of"
        );
    }

    #[test]
    fn an_identity_always_column_is_left_to_the_server_too() {
        // PostgreSQL refuses a plain `INSERT` into one without
        // `OVERRIDING SYSTEM VALUE`, which the shared renderer cannot emit.
        let mut t = table("orders");
        t.schema = Some("public".to_string());
        t.columns.push(ColumnInfo {
            name: "seq".to_string(),
            type_name: "int".to_string(),
            identity_always: true,
            ..Default::default()
        });
        let s = schema_of(vec![t]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        );
        let select = select_of(&p, "orders");
        assert!(select.starts_with("SELECT \"id\" FROM"));
        assert!(
            !select.contains("seq"),
            "PostgreSQL refuses a plain INSERT into it, and the shared renderer \
             has no OVERRIDING SYSTEM VALUE to offer"
        );
    }

    #[test]
    fn a_table_the_server_fills_entirely_gets_no_data_step() {
        // Nothing about it is insertable, so there is no statement to write —
        // and `SELECT ` with an empty column list is a syntax error.
        let mut t = table("computed");
        t.columns = vec![server_assigned("total")];
        let s = schema_of(vec![t]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        );
        assert!(!p.steps.iter().any(|s| matches!(s, DumpStep::Rows { .. })));
        assert!(text_of(&p).contains("CREATE TABLE"), "structure still goes");
    }

    #[test]
    fn a_foreign_key_to_a_table_outside_the_selection_is_not_restated() {
        // The single-table export makes this the common case, and PostgreSQL has
        // no guard to hide it behind: the `ALTER` would fail on a table the file
        // never creates, *after* the rows had landed.
        let s = schema_of(vec![refs(table("orders"), "customers"), table("customers")]);
        let text = text_of(&plan(
            &s,
            "shop",
            &["orders".to_string()],
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        assert!(
            !text.contains("ADD CONSTRAINT"),
            "`customers` is not in this file, so the key cannot be put back"
        );
        assert!(
            text.to_lowercase().contains("foreign key"),
            "and the header has to say a constraint was left out"
        );
    }

    #[test]
    fn a_foreign_key_between_two_chosen_tables_is_still_restated() {
        let s = schema_of(vec![refs(table("orders"), "customers"), table("customers")]);
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        assert!(text.contains("ADD CONSTRAINT"));
    }

    #[test]
    fn a_sequence_owned_by_a_same_named_table_in_another_namespace_is_kept() {
        // The owner check has to compare `(namespace, name)`: `sales.orders` owns
        // this counter and is *not* in the export, so dropping it on the strength
        // of the chosen `public.orders` leaves a default with nothing behind it.
        let seq = crate::schema::SequenceInfo {
            name: "orders_id_seq".to_string(),
            schema: Some("sales".to_string()),
            owned_by: Some(crate::schema::SequenceOwner {
                table: "orders".to_string(),
                column: "id".to_string(),
                internal: false,
            }),
            ..Default::default()
        };
        let mut public_orders = table("orders");
        public_orders.schema = Some("public".to_string());
        let mut sales_invoices = table("invoices");
        sales_invoices.schema = Some("sales".to_string());
        let s = DbSchema {
            tables: vec![public_orders, sales_invoices],
            sequences: vec![seq],
            ..Default::default()
        };
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        assert!(text.contains("orders_id_seq"));
    }

    #[test]
    fn needs_fk_section_answers_for_the_three_tables_that_differ() {
        // A view has no keys of its own to restate; a table with none has nothing
        // to say; a table whose DDL is the engine's own verbatim text already
        // carries them and has no `ADD CONSTRAINT` to restate them with.
        assert!(!needs_fk_section(&view("v")));
        assert!(!needs_fk_section(&table("plain")));
        assert!(needs_fk_section(&refs(table("orders"), "customers")));

        let mut verbatim = refs(table("orders"), "customers");
        verbatim.create_sql =
            Some("CREATE TABLE orders (id INTEGER REFERENCES customers(id))".into());
        assert!(!needs_fk_section(&verbatim));

        // Whitespace is not a statement: a blank `create_sql` is no DDL at all,
        // so the keys still have to be restated.
        let mut blank = refs(table("orders"), "customers");
        blank.create_sql = Some("   ".to_string());
        assert!(needs_fk_section(&blank));
    }

    #[test]
    fn a_sequence_a_dumped_table_owns_is_not_restated() {
        // The column's own definition creates it, so emitting `CREATE SEQUENCE`
        // as well fails the load on a name that already exists. `is_internal` is
        // not enough on its own: a catalogue can report the link as external
        // while the column still owns the counter, which is why
        // `DbSchema::create_ddl_script` filters on the owner too — and why this
        // asserts on the *plan*, not on the filter.
        let seq = |name: &str, owner: Option<&str>| crate::schema::SequenceInfo {
            name: name.to_string(),
            schema: Some("public".to_string()),
            owned_by: owner.map(|t| crate::schema::SequenceOwner {
                table: t.to_string(),
                column: "id".to_string(),
                internal: false,
            }),
            ..Default::default()
        };
        let mut t = table("orders");
        t.schema = Some("public".to_string());
        let s = DbSchema {
            tables: vec![t],
            sequences: vec![seq("orders_id_seq", Some("orders")), seq("ticket_no", None)],
            ..Default::default()
        };
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        assert!(
            text.contains("ticket_no"),
            "a standalone sequence is the table's dependency and has to be in the file"
        );
        assert!(
            !text.contains("orders_id_seq"),
            "the owning column already creates this one"
        );
    }

    #[test]
    fn only_mysql_points_the_file_at_a_database_and_does_it_first() {
        // The `USE` is what reconciles the two emitters: `create_ddl` names a
        // MySQL table bare, the export renderer's `INSERT` names it
        // `shop`.`orders`. Without the line they are two different tables.
        assert_eq!(
            target_database_sql(SqlDialect::MySql, "shop").as_deref(),
            Some("USE `shop`;")
        );
        assert_eq!(target_database_sql(SqlDialect::Postgres, "shop"), None);
        assert_eq!(target_database_sql(SqlDialect::Sqlite, "shop"), None);

        // And it lands before anything that depends on it — asserted on the file,
        // since a `USE` after the first `CREATE` is the bug worth catching.
        let s = schema_of(vec![table("orders")]);
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        ));
        assert!(pos(&file, "USE `shop`;") < pos(&file, "CREATE TABLE"));
        assert!(pos(&file, "USE `shop`;") < pos(&file, "<<rows orders"));
    }

    #[test]
    fn the_plan_counts_the_tables_it_covers() {
        let s = schema_of(vec![table("a"), table("b"), view("v")]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        );
        assert_eq!(p.tables, 3);
    }

    #[test]
    fn nothing_at_all_is_planned_when_neither_half_was_asked_for() {
        let s = schema_of(vec![table("orders")]);
        let opts = DumpOptions {
            structure: false,
            data: false,
            ..Default::default()
        };
        assert!(opts.is_empty());
        let p = plan(&s, "shop", &all(&s), opts, SqlDialect::MySql);
        assert!(p.steps.is_empty());
    }
}
