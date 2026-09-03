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
use crate::export::{ExportFormat, export_file_names, ident_sql, qualified_table};
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
    /// Nothing to write — none of the three sections was asked for. What the
    /// modal's button gates on.
    ///
    /// **`other_objects` counts.** It is a peer checkbox in the modal, so ticking
    /// it alone is a thing a user can do; leaving it out of this predicate left
    /// the Export button permanently grey with a box ticked and nothing saying
    /// why.
    pub fn is_empty(self) -> bool {
        !self.structure && !self.data && !self.other_objects
    }
}

/// One thing the driver does, in file order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DumpStep {
    /// SQL (or a comment) to write verbatim.
    Text(String),
    /// A table to stream: run `select` against `database`, render the rows as
    /// `INSERT`s naming `insert_database`/`schema`/`table`, append them.
    Rows {
        /// The database `select` reads from — the **source**.
        database: String,
        /// How the generated `INSERT` names its target database. **Empty
        /// wherever the file already points itself at one**, which is MySQL and
        /// its `USE` line ([`target_database_sql`]).
        ///
        /// Not the same string as `database`, and the difference is the whole
        /// point: the file's `CREATE`/`DROP` name a MySQL table bare, so editing
        /// the `USE` line — the retarget gesture this module's own doc
        /// prescribes — used to move the structure and leave every `INSERT`
        /// pointed at the source. The target came back empty and every exported
        /// row was written back into the live database it came from, with no
        /// duplicate-key error to stop it and a success report at the end.
        insert_database: String,
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
    /// Tables the user ticked that the dump's own fresh introspection could not
    /// find — renamed, dropped, or permission-revoked between the picker and the
    /// save dialog.
    ///
    /// **Reported, never silent.** The re-introspection is deliberate (a backup
    /// of a shape the server no longer has is not a backup), but its cost is that
    /// a selection can go stale, and a file one table short of what was ticked
    /// looks exactly like a complete one. Only an *all* missing selection used to
    /// say anything, while the sibling vanished-preselect case was named.
    pub missing: Vec<String>,
}

impl DumpPlan {
    /// How many tables the progress line counts against.
    ///
    /// **The tables that will be *streamed*, not [`DumpPlan::tables`].** A view
    /// has structure and no rows, and a structure-only dump streams nothing at
    /// all, so counting tables promises a "12 of 12" that never arrives.
    pub fn streamed_tables(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| matches!(s, DumpStep::Rows { .. }))
            .count()
    }
}

/// How the reader half of a dump ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadEnd {
    /// Every table streamed.
    Clean,
    /// The user stopped it.
    Cancelled,
    /// The server said no.
    Failed(String),
}

/// How the writer half ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteEnd {
    /// The file was published.
    Wrote,
    /// A disk or permission failure, in the writer's own words.
    Failed(String),
    /// The worker task itself did not come back.
    Died(String),
}

/// What to report about a finished dump.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DumpVerdict {
    Done,
    Cancelled,
    /// `partial` means a `.part` fragment is on disk and worth naming.
    Failed {
        message: String,
        partial: bool,
    },
}

/// Which of the two halves' endings the user is told about.
///
/// **Cancel is the reader's to declare; every other failure is the writer's to
/// describe.** A cancelled read closes the channels, which the writer sees as an
/// ordinary end of stream — on its own it would call a truncated file finished.
/// Anything else (a full disk, a revoked permission) fails the *writer* first,
/// and the reader then only ever sees "nobody is reading any more", which is a
/// worse sentence than the real cause.
///
/// The five arms were written out inside an `async fn` that needs a `Db`, a
/// runtime handle and two channels to reach, so nothing could test them: swapping
/// two of them turns "The disk is full" into "connection reset" with the suite
/// still green.
pub fn dump_verdict(read: ReadEnd, write: WriteEnd) -> DumpVerdict {
    let failed = |message: String| DumpVerdict::Failed {
        message,
        partial: true,
    };
    match (read, write) {
        (ReadEnd::Cancelled, _) => DumpVerdict::Cancelled,
        (_, WriteEnd::Failed(e)) => failed(e),
        (_, WriteEnd::Died(e)) => failed(format!("Export failed: worker died: {e}")),
        (ReadEnd::Failed(e), _) => failed(format!("Export failed: {e}")),
        (ReadEnd::Clean, WriteEnd::Wrote) => DumpVerdict::Done,
    }
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

/// What a `DROP` in this file has to carry to succeed on a database that still
/// holds the objects depending on it.
///
/// **PostgreSQL, and only PostgreSQL.** MySQL and SQLite both have a session
/// switch that turns foreign-key enforcement off for the whole load
/// ([`fk_guard_sql`]); PostgreSQL's is superuser-only and returns `None` there,
/// so nothing else in the file protects the `DROP`. Replaying a default dump onto
/// the database it came from — the primary way anyone tests a dump — stopped at
/// the first parent table with *"cannot drop table customers because other
/// objects depend on it"*, and the rest of the file was never reached.
///
/// `CASCADE` drops the dependants too, which is right precisely because this file
/// is about to recreate them: the section below it is their `CREATE`, and the
/// closing constraints section puts the keys back.
pub fn drop_cascade(dialect: SqlDialect) -> &'static str {
    match dialect {
        SqlDialect::Postgres => " CASCADE",
        SqlDialect::MySql | SqlDialect::Sqlite => "",
    }
}

/// The statements that make the file's own container before it is used —
/// `CREATE DATABASE` on MySQL, `CREATE SCHEMA` for every non-default namespace
/// on PostgreSQL.
///
/// **The primary use case is a restore onto a fresh server**, and without these
/// the file failed on line 1: MySQL's `USE shop` is `ERROR 1049 Unknown database`,
/// and a PostgreSQL table in `sales` is `schema "sales" does not exist`.
/// `mysqldump --databases` emits the same `CREATE DATABASE IF NOT EXISTS`, for
/// the same reason.
///
/// `IF NOT EXISTS` throughout, because the *other* use case — replaying onto the
/// database it came from — must not start with an error either.
///
/// PostgreSQL gets no `CREATE DATABASE`: it cannot be run from inside the
/// database being restored into, and the connection is already pointed at one.
/// `public` is skipped — every PostgreSQL database has it.
pub fn create_container_sql(
    dialect: SqlDialect,
    database: &str,
    namespaces: &[Option<String>],
) -> Vec<String> {
    match dialect {
        SqlDialect::MySql => vec![format!(
            "CREATE DATABASE IF NOT EXISTS {};",
            ident_sql(database, dialect)
        )],
        SqlDialect::Postgres => namespaces
            .iter()
            .filter_map(|ns| ns.as_deref())
            .filter(|ns| *ns != "public")
            .map(|ns| format!("CREATE SCHEMA IF NOT EXISTS {};", ident_sql(ns, dialect)))
            .collect(),
        SqlDialect::Sqlite => Vec::new(),
    }
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

/// The tables of one namespace, out of the whole database's list.
///
/// `names` are `display_name`s — `schema.table`, or a bare name where the
/// namespace is the default one. `namespace` is `None` when the picker was opened
/// on a database rather than on a PostgreSQL schema, and then everything stays.
///
/// **Through [`crate::schema::sql_qualifier`], not a bare `"{ns}."` prefix.**
/// `display_name` *omits* `public`, so matching on the prefix filtered a `public`
/// dump down to nothing — none of its tables carries one. `None` from the
/// qualifier means "the unqualified ones are mine", which is exactly the set to
/// keep.
pub fn tables_in_namespace(names: &[String], namespace: Option<&str>) -> Vec<String> {
    let Some(ns) = namespace else {
        return names.to_vec();
    };
    match crate::schema::sql_qualifier(Some(ns)) {
        Some(q) => {
            let prefix = format!("{q}.");
            names
                .iter()
                .filter(|n| n.starts_with(&prefix))
                .cloned()
                .collect()
        }
        None => names.iter().filter(|n| !n.contains('.')).cloned().collect(),
    }
}

/// What the Export picker opens with: the ticked tables, and the message to show
/// if the click named one that is not there.
///
/// Everything is ticked when the picker was opened on a *database* — the common
/// case is "all of it", and unticking a few is less work than ticking forty. When
/// it was opened on a *table*, that one table instead: the click said which one,
/// and re-ticking the other thirty-nine is the opposite of what was asked.
///
/// **A preselect the list does not contain is named**, not silently ignored. The
/// table was dropped or renamed since the tree last refreshed, and a modal that
/// opens with a full list, nothing ticked and a dead Export button reads as broken
/// rather than as an answer.
pub fn initial_selection(
    names: &[String],
    preselect: Option<&str>,
) -> (Vec<String>, Option<String>) {
    match preselect {
        None => (names.to_vec(), None),
        Some(t) if names.iter().any(|n| n == t) => (vec![t.to_string()], None),
        Some(t) => (
            Vec::new(),
            Some(format!(
                "{t} is no longer in this database — pick the tables to export."
            )),
        ),
    }
}

/// The statements that put a table's key counter back where the data left it.
///
/// **PostgreSQL only, and it is the difference between a restore that works and
/// one that reports success and then fails.** The rows come back with their
/// original keys — [`exported_columns`] carries a `serial` or a
/// `GENERATED BY DEFAULT AS IDENTITY` column deliberately, because someone
/// re-importing their own keys wants them — but an *explicit* insert does not
/// advance the sequence behind the column. The restored table therefore holds
/// keys 1..10000 with its counter still at 1, and the first ordinary insert
/// afterwards is a duplicate-key error that repeats until the counter catches up.
/// Live-verified; `pg_dump` emits the same `setval` for the same reason.
///
/// MySQL needs none: `AUTO_INCREMENT` is a table property the server raises as
/// rows land. SQLite's `sqlite_sequence` is maintained the same way.
///
/// The statement is written so that a column with **no** sequence behind it is a
/// no-op rather than an error: `pg_get_serial_sequence` answers `NULL` there, and
/// `setval(NULL, …)` would fail the load. Selecting through a subquery with a
/// `WHERE … IS NOT NULL` means no row is produced and `setval` is never called —
/// which also covers an empty table, where there is no maximum to set.
///
/// Gated on the **capability** — [`crate::ddl::supports_sequence_resync`], an
/// exhaustive `match` — rather than on `dialect != Postgres`, which would sort a
/// fourth engine onto MySQL's side without a comparison to grep for.
pub fn sequence_resync_sql(t: &TableInfo, dialect: SqlDialect) -> Vec<String> {
    if !crate::ddl::supports_sequence_resync(dialect) || t.is_view {
        return Vec::new();
    }
    let q = |s: &str| ident_sql(s, dialect);
    let table = match crate::schema::sql_qualifier(t.schema.as_deref()) {
        Some(s) => format!("{}.{}", q(s), q(&t.name)),
        None => q(&t.name),
    };
    t.columns
        .iter()
        .filter(|c| c.auto_increment && c.generated.is_none())
        .map(|c| {
            format!(
                "SELECT setval(s, v) FROM (SELECT pg_get_serial_sequence({}, {}) AS s, \
                 (SELECT MAX({}) FROM {table}) AS v) r WHERE s IS NOT NULL AND v IS NOT NULL;",
                crate::schema::ddl_string(&table, dialect),
                crate::schema::ddl_string(&c.name, dialect),
                q(&c.name),
            )
        })
        .collect()
}

/// Does `fk`, declared on `owner`, point at `cand`?
///
/// A key with no namespace of its own means **"in `owner`'s"** — not "in any".
/// The distinction was invisible while the premise held that only MySQL leaves
/// `ref_schema` empty, where there are no namespaces to confuse; PostgreSQL
/// leaves it empty too (`grep ref_schema` over `schemaic-db` finds no writer for
/// it), so a selection spanning two schemas matched a key on nothing but the
/// table's *name*. A `sales.orders` key was then restated bare against a
/// same-named `archive.orders`, and counted as carried rather than as one of the
/// keys `dropped_fks` reports.
///
/// Only two namespaces that both exist and differ are a miss: one side unknown
/// still cannot be answered no.
fn fk_targets(fk: &crate::schema::ForeignKeyInfo, owner: &TableInfo, cand: &TableInfo) -> bool {
    fk.ref_table == cand.name
        && match (
            fk.ref_schema.as_deref().or(owner.schema.as_deref()),
            cand.schema.as_deref(),
        ) {
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
pub fn order_tables(
    tables: &[TableInfo],
    chosen: &[String],
    dialect: SqlDialect,
) -> (Vec<usize>, bool) {
    let key = |i: usize| display_name(tables[i].schema.as_deref(), &tables[i].name);
    let mut picked: Vec<usize> = (0..tables.len())
        .filter(|&i| chosen.iter().any(|c| *c == key(i)))
        .collect();
    picked.sort_by_key(|&i| key(i));
    // Views after every base table: a view's body selects from the tables above
    // it, and it holds no rows to order against anything.
    let (views, base): (Vec<usize>, Vec<usize>) =
        picked.into_iter().partition(|&i| tables[i].is_view);

    // Edges point *into* the table that has to wait. A self-reference is not an
    // edge — it is one table, and it can only ever be created before itself.
    let base_edges: Vec<Vec<usize>> = base
        .iter()
        .map(|&i| {
            base.iter()
                .enumerate()
                .filter(|&(_, &j)| {
                    j != i
                        && tables[i]
                            .foreign_keys
                            .iter()
                            .any(|fk| fk_targets(fk, &tables[i], &tables[j]))
                })
                .map(|(pos, _)| pos)
                .collect()
        })
        .collect();

    // **Views need the same treatment, for a different reason.** Sorting them by
    // name put a view built on another view first, and `CREATE VIEW … SELECT …
    // FROM other_view` on a target where `other_view` does not exist yet is
    // ERROR 1146 — after `DROP VIEW IF EXISTS` has already removed it from that
    // target. The dependency walk above is built from `foreign_keys`, and a view
    // has none, so nothing ordered them at all.
    //
    // A view's edges are the other picked views its body names, matched as whole
    // words in code (`intel::code_word_hits`) so a name inside a comment, a
    // string literal or a longer identifier is not an edge.
    let view_edges: Vec<Vec<usize>> = views
        .iter()
        .map(|&i| {
            let Some(def) = tables[i].view_definition.as_deref() else {
                return Vec::new();
            };
            // Lexed **once** per definition, then asked about every candidate
            // name. `code_word_hits` builds this mask itself, so calling it in
            // the loop below re-lexed each definition once per picked view.
            let code = crate::intel::code_mask(def, dialect);
            views
                .iter()
                .enumerate()
                .filter(|&(_, &j)| {
                    j != i
                        && !crate::intel::code_word_hits_in(def, &code, &tables[j].name).is_empty()
                })
                .map(|(pos, _)| pos)
                .collect()
        })
        .collect();

    let (base_order, base_cycle) = topo_order(&base_edges);
    let (view_order, view_cycle) = topo_order(&view_edges);
    let mut out: Vec<usize> = Vec::with_capacity(base.len() + views.len());
    out.extend(base_order.into_iter().map(|p| base[p]));
    out.extend(view_order.into_iter().map(|p| views[p]));
    (out, base_cycle || view_cycle)
}

/// Positions `0..waits_for.len()` in an order where every member comes after the
/// ones it waits for, plus whether a cycle had to be broken.
///
/// The caller passes members **already in name order**, so "the first ready one"
/// is the name tie-break and two dumps of one schema are byte-identical.
fn topo_order(waits_for: &[Vec<usize>]) -> (Vec<usize>, bool) {
    let n = waits_for.len();
    let mut done = vec![false; n];
    let mut out: Vec<usize> = Vec::with_capacity(n);
    let mut cycles = false;
    for _ in 0..n {
        let next = (0..n)
            .find(|&p| !done[p] && waits_for[p].iter().all(|&d| done[d]))
            .or_else(|| {
                // Nothing is ready and something is left: a cycle. Break it at
                // the smallest name and say so.
                cycles = true;
                (0..n).find(|&p| !done[p])
            });
        let Some(p) = next else { break };
        done[p] = true;
        out.push(p);
    }
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
    let (order, cycles) = order_tables(&schema.tables, chosen, dialect);
    // Everything ticked that this introspection could not resolve. Computed even
    // when nothing resolved, so the empty-plan arm can carry it too.
    let missing: Vec<String> = chosen
        .iter()
        .filter(|c| {
            !order.iter().any(|&i| {
                display_name(schema.tables[i].schema.as_deref(), &schema.tables[i].name) == **c
            })
        })
        .cloned()
        .collect();
    if order.is_empty() {
        return DumpPlan {
            missing,
            ..DumpPlan::default()
        };
    }
    let q = |s: &str| ident_sql(s, dialect);
    // Only the namespaces the chosen tables live in: a dump of `sales` has no
    // business recreating `archive`'s types — nor creating `archive` itself.
    let namespaces: Vec<Option<String>> = {
        let mut ns: Vec<Option<String>> = order
            .iter()
            .map(|&i| schema.tables[i].schema.clone())
            .collect();
        ns.sort();
        ns.dedup();
        ns
    };
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
                .partition(|fk| order.iter().any(|&j| fk_targets(fk, t, &schema.tables[j])));
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
    // A ticked table the fresh introspection could not find. Said here as well as
    // in the modal's report, because the file outlives the modal and this is what
    // makes it a backup that is one table short rather than one that looks whole.
    if !missing.is_empty() {
        header.push_str(&format!(
            "\n--\n-- {} {} ticked for export {} not found when the file was written\n\
             -- (renamed, dropped, or no longer readable): {}.",
            missing.len(),
            crate::text::plural(missing.len(), "table", "tables"),
            crate::text::plural(missing.len(), "was", "were"),
            missing.join(", "),
        ));
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

    // The container before the thing that enters it: `USE shop` on a server that
    // has no `shop` is ERROR 1049 on line 1, and restoring onto a fresh server is
    // what a dump is mostly for.
    if opts.structure {
        for sql in create_container_sql(dialect, database, &namespaces) {
            text!(sql);
        }
    }
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
    //
    // **Split in two, and the split is the ordering rule**
    // `DbSchema::create_ddl_script` already states: a *type* is what a column is
    // declared with, so it has to exist before the table; a *routine* reads the
    // tables, so it cannot be created until they do. With `check_function_bodies`
    // on — PostgreSQL's default — a `LANGUAGE sql` function naming a table that
    // is not there yet fails at `CREATE`, and the whole array used to be emitted
    // ahead of the table loop.
    let mut routines: Vec<String> = Vec::new();
    // **`other_objects` alone, not `structure && other_objects`.** The modal
    // draws it as a peer of Structure and Data, so ticking it by itself asks for
    // a file of the database's types, sequences and routines — a coherent thing
    // to want, and one that silently emitted nothing.
    if opts.other_objects {
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
                    let after_tables = matches!(
                        kind,
                        ObjectKind::Function | ObjectKind::Procedure | ObjectKind::Event
                    );
                    if after_tables {
                        routines.push(o.create_sql(dialect));
                    } else {
                        objects.push(o.create_sql(dialect));
                    }
                }
            }
        }
        if !objects.is_empty() {
            text!("-- Types and sequences".to_string());
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
                text!(format!(
                    "DROP {kw} IF EXISTS {}{};",
                    qname(t),
                    drop_cascade(dialect)
                ));
            }
            text!(t.create_ddl(dialect));
            // **Through the shared client wrapper**, which is what puts
            // `DELIMITER` around a compound body on MySQL. Written raw, the file
            // died at the first `BEGIN … END` trigger with ERROR 1064 — after the
            // `DROP` above it had already run against the target. The routine and
            // event path has always gone through this; the trigger path did not.
            if !t.triggers.is_empty() {
                let bodies: Vec<String> =
                    t.triggers.iter().map(|tr| tr.create_sql(dialect)).collect();
                text!(crate::ddl::client_script(&bodies, dialect));
            }
        }
        // Named columns, never `*` — see [`exported_columns`]. A table the server
        // fills entirely has nothing insertable and gets no data step at all;
        // `SELECT  FROM` would not even parse.
        let cols = exported_columns(t);
        if opts.data && !t.is_view && !cols.is_empty() {
            steps.push(DumpStep::Rows {
                database: database.to_string(),
                // Whatever `target_database_sql` wrote is what the `INSERT`s
                // inherit: where the file points itself at a database, they must
                // not name one, or the retarget gesture moves half the file.
                insert_database: if target_database_sql(dialect, database).is_some() {
                    String::new()
                } else {
                    database.to_string()
                },
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

    // ── Key counters, once the rows they have to clear are in ────────────────
    if opts.data {
        let resync: Vec<String> = order
            .iter()
            .flat_map(|&i| sequence_resync_sql(&schema.tables[i], dialect))
            .collect();
        if !resync.is_empty() {
            steps.push(DumpStep::Text("-- Key sequences".to_string()));
            steps.extend(resync.into_iter().map(DumpStep::Text));
        }
    }

    // ── Routines and events, once the tables they read exist ─────────────────
    if !routines.is_empty() {
        steps.push(DumpStep::Text("-- Routines and events".to_string()));
        // Through the client wrapper, so a MySQL compound body gets its
        // `DELIMITER` — the same rule the triggers above follow.
        steps.push(DumpStep::Text(crate::ddl::client_script(
            &routines, dialect,
        )));
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
        missing,
    }
}

// ── The folder export: one file per table ────────────────────────────────────

/// One table's file, in a folder export.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStep {
    /// The table's namespace, where the engine has them — carried so the
    /// renderer can name the source the way the SQL export does.
    pub schema: Option<String>,
    pub table: String,
    /// What to run for this table's rows.
    pub select: String,
    /// The file's name **under the chosen folder** — never a path. Sanitized and
    /// unique across the plan; see [`crate::export::export_file_names`].
    pub file: String,
}

/// A folder export, decided: which table goes into which file, and what to read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilePlan {
    pub files: Vec<FileStep>,
    /// Tables the user ticked that this run's fresh introspection could not
    /// find. Same guarantee, and the same reason, as [`DumpPlan::missing`]: a
    /// folder one file short of what was ticked looks exactly like a complete
    /// one.
    pub missing: Vec<String>,
}

/// Plan a **folder** export — the schema tree's `Export ▸ CSV` and its siblings,
/// which write one file per table rather than one file for the set.
///
/// **Not [`plan`] with the options turned down, and the row step is why.** A
/// dump's `SELECT` names its columns through [`exported_columns`], which leaves
/// out everything the server assigns for itself: an `INSERT` that named an
/// identity column would be an error rather than a value. A CSV of `orders`
/// without `orders.id` is not the table, so this reads `*` — every column the
/// row has. The two differ in exactly the place a shared step would have hidden.
///
/// Views are included for the mirror-image reason: [`plan`] gives a view
/// structure and no rows because an `INSERT` into one is not a restore, while a
/// CSV of a view is simply its rows.
///
/// `chosen` holds display names ([`display_name`]); the plan keeps their order,
/// and anything not resolvable against `schema` lands in
/// [`FilePlan::missing`] rather than being dropped.
pub fn file_plan(
    schema: &DbSchema,
    database: &str,
    chosen: &[String],
    format: ExportFormat,
    dialect: SqlDialect,
) -> FilePlan {
    // **`Sql` is the dump's, and refused here rather than merely documented.**
    // Every doc around this path says the format is never `Sql`, and until this
    // line that held only because `run_export` branches on `writes_folder()`
    // first — one new call site away from a corrupt file. With `Sql` the steps
    // below would render `INSERT`s from `SELECT *`, naming exactly the
    // server-assigned columns [`exported_columns`] keeps out of them, and the
    // file would fail at restore *after* its rows had landed.
    //
    // An empty plan, not a panic: the caller already reports one as "Nothing to
    // export", which is a refusal the user can read.
    if matches!(format, ExportFormat::Sql) {
        return FilePlan::default();
    }
    let mut found: Vec<&TableInfo> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for name in chosen {
        match schema
            .tables
            .iter()
            .find(|t| display_name(t.schema.as_deref(), &t.name) == *name)
        {
            Some(t) => found.push(t),
            None => missing.push(name.clone()),
        }
    }
    // Named from the *resolved* tables, so the counter that breaks a collision
    // is not spent on a table that will never be written.
    let names: Vec<String> = found
        .iter()
        .map(|t| display_name(t.schema.as_deref(), &t.name))
        .collect();
    let files = export_file_names(&names, format);
    FilePlan {
        files: found
            .into_iter()
            .zip(files)
            .map(|(t, file)| FileStep {
                schema: t.schema.clone(),
                table: t.name.clone(),
                select: format!(
                    "SELECT * FROM {}",
                    qualified_table(database, t.schema.as_deref(), &t.name, dialect)
                ),
                file,
            })
            .collect(),
        missing,
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

    // ── file_plan ────────────────────────────────────────────────────────────

    #[test]
    fn file_plan_names_one_file_per_chosen_table() {
        let schema = schema_of(vec![table("actor"), table("film")]);
        let p = file_plan(
            &schema,
            "sakila",
            &all(&schema),
            ExportFormat::Csv,
            SqlDialect::MySql,
        );
        assert_eq!(
            p.files.iter().map(|f| f.file.clone()).collect::<Vec<_>>(),
            vec!["actor.csv", "film.csv"]
        );
        assert!(p.missing.is_empty());
    }

    #[test]
    fn file_plan_selects_every_column_including_the_server_assigned_ones() {
        // The difference from `plan`, and the reason this is not a thin wrapper
        // over it: an `INSERT` may not name an identity column, so `plan`'s row
        // step leaves it out — but a CSV of `orders` without `orders.id` is not
        // the table. A folder export reads the whole row.
        let mut t = table("orders");
        t.columns.push(ColumnInfo {
            name: "id".to_string(),
            type_name: "int".to_string(),
            auto_increment: true,
            ..Default::default()
        });
        let schema = schema_of(vec![t]);
        let p = file_plan(
            &schema,
            "shop",
            &all(&schema),
            ExportFormat::Csv,
            SqlDialect::MySql,
        );
        assert_eq!(p.files.len(), 1);
        assert!(
            p.files[0].select.contains('*'),
            "every column, not a named list: {}",
            p.files[0].select
        );
    }

    #[test]
    fn file_plan_includes_views() {
        // `plan` gives a view structure and no rows, because an `INSERT` into one
        // is not a restore. A CSV of a view is just its rows, so it is offered.
        let schema = schema_of(vec![table("actor"), view("actor_info")]);
        let p = file_plan(
            &schema,
            "sakila",
            &all(&schema),
            ExportFormat::Json,
            SqlDialect::MySql,
        );
        assert_eq!(
            p.files.iter().map(|f| f.table.clone()).collect::<Vec<_>>(),
            vec!["actor", "actor_info"]
        );
    }

    #[test]
    fn file_plan_reports_a_table_the_introspection_lost() {
        // Same guarantee as `DumpPlan::missing`: a folder one file short of what
        // was ticked must not read as a clean success.
        let schema = schema_of(vec![table("actor")]);
        let chosen = vec!["actor".to_string(), "ghost".to_string()];
        let p = file_plan(
            &schema,
            "sakila",
            &chosen,
            ExportFormat::Csv,
            SqlDialect::MySql,
        );
        assert_eq!(p.files.len(), 1);
        assert_eq!(p.missing, vec!["ghost".to_string()]);
    }

    #[test]
    fn file_plan_qualifies_the_select_per_engine() {
        let mut t = table("orders");
        t.schema = Some("sales".to_string());
        let schema = schema_of(vec![t]);
        let chosen = all(&schema);
        let pg = file_plan(
            &schema,
            "shop",
            &chosen,
            ExportFormat::Csv,
            SqlDialect::Postgres,
        );
        assert_eq!(pg.files[0].select, "SELECT * FROM \"sales\".\"orders\"");
        // And the file name keeps the namespace, so two same-named tables in
        // different schemas do not need a counter to tell them apart.
        assert_eq!(pg.files[0].file, "sales.orders.csv");
    }

    #[test]
    fn file_plan_breaks_a_file_name_collision() {
        // Straight through to `export_file_names` — stated here because the seam
        // between the two is where a silent overwrite would live.
        let schema = schema_of(vec![table("a:b"), table("a*b")]);
        let p = file_plan(
            &schema,
            "db",
            &all(&schema),
            ExportFormat::Csv,
            SqlDialect::MySql,
        );
        assert_eq!(
            p.files.iter().map(|f| f.file.clone()).collect::<Vec<_>>(),
            vec!["a_b.csv", "a_b_2.csv"]
        );
    }

    /// **The invariant is enforced, not narrated.** Every doc around this path
    /// says the format is never `Sql`, and today that holds only because
    /// `run_export` branches on `writes_folder()` before reaching it — one call
    /// site away from a corrupt file. `Sql` here would emit `INSERT`s built from
    /// `SELECT *`, naming the identity columns `exported_columns` exists to keep
    /// out of them: a file that fails at restore, after the rows have landed.
    ///
    /// An empty plan rather than a panic, so the caller reports a refusal
    /// instead of taking the window down.
    #[test]
    fn file_plan_refuses_sql_rather_than_writing_inserts() {
        let schema = schema_of(vec![table("actor")]);
        let p = file_plan(
            &schema,
            "sakila",
            &all(&schema),
            ExportFormat::Sql,
            SqlDialect::MySql,
        );
        assert!(p.files.is_empty(), "SQL is the dump's, not this path's");
        // Every other format still plans normally — the refusal is one variant
        // wide, not a general timidity.
        for f in ExportFormat::ALL {
            if f == ExportFormat::Sql {
                continue;
            }
            assert_eq!(
                file_plan(&schema, "sakila", &all(&schema), f, SqlDialect::MySql)
                    .files
                    .len(),
                1,
                "{} should still plan",
                f.label()
            );
        }
    }

    #[test]
    fn file_plan_of_nothing_is_empty() {
        let schema = schema_of(vec![table("actor")]);
        let p = file_plan(&schema, "db", &[], ExportFormat::Csv, SqlDialect::MySql);
        assert!(p.files.is_empty() && p.missing.is_empty());
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
        let (order, cycles) = order_tables(&s.tables, &all(&s), SqlDialect::MySql);
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
        let (order, cycles) = order_tables(&s.tables, &all(&s), SqlDialect::MySql);
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
        let (order, cycles) = order_tables(&s.tables, &all(&s), SqlDialect::MySql);
        assert_eq!(names(&s, &order), vec!["employees"]);
        assert!(!cycles, "a self-reference orders fine — it is one table");
    }

    #[test]
    fn a_two_table_cycle_still_dumps_every_table_and_says_so() {
        let s = schema_of(vec![refs(table("a"), "b"), refs(table("b"), "a")]);
        let (order, cycles) = order_tables(&s.tables, &all(&s), SqlDialect::MySql);
        assert!(
            cycles,
            "no order satisfies both keys — the caller must know"
        );
        assert_eq!(order.len(), 2, "a cycle must not drop a table");
    }

    #[test]
    fn an_fk_to_a_table_outside_the_selection_does_not_order_it_in() {
        let s = schema_of(vec![refs(table("orders"), "archive"), table("archive")]);
        let (order, cycles) = order_tables(&s.tables, &["orders".to_string()], SqlDialect::MySql);
        assert_eq!(names(&s, &order), vec!["orders"]);
        assert!(!cycles);
    }

    #[test]
    fn views_come_after_every_base_table() {
        let s = schema_of(vec![view("v_recent"), table("orders"), table("customers")]);
        let (order, _) = order_tables(&s.tables, &all(&s), SqlDialect::MySql);
        assert_eq!(names(&s, &order).last().unwrap(), "v_recent");
    }

    #[test]
    fn ties_break_by_name_so_two_dumps_of_one_schema_match() {
        let s = schema_of(vec![table("zebra"), table("apple"), table("mango")]);
        let (order, _) = order_tables(&s.tables, &all(&s), SqlDialect::MySql);
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

    // ── which of the two halves' failures the user is told about ─────────────

    /// The rule, and the reason it is not symmetric: a cancelled read closes the
    /// channels, which the writer sees as an ordinary end of stream — so on its
    /// own the writer would call a truncated file finished.
    #[test]
    fn a_cancel_is_the_readers_to_declare_whatever_the_writer_saw() {
        for write in [
            WriteEnd::Wrote,
            WriteEnd::Failed("disk full".to_string()),
            WriteEnd::Died("panic".to_string()),
        ] {
            assert_eq!(
                dump_verdict(ReadEnd::Cancelled, write.clone()),
                DumpVerdict::Cancelled,
                "{write:?}"
            );
        }
    }

    /// And the other direction: anything that is *not* a cancel failed the writer
    /// first, and the reader then only ever saw "nobody is reading any more".
    /// Preferring the reader's words there is how "The disk is full" became
    /// "connection reset".
    #[test]
    fn the_writers_words_win_over_the_readers_for_a_real_failure() {
        assert_eq!(
            dump_verdict(
                ReadEnd::Failed("connection reset".to_string()),
                WriteEnd::Failed("Export failed: disk full".to_string())
            ),
            DumpVerdict::Failed {
                message: "Export failed: disk full".to_string(),
                partial: true,
            }
        );
        // A worker that did not come back is named as such.
        assert_eq!(
            dump_verdict(ReadEnd::Clean, WriteEnd::Died("panicked".to_string())),
            DumpVerdict::Failed {
                message: "Export failed: worker died: panicked".to_string(),
                partial: true,
            }
        );
        // The reader's reason is used only when the writer had none.
        assert_eq!(
            dump_verdict(
                ReadEnd::Failed("connection reset".to_string()),
                WriteEnd::Wrote
            ),
            DumpVerdict::Failed {
                message: "Export failed: connection reset".to_string(),
                partial: true,
            }
        );
    }

    #[test]
    fn only_two_clean_halves_are_a_finished_dump() {
        assert_eq!(
            dump_verdict(ReadEnd::Clean, WriteEnd::Wrote),
            DumpVerdict::Done
        );
    }

    /// A view has structure and no rows, and a structure-only dump streams
    /// nothing at all — so counting *tables* promised a "12 of 12" the progress
    /// line never reached.
    #[test]
    fn the_progress_denominator_counts_what_will_actually_stream() {
        let s = schema_of(vec![table("orders"), view("v_orders")]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        );
        assert_eq!(p.tables, 2, "the file covers both");
        assert_eq!(p.streamed_tables(), 1, "only one of them has rows");

        let structure_only = DumpOptions {
            data: false,
            ..Default::default()
        };
        let p = plan(&s, "shop", &all(&s), structure_only, SqlDialect::MySql);
        assert_eq!(p.streamed_tables(), 0);
    }

    // ── what the picker opens with ───────────────────────────────────────────

    #[test]
    fn a_namespace_keeps_its_own_tables_and_public_keeps_the_unqualified_ones() {
        let names = [
            "orders".to_string(),
            "customers".to_string(),
            "sales.orders".to_string(),
            "archive.orders".to_string(),
        ];
        assert_eq!(
            tables_in_namespace(&names, Some("sales")),
            vec!["sales.orders".to_string()]
        );
        // **`public` is the trap.** `display_name` omits it, so a `"public."`
        // prefix match filters a `public` dump down to nothing.
        assert_eq!(
            tables_in_namespace(&names, Some("public")),
            vec!["orders".to_string(), "customers".to_string()]
        );
        // Opened on a database rather than a namespace: everything stays.
        assert_eq!(tables_in_namespace(&names, None), names.to_vec());
    }

    #[test]
    fn the_picker_ticks_everything_unless_a_table_was_named() {
        let names = ["orders".to_string(), "customers".to_string()];
        assert_eq!(
            initial_selection(&names, None),
            (names.to_vec(), None),
            "opened on a database: all of it"
        );
        assert_eq!(
            initial_selection(&names, Some("orders")),
            (vec!["orders".to_string()], None),
            "opened on a table: that table"
        );
    }

    /// A modal that opens with a full list, nothing ticked and a dead Export
    /// button reads as broken. The table was dropped or renamed since the tree
    /// last refreshed, and that is worth a sentence.
    #[test]
    fn a_preselect_the_list_has_lost_is_named() {
        let names = ["orders".to_string()];
        let (chosen, error) = initial_selection(&names, Some("gone"));
        assert!(chosen.is_empty());
        assert!(
            error.as_deref().is_some_and(|e| e.contains("gone")),
            "{error:?}"
        );
    }

    // ── the file has to address one database, and it has to be the target ────

    /// The Critical: `DROP`/`CREATE` name a MySQL table bare so the `USE` line is
    /// the one thing to edit, while the `INSERT`s qualified with the **source**.
    /// Editing that line — the retarget this module's own doc prescribes — left
    /// the target empty and refilled the live source, with a success report.
    #[test]
    fn a_mysql_insert_does_not_name_the_source_database_the_use_line_already_points_at() {
        let s = schema_of(vec![table("orders")]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        );
        let DumpStep::Rows {
            database,
            insert_database,
            ..
        } = p
            .steps
            .iter()
            .find(|s| matches!(s, DumpStep::Rows { .. }))
            .expect("a data step")
        else {
            unreachable!()
        };
        // The read still comes from the source; only the write target is bare.
        assert_eq!(database, "shop");
        assert_eq!(insert_database, "");
        // And that is exactly what `qualified_table` renders as a bare name, so
        // the `INSERT` matches the `CREATE` above it.
        assert_eq!(
            qualified_table(insert_database, None, "orders", SqlDialect::MySql),
            "`orders`"
        );
        assert_eq!(
            qualified_table(database, None, "orders", SqlDialect::MySql),
            "`shop`.`orders`"
        );
    }

    /// The counterweight: PostgreSQL has no `USE` line, so its `INSERT`s must
    /// keep naming the namespace — both halves of the file agree there already.
    #[test]
    fn a_postgres_insert_keeps_its_namespace() {
        let mut t = table("orders");
        t.schema = Some("sales".to_string());
        let s = schema_of(vec![t]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        );
        let DumpStep::Rows {
            insert_database,
            schema,
            ..
        } = p
            .steps
            .iter()
            .find(|s| matches!(s, DumpStep::Rows { .. }))
            .expect("a data step")
        else {
            unreachable!()
        };
        assert_eq!(insert_database, "shop");
        assert_eq!(schema.as_deref(), Some("sales"));
    }

    // ── the file has to be replayable ────────────────────────────────────────

    /// A compound trigger body holds its own semicolons; without `DELIMITER` the
    /// file dies at the first one (live ERROR 1064) — after the `DROP` above it
    /// has already run against the target.
    #[test]
    fn a_mysql_trigger_is_wrapped_for_a_client_that_splits_on_semicolons() {
        let mut t = table("orders");
        t.triggers.push(TriggerInfo {
            name: "trg_orders".to_string(),
            table: "orders".to_string(),
            timing: TriggerTiming::Before,
            events: vec![TriggerEvent::Insert],
            action: TriggerAction::Body("BEGIN\n  SET NEW.id = 1;\nEND".to_string()),
            ..Default::default()
        });
        let s = schema_of(vec![t]);
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        ));
        assert!(file.contains("DELIMITER $$"), "{file}");
        assert!(file.contains("DELIMITER ;"), "{file}");
    }

    /// PostgreSQL has no FK guard an ordinary role can throw, so a bare `DROP`
    /// stopped the very case a dump is most often tested with — replaying onto
    /// the database it came from.
    #[test]
    fn a_postgres_drop_cascades_and_the_others_do_not() {
        assert_eq!(drop_cascade(SqlDialect::Postgres), " CASCADE");
        assert_eq!(drop_cascade(SqlDialect::MySql), "");
        assert_eq!(drop_cascade(SqlDialect::Sqlite), "");
        let mut t = table("orders");
        t.schema = Some("public".to_string());
        let s = schema_of(vec![t]);
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        // `public` is the default namespace, so `sql_qualifier` leaves it off —
        // the point here is the ` CASCADE`, and that the `DROP` still names the
        // table its `CREATE` is about to make.
        assert!(
            text.contains(r#"DROP TABLE IF EXISTS "orders" CASCADE;"#),
            "{text}"
        );
    }

    /// `USE shop` on a server with no `shop` is ERROR 1049 on line 1, and
    /// restoring onto a fresh server is the primary use case.
    #[test]
    fn the_file_creates_its_own_container_before_entering_it() {
        let s = schema_of(vec![table("orders")]);
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        ));
        let create = text
            .find("CREATE DATABASE IF NOT EXISTS `shop`;")
            .expect("a CREATE DATABASE");
        let use_line = text.find("USE `shop`;").expect("a USE");
        assert!(create < use_line, "{text}");

        // PostgreSQL: the namespace, not the database — the connection is already
        // pointed at one, and `public` always exists.
        let mut t = table("orders");
        t.schema = Some("sales".to_string());
        let s = schema_of(vec![t]);
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        assert!(
            text.contains(r#"CREATE SCHEMA IF NOT EXISTS "sales";"#),
            "{text}"
        );
        assert!(!text.contains("CREATE DATABASE"), "{text}");

        let mut t = table("orders");
        t.schema = Some("public".to_string());
        let s = schema_of(vec![t]);
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        assert!(!text.contains("CREATE SCHEMA"), "{text}");
    }

    /// A view on a view was created first because views left in *name* order and
    /// the dependency walk is built from foreign keys, which a view has none of.
    /// Live ERROR 1146 — and `DROP VIEW IF EXISTS` had already removed it.
    #[test]
    fn a_view_built_on_another_view_comes_after_it() {
        let mut base = view("a_summary");
        base.view_definition = Some("SELECT * FROM orders".to_string());
        let mut on_top = view("b_detail");
        on_top.view_definition = Some("SELECT * FROM z_totals".to_string());
        let mut last = view("z_totals");
        last.view_definition = Some("SELECT * FROM orders".to_string());
        let s = schema_of(vec![table("orders"), base, on_top, last]);
        let (order, cycles) = order_tables(&s.tables, &all(&s), SqlDialect::MySql);
        assert!(!cycles);
        let got = names(&s, &order);
        let at = |n: &str| got.iter().position(|g| g == n).unwrap();
        assert!(at("z_totals") < at("b_detail"), "{got:?}");
        // Base tables still come before every view, and ties are still by name.
        assert_eq!(got[0], "orders");
        assert!(at("a_summary") < at("b_detail"), "{got:?}");
    }

    /// The counterweight to the scan: a name inside a comment or a string
    /// literal is not a dependency, and neither is one buried in a longer
    /// identifier.
    #[test]
    fn a_view_name_that_is_only_mentioned_is_not_a_dependency() {
        let mut first = view("a_view");
        first.view_definition =
            Some("-- see z_view\nSELECT 'z_view' AS note, z_view_backup FROM orders".to_string());
        let s = schema_of(vec![table("orders"), first, view("z_view")]);
        let (order, _) = order_tables(&s.tables, &all(&s), SqlDialect::MySql);
        let got = names(&s, &order);
        // No edge, so the name tie-break stands.
        assert_eq!(got, vec!["orders", "a_view", "z_view"]);
    }

    /// A `LANGUAGE sql` function naming a table that does not exist yet fails at
    /// `CREATE` with `check_function_bodies` on — PostgreSQL's default — and the
    /// whole standalone-object array was emitted ahead of the table loop.
    #[test]
    fn routines_come_after_the_tables_they_read() {
        let mut t = table("orders");
        t.schema = Some("public".to_string());
        let mut s = schema_of(vec![t]);
        s.routines
            .push(std::sync::Arc::new(crate::schema::RoutineInfo {
                name: "orders_count".to_string(),
                schema: Some("public".to_string()),
                kind: crate::schema::RoutineKind::Function,
                language: "sql".to_string(),
                body: "SELECT count(*) FROM orders".to_string(),
                ..Default::default()
            }));
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        let routines = file
            .find("-- Routines and events")
            .expect("a routine section");
        let table_ddl = file.find("CREATE TABLE").expect("the table");
        assert!(table_ddl < routines, "{file}");
    }

    /// The rows come back with their keys, but an explicit insert does not move
    /// the sequence: the first ordinary insert after a "successful" restore is a
    /// duplicate-key error, and it repeats until the counter catches up.
    #[test]
    fn a_postgres_key_counter_is_moved_past_the_rows_that_were_loaded() {
        let mut t = table("orders");
        t.schema = Some("public".to_string());
        t.columns[0].auto_increment = true;
        let s = schema_of(vec![t]);
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        let setval = file.find("setval").expect("a setval");
        let rows = file.find("<<rows orders").expect("the data step");
        assert!(rows < setval, "the counter is set from the rows: {file}");
        assert!(file.contains("pg_get_serial_sequence"), "{file}");
        // A column with no sequence behind it, and an empty table, both have to
        // be no-ops rather than errors.
        assert!(
            file.contains("WHERE s IS NOT NULL AND v IS NOT NULL"),
            "{file}"
        );

        // MySQL and SQLite maintain their counters as rows land.
        for dialect in [SqlDialect::MySql, SqlDialect::Sqlite] {
            let file = file_of(&plan(&s, "shop", &all(&s), DumpOptions::default(), dialect));
            assert!(!file.contains("setval"), "{dialect:?}: {file}");
        }
    }

    /// A key with no namespace means "in the owner's", not "in any": a selection
    /// spanning two schemas matched on the table's *name* alone, so a
    /// `sales.orders` key was restated bare against `archive.orders` — and
    /// counted as carried rather than reported as dropped.
    #[test]
    fn a_key_without_a_namespace_means_the_owners_not_any() {
        let mut owner = refs(table("lines"), "orders");
        owner.schema = Some("sales".to_string());
        let mut other = table("orders");
        other.schema = Some("archive".to_string());
        let s = schema_of(vec![owner, other]);
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        // The key points into `sales`, which is not in the file: dropped, and
        // said so — not restated against the `archive` table that happens to
        // share the name.
        assert!(text.contains("foreign key is not restated"), "{text}");
        assert!(!text.contains("ADD CONSTRAINT"), "{text}");
    }

    /// The re-introspection is deliberate, but its cost is that a selection can
    /// go stale — and a file one table short of what was ticked looks exactly
    /// like a whole one.
    #[test]
    fn a_ticked_table_that_vanished_is_named_rather_than_dropped_in_silence() {
        let s = schema_of(vec![table("orders")]);
        let p = plan(
            &s,
            "shop",
            &["orders".to_string(), "customers".to_string()],
            DumpOptions::default(),
            SqlDialect::MySql,
        );
        assert_eq!(p.missing, vec!["customers".to_string()]);
        assert!(text_of(&p).contains("customers"), "{}", text_of(&p));
        // And the whole selection vanishing still carries the names, so the
        // "nothing matched" arm can say which.
        let p = plan(
            &s,
            "shop",
            &["gone".to_string()],
            DumpOptions::default(),
            SqlDialect::MySql,
        );
        assert!(p.steps.is_empty());
        assert_eq!(p.missing, vec!["gone".to_string()]);
    }

    #[test]
    fn nothing_at_all_is_planned_when_no_section_was_asked_for() {
        let s = schema_of(vec![table("orders")]);
        let opts = DumpOptions {
            structure: false,
            data: false,
            other_objects: false,
            ..Default::default()
        };
        assert!(opts.is_empty());
        let p = plan(&s, "shop", &all(&s), opts, SqlDialect::MySql);
        assert!(p.steps.is_empty());
    }

    /// "Other objects" is a peer checkbox in the modal, so ticking it alone is
    /// something a user can do — and it left the Export button permanently grey
    /// while `plan` would have emitted nothing anyway.
    #[test]
    fn other_objects_alone_is_a_file_worth_writing() {
        let opts = DumpOptions {
            structure: false,
            data: false,
            other_objects: true,
            ..Default::default()
        };
        assert!(!opts.is_empty(), "the Export button must not be grey");

        let mut t = table("orders");
        t.schema = Some("public".to_string());
        let mut s = schema_of(vec![t]);
        s.enums.push(crate::schema::EnumInfo {
            name: "mood".to_string(),
            schema: Some("public".to_string()),
            values: vec!["ok".to_string()],
            comment: None,
        });
        let text = text_of(&plan(&s, "shop", &all(&s), opts, SqlDialect::Postgres));
        assert!(text.contains("mood"), "{text}");
        // …and nothing else: no `CREATE TABLE`, no rows.
        assert!(!text.contains("CREATE TABLE"), "{text}");
    }
}
