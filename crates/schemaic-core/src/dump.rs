//! Schema + data dump: the plan for one `.sql` file that recreates a set of
//! tables and refills them.
//!
//! Both halves of a dump already existed and had never been joined:
//! [`crate::schema::TableInfo::create_ddl`] is the structure (Copy DDL's own
//! emitter, split around the rows by `create_ddl_holding`) and the grid's SQL
//! export ([`crate::export::ExportFormat::Sql`]) is the data (one `INSERT` per
//! row, streamed). This module decides **what goes in the file and in what
//! order**; it writes nothing and connects to nothing, so every decision in it
//! is unit-testable. `schemaic-app` executes the plan — a [`DumpStep::Text`] is
//! written straight out, a [`DumpStep::Rows`] is streamed through
//! [`render_rows`] into the same writer. That shares the export's renderer,
//! `export_inserts_ending`, but is not the export: on a batch-separated engine
//! every `INSERT` closes its own `GO` batch, and the columns a text cell cannot
//! carry are written from the literals the server rendered
//! ([`DumpStep::Rows::server`]). SQL Server's standalone objects have emitters
//! of their own here and in [`crate::schema::TsqlObject`].
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
//! apply path uses — not a second one. Triggers are restated for the same
//! reason, and for the same reason as the keys they come after the data: a
//! trigger created before the rows fires on every one of them.
//!
//! That section is skipped for a table whose model carries **verbatim** DDL
//! ([`crate::schema::TableInfo::create_sql`], which is SQLite's whole captured
//! statement): its `CREATE TABLE` already contains the constraints, and there is
//! no `ALTER TABLE … ADD CONSTRAINT` to add them with. The question is asked of
//! the *table*, not of the engine — the data answers it directly.

use crate::ddl::{Change, ChangeSet, ObjectKind};
use crate::export::{ExportFormat, export_file_names, ident_sql, qualified_table};
use crate::intel::SqlDialect;
use crate::schema::{DbSchema, ServerFlavour, TableInfo, TableShape, display_name};

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
    /// `DROP TABLE IF EXISTS` before each `CREATE` — and, where the file
    /// drops up front ([`drops_up_front`]) inside its transaction, every
    /// routine in the dumped namespaces too ([`drop_before_create_hint`] is
    /// how the modal says so). Never what the file cannot put back as it is.
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
        /// The columns `select` asks the server to render as SQL, because a
        /// text cell cannot carry them ([`literal_select`]), and in which
        /// form — what [`render_rows`] writes them from.
        server: Vec<(String, crate::export::ServerLiteral)>,
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
    /// What the file creates but cannot put back as it is, each with why —
    /// `dbo.knows (a graph edge table: …)`. A replay never drops these; it
    /// stops before dropping anything where one is there
    /// ([`refuse_if_present_sql`]). Named in the header and, through
    /// [`refused_note`], in the modal.
    pub refused: Vec<String>,
    /// What the file leaves out though the export asked for it, each with
    /// why — a table no script from the model restates (`dbo.emp (a
    /// system-versioned temporal table)`), and every view, routine or table
    /// that could not be created without one (`dbo.emp_v (a view reading
    /// dbo.emp, …)`).
    ///
    /// **Named in the modal as well as the header**, beside
    /// [`DumpPlan::missing`] and for its reason: a file a table short of what
    /// was ticked looks exactly like a complete one, and the green report
    /// said "Wrote 2 tables." over it. Its dependents are left out with it
    /// because one kept stopped the whole restore (Msg 208), which *One
    /// transaction* then rolled back entirely.
    pub left_out: Vec<String>,
    /// The tables whose rows the file does not carry though it creates them
    /// — a graph edge's (see [`plan`]'s header) — named in the modal for
    /// [`DumpPlan::left_out`]'s reason.
    pub rows_left_out: Vec<String>,
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
    ///
    /// `opened` says whether the `.part` file was ever created — which is the
    /// whole of what [`DumpVerdict::Failed::partial`] means, and what nothing
    /// used to carry. `File::create` is the writer's first statement, so a
    /// read-only folder, a full volume or a share that has just dropped fails
    /// *before* any fragment exists — and the note told the user "the rows that
    /// were written are in shop.sql.part" about a file that had never been
    /// opened, on the one arm where they are least likely to look twice.
    Failed { message: String, opened: bool },
    /// The worker task itself did not come back.
    Died(String),
}

/// What to report about a finished dump.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DumpVerdict {
    Done,
    /// `partial` means the same thing it does on [`DumpVerdict::Failed`], and it
    /// is here for the same reason it was added there.
    ///
    /// **A cancel during the schema read never opened a file.** `fetch_schema`
    /// takes the token precisely because it is the longest phase on a large
    /// database, and it returns before `part_of` is computed and before the
    /// writer is spawned — so no `shop.sql.part` exists anywhere, while the
    /// modal said "what had been written is in shop.sql.part" and sent the user
    /// to look for it. That is the identical false claim `c3c2127` removed from
    /// the `Failed` arm one line over, which this enum did not follow.
    Cancelled {
        partial: bool,
    },
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
    // **`partial` is the writer's answer, not a constant.** It was hardcoded
    // `true` for every failure while its own doc says it "means a `.part`
    // fragment is on disk and worth naming" — a claim the code never made. The
    // writer is the only thing that knows, so it says.
    let failed = |message: String, partial: bool| DumpVerdict::Failed { message, partial };
    match (read, write) {
        // The writer is the only thing that knows whether a fragment exists —
        // the same rule `partial` follows on the `Failed` arms. `Wrote` reaches
        // here when the reader was cancelled after the writer had finished its
        // own stream, and `Died` when it was holding the file.
        (ReadEnd::Cancelled, WriteEnd::Failed { opened, .. }) => {
            DumpVerdict::Cancelled { partial: opened }
        }
        (ReadEnd::Cancelled, _) => DumpVerdict::Cancelled { partial: true },
        (_, WriteEnd::Failed { message, opened }) => failed(message, opened),
        // The worker died mid-run, so whatever it had opened is still there.
        (_, WriteEnd::Died(e)) => failed(format!("Export failed: worker died: {e}"), true),
        // The *reader* failed, which means the writer was running and had its
        // file.
        (ReadEnd::Failed(e), _) => failed(format!("Export failed: {e}"), true),
        (ReadEnd::Clean, WriteEnd::Wrote) => DumpVerdict::Done,
    }
}

/// What a **cancelled dump** says.
///
/// [`crate::export::export_cancel_note`]'s twin, and deliberately not that
/// function: it ends "the rows that were written are in …", which is right for a
/// result export, because a result export is nothing but rows. This file is
/// `CREATE TABLE`s and triggers as well, and a structure-only one has no rows at
/// all — so borrowing that wording would describe a file that is not there. The
/// half that must not drift is *where the fragment went*, and both spellings
/// take it from [`crate::export::part_path`], the one function that knows the
/// suffix.
///
/// `partial` is the same fact [`DumpVerdict::Failed::partial`] carries, and it
/// is here for the same reason it was added there: `fetch_schema` takes the
/// cancellation token because it is the longest phase on a large database, and
/// it returns *before* the writer is spawned — so a cancel during the schema
/// read has created nothing, while the sentence pointed at a `.part` regardless
/// and sent the user to look for it.
///
/// Here rather than in the view for [`dump_verdict`]'s reason: it was a
/// `format!` inside a Floem closure, which is why the unconditional half went
/// unnoticed.
pub fn cancel_note(name: &str, partial: bool) -> String {
    if partial {
        format!(
            "Export cancelled — {name} was not changed; what had been written is in {}",
            crate::export::part_path(name)
        )
    } else {
        format!("Export cancelled — {name} was not changed.")
    }
}

/// Can a database be dumped to a `.sql` file on `dialect`?
///
/// Every engine can now — SQL Server's took [`identity_insert_sql`] around an
/// identity table's rows and [`close_batches`]' `GO` lines — but the match
/// stays exhaustive, so an engine added later has to answer it rather than
/// inherit a yes. The Export menu's *SQL* entry asks this.
pub fn supports_dump(dialect: SqlDialect) -> bool {
    match dialect {
        SqlDialect::MySql | SqlDialect::Postgres | SqlDialect::Sqlite | SqlDialect::MsSql => true,
    }
}

/// The statements that let a table's rows name its identity column, around
/// them — or `None` where the engine has no such switch.
///
/// **SQL Server's, and the reason its identities are carried at all.** Its
/// identity refuses an explicit value (`is_server_assigned` says so, for the
/// grid and the import), except under `SET IDENTITY_INSERT t ON`, one table
/// at a time per session. Carrying the values keeps every key the foreign keys
/// onto the table name; renumbering them, the other engines' cost for
/// PostgreSQL's `GENERATED ALWAYS`, would break each one. The server moves the
/// identity's next value past the highest one inserted, so no resync follows.
pub fn identity_insert_sql(dialect: SqlDialect, table: &str) -> Option<(String, String)> {
    match dialect {
        SqlDialect::MsSql => Some((
            format!("SET IDENTITY_INSERT {table} ON;"),
            format!("SET IDENTITY_INSERT {table} OFF;"),
        )),
        SqlDialect::MySql | SqlDialect::Postgres | SqlDialect::Sqlite => None,
    }
}

/// Is `c` an identity a dump in `dialect` writes back under
/// [`identity_insert_sql`], though the server would otherwise assign it?
fn carries_identity(c: &crate::schema::ColumnInfo, dialect: SqlDialect) -> bool {
    c.auto_increment && c.generated.is_none() && identity_insert_sql(dialect, "").is_some()
}

/// **Every statement closes its batch with `GO`**, on an engine whose scripts
/// are cut into batches (`SqlDialect::batch_separator`) — SQL Server, where
/// `CREATE VIEW`, `CREATE TRIGGER` and a routine must each open one, and a
/// restore (`Db::run_script`, `sqlcmd`) splits the file at its `GO` lines. A
/// row step closes its own, one per `INSERT` ([`render_rows`]); a comment
/// needs none; a script already ending in one (`ddl::client_script`'s) gets no
/// second.
fn close_batches(steps: Vec<DumpStep>, dialect: SqlDialect) -> Vec<DumpStep> {
    if !dialect.batch_separator() {
        return steps;
    }
    let mut out = Vec::with_capacity(steps.len() * 2);
    for step in steps {
        match step {
            DumpStep::Text(t)
                if t.lines()
                    .all(|l| l.trim().is_empty() || l.trim_start().starts_with("--")) =>
            {
                out.push(DumpStep::Text(t));
            }
            DumpStep::Text(t) => {
                let body = t.trim_end();
                if crate::ddl::ends_in_go(body, dialect) {
                    out.push(DumpStep::Text(body.to_string()));
                } else {
                    out.push(DumpStep::Text(format!("{body}\nGO")));
                }
            }
            rows @ DumpStep::Rows { .. } => out.push(rows),
        }
    }
    out
}

/// Render one [`DumpStep::Rows`] step: its rows, streamed from `src`, as the
/// `INSERT`s the file carries into `target` — what `schemaic-app`'s dump
/// writer runs for every such step, so the plan and the renderer agree on how
/// the step's batches close.
///
/// **On a batch-separated engine every `INSERT` closes its own batch.** All
/// of a table's statements used to share the one `GO` [`close_batches`] put
/// after the step, and SQL Server refuses a batch longer than 65,536 network
/// packets — 256 MB at the default 4 KB — so a `sqlcmd` restore of a large
/// table was dropped mid-file ("Communication link failure"), and the app's
/// own restore got through only because its script splitter also cut at every
/// `;`. A statement is at most `export::INSERT_BATCH_BYTES` plus one row, so a
/// batch per statement stays far inside the limit whatever the table's size.
/// Session state the rows need — `SET IDENTITY_INSERT`, the transaction —
/// outlives a `GO` on the one connection a restore holds.
///
/// `server` is the step's [`DumpStep::Rows::server`]: the columns written from
/// the SQL the server rendered them as.
pub fn render_rows<W: std::io::Write>(
    w: &mut W,
    src: &mut dyn crate::export::RowChunks,
    target: (&str, Option<&str>, &str),
    server: &[(String, crate::export::ServerLiteral)],
    dialect: SqlDialect,
) -> std::io::Result<crate::export::ExportTally> {
    let end = if dialect.batch_separator() {
        ";\nGO\n"
    } else {
        ";\n"
    };
    crate::export::export_inserts_ending(w, src, Some(target), dialect, end, server)
}

/// What a dump's `SELECT` reads for column `c` when a text cell cannot carry
/// its value: an expression the **server** renders as SQL, and the form the
/// renderer checks it against — or `None` to read the column as it is.
///
/// **SQL Server's bytes and variants.** A `varbinary`, `binary` or `image`
/// cell holds the `<n bytes>` placeholder, and so do the CLR types
/// `hierarchyid`, `geography` and `geometry`: a dump wrote that text as the
/// value (`N'<22 bytes>'`, refused on restore with Msg 24114), where other
/// blobs are withheld as `NULL`, which a `NOT NULL` column refuses as well.
/// Read as `0x` and hex instead, every one of them restores exactly — T-SQL
/// converts the bytes back to the CLR type, SRID, Z and M included (measured
/// on 2022). A `sql_variant` arrived as its value's text, so a `date` or
/// `decimal` variant restored as an `nvarchar` one and compared differently;
/// read as its base type and its value as text, it is written back as
/// `CAST(… AS <base type>)`. The text forms are the exact, language-proof
/// ones — `datetime` in the `T` form, a float with 17 digits (`CONVERT` style
/// 3), money with 4, and every other date and time type through its own type:
/// a variant's own text of one is style 0, `Jan  2 2026  3:04AM`, which drops
/// the seconds (measured).
///
/// **A character variant is read as the server's `FOR JSON` of it**, because
/// `CAST(v AS nvarchar(max))` stops at 4,000 characters — the `max` is not
/// honoured from a variant — while a `char`/`varchar` one holds up to 8,000,
/// and a 5,000-character value restored cut to 4,000 with nothing said
/// (measured on 2022 and 2025). `CAST(v AS varchar(8000))` would keep the
/// length but converts to the *database's* code page, which a Greek variant
/// in a Latin-1 database does not survive. `FOR JSON` converts under the
/// variant's own collation, keeps the whole text and escapes every control
/// character, where `FOR XML` refuses one (Msg 6841); it needs no
/// compatibility level, unlike `OPENJSON`, so the document is unpacked by the
/// renderer rather than the server.
///
/// **A column typed by an alias is judged by the alias's base**, looked up in
/// `aliases` (`DbSchema::tsql_objects`): its `type_name` is the alias's own
/// qualified name, `[dbo].[Hash]`, so an alias over `binary` was written as a
/// withheld blob's `NULL` — which a `NOT NULL` alias refuses on restore
/// (Msg 515) — and one over `sql_variant` came back an `nvarchar` variant.
///
/// Every other engine reads its columns as they are: a blob there stays
/// withheld and noted, as the export's own rule has it.
pub fn literal_select(
    c: &crate::schema::ColumnInfo,
    aliases: &[crate::schema::TsqlObject],
    dialect: SqlDialect,
) -> Option<(String, crate::export::ServerLiteral)> {
    match dialect {
        SqlDialect::MsSql => {}
        SqlDialect::MySql | SqlDialect::Postgres | SqlDialect::Sqlite => return None,
    }
    let col = ident_sql(&c.name, dialect);
    let type_name = aliases
        .iter()
        .find_map(|o| match (&o.kind, o.schema.as_deref()) {
            (crate::schema::TsqlObjectKind::AliasType { base, .. }, Some(schema))
                if format!(
                    "{}.{}",
                    ident_sql(schema, dialect),
                    ident_sql(&o.name, dialect)
                ) == c.type_name =>
            {
                Some(base.as_str())
            }
            _ => None,
        })
        .unwrap_or(&c.type_name);
    let head = type_name
        .split(|ch: char| ch == '(' || ch.is_whitespace())
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match head.as_str() {
        "binary" | "varbinary" | "image" | "hierarchyid" | "geography" | "geometry" => Some((
            format!(
                "CASE WHEN {col} IS NULL THEN NULL ELSE '0x' + \
                 CONVERT(varchar(max), CAST({col} AS varbinary(max)), 2) END AS {col}"
            ),
            crate::export::ServerLiteral::Hex,
        )),
        "sql_variant" => {
            let prop = |p: &str| format!("SQL_VARIANT_PROPERTY({col}, '{p}')");
            let base = format!("CAST({} AS sysname)", prop("BaseType"));
            let int = |p: &str| format!("CAST({} AS int)", prop(p));
            Some((
                format!(
                    "CASE WHEN {col} IS NULL THEN NULL ELSE CONCAT(\
                     {base}, \
                     CASE WHEN {base} IN (N'decimal', N'numeric') \
                          THEN CONCAT(N'(', {precision}, N',', {scale}, N')') \
                          WHEN {base} IN (N'datetime2', N'time', N'datetimeoffset') \
                          THEN CONCAT(N'(', {scale}, N')') \
                          WHEN {base} IN (N'char', N'varchar', N'binary', N'varbinary') \
                          THEN CONCAT(N'(', {len}, N')') \
                          WHEN {base} IN (N'nchar', N'nvarchar') \
                          THEN CONCAT(N'(', {len} / 2, N')') \
                          ELSE N'' END, \
                     CASE WHEN {base} IN (N'char', N'varchar', N'nchar', N'nvarchar') \
                          THEN CONCAT(N' COLLATE ', CAST({collation} AS nvarchar(128))) \
                          ELSE N'' END, \
                     N'|', \
                     CASE WHEN {base} IN (N'binary', N'varbinary') \
                          THEN '0x' + CONVERT(varchar(max), CAST({col} AS varbinary(8000)), 2) \
                          WHEN {base} IN (N'char', N'varchar', N'nchar', N'nvarchar') \
                          THEN (SELECT {col} AS x FOR JSON PATH, WITHOUT_ARRAY_WRAPPER) \
                          WHEN {base} IN (N'float', N'real') \
                          THEN CONVERT(nvarchar(64), CAST({col} AS float), 3) \
                          WHEN {base} IN (N'datetime', N'smalldatetime') \
                          THEN CONVERT(nvarchar(64), CAST({col} AS datetime), 126) \
                          WHEN {base} IN (N'money', N'smallmoney') \
                          THEN CONVERT(nvarchar(64), CAST({col} AS money), 2) \
                          WHEN {base} = N'date' \
                          THEN CONVERT(nvarchar(64), CAST({col} AS date), 23) \
                          WHEN {base} = N'time' \
                          THEN CAST(CAST({col} AS time(7)) AS nvarchar(64)) \
                          WHEN {base} = N'datetime2' \
                          THEN CAST(CAST({col} AS datetime2(7)) AS nvarchar(64)) \
                          WHEN {base} = N'datetimeoffset' \
                          THEN CAST(CAST({col} AS datetimeoffset(7)) AS nvarchar(64)) \
                          ELSE CAST({col} AS nvarchar(4000)) END) END AS {col}",
                    precision = int("Precision"),
                    scale = int("Scale"),
                    len = int("MaxLength"),
                    collation = prop("Collation"),
                ),
                crate::export::ServerLiteral::Variant,
            ))
        }
        _ => None,
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
        // SQL Server's switch is per table (`ALTER TABLE … NOCHECK CONSTRAINT
        // ALL`), and turning it back on with `CHECK` leaves every key marked
        // untrusted unless it is re-validated. Like PostgreSQL, the ordering
        // and the closing constraints section are the answer.
        SqlDialect::Postgres | SqlDialect::MsSql => None,
    }
}

/// How the file makes its string literals mean, on restore, what they meant
/// when they were written — or `None` where they always do.
///
/// The statement itself is [`crate::export::literal_mode_sql`]'s, which is
/// where the reasoning lives: it belongs next to the function that writes the
/// literals, and the live DDL runner needs the same statement without the
/// save-and-restore a *file* needs around it. `mysqldump` pins the mode at the
/// head of every file it writes for the same reason this does.
///
/// The restore half exists because a `.sql` file is replayed into a session the
/// user may go on using — `Db::run_script` holds one connection for the whole
/// file, and the psql/mysql client a user replays it in holds one for the
/// evening.
/// **The `@SCHEMAIC_OLD_SQL_MODE` wrapper is MySQL's syntax**, so this leans on
/// [`crate::export::literal_mode_sql`] answering for MySQL alone. If a second
/// dialect ever needs a statement there, this has to grow an arm rather than
/// wrap that dialect's statement in these two.
///
/// Whether a PostgreSQL dump should pin `standard_conforming_strings` the way
/// `pg_dump` does is a separate question and an open one — the *live* path has
/// no need of it, because `pg::connect_probe` pins the GUC on the startup
/// packet, but a `.sql` file is replayed in somebody else's client.
pub fn literal_mode_guard_sql(dialect: SqlDialect) -> Option<(String, &'static str)> {
    let set = crate::export::literal_mode_sql(dialect)?;
    debug_assert_eq!(
        dialect,
        SqlDialect::MySql,
        "the guard below is MySQL's session-variable syntax"
    );
    Some((
        format!("SET @SCHEMAIC_OLD_SQL_MODE = @@SESSION.sql_mode;\n{set};"),
        "SET SESSION sql_mode = @SCHEMAIC_OLD_SQL_MODE;",
    ))
}

/// The statement that makes the restoring session read the file's date
/// literals the way they were written — or `None` where they always are.
///
/// **SQL Server's `datetime` and `smalldatetime` read `2026-01-02 03:04:05`
/// by the session's language.** Under a day-first one — `british`, and the
/// defaults of German, French, Italian and Spanish logins — the string is
/// year-*day*-month: the 2nd of January restores as the 1st of February, and
/// the 25th of March fails the batch (Msg 242). `SET DATEFORMAT` is session
/// state, so one line at the head of the file holds for every later `GO`
/// batch on both restore paths — `Db::run_script`'s one connection and a
/// `sqlcmd` session. The newer types (`date`, `datetime2`,
/// `datetimeoffset`) read that form as ISO whatever the language says.
///
/// No restore half, unlike [`literal_mode_guard_sql`]: T-SQL has no
/// statement that reads the session's current format back to put it there.
pub fn date_format_sql(dialect: SqlDialect) -> Option<&'static str> {
    match dialect {
        SqlDialect::MsSql => Some("SET DATEFORMAT ymd;"),
        SqlDialect::MySql | SqlDialect::Postgres | SqlDialect::Sqlite => None,
    }
}

/// The settings a restoring SQL Server session must have for what the file
/// creates — each a batch of its own, at the head beside
/// [`date_format_sql`]; empty on the other engines.
///
/// **`sqlcmd` opens with `QUOTED_IDENTIFIER` OFF unless given `-I`**, and a
/// filtered index, an index on a computed column and an indexed view's index
/// are each refused under it (Msg 1934/1935). Every module's own script states
/// both settings it was created under (`ddl::tsql_settings_around`); this is
/// what the tables before them are created under.
pub fn ansi_settings_sql(dialect: SqlDialect) -> &'static [&'static str] {
    match dialect {
        SqlDialect::MsSql => &["SET ANSI_NULLS ON;", "SET QUOTED_IDENTIFIER ON;"],
        SqlDialect::MySql | SqlDialect::Postgres | SqlDialect::Sqlite => &[],
    }
}

/// How this dialect opens and closes the load's transaction.
pub fn transaction_sql(dialect: SqlDialect) -> (&'static str, &'static str) {
    match dialect {
        SqlDialect::MySql => ("START TRANSACTION;", "COMMIT;"),
        SqlDialect::Postgres | SqlDialect::Sqlite => ("BEGIN;", "COMMIT;"),
        // A bare `BEGIN` opens a block there, not a transaction.
        SqlDialect::MsSql => ("BEGIN TRANSACTION;", "COMMIT TRANSACTION;"),
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
        // T-SQL's `DROP TABLE` has no `CASCADE`; `drops_up_front` is its
        // answer.
        SqlDialect::MySql | SqlDialect::Sqlite | SqlDialect::MsSql => "",
    }
}

/// Does the file drop everything it recreates **up front**, in a section of
/// its own before any `CREATE`, rather than each object beside its `CREATE`?
///
/// **Where the engine has neither of the two answers above** — no session
/// switch for foreign keys ([`fk_guard_sql`]) and no `DROP … CASCADE`
/// ([`drop_cascade`]) — which is SQL Server. Tables are created parents
/// first, so a `DROP TABLE` beside its `CREATE` found the child's key still
/// standing, and replaying a default dump onto its source stopped at the
/// first referenced table (Msg 3726); a schema-bound view blocks its table's
/// drop the same way (Msg 3729). Computed from the two answers rather than
/// matched on the engine, so an engine that gains either stops needing it.
///
/// [`plan`] writes the section only inside the file's own transaction
/// ([`DumpOptions::wrap_transaction`]): it is safe to fail in only because a
/// failure undoes it.
pub fn drops_up_front(dialect: SqlDialect) -> bool {
    fk_guard_sql(dialect).is_none() && drop_cascade(dialect).is_empty()
}

/// The statement that stops a replay **before it drops anything** where
/// `object` (an already-quoted, qualified name) is there — for something the
/// file creates but cannot put back as it is, so must not drop — or `None`
/// where the engine has no such statement.
///
/// A replay is the file run onto a database that already holds what it
/// recreates. Something it cannot restate whole — a graph edge whose rows it
/// does not carry, an encrypted or signed module — was dropped there with
/// the rest and came back empty, as a comment, or unsigned, and the run
/// reported success. Left out of the drops, its `CREATE` would stop the run
/// further down instead, after other objects had been dropped, and with no
/// word of why. So the file says why, first: `why` is the error the run stops
/// with.
///
/// **SQL Server's**: `THROW` ends the batch with the message, and the
/// restore stops there, inside the file's transaction where it has one.
pub fn refuse_if_present_sql(dialect: SqlDialect, object: &str, why: &str) -> Option<String> {
    match dialect {
        SqlDialect::MsSql => Some(format!(
            "IF OBJECT_ID({}) IS NOT NULL THROW 50000, {}, 1;",
            crate::schema::ddl_string(object, dialect),
            crate::schema::ddl_string(why, dialect),
        )),
        SqlDialect::MySql | SqlDialect::Postgres | SqlDialect::Sqlite => None,
    }
}

/// The hint under the modal's *Drop before create* toggle on `dialect` —
/// the only consent the export asks for what a replay of the file drops.
///
/// **Where the file drops up front ([`drops_up_front`]) it says the
/// routines too.** There the toggle also drops and replaces every function
/// and procedure in the dumped schemas, used by the tables or not, so a
/// replay of an older dump puts older routine code in place of newer — and
/// the hint spoke of tables alone, which is all it means on the other
/// engines.
pub fn drop_before_create_hint(dialect: SqlDialect) -> &'static str {
    if drops_up_front(dialect) {
        "Drop each table and view before its CREATE, so the file loads onto a database that \
         already holds them. With One transaction it also replaces every routine in their \
         schemas, and stops before dropping anything it cannot put back as it is."
    } else {
        "DROP TABLE IF EXISTS before each CREATE, so the file loads onto a database \
         that already holds these tables."
    }
}

/// What the modal adds to a finished dump's report about
/// [`DumpPlan::refused`] — empty when there is nothing to say.
///
/// Said there as well as in the file's header because the modal is where the
/// user decides what to do with the file, and a replay that will stop is the
/// thing to know before trying one.
pub fn refused_note(refused: &[String]) -> String {
    if refused.is_empty() {
        return String::new();
    }
    let n = refused.len();
    format!(
        " A replay onto a database that already holds {} stops before it drops anything: \
         the file cannot put {} back as {}: {}.",
        crate::text::plural(n, "this", "any of these"),
        crate::text::plural(n, "it", "them"),
        crate::text::plural(n, "it is", "they are"),
        refused.join(", "),
    )
}

/// The modal's report on a finished dump: how many tables the file covers,
/// then — each only when there is something to say — the export renderer's
/// own caveat (`export_note`), the ticked tables the dump could not find,
/// what it left out ([`DumpPlan::left_out`]), the tables whose rows it does
/// not carry ([`DumpPlan::rows_left_out`]) and what a replay will not
/// replace ([`refused_note`]).
///
/// **Each a sentence of its own, one space between them.** It was a
/// `format!` in the view that put a space after the tally whatever followed
/// it, so a dump with nothing to add read "Wrote 2 tables. " and one with a
/// missing table "Wrote 2 tables.  1 ticked …".
pub fn done_note(
    tables: usize,
    export_note: Option<&str>,
    missing: &[String],
    left_out: &[String],
    rows_left_out: &[String],
    refused: &[String],
) -> String {
    let mut parts = vec![format!(
        "Wrote {tables} {}.",
        crate::text::plural(tables, "table", "tables")
    )];
    parts.extend(
        export_note
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string),
    );
    // A ticked table the dump's own fresh introspection could not find is the
    // difference between a backup and a file that looks like one, so it goes
    // in the same sentence as the tally rather than only into the header.
    if !missing.is_empty() {
        let n = missing.len();
        parts.push(format!(
            "{n} ticked {} not found and {} not in the file: {}.",
            crate::text::plural(n, "table", "tables"),
            crate::text::plural(n, "is", "are"),
            missing.join(", "),
        ));
    }
    // So is one it found and could not write, and whatever went with it.
    if !left_out.is_empty() {
        let n = left_out.len();
        parts.push(format!(
            "{n} {} the export asked for {} not in the file: {}.",
            crate::text::plural(n, "object", "objects"),
            crate::text::plural(n, "is", "are"),
            left_out.join(", "),
        ));
    }
    if !rows_left_out.is_empty() {
        let n = rows_left_out.len();
        parts.push(format!(
            "The rows of graph edge {} {} are not in the file; {} created empty.",
            crate::text::plural(n, "table", "tables"),
            rows_left_out.join(", "),
            crate::text::plural(n, "it is", "they are"),
        ));
    }
    let refused = refused_note(refused);
    parts.extend(Some(refused.trim().to_string()).filter(|r| !r.is_empty()));
    parts.join(" ")
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
        // No `IF NOT EXISTS`, and `CREATE SCHEMA` must be alone in its batch,
        // so it goes through `EXEC` behind a test. `dbo` is in every database.
        SqlDialect::MsSql => namespaces
            .iter()
            .filter_map(|ns| ns.as_deref())
            .filter(|ns| !ns.eq_ignore_ascii_case("dbo"))
            .map(|ns| {
                let lit =
                    crate::export::sql_literal(&crate::model::Value::Str(ns.to_string()), dialect);
                let stmt = format!("CREATE SCHEMA {}", ident_sql(ns, dialect));
                let stmt_lit = crate::export::sql_literal(&crate::model::Value::Str(stmt), dialect);
                format!("IF SCHEMA_ID({lit}) IS NULL EXEC({stmt_lit});")
            })
            .collect(),
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
        // SQL Server names the schema on both halves, as PostgreSQL does.
        SqlDialect::Postgres | SqlDialect::Sqlite | SqlDialect::MsSql => None,
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
/// columns. **The cost is stated rather than hidden**: such an identity
/// column's values are not carried, so the restored rows are renumbered, which
/// is why `plan` says so in the file.
///
/// **Except an identity the file can carry** ([`carries_identity`]) — SQL
/// Server's, written back under [`identity_insert_sql`] — since renumbering
/// the keys a foreign key names breaks the key.
pub fn dump_columns(t: &TableInfo, dialect: SqlDialect) -> Vec<&str> {
    t.columns
        .iter()
        .filter(|c| !c.is_server_assigned() || carries_identity(c, dialect))
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

/// How far the Export modal's table listing has got.
///
/// A `bool` was enough while the only two states it could be in were "out" and
/// "back"; it is not enough to tell "back, and the database really is empty"
/// from "never came back". The picker branched on the `bool` and then on
/// `names.is_empty()`, so a listing that *failed* landed in the reassuring arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Listing {
    /// The read is out; nothing can be said about the database yet.
    Reading,
    /// The read came back, and what it found is in the list.
    Done,
    /// The read did not finish — unreachable server, or an account that cannot
    /// see the catalog. The list is empty because nothing was read into it.
    Failed,
}

/// What the Export modal's table picker should render.
///
/// **Same shape and same reason as [`crate::script::ProbeSummary`]**, which
/// exists because the sibling modal's `== 0` branch made exactly this mistake:
/// it described a file the probe had never finished reading as holding *"no
/// statements Schemaic can run"*. Here the claim was *"This database has no
/// tables."*, printed four lines above the connection error that explained why
/// the list was empty — two contradictory statements about one database, with
/// the reassuring one in the panel the user is reading.
///
/// An enum matched exhaustively rather than a second `if`, so that a fourth
/// state cannot fall into the reassuring arm by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerBody {
    /// Say the list is being read.
    Reading,
    /// The read succeeded and the database holds nothing to export.
    NoTables,
    /// The read failed. Say *that*, and let the error line say why.
    Unreadable,
    /// There are names to offer.
    Tables,
}

/// Which of the four states the picker is in. See [`PickerBody`].
///
/// A failed listing that nevertheless has names is [`PickerBody::Tables`] — it
/// cannot arise today (the error arm leaves the list untouched at empty), and
/// showing the names would still be the right answer if it ever did. What must
/// never happen is the reverse: an empty list from a read that failed being
/// described as an empty database.
pub fn picker_body(listing: Listing, tables: usize) -> PickerBody {
    match (listing, tables) {
        (Listing::Reading, _) => PickerBody::Reading,
        (_, 1..) => PickerBody::Tables,
        (Listing::Failed, 0) => PickerBody::Unreadable,
        (Listing::Done, 0) => PickerBody::NoTables,
    }
}

/// The statements that put a table's key counter back where the data left it.
///
/// **PostgreSQL only, and it is the difference between a restore that works and
/// one that reports success and then fails.** The rows come back with their
/// original keys — [`dump_columns`] carries a `serial` or a
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
    // `public` named, as in the `CREATE` and the rows: a bare name resolves
    // through `search_path`, which leads with `"$user"`.
    let table = crate::schema::qualified_ident(&t.name, t.schema.as_deref(), dialect);
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
///
/// **`home` is what makes that answerable on MySQL at all.** There
/// `ref_schema` is the *database* ([`crate::ddl::ref_schema_is_database`]) and
/// `TableInfo::schema` is always `None` — a database *is* its namespace — so
/// both sides of the comparison were unknown, the `_` arm fired, and a key
/// matched on `ref_table` **alone**. A cross-database
/// `REFERENCES archive.customers` was classified as in-dump because a table
/// called `customers` was in the dump; the caller then stripped its qualifier
/// and the restored file pointed the key at its own `customers`. Passing the
/// database being dumped gives every candidate the namespace it really has.
/// `None` where there is no single home database — PostgreSQL, where the
/// namespace is on the object, and the comparison's ordering, where the two
/// sides are two different databases.
fn fk_targets<'a>(
    fk: &'a crate::schema::ForeignKeyInfo,
    owner: &'a TableInfo,
    cand: &'a TableInfo,
    home: Option<&'a str>,
) -> bool {
    let ns = |t: &'a TableInfo| t.schema.as_deref().or(home);
    fk.ref_table == cand.name
        && match (fk.ref_schema.as_deref().or_else(|| ns(owner)), ns(cand)) {
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
///
/// `home` is the database these tables were read from, on an engine where a
/// foreign key's `ref_schema` names a database — see [`fk_targets`], which is
/// what needs it. `None` everywhere else.
pub fn order_tables(
    tables: &[TableInfo],
    chosen: &[String],
    dialect: SqlDialect,
    home: Option<&str>,
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
                            .any(|fk| fk_targets(fk, &tables[i], &tables[j], home))
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

/// Re-order `(name, sql)` so a statement that **names** another comes after it.
///
/// **The third instance of one edge.** Base tables are ordered by their foreign
/// keys and views by the views their bodies name; a routine that calls another
/// routine, and a domain built on another domain, had nothing — `objects_where`
/// is a `filter().cloned()` over the catalogue vector, no sort and no walk. With
/// `check_function_bodies` on, PostgreSQL's default, `CREATE FUNCTION a_total()
/// … SELECT b_base()` ahead of `b_base` stops the restore, and by then the
/// file's `DROP TABLE`s have run against the target.
///
/// The same rule as the view walk, and deliberately so: each item's text is
/// lexed **once** with [`crate::intel::code_mask`], then asked about every other
/// name as a whole word in code, so a name inside a comment, a string literal or
/// a longer identifier is not an edge. Input order is the tie-break — it is
/// already the caller's kind-then-catalogue order — so two dumps of one schema
/// stay byte-identical.
///
/// **The text searched is the body, not the `CREATE` that carries it.** A
/// PostgreSQL function's `CREATE` wraps its body in `$$ … $$`, and a dollar-quote
/// is exactly what [`crate::intel::code_mask`] marks as *not* code — so asking
/// the emitted statement finds nothing, every time, and the walk would be a
/// no-op that looked like a fix.
///
/// A cycle is broken by [`topo_order`] rather than dropped: mutually recursive
/// routines are legal and the file must still hold both.
fn order_by_mention(items: Vec<Emitted>, dialect: SqlDialect) -> Vec<Emitted> {
    let edges: Vec<Vec<usize>> = items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let code = crate::intel::code_mask(&it.body, dialect);
            items
                .iter()
                .enumerate()
                // A statement always names itself, and that is not an edge.
                .filter(|&(j, other)| {
                    j != i
                        && !crate::intel::code_word_hits_in(&it.body, &code, &other.name).is_empty()
                })
                .map(|(j, _)| j)
                .collect()
        })
        .collect();
    let (order, _) = topo_order(&edges);
    let mut items: Vec<Option<Emitted>> = items.into_iter().map(Some).collect();
    order.into_iter().filter_map(|p| items[p].take()).collect()
}

/// The types and sequences the chosen tables **name** that this file will not
/// create, as display names, in the order they would have been emitted.
///
/// **The accounting the namespace filter never had.** `plan` emits only objects
/// belonging to the namespaces the chosen tables live in — a dump of `sales` has
/// no business recreating `archive`'s types — and that is a choice about what to
/// emit, not a licence to say nothing. A `public` enum used by a `sales` column
/// is simply absent from the file, and the `CREATE TABLE` declares the column
/// with it: on a fresh server the restore stops before any data lands.
///
/// The same trade the dropped foreign keys already make, whose doc calls the
/// header sentence "the honest half of the trade: silently emitting a statement
/// that cannot succeed is not the alternative".
///
/// A column names an object in one of two places, and both are read as text
/// because that is what the catalogue hands back: the declared type
/// (`order_status`, or `public.order_status` when the search path does not cover
/// it) and the default expression (`nextval('public.order_seq'::regclass)`).
/// Matched as a whole identifier so `order_status_v2` is not a hit — and *not*
/// through `intel::code_word_hits_in`, because a sequence's name lives inside a
/// string literal in that default, which is exactly what a code mask hides.
fn outside_dependencies(
    schema: &DbSchema,
    order: &[usize],
    namespaces: &[Option<String>],
    carried: bool,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for kind in [ObjectKind::Enum, ObjectKind::Domain, ObjectKind::Sequence] {
        // PostgreSQL's: a SQL Server object an outside table names is
        // `tsql_named`'s question, asked of the T-SQL pass.
        let all: Vec<_> = schema
            .objects_all(kind)
            .into_iter()
            .filter(|o| o.tsql().is_none())
            .collect();
        // **The names of this kind the file will itself create.** An unqualified
        // mention resolves through the search path, and if this dump emits an
        // object of that name the mention is satisfied by it. Without this, a
        // `sales` table whose column is typed `status` reported `archive.status`
        // as an outside dependency while the file emits `CREATE TYPE
        // sales.status` — an object it does have a local twin of.
        //
        // Per kind, not across all three: a sequence named `status` must not
        // suppress a real dependency on an enum of the same name, which is a
        // separate catalogue in PostgreSQL.
        let local: std::collections::HashSet<&str> = all
            .iter()
            .filter(|o| !o.is_internal() && namespaces.iter().any(|ns| ns.as_deref() == o.schema()))
            .map(|o| o.name())
            .collect();
        for o in &all {
            // An internal sequence is the column's own counter — it comes back
            // with the column, so it is not missing.
            if o.is_internal() || namespaces.iter().any(|ns| ns.as_deref() == o.schema()) {
                continue;
            }
            // **Qualified first, and it is definitive.** `archive.status` in a
            // type or a `nextval('archive.status_seq'::regclass)` names this
            // object and no other, whatever else the file creates.
            // `names_identifier` only checks the run's outer boundaries, so a
            // dotted needle works unchanged.
            let qualified = o.schema().map(|ns| format!("{ns}.{}", o.name()));
            let bare = !local.contains(o.name());
            let mentions = |text: &str| {
                qualified
                    .as_deref()
                    .is_some_and(|q| names_identifier(text, q))
                    || (bare && names_identifier(text, o.name()))
            };
            let named = order.iter().any(|&i| {
                schema.tables[i]
                    .columns
                    .iter()
                    .any(|c| mentions(&c.type_name) || c.default.as_deref().is_some_and(mentions))
            });
            if named {
                // Qualified unconditionally, unlike `display_name`: the whole
                // point of the sentence is *where* the missing object lives, and
                // `public` is exactly the namespace this case is usually about —
                // the one `display_name` drops as the default.
                let name = match o.schema() {
                    Some(ns) => format!("{ns}.{}", o.name()),
                    None => o.name().to_string(),
                };
                if !out.contains(&name) {
                    out.push(name);
                }
            }
        }
    }
    // **SQL Server's, by the same rule** — the ones the file does not carry
    // itself (`carried`: a dump with its other objects carries every one its
    // tables name, wherever it lives).
    for o in &schema.tsql_objects {
        if carried || namespaces.contains(&o.schema) || !tsql_named(schema, order, namespaces, o) {
            continue;
        }
        let name = match &o.schema {
            Some(ns) => format!("{ns}.{}", o.name),
            None => o.name.clone(),
        };
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// Does a chosen table name SQL Server object `o` — as a column's type or in
/// a default (`NEXT VALUE FOR`)?
///
/// A column names one bracketed — `[Sequences].[OrderID]`, `[dbo].[Phone]` —
/// so the text is read with its brackets taken off, which leaves the dotted
/// name [`outside_dependencies`]' walk matches; a bare name counts unless an
/// object of that name lives in one of the export's own namespaces.
fn tsql_named(
    schema: &DbSchema,
    order: &[usize],
    namespaces: &[Option<String>],
    o: &crate::schema::TsqlObject,
) -> bool {
    let local = schema
        .tsql_objects
        .iter()
        .any(|l| l.name == o.name && namespaces.contains(&l.schema));
    let qualified = o.schema.as_ref().map(|ns| format!("{ns}.{}", o.name));
    let mentions = |text: &str| {
        let text = text.replace(['[', ']'], "");
        qualified
            .as_deref()
            .is_some_and(|q| names_identifier(&text, q))
            || (!local && names_identifier(&text, &o.name))
    };
    order.iter().any(|&i| {
        schema.tables[i]
            .columns
            .iter()
            .any(|c| mentions(&c.type_name) || c.default.as_deref().is_some_and(mentions))
    })
}

/// Does a routine the file creates name SQL Server object `o` — a parameter's
/// or the return's type (`@h [dbo].[Hash]`, `xml([dbo].[coll])`), or, by its
/// qualified name, in the body (`NEXT VALUE FOR [seq].[n]`)?
///
/// **[`tsql_named`] asks only the tables**, and the routines of the export's
/// namespaces are emitted whatever the tables name, so an alias type outside
/// those namespaces that only a procedure's parameter used was neither
/// carried nor named, and the restore stopped at that procedure (Msg 2715).
/// A bare name in a body is not asked: there it is far more often a column
/// or a variable than the object, and one wrongly carried makes a schema the
/// restore did not need.
fn tsql_named_by_routines(
    schema: &DbSchema,
    namespaces: &[Option<String>],
    skip: &RoutineKeys,
    o: &crate::schema::TsqlObject,
) -> bool {
    let local = schema
        .tsql_objects
        .iter()
        .any(|l| l.name == o.name && namespaces.contains(&l.schema));
    let qualified = o.schema.as_ref().map(|ns| format!("{ns}.{}", o.name));
    let strip = |text: &str| text.replace(['[', ']'], "");
    schema
        .routines
        .iter()
        .filter(|r| {
            namespaces.contains(&r.schema) && !skip.contains(&(r.schema.clone(), r.name.clone()))
        })
        .any(|r| {
            let sig = strip(&format!("{}\n{}", r.arguments, r.returns));
            let body = strip(&r.body);
            qualified
                .as_deref()
                .is_some_and(|q| names_identifier(&sig, q) || names_identifier(&body, q))
                || (!local && names_identifier(&sig, &o.name))
        })
}

/// Does `text` name `word` as a whole identifier?
///
/// Byte-wise on [`crate::sql::is_word_byte`], the one definition of where an
/// identifier runs — so `order_status` does not match inside `order_status_v2`,
/// and a multi-byte name is not cut at a character.
fn names_identifier(text: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let (t, w) = (text.as_bytes(), word.as_bytes());
    t.windows(w.len()).enumerate().any(|(i, win)| {
        win == w
            && !(i > 0 && crate::sql::is_word_byte(t[i - 1]))
            && !t
                .get(i + w.len())
                .copied()
                .is_some_and(crate::sql::is_word_byte)
    })
}

/// One standalone object on its way into the file: what it is called, the text
/// that decides what it waits for, and the statement that creates it.
struct Emitted {
    name: String,
    /// The routine or event body — see [`order_by_mention`] for why this is not
    /// the `CREATE`. For an object with no body it *is* the `CREATE`, which is
    /// where a domain names the type it is built on.
    body: String,
    sql: String,
    /// The statement that removes it first, where the file drops what it
    /// recreates up front ([`drops_up_front`]) and the object is one it drops.
    drop: Option<String>,
    /// A function — the one kind a table's expression or a view can call, so
    /// the one [`routine_slots`] may move ahead of the tables.
    function: bool,
}

/// Why a replay must leave standing something the file creates.
///
/// **The file never drops what it cannot put back as it is.** A replay —
/// the file run onto a database that already holds what it recreates — drops
/// before it creates, and what the file restates only in part came back
/// empty, as a comment, or unsigned, with the run reporting success.
enum Hold {
    /// The server shows no text for it, so the file's `CREATE` is a comment:
    /// left as it is, and nothing collides with it.
    Keep,
    /// The file recreates it, but not as it is: a replay stops, before it
    /// drops anything, where `object` (quoted and qualified) is there
    /// ([`refuse_if_present_sql`]).
    Refuse { object: String, why: String },
}

/// The [`Hold`] on table or view `t`, or `None` for one a replay may drop
/// and put back. `any_edge` is whether the database has a graph edge table
/// anywhere, in the export or not.
fn table_hold(t: &TableInfo, any_edge: bool, dialect: SqlDialect) -> Option<Hold> {
    let q = |name: &str, ns: Option<&str>| crate::schema::qualified_ident(name, ns, dialect);
    let refuse = |why: String| {
        Some(Hold::Refuse {
            object: q(&t.name, t.schema.as_deref()),
            why,
        })
    };
    if t.is_view {
        return t
            .view_options
            .as_ref()
            .is_some_and(|o| o.tsql.hidden)
            .then_some(Hold::Keep);
    }
    // An edge's rows name the nodes they join by node id, which a restore
    // assigns afresh, so they are not in the file at all; a node's rows are,
    // under new ids that no edge — in the file or not — points at.
    if t.tsql_kind.edge {
        return refuse("a graph edge table: its rows are not in this file".to_string());
    }
    if t.tsql_kind.node && any_edge {
        return refuse(
            "a graph node table: its rows would come back under new node ids, which no edge \
             points at"
                .to_string(),
        );
    }
    // Dropping the table drops its triggers.
    for tr in &t.triggers {
        let why = if tr.tsql.hidden {
            "is encrypted, so no script can recreate it"
        } else if tr.tsql.module.signed {
            "is signed, and no script can carry the signature"
        } else {
            continue;
        };
        // Asked of the table, as every refusal here is: it is the table the
        // file would drop, and one left standing collides with its `CREATE`.
        return refuse(format!("its trigger {} {why}", tr.name));
    }
    None
}

/// Does `text`, written in namespace `own`, name the object `name` in
/// namespace `ns` — qualified (`s.n`, `[s].[n]`, `"s"."n"`), or bare where a
/// bare name reaches it: in its own namespace, or the engine's default one?
///
/// Matched in code as a whole word ([`crate::intel::code_word_hits_in`]),
/// with the qualifier in front of the hit read off rather than the name
/// searched for twice, so `sales.emp` is not taken for `dbo.emp`.
fn names_object(
    text: &str,
    code: &[bool],
    dialect: SqlDialect,
    own: Option<&str>,
    ns: Option<&str>,
    name: &str,
) -> bool {
    let b = text.as_bytes();
    let same = |a: Option<&str>, z: Option<&str>| match (a, z) {
        (Some(a), Some(z)) => a.eq_ignore_ascii_case(z),
        (None, None) => true,
        _ => false,
    };
    let opener = |c: u8| matches!(c, b'[' | b'"' | b'`');
    let closer = |c: u8| matches!(c, b']' | b'"' | b'`');
    crate::intel::code_word_hits_in(text, code, name)
        .into_iter()
        .any(|(i, _)| {
            let mut j = i;
            if j > 0 && opener(b[j - 1]) {
                j -= 1;
            }
            if j == 0 || b[j - 1] != b'.' {
                return same(own, ns) || same(ns, crate::schema::default_namespace(dialect));
            }
            // The qualifier: a quoted run up to its opener, or a word.
            let stop = j - 1;
            let (from, to) = if stop > 0 && closer(b[stop - 1]) {
                let close = stop - 1;
                let open = text[..close].rfind(['[', '"', '`']).unwrap_or(close);
                (open + 1, close)
            } else {
                let mut k = stop;
                while k > 0 && crate::sql::is_word_byte(b[k - 1]) {
                    k -= 1;
                }
                (k, stop)
            };
            let qual = &text[from..to];
            // `db..n` is the default namespace of database `db`.
            if qual.is_empty() {
                return same(ns, crate::schema::default_namespace(dialect));
            }
            ns.is_some_and(|ns| ns.eq_ignore_ascii_case(qual))
        })
}

/// Does a routine bind to what it names when it is created, so that one
/// naming an object the file leaves out stops the restore at its `CREATE`?
/// A schema-bound or natively compiled module, and an inline table-valued
/// function, do; any other routine resolves its names when it runs.
fn binds_at_create(r: &crate::schema::RoutineInfo) -> bool {
    use crate::schema::TsqlRoutineOption as O;
    r.tsql
        .options
        .iter()
        .any(|o| matches!(o, O::SchemaBinding | O::NativeCompilation))
        || (r.kind == crate::schema::RoutineKind::Function
            && r.returns.trim().eq_ignore_ascii_case("TABLE"))
}

/// Routines by `(namespace, name)`.
type RoutineKeys = std::collections::HashSet<(Option<String>, String)>;

/// What has to be left out of the file **with** the tables in `gone` (indices
/// into `schema.tables`), which it does not create: every view in `order`
/// reading one, every table in `order` whose expressions call a routine
/// left out, and every routine in the export's namespaces that binds to one
/// at `CREATE` ([`binds_at_create`]) — each in turn counting as gone, until
/// nothing more is. Returns the tables and views (indices), the routines
/// (`(namespace, name)`) and a label for each, saying why.
///
/// **One left in stops the whole restore.** A view over a temporal table
/// the file leaves out failed at its `CREATE VIEW` (Msg 208), and *One
/// transaction* rolled back everything before it — where without such a
/// view the file restored, one table short and saying so.
fn dependents_left_out(
    schema: &DbSchema,
    order: &[usize],
    gone: &[usize],
    dialect: SqlDialect,
) -> (std::collections::HashSet<usize>, RoutineKeys, Vec<String>) {
    let mut out_tables = std::collections::HashSet::new();
    let mut out_routines = std::collections::HashSet::new();
    let mut labels = Vec::new();
    if gone.is_empty() {
        return (out_tables, out_routines, labels);
    }
    // `(namespace, name, a routine)` of everything gone so far.
    let mut gone_objs: Vec<(Option<String>, String, bool)> = gone
        .iter()
        .map(|&i| {
            let t = &schema.tables[i];
            (t.schema.clone(), t.name.clone(), false)
        })
        .collect();
    let namespaces: Vec<Option<&str>> = order
        .iter()
        .map(|&i| schema.tables[i].schema.as_deref())
        .collect();
    let callers = caller_texts(&schema.tables, order);
    let caller_code: Vec<Vec<bool>> = callers
        .iter()
        .map(|t| crate::intel::code_mask(t, dialect))
        .collect();
    let routines: Vec<&crate::schema::RoutineInfo> = schema
        .routines
        .iter()
        .map(|r| r.as_ref())
        .filter(|r| namespaces.contains(&r.schema.as_deref()) && binds_at_create(r))
        .collect();
    let routine_code: Vec<Vec<bool>> = routines
        .iter()
        .map(|r| crate::intel::code_mask(&r.body, dialect))
        .collect();
    // The first gone object `text` names — only a routine, where `calls`:
    // a table's expressions can call one, and a column there named like a
    // table is not a mention of it.
    let first_named = |text: &str,
                       code: &[bool],
                       own: Option<&str>,
                       calls: bool,
                       gone: &[(Option<String>, String, bool)]| {
        gone.iter()
            .filter(|(.., routine)| *routine || !calls)
            .find(|(ns, name, _)| names_object(text, code, dialect, own, ns.as_deref(), name))
            .map(|(ns, name, _)| display_name(ns.as_deref(), name))
    };
    loop {
        let mut more = false;
        for (k, &i) in order.iter().enumerate() {
            let t = &schema.tables[i];
            if out_tables.contains(&i) {
                continue;
            }
            // A view reads a table, a view or a routine; a table's
            // expressions call a routine, gone only once one is left out
            // below.
            let Some(g) = first_named(
                &callers[k],
                &caller_code[k],
                t.schema.as_deref(),
                !t.is_view,
                &gone_objs,
            ) else {
                continue;
            };
            let what = if t.is_view {
                format!("a view reading {g}")
            } else {
                format!("a table whose columns or checks call {g}")
            };
            labels.push(format!(
                "{} ({what}, which is not in this file)",
                display_name(t.schema.as_deref(), &t.name)
            ));
            out_tables.insert(i);
            gone_objs.push((t.schema.clone(), t.name.clone(), false));
            more = true;
        }
        for (r, code) in routines.iter().zip(&routine_code) {
            let key = (r.schema.clone(), r.name.clone());
            if out_routines.contains(&key) {
                continue;
            }
            let Some(g) = first_named(&r.body, code, r.schema.as_deref(), false, &gone_objs) else {
                continue;
            };
            labels.push(format!(
                "{} (a {} bound to {g}, which is not in this file)",
                display_name(r.schema.as_deref(), &r.name),
                match r.kind {
                    crate::schema::RoutineKind::Procedure => "procedure",
                    _ => "function",
                }
            ));
            gone_objs.push((key.0.clone(), key.1.clone(), true));
            out_routines.insert(key);
            more = true;
        }
        if !more {
            break;
        }
    }
    (out_tables, out_routines, labels)
}

/// The [`Hold`] on routine `r`, or `None` for one a replay may drop and put
/// back.
fn routine_hold(r: &crate::schema::RoutineInfo, dialect: SqlDialect) -> Option<Hold> {
    // The condition under which its `CREATE` is a comment.
    if r.tsql.verbatim.is_none() && (r.tsql.hidden || r.body.trim().is_empty()) {
        return Some(Hold::Keep);
    }
    let object = crate::schema::qualified_ident(&r.name, r.schema.as_deref(), dialect);
    // `DROP PROCEDURE grp` drops every member of the group.
    let unread: Vec<String> = r
        .tsql
        .numbered
        .iter()
        .filter(|(_, text)| text.trim().is_empty())
        .map(|(n, _)| format!("{};{n}", r.name))
        .collect();
    if !unread.is_empty() {
        return Some(Hold::Refuse {
            object,
            why: format!(
                "a numbered procedure group, and the server shows no text for {}",
                unread.join(", ")
            ),
        });
    }
    r.tsql.module.signed.then(|| Hold::Refuse {
        object,
        why: "signed, and no script can carry the signature".to_string(),
    })
}

/// The tables and views in creation order **together with the functions they
/// call**, and where each routine goes among them: `Some(k)` is "just before
/// the returned order's `k`th section" (`k == order.len()` after the last),
/// `None` is the trailing routines section.
///
/// **A function a table or view calls is created before it.** SQL Server
/// resolves one at `CREATE TABLE`/`CREATE VIEW` time, so a computed column, a
/// check, a default or a view calling a function the file created afterwards
/// stopped the restore (Msg 4121), and PostgreSQL resolves a default's and a
/// view's function the same way. So a function a table's expressions or a
/// view's definition name — and every function one of those calls, since it
/// has to exist first — moves up, to just after the last table *it* names:
/// the other half of the edge the trailing section exists for, a function
/// reading a table that is not there yet (`check_function_bodies`).
///
/// **One sort over all three, not a function fitted into a table order fixed
/// without it.** The tables and views were ordered first, by their keys,
/// their mentions of one another and name, and a function whose two edges
/// could not both hold in that order took its caller's side — so a view
/// reaching another view only through an inline function came out ahead of
/// it and stopped the restore at the function (Msg 208), and a table whose
/// check calls a function counting another table had its rows refused before
/// that table existed. Here the caller waits instead. `order` is the order
/// the tables and views had without any function, and it is the tie-break:
/// whichever ready table or view comes first in it goes next, and a function
/// goes as soon as everything it names is there — so where no function is
/// called the file is exactly what it was. Only a real cycle — a function
/// reading a table whose own column calls it — is broken, and at the
/// function, since that is the statement deferred name resolution lets
/// stand first.
///
/// Every other routine stays where it was, after the data. `routines` is
/// already in dependency order among itself ([`order_by_mention`]), and names
/// are matched as whole words in code, as that walk matches them.
fn creation_order(
    routines: &[Emitted],
    tables: &[TableInfo],
    order: &[usize],
    home: Option<&str>,
    dialect: SqlDialect,
) -> (Vec<usize>, Vec<Option<usize>>) {
    let slots = routine_slots(routines, tables, order, dialect);
    let moved: Vec<usize> = (0..routines.len())
        .filter(|&r| slots[r].is_some())
        .collect();
    if moved.is_empty() {
        return (order.to_vec(), slots);
    }
    let code = |t: &str| crate::intel::code_mask(t, dialect);
    let names = |text: &str, mask: &[bool], word: &str| {
        !crate::intel::code_word_hits_in(text, mask, word).is_empty()
    };
    // Nodes: `order`'s positions, then the moved functions.
    let m = order.len();
    let callers = caller_texts(tables, order);
    let caller_code: Vec<Vec<bool>> = callers.iter().map(|t| code(t)).collect();
    let mut waits: Vec<Vec<usize>> = vec![Vec::new(); m + moved.len()];
    for (k, &i) in order.iter().enumerate() {
        let t = &tables[i];
        // What it waited for before — kept only as `order` already has it,
        // so a cycle `order_tables` broke stays broken the same way.
        let def = t.view_definition.as_deref().filter(|_| t.is_view);
        let def_code = def.map(code);
        for (p, &j) in order.iter().enumerate().take(k) {
            let keyed = t
                .foreign_keys
                .iter()
                .any(|fk| fk_targets(fk, t, &tables[j], home));
            let read = def
                .zip(def_code.as_deref())
                .is_some_and(|(d, c)| names(d, c, &tables[j].name));
            if j != i && (keyed || read) {
                waits[k].push(p);
            }
        }
        for (f, &r) in moved.iter().enumerate() {
            if names(&callers[k], &caller_code[k], &routines[r].name) {
                waits[k].push(m + f);
            }
        }
    }
    for (f, &r) in moved.iter().enumerate() {
        let body_code = code(&routines[r].body);
        for (k, &i) in order.iter().enumerate() {
            if names(&routines[r].body, &body_code, &tables[i].name) {
                waits[m + f].push(k);
            }
        }
        // A function it calls that `routines` already puts first.
        for (g, &q) in moved.iter().enumerate().take(f) {
            if names(&routines[r].body, &body_code, &routines[q].name) {
                waits[m + f].push(m + g);
            }
        }
    }
    let n = waits.len();
    let mut done = vec![false; n];
    let ready = |p: usize, done: &[bool]| !done[p] && waits[p].iter().all(|&d| done[d]);
    let mut new_order: Vec<usize> = Vec::with_capacity(m);
    let mut new_slots: Vec<Option<usize>> = vec![None; routines.len()];
    for _ in 0..n {
        // A ready function first — as early as it can go — then the first
        // ready table or view in the old order.
        let next = (m..n)
            .find(|&p| ready(p, &done))
            .or_else(|| (0..m).find(|&p| ready(p, &done)))
            .or_else(|| {
                // A cycle: broken at a function on it, or else where
                // `order_tables` would have broken it.
                (m..n)
                    .find(|&p| !done[p] && on_cycle(p, &waits, &done))
                    .or_else(|| (0..n).find(|&p| !done[p]))
            });
        let Some(p) = next else { break };
        done[p] = true;
        if p < m {
            new_order.push(order[p]);
        } else {
            new_slots[moved[p - m]] = Some(new_order.len());
        }
    }
    (new_order, new_slots)
}

/// Does `p` wait, through members not yet `done`, on itself?
fn on_cycle(p: usize, waits: &[Vec<usize>], done: &[bool]) -> bool {
    let mut seen = vec![false; waits.len()];
    let mut stack: Vec<usize> = waits[p].clone();
    while let Some(q) = stack.pop() {
        if q == p {
            return true;
        }
        if done[q] || std::mem::replace(&mut seen[q], true) {
            continue;
        }
        stack.extend(&waits[q]);
    }
    false
}

/// What each table's expressions and each view's definition say, per
/// position in `order` — the text a function has to exist for. Not the
/// `CREATE`: a column named like a function is not a call.
fn caller_texts(tables: &[TableInfo], order: &[usize]) -> Vec<String> {
    order
        .iter()
        .map(|&i| {
            let t = &tables[i];
            if t.is_view {
                return t
                    .create_sql
                    .clone()
                    .or_else(|| t.view_definition.clone())
                    .unwrap_or_default();
            }
            let mut text: Vec<&str> = Vec::new();
            for c in &t.columns {
                text.extend(c.generated.as_deref());
                text.extend(c.default.as_deref());
            }
            for ck in &t.check_constraints {
                text.push(&ck.expression);
            }
            text.join("\n")
        })
        .collect()
}

/// Which routines [`creation_order`] moves ahead of the tables, as the slot
/// each would take in `order` as it stands — `None` for one that stays in
/// the trailing section. A function moves when a table's expressions or a
/// view's definition name it, or a moved function calls it.
fn routine_slots(
    routines: &[Emitted],
    tables: &[TableInfo],
    order: &[usize],
    dialect: SqlDialect,
) -> Vec<Option<usize>> {
    let callers = caller_texts(tables, order);
    // Each text lexed once, then asked about every name — `order_by_mention`'s
    // rule, for its reason.
    let caller_code: Vec<Vec<bool>> = callers
        .iter()
        .map(|t| crate::intel::code_mask(t, dialect))
        .collect();
    let body_code: Vec<Vec<bool>> = routines
        .iter()
        .map(|r| crate::intel::code_mask(&r.body, dialect))
        .collect();
    let caller_names = |k: usize, word: &str| {
        !crate::intel::code_word_hits_in(&callers[k], &caller_code[k], word).is_empty()
    };
    let body_names = |i: usize, word: &str| {
        !crate::intel::code_word_hits_in(&routines[i].body, &body_code[i], word).is_empty()
    };
    let n = routines.len();
    // The latest slot each function may take: before the first table or
    // view that calls it, then pulled earlier by every moved function that
    // calls it — walked callers-first, which is `routines` reversed.
    let mut latest: Vec<Option<usize>> = routines
        .iter()
        .map(|r| {
            r.function
                .then(|| (0..callers.len()).find(|&k| caller_names(k, &r.name)))
                .flatten()
        })
        .collect();
    for i in (0..n).rev() {
        let Some(cap) = latest[i] else { continue };
        for j in 0..i {
            if routines[j].function && body_names(i, &routines[j].name) {
                latest[j] = Some(latest[j].map_or(cap, |c| c.min(cap)));
            }
        }
    }
    // The earliest slot: after the last table or view it names, and after
    // every moved function it calls.
    let mut slots: Vec<Option<usize>> = vec![None; n];
    for i in 0..n {
        let Some(cap) = latest[i] else { continue };
        let mut earliest = order
            .iter()
            .enumerate()
            .filter(|&(_, &t)| body_names(i, &tables[t].name))
            .map(|(k, _)| k + 1)
            .max()
            .unwrap_or(0);
        for j in 0..i {
            if let Some(s) = slots[j]
                && body_names(i, &routines[j].name)
            {
                earliest = earliest.max(s);
            }
        }
        slots[i] = Some(earliest.min(cap));
    }
    slots
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
    // The database being dumped is the namespace every table in it is in, on
    // the engine that reports a key's target as a database — see `fk_targets`.
    let home = crate::ddl::ref_schema_is_database(dialect).then_some(database);
    let (order, cycles) = order_tables(&schema.tables, chosen, dialect, home);
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
    // **A table no `CREATE` from the model restates is left out of the file
    // and named in it** — a system-versioned or memory-optimised SQL Server
    // table, whose structure step would be a comment (`create_ddl`). Kept, its
    // `DROP` destroyed what the file could not put back and its rows landed in
    // nothing. Out of `order`, a key onto it is one more key to a table outside
    // the export, and is accounted for as one.
    let mut unrestated: Vec<String> = Vec::new();
    let mut gone: Vec<usize> = Vec::new();
    let order: Vec<usize> = order
        .into_iter()
        .filter(|&i| {
            let t = &schema.tables[i];
            // A view too: a ledger table's ledger view.
            match t.tsql_kind.unrestatable() {
                Some(what) => {
                    unrestated.push(format!(
                        "{} ({what})",
                        display_name(t.schema.as_deref(), &t.name)
                    ));
                    gone.push(i);
                    false
                }
                _ => true,
            }
        })
        .collect();
    // **And whatever cannot be created without them** — see
    // `dependents_left_out`. The temporal table alone used to go, and a view
    // over it stopped the whole restore.
    let (dependent_tables, dependent_routines, dependents) =
        dependents_left_out(schema, &order, &gone, dialect);
    let order: Vec<usize> = order
        .into_iter()
        .filter(|i| !dependent_tables.contains(i))
        .collect();
    let left_out: Vec<String> = unrestated.iter().chain(&dependents).cloned().collect();
    if order.is_empty() {
        return DumpPlan {
            missing,
            left_out,
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
    // The same qualification `TableInfo::create_ddl` uses — the one builder,
    // which names `public` too — so a `DROP` names the table its `CREATE` is
    // about to make.
    let qname =
        |t: &TableInfo| crate::schema::qualified_ident(&t.name, t.schema.as_deref(), dialect);

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
    // **The other half of the same accounting, and it had none.** A table whose
    // DDL is the engine's own captured text carries its keys *inside* the
    // `CREATE TABLE`, so there is nothing to restate and nothing to drop — but a
    // key pointing outside the export is still a key pointing at a table this
    // file does not create. `needs_fk_section` used to gate the whole loop body,
    // so on SQLite the count stayed 0 and the header said nothing: the restore
    // succeeds (`PRAGMA foreign_keys = ON` does not validate existing rows), and
    // the table is unusable from the next write on. The question that predicate
    // answers is *"restate it separately?"*, not *"is it carried?"*, and only the
    // first belongs in front of the emit.
    let mut dangling_fks = 0usize;
    // Decided here, beside the other cross-selection census, for the same reason
    // it gives: the header is written before the objects are.
    let outside_deps = outside_dependencies(schema, &order, &namespaces, opts.other_objects);
    // **A SQL Server object a chosen table names is carried wherever it
    // lives.** WideWorldImporters keeps every key's sequence in a schema of
    // its own with no table in it, so the namespace rule above left them all
    // out and the restore stopped at the first `NEXT VALUE FOR` (Msg 208).
    // One outside the export's namespaces is not the file's to own, so it is
    // created only where it is missing and never dropped.
    // And one only a routine the file creates names, since the routines come
    // with the namespace (`tsql_named_by_routines`).
    let carried_outside: Vec<&crate::schema::TsqlObject> = if opts.other_objects {
        schema
            .tsql_objects
            .iter()
            .filter(|o| {
                !namespaces.contains(&o.schema)
                    && (tsql_named(schema, &order, &namespaces, o)
                        || tsql_named_by_routines(schema, &namespaces, &dependent_routines, o))
            })
            .collect()
    } else {
        Vec::new()
    };
    // The containers the file makes: its tables' namespaces, and those of the
    // objects it carries from outside them.
    let containers: Vec<Option<String>> = {
        let mut ns = namespaces.clone();
        ns.extend(carried_outside.iter().map(|o| o.schema.clone()));
        ns.sort();
        ns.dedup();
        ns
    };
    if opts.structure {
        for &i in &order {
            let t = &schema.tables[i];
            if t.is_view || t.foreign_keys.is_empty() {
                continue;
            }
            let (here, elsewhere): (Vec<_>, Vec<_>) =
                t.foreign_keys.iter().cloned().partition(|fk| {
                    order
                        .iter()
                        .any(|&j| fk_targets(fk, t, &schema.tables[j], home))
                });
            if !needs_fk_section(t) {
                dangling_fks += elsewhere.len();
                continue;
            }
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
                    .map(|mut fk| {
                        // **A key whose target is in this dump names it the way
                        // the file names its own tables — bare.**
                        //
                        // On MySQL and MariaDB `ref_schema` is the *database*
                        // (`ddl::ref_schema_is_database`), and `information_
                        // schema` reports it even for an ordinary same-database
                        // key. `fk_clause` then hard-qualified the `REFERENCES`
                        // while the `ALTER TABLE` above it stayed bare, so
                        // editing the `USE` line — the retarget gesture this
                        // module documents — moved every table, row and trigger
                        // and left every foreign key pointing at the source. On
                        // the same server that succeeds *silently* and
                        // constrains the copy against production.
                        //
                        // This is `DumpStep::Rows::insert_database`'s rule, one
                        // section further down the file; it exists because the
                        // identical bug was found and fixed on the `INSERT`
                        // half. PostgreSQL's `ref_schema` is a namespace — part
                        // of the object rather than its address — so the
                        // capability decides, not the engine.
                        //
                        // **Only when it really is this database's name.**
                        // `fk_targets` is what decides `here`, and until it was
                        // given `home` it matched a cross-database key on
                        // `ref_table` alone — so `REFERENCES archive.customers`
                        // landed in `here` and this strip retargeted it at the
                        // dump's own `customers`. The guard is restated here as
                        // well so the rule holds at the line that does the
                        // stripping, not only at the one that partitions.
                        if home.is_some() && fk.ref_schema.as_deref() == home {
                            fk.ref_schema = None;
                        }
                        Change::AddForeignKey(Box::new(fk))
                    })
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
    // **Every server-supplied name on a comment line goes through
    // `comment_text`.** A `--` comment ends at the first newline and an
    // identifier may hold one, so a table named `orders\nDROP TABLE customers;`
    // otherwise turned this header into a top-level statement in a file the user
    // takes for a backup — and it runs at *restore*, against whichever database
    // the restore targets. `ident_sql` is not the fix: it doubles a quote
    // character and says nothing about `\n`.
    let mut header = format!(
        "-- Schemaic dump of {}\n-- {} {}, {what}.",
        crate::export::comment_text(database),
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
    // **The columns the file cannot carry, named in it.** `dump_columns`
    // leaves out what the server assigns for itself, because an `INSERT` that
    // names one is an error rather than a value — but for an identity column that
    // also means the values are gone and the restored rows are renumbered. The
    // person replaying the file is the one who needs to know, and the same
    // silence about a `NULL`ed blob is what the tally exists to break.
    // **Renumbered only when a lost column is a counter** (`auto_increment`):
    // SQL Server carries its identities, so what it loses is a `rowversion` or
    // a computed column, which the server recomputes rather than renumbers.
    if opts.data {
        let mut renumbered = false;
        let lost: Vec<String> = order
            .iter()
            .flat_map(|&i| {
                let t = &schema.tables[i];
                t.columns
                    .iter()
                    .filter(|c| c.is_server_assigned() && !carries_identity(c, dialect))
                    .map(|c| (t, c))
                    .collect::<Vec<_>>()
            })
            .map(|(t, c)| {
                renumbered |= c.auto_increment;
                crate::export::comment_text(&format!("{}.{}", t.name, c.name))
            })
            .collect();
        if !lost.is_empty() {
            let tail = if renumbered {
                " and the\n-- restored rows are renumbered: "
            } else {
                ":\n-- "
            };
            header.push_str(&format!(
                "\n--\n-- The server assigns {} itself, so {} not in this file{tail}{}.",
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
            crate::export::comment_text(&missing.join(", ")),
        ));
    }
    // The tables `order` left out because nothing here can restate them.
    if !unrestated.is_empty() {
        header.push_str(&format!(
            "\n--\n-- {} {} ticked for export {} not in this file: Schemaic cannot restate {}\n\
             -- from what it reads. Script {} from the source server: {}.",
            unrestated.len(),
            crate::text::plural(unrestated.len(), "table", "tables"),
            crate::text::plural(unrestated.len(), "is", "are"),
            crate::text::plural(unrestated.len(), "it", "them"),
            crate::text::plural(unrestated.len(), "it", "them"),
            crate::export::comment_text(&unrestated.join(", ")),
        ));
    }
    // And what reads them, which could not be created without them.
    if !dependents.is_empty() {
        let n = dependents.len();
        header.push_str(&format!(
            "\n--\n-- {n} {} {} left out with {}: created without what {} {},\n\
             -- {} would stop the restore. Script {} from the source server too: {}.",
            crate::text::plural(n, "object", "objects"),
            crate::text::plural(n, "is", "are"),
            crate::text::plural(unrestated.len(), "it", "them"),
            crate::text::plural(n, "it", "they"),
            crate::text::plural(n, "reads", "read"),
            crate::text::plural(n, "it", "they"),
            crate::text::plural(n, "it", "them"),
            crate::export::comment_text(&dependents.join(", ")),
        ));
    }
    // A graph edge's rows name the nodes they join by node id, and a restore
    // assigns every node a fresh one: carried, they would join the wrong nodes
    // or none. The edge itself is restated, empty.
    let edges: Vec<String> = order
        .iter()
        .map(|&i| &schema.tables[i])
        .filter(|t| t.tsql_kind.edge)
        .map(|t| display_name(t.schema.as_deref(), &t.name))
        .collect();
    if opts.data && !edges.is_empty() {
        header.push_str(&format!(
            "\n--\n-- The rows of {} graph edge {} are not in this file: an edge names the nodes it\n\
             -- joins by node id, and a restore gives every node a new one. {} created empty: {}.",
            edges.len(),
            crate::text::plural(edges.len(), "table", "tables"),
            crate::text::plural(edges.len(), "It is", "They are"),
            crate::export::comment_text(&edges.join(", ")),
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
    // **The other cross-selection dependency, which had no accounting at all.**
    // The namespace filter below keeps `archive`'s types out of a dump of
    // `sales` — a defensible choice about what to *emit*, and it said nothing
    // about what to *report*. A `public` enum used by a `sales` column is then
    // simply absent, and the `CREATE TABLE` that follows declares the column
    // with it: on a fresh server the restore stops at `ERROR: type
    // "order_status" does not exist`, before any data lands. Strictly louder
    // than the dropped key above — that is a constraint the restore survives
    // without, this is a `CREATE TABLE` that cannot run — and it was the half
    // with no sentence.
    // **Gated on `opts.structure`, because the sentence is about the structure
    // section.** It says "the CREATE TABLE statements name it", and a data-only
    // dump has none — the file is `INSERT`s, and a data-only restore into an
    // existing database needs the type no more and no less than the target
    // already has. So the user was sent to satisfy a dependency the file does
    // not have. Its sibling one block up is gated by construction, because
    // `dropped_fks` is only incremented inside the structure step; this one is
    // computed above it and had no such arithmetic in front of it.
    if opts.structure && !outside_deps.is_empty() {
        header.push_str(&format!(
            "\n--\n-- {} {} used by the tables above {} outside this export and {} not in\n\
             -- this file; the CREATE TABLE statements name {}: {}.\n\
             -- Restore onto a server that already has {}, or export those namespaces too.",
            outside_deps.len(),
            crate::text::plural(
                outside_deps.len(),
                "type or sequence",
                "types and sequences"
            ),
            crate::text::plural(outside_deps.len(), "lives", "live"),
            crate::text::plural(outside_deps.len(), "is", "are"),
            crate::text::plural(outside_deps.len(), "it", "them"),
            crate::export::comment_text(&outside_deps.join(", ")),
            crate::text::plural(outside_deps.len(), "it", "them"),
        ));
    }
    // The verbatim-DDL half, and deliberately a different sentence: nothing was
    // dropped, so saying the key is gone would be the opposite lie. It is in the
    // file, it restores without complaint, and it points at nothing.
    if dangling_fks > 0 {
        header.push_str(&format!(
            "\n--\n-- {dangling_fks} foreign {} at tables outside this export, and this engine\n\
             -- writes {} inside the CREATE TABLE above. The restore will not complain; the\n\
             -- restored {} nothing to point at. Add the missing tables.",
            crate::text::plural(dangling_fks, "key points", "keys point"),
            crate::text::plural(dangling_fks, "it", "them"),
            crate::text::plural(dangling_fks, "key has", "keys have"),
        ));
    }
    // **An alias type's bound default and rule are not in its `CREATE
    // TYPE`** — `sp_bindefault`/`sp_bindrule`, deprecated, and restated
    // nowhere — so the restored columns of that type silently lost their
    // default and their check. Named for every alias type the file creates.
    if opts.other_objects {
        let unbound: Vec<String> = schema
            .tsql_type_bindings
            .iter()
            .filter(|b| {
                schema.tsql_objects.iter().any(|o| {
                    o.schema == b.schema
                        && o.name == b.type_name
                        && matches!(o.kind, crate::schema::TsqlObjectKind::AliasType { .. })
                        && (namespaces.contains(&o.schema)
                            || carried_outside.iter().any(|c| std::ptr::eq(*c, o)))
                })
            })
            .map(|b| {
                let bound: Vec<String> = [("default", &b.default), ("rule", &b.rule)]
                    .into_iter()
                    .filter_map(|(what, name)| name.as_ref().map(|n| format!("{what} {n}")))
                    .collect();
                format!(
                    "{} ({})",
                    display_name(b.schema.as_deref(), &b.type_name),
                    bound.join(", ")
                )
            })
            .collect();
        if !unbound.is_empty() {
            let n = unbound.len();
            header.push_str(&format!(
                "\n--\n-- {n} alias {} here {} a bound default or rule, which its CREATE TYPE cannot\n\
                 -- restate: the restored columns of {} lose {}. Bind {} again with\n\
                 -- sp_bindefault/sp_bindrule after the restore: {}.",
                crate::text::plural(n, "type", "types"),
                crate::text::plural(n, "carries", "carry"),
                crate::text::plural(n, "that type", "those types"),
                crate::text::plural(n, "it", "them"),
                crate::text::plural(n, "it", "them"),
                crate::export::comment_text(&unbound.join(", ")),
            ));
        }
    }
    // **Written last, at the head of the file**: what a replay leaves
    // standing is known only once the routines are collected, below.

    // ── The literal guard, outside everything ────────────────────────────────
    //
    // Before the container statements, not beside the FK guard below: this one
    // decides what every `'…'` in the file *means*, and the `CREATE DATABASE`
    // and `CREATE TABLE`s ahead of the transaction carry literals too (a column
    // comment, a quoted default). It is also not one of the two optional
    // scaffolds — those choose how the load behaves, this one is the file
    // saying what it says.
    let literal_guard = literal_mode_guard_sql(dialect);
    if let Some((open, _)) = &literal_guard {
        text!(open.clone());
    }
    // Beside it, for the same reason: it decides what a date literal means.
    if let Some(sql) = date_format_sql(dialect) {
        text!(sql.to_string());
    }
    for sql in ansi_settings_sql(dialect) {
        text!(sql.to_string());
    }

    // The container before the thing that enters it: `USE shop` on a server that
    // has no `shop` is ERROR 1049 on line 1, and restoring onto a fresh server is
    // what a dump is mostly for.
    if opts.structure {
        for sql in create_container_sql(dialect, database, &containers) {
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
    // ahead of the table loop. **Except a function a table or view calls**,
    // which has to exist before that table or view does — `routine_slots`
    // moves it in just ahead of its first caller.
    // Carried with their names so the dependency walk below can edge one to
    // another; the strings alone said nothing about what they call.
    let mut routines: Vec<Emitted> = Vec::new();
    let mut objects: Vec<Emitted> = Vec::new();
    // The file drops what it recreates in one section before any `CREATE` —
    // see `drops_up_front`. Asked once; the table loop and the routines both
    // answer to it.
    //
    // **Only inside the file's transaction.** The section is safe to fail in
    // only because a failure undoes it: without one, a replay stopped at an
    // up-front `DROP` — a key from a table outside the export (Msg 3726) —
    // left everything dropped ahead of it gone, with every `CREATE` still
    // below the failure. Without the transaction each table and view is
    // dropped beside its own `CREATE`, as on the other engines, so a failure
    // has dropped only what was already put back.
    let up_front =
        opts.structure && opts.drop_if_exists && opts.wrap_transaction && drops_up_front(dialect);
    // **What a replay must leave standing** — see `Hold`. A table or view is
    // dropped wherever the file drops at all; a routine only up front.
    let any_edge = schema.tables.iter().any(|t| t.tsql_kind.edge);
    // `(object, label)` for each refusal, and the label of each kept one.
    let mut refused: Vec<(String, String)> = Vec::new();
    let mut kept: Vec<String> = Vec::new();
    // The routines a replay drops and recreates — every one in the dumped
    // namespaces, used by the tables or not, which the header names.
    let mut replaced: Vec<String> = Vec::new();
    let mut hold = |h: Hold, name: String| match h {
        Hold::Keep => kept.push(name),
        Hold::Refuse { object, why } => refused.push((object, format!("{name} ({why})"))),
    };
    // The tables and views no `DROP` may name, by index into `schema.tables`.
    let held: std::collections::HashSet<usize> = order
        .iter()
        .copied()
        .filter(|&i| {
            let t = &schema.tables[i];
            let h = (opts.structure && opts.drop_if_exists)
                .then(|| table_hold(t, any_edge, dialect))
                .flatten();
            let is_held = h.is_some();
            if let Some(h) = h {
                hold(h, display_name(t.schema.as_deref(), &t.name));
            }
            is_held
        })
        .collect();
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
        for kind in kinds {
            for o in schema.objects_all(kind) {
                // `is_internal` is what keeps a `serial`'s own sequence out. A
                // SQL Server object is the T-SQL pass's below, which orders
                // them by what names what.
                if o.is_internal() || o.tsql().is_some() {
                    continue;
                }
                if let crate::schema::ObjectItem::Sequence(s) = &o
                    && s.owned_by
                        .as_ref()
                        .is_some_and(|w| owned_here.contains(&(s.schema.as_deref(), &w.table)))
                {
                    continue;
                }
                // One bound to what the file leaves out goes with it.
                if o.routine().is_some_and(|r| {
                    dependent_routines.contains(&(r.schema.clone(), r.name.clone()))
                }) {
                    continue;
                }
                if namespaces.iter().any(|ns| ns.as_deref() == o.schema()) {
                    let after_tables = matches!(
                        kind,
                        ObjectKind::Function | ObjectKind::Procedure | ObjectKind::Event
                    );
                    let sql = o.create_sql(dialect);
                    // A routine the file recreates is dropped with the tables:
                    // replayed onto its source, its `CREATE` otherwise stops
                    // at a name that is already there. **Not one it cannot
                    // restate whole** — see `routine_hold`.
                    let drop = o.routine().filter(|_| up_front).and_then(|r| {
                        if let Some(h) = routine_hold(r, dialect) {
                            hold(h, display_name(r.schema.as_deref(), &r.name));
                            return None;
                        }
                        replaced.push(display_name(r.schema.as_deref(), &r.name));
                        Some(format!(
                            "DROP {} IF EXISTS {};",
                            kind.sql_keyword(),
                            crate::schema::qualified_ident(o.name(), o.schema(), dialect)
                        ))
                    });
                    let item = Emitted {
                        name: o.name().to_string(),
                        body: match (o.routine(), o.event()) {
                            (Some(r), _) => r.body.clone(),
                            (_, Some(e)) => e.body.clone(),
                            _ => sql.clone(),
                        },
                        sql,
                        drop,
                        function: kind == ObjectKind::Function,
                    };
                    if after_tables {
                        routines.push(item);
                    } else {
                        objects.push(item);
                    }
                }
            }
        }
        // **SQL Server's own standalone objects**, which a column's type, a
        // default's `NEXT VALUE FOR`, a typed `xml` column or a view can name
        // — none was read, and a table naming one stopped the restore at its
        // `CREATE TABLE` (Msg 208). In the order one can name another: a
        // collection or an alias type before a sequence typed with it, and a
        // synonym last.
        for rank in 0..4 {
            for o in &schema.tsql_objects {
                let at = match o.kind {
                    crate::schema::TsqlObjectKind::XmlSchemaCollection { .. } => 0,
                    crate::schema::TsqlObjectKind::AliasType { .. } => 1,
                    crate::schema::TsqlObjectKind::Sequence { .. } => 2,
                    crate::schema::TsqlObjectKind::Synonym { .. } => 3,
                };
                if at != rank {
                    continue;
                }
                // In a namespace of the export, or only needed.
                let in_namespace = namespaces.contains(&o.schema);
                if !in_namespace && !carried_outside.iter().any(|c| std::ptr::eq(*c, o)) {
                    continue;
                }
                // **Replaced on a replay only where the file owns it and a
                // script restates it whole**: in a namespace of the export,
                // named by a chosen table, and not a sequence. Every object in
                // the namespace used to be dropped and recreated, so replaying
                // an older one-table dump rewound a sequence other tables draw
                // from — their next insert took a key already used (Msg 2627,
                // measured) — and one an unexported table's default names
                // refused its drop (Msg 3729). A sequence's position is state
                // no script carries; everything else is created only where it
                // is missing and left standing.
                let dropped = up_front
                    && in_namespace
                    && !matches!(o.kind, crate::schema::TsqlObjectKind::Sequence { .. })
                    && tsql_named(schema, &order, &namespaces, o);
                // A sequence's counter goes on from where the source's was —
                // on one this file made, inside the same `EXEC`: the rows
                // carry their own values, so this need not wait for them, and
                // moving on one a replay found already there would rewind or
                // double it.
                let restart = opts.data.then(|| o.restart_sql()).flatten();
                let sql = if dropped {
                    o.create_sql()
                } else {
                    o.create_if_absent_sql(restart.as_deref())
                };
                objects.push(Emitted {
                    name: o.name.clone(),
                    body: o.create_sql(),
                    sql,
                    drop: dropped.then(|| o.drop_sql()),
                    function: false,
                });
            }
        }
    }
    // A domain over a domain is the same edge as a routine over a routine —
    // `kinds` above orders Enum before Domain, and nothing ordered two Domains
    // against each other. Ordered here, before the drops, as the routines are.
    let objects = order_by_mention(objects, dialect);

    // **Ordered against each other, not just against the tables.** The
    // table→routine edge was the split above; the routine→routine edge had
    // nothing at all, so two `LANGUAGE sql` functions came out in catalogue
    // order and `CREATE FUNCTION a_total() … SELECT b_base()` ahead of
    // `b_base` fails at `CREATE` under `check_function_bodies` — the very
    // fact the split rests on — after the file's `DROP TABLE`s have run.
    // Ordered here, before the drops, so those can run in the reverse.
    let routines = order_by_mention(routines, dialect);
    // A function a table or view calls goes just before it, and the tables
    // and views wait for what it reads — see `creation_order`; `None` stays
    // in the trailing section. Decided before the drops, which mirror it.
    let (order, slots) = if opts.structure {
        creation_order(&routines, &schema.tables, &order, home, dialect)
    } else {
        (order, vec![None; routines.len()])
    };

    // ── What a replay must not replace, refused before anything is dropped ───
    //
    // Inside the transaction, ahead of every `DROP`: where one of these is
    // there, the run stops with the reason before it has changed anything.
    // Left out of the drops alone, its `CREATE` stopped the run further down
    // instead, after everything ahead of it had been dropped, and said only
    // that the name was taken.
    let stops: Vec<String> = refused
        .iter()
        .filter_map(|(object, label)| {
            refuse_if_present_sql(
                dialect,
                object,
                &format!(
                    "Schemaic: this file cannot put {label} back as it is, so it does not \
                     replace it. The replay stopped here, before dropping anything."
                ),
            )
        })
        .collect();
    if !stops.is_empty() {
        text!(
            "-- Not replaced: a replay stops here, before it drops anything, where one is there"
                .to_string()
        );
        for s in stops {
            text!(s);
        }
    }

    // ── What the file recreates, dropped before any of it is created ─────────
    //
    // Only where `drops_up_front` says so. The keys between the dumped tables
    // go first, each only if it is there (a fresh database has none), then
    // everything else **in the mirror of the order the file creates it in**:
    // the trailing routines, then each table or view with the functions moved
    // in just ahead of it — views before the tables they read, a referencing
    // table before the one it references, a function after the table whose
    // column calls it and before the tables a schema-bound one reads. The
    // routines went as one block after every table, which met the second edge
    // (Msg 3729), and no single place for the block meets both. A key from a
    // table outside the export still blocks its target's drop, loudly: the
    // file does not drop what it cannot put back.
    if up_front {
        let mut drops: Vec<String> = Vec::new();
        for &i in &order {
            let t = &schema.tables[i];
            if t.is_view {
                continue;
            }
            for fk in &t.foreign_keys {
                if fk.name.is_empty()
                    || !order
                        .iter()
                        .any(|&j| fk_targets(fk, t, &schema.tables[j], home))
                {
                    continue;
                }
                let fk_name =
                    crate::schema::qualified_ident(&fk.name, t.schema.as_deref(), dialect);
                drops.push(format!(
                    "IF OBJECT_ID({}, N'F') IS NOT NULL ALTER TABLE {} DROP CONSTRAINT {};",
                    crate::schema::ddl_string(&fk_name, dialect),
                    qname(t),
                    q(&fk.name)
                ));
            }
        }
        // The routines in slot `slot`, in the reverse of their creation order.
        let routine_drops = |slot: Option<usize>| -> Vec<String> {
            routines
                .iter()
                .zip(&slots)
                .rev()
                .filter(|(_, s)| **s == slot)
                .filter_map(|(r, _)| r.drop.clone())
                .collect()
        };
        drops.extend(routine_drops(None));
        drops.extend(routine_drops(Some(order.len())));
        for (k, &i) in order.iter().enumerate().rev() {
            let t = &schema.tables[i];
            if t.shape() != TableShape::Sequence && !held.contains(&i) {
                let kw = if t.is_view { "VIEW" } else { "TABLE" };
                drops.push(format!("DROP {kw} IF EXISTS {};", qname(t)));
            }
            drops.extend(routine_drops(Some(k)));
        }
        // Then what the tables and routines were typed with or named — every
        // one of them is gone by here.
        drops.extend(objects.iter().rev().filter_map(|o| o.drop.clone()));
        text!("-- Dropped first, to be recreated below".to_string());
        for d in drops {
            text!(d);
        }
    }

    if !objects.is_empty() {
        text!("-- Types and sequences".to_string());
        // Already ordered against each other, above the drops.
        for o in objects {
            text!(o.sql);
        }
    }

    // ── Each table: structure, then its rows ─────────────────────────────────
    //
    // **Its triggers are not written with it.** Created before the rows, every
    // `INSERT` trigger fired once per restored row: an audit trigger appended a
    // second copy of each audited row to a table the file had already filled,
    // a stamping trigger rewrote every restored value, and the restore said
    // nothing — on every engine. They are collected here, one set per table in
    // file order, and written after the data and the routines (a PostgreSQL
    // trigger names its function, which has to exist first). `mysqldump`
    // writes triggers after the data for the same reason.
    let mut triggers: Vec<String> = Vec::new();
    let mut held_checks: Vec<String> = Vec::new();
    let moved_to = |k: usize| -> Vec<String> {
        routines
            .iter()
            .zip(&slots)
            .filter(|(_, s)| **s == Some(k))
            .map(|(r, _)| r.sql.clone())
            .collect()
    };
    for (k, &i) in order.iter().enumerate() {
        let moved = moved_to(k);
        if !moved.is_empty() {
            text!(crate::ddl::client_script(&moved, dialect));
        }
        let t = &schema.tables[i];
        // The per-table header, and the site an attacker controls most cheaply —
        // see the header's `comment_text` note above.
        text!(format!(
            "-- {}",
            crate::export::comment_text(&display_name(t.schema.as_deref(), &t.name))
        ));
        if opts.structure {
            // **A file must not name an object it does not create.** A sequence's
            // structure step is a comment — `create_ddl` reads the definition
            // from the row, not the catalogue, so it cannot restate one — and
            // this `DROP` asked the two-answer `is_view`, so the file destroyed
            // the sequence (MariaDB accepts `DROP TABLE` on one) and left
            // nothing behind it. Withheld rather than spelled `DROP SEQUENCE`:
            // dropping what the file cannot put back is destruction, not a dump,
            // which is the same reason `data_only_plans_no_create_and_no_drop`
            // gives one file down.
            // Nor one it cannot put back as it is — see `Hold`.
            if opts.drop_if_exists
                && !up_front
                && t.shape() != TableShape::Sequence
                && !held.contains(&i)
            {
                let kw = if t.is_view { "VIEW" } else { "TABLE" };
                text!(format!(
                    "DROP {kw} IF EXISTS {}{};",
                    qname(t),
                    drop_cascade(dialect)
                ));
            }
            // What must wait for the rows — a check the server keeps over
            // rows that violate it, SQL Server's disabled or untrusted one
            // and PostgreSQL's `NOT VALID` one, which inline would refuse
            // them — is held for the section after the data.
            let (create, held) = t.create_ddl_holding(dialect);
            text!(create);
            held_checks.extend(held);
            // **Through the shared client wrapper**, which is what puts
            // `DELIMITER` around a compound body on MySQL. Written raw, the file
            // died at the first `BEGIN … END` trigger with ERROR 1064 — after the
            // `DROP` above it had already run against the target. The routine and
            // event path has always gone through this; the trigger path did not.
            // **Through the set emitter, not one `create_sql` per trigger.**
            // The catalogue gives a group's leader `PRECEDES <successor>`, and
            // a restore reads the file top to bottom — so the first
            // `CREATE TRIGGER` named a trigger the file had not created yet and
            // both servers refused it (`ERROR 3011` / `ERROR 4031`), after the
            // `DROP TABLE` above had already run. See
            // `TriggerInfo::with_resolvable_order`.
            // **Held for the trailing section, not written here** — see
            // `triggers` above.
            if !t.triggers.is_empty() {
                let bodies = crate::schema::TriggerInfo::create_set_sql(&t.triggers, dialect);
                triggers.push(crate::ddl::client_script(&bodies, dialect));
            }
        }
        // Named columns, never `*` — see [`dump_columns`]. A table the server
        // fills entirely has nothing insertable and gets no data step at all;
        // `SELECT  FROM` would not even parse.
        let cols = dump_columns(t, dialect);
        // `shape()`, not `!is_view`: a sequence's eight `bigint` counter columns
        // are server-assigned by nothing, so `dump_columns` keeps them all
        // and the file grew an `INSERT INTO sq1` under a structure step that
        // creates no `sq1` — the restore then stops there, and every later
        // table's structure and rows are never applied.
        // And not a graph edge's, which the header explains.
        if opts.data && t.shape() == TableShape::Table && !cols.is_empty() && !t.tsql_kind.edge {
            // An identity the rows carry needs its switch thrown around them.
            let identity = t
                .columns
                .iter()
                .any(|c| carries_identity(c, dialect))
                .then(|| identity_insert_sql(dialect, &qname(t)))
                .flatten();
            if let Some((on, _)) = &identity {
                text!(on.clone());
            }
            // What a text cell cannot carry, read as SQL the server renders.
            let read_as: Vec<(&str, (String, crate::export::ServerLiteral))> = cols
                .iter()
                .filter_map(|&name| {
                    let c = t.columns.iter().find(|c| c.name == name)?;
                    Some((name, literal_select(c, &schema.tsql_objects, dialect)?))
                })
                .collect();
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
                    cols.iter()
                        .map(|c| read_as
                            .iter()
                            .find(|(n, _)| n == c)
                            .map_or_else(|| q(c), |(_, (expr, _))| expr.clone()))
                        .collect::<Vec<_>>()
                        .join(", "),
                    qualified_table(database, t.schema.as_deref(), &t.name, dialect)
                ),
                server: read_as
                    .iter()
                    .map(|(n, (_, form))| (n.to_string(), *form))
                    .collect(),
            });
            if let Some((_, off)) = identity {
                text!(off);
            }
        }
    }
    let moved = moved_to(order.len());
    if !moved.is_empty() {
        text!(crate::ddl::client_script(&moved, dialect));
    }

    // ── Key counters, once the rows they have to clear are in ────────────────
    if opts.data {
        // SQL Server's sequences are objects of their own, and move on beside
        // their `CREATE` (`TsqlObject::restart_sql`).
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
    // What nothing in the file calls; the rest went in ahead of their callers.
    let ordered: Vec<String> = routines
        .into_iter()
        .zip(slots)
        .filter(|(_, s)| s.is_none())
        .map(|(r, _)| r.sql)
        .collect();
    if !ordered.is_empty() {
        steps.push(DumpStep::Text("-- Routines and events".to_string()));
        // Already ordered against each other, above the drops.
        // Through the client wrapper, so a MySQL compound body gets its
        // `DELIMITER` — the same rule the triggers above follow.
        steps.push(DumpStep::Text(crate::ddl::client_script(&ordered, dialect)));
    }

    // ── Triggers, once no restored row can fire them ─────────────────────────
    if !triggers.is_empty() {
        steps.push(DumpStep::Text("-- Triggers".to_string()));
        steps.extend(triggers.into_iter().map(DumpStep::Text));
    }

    // ── Checks that were off, once the rows they spared are in ───────────────
    if !held_checks.is_empty() {
        steps.push(DumpStep::Text("-- Checks".to_string()));
        steps.extend(held_checks.into_iter().map(DumpStep::Text));
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
    // Outermost open, outermost close — the mode goes back the way the session
    // had it, after everything that was written under it.
    if let Some((_, close)) = literal_guard {
        steps.push(DumpStep::Text(close.to_string()));
    }

    // ── The header's last word: what a replay replaces and leaves standing ───
    //
    // The routines first: the toggle that drops them is the export's only
    // consent, and they are not what anyone ticked.
    if !replaced.is_empty() {
        let n = replaced.len();
        header.push_str(&format!(
            "\n--\n-- Replayed onto a database that already holds them, this file replaces {n} {} in these schemas,\n\
             -- used by the tables above or not, with the {} it carries: {}.",
            crate::text::plural(n, "routine", "routines"),
            crate::text::plural(n, "version", "versions"),
            crate::export::comment_text(&replaced.join(", ")),
        ));
    }
    let refused: Vec<String> = refused.into_iter().map(|(_, label)| label).collect();
    if !refused.is_empty() {
        let n = refused.len();
        header.push_str(&format!(
            "\n--\n-- A replay onto a database that already holds {} stops before it drops\n\
             -- anything: this file cannot put {} back as {}, so it does not replace {}: {}.",
            crate::text::plural(n, "this", "any of these"),
            crate::text::plural(n, "it", "them"),
            crate::text::plural(n, "it is", "they are"),
            crate::text::plural(n, "it", "them"),
            crate::export::comment_text(&refused.join(", ")),
        ));
    }
    if !kept.is_empty() {
        let n = kept.len();
        header.push_str(&format!(
            "\n--\n-- The server shows no text for {n} {} (encrypted, or not visible to this\n\
             -- login), so {} left as {}: this file neither drops nor recreates {}: {}.",
            crate::text::plural(n, "object", "objects"),
            crate::text::plural(n, "it is", "they are"),
            crate::text::plural(n, "it is", "they are"),
            crate::text::plural(n, "it", "them"),
            crate::export::comment_text(&kept.join(", ")),
        ));
    }
    // **`sqlcmd` rewrites `$(name)` even inside a string literal**, from its
    // variables and the environment, and only its `-x` flag stops it. The
    // rows' literals hold no `$(` (`export::script_literal` cuts each one),
    // but a module body, a default or a name is restated as the server has
    // it — so where one holds a `$(`, the header says how to restore. The rows'
    // own `INSERT`s carry a note of their own (`export::export_inserts_ending`).
    if crate::export::client_substitutes_variables(dialect)
        && (header.contains("$(")
            || steps
                .iter()
                .any(|s| matches!(s, DumpStep::Text(t) if t.contains("$("))))
    {
        header.push_str(
            "\n--\n-- Some names or definitions below hold a `$` followed by `(`, which sqlcmd\n\
             -- reads as a variable even inside a string: restore this file with `sqlcmd -x`,\n\
             -- or with Run SQL file, to keep that text as it is.",
        );
    }
    steps.insert(0, DumpStep::Text(header));

    DumpPlan {
        steps: close_batches(steps, dialect),
        tables: order.len(),
        cycles,
        missing,
        refused,
        left_out,
        rows_left_out: if opts.data { edges } else { Vec::new() },
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

/// Whether a folder export may write, or must ask the user first.
///
/// **The launch guard the folder export did not have.** The single-file export's
/// consent is the save dialog's own "replace?"; a directory picker has no such
/// prompt, and the per-table names are *constructed* by [`file_plan`] rather
/// than typed by the user — so aiming an export at a `sql/` directory holding
/// hand-written `orders.sql` and `customers.sql` destroyed both, with no prompt,
/// no `.bak` and no undo. The collision list was already being computed at
/// exactly the right moment, and used only to *report* the loss afterwards.
///
/// Against the app's own invariant: a destructive modal action guards its own
/// launch, in the same step that launches it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderVerdict {
    /// Nothing in the folder is at risk, or the user has already said yes.
    Write,
    /// These files exist and would be replaced. Ask before the first `rename`.
    Ask(Vec<String>),
}

/// The plan's file names that already exist in the folder — what this export is
/// about to destroy.
///
/// `exists` answers "is there already a file of this name" rather than being
/// reached for, so the census is unit-tested without a filesystem (the suite's
/// rule) and the caller's one-line `folder.join(f).is_file()` is the only
/// untested part.
///
/// Order follows the plan, not the directory: that is the order the export would
/// replace them in, and the order the prompt should read in.
///
/// **Each file's `.part` sibling is censused with it**, because the export
/// destroys that too and this is the only guard in front of it.
/// `write_one` opens the fragment with `File::create`, which truncates, and the
/// failure path removes it — so a fragment left behind by an earlier failed
/// export, which `export_failure_note` has just told the user holds their rows
/// ("`orders.csv` was not changed; the rows that were written are in
/// `orders.csv.part`"), was replaced by a retry and swept by its cancel with no
/// prompt naming it. The one file in that directory this app teaches the user to
/// care about was the one file the guard could not see.
///
/// The name comes from [`crate::export::part_path`], the one function that
/// decides that suffix — see `dump::part_of`, and the source gate that keeps it
/// the only spelling.
pub fn colliding_files(plan: &FilePlan, exists: impl Fn(&str) -> bool) -> Vec<String> {
    plan.files
        .iter()
        .flat_map(|f| [f.file.clone(), crate::export::part_path(&f.file)])
        .filter(|f| exists(f))
        .collect()
}

/// Of the files this export was going to replace, the ones it actually **has**.
///
/// The census ([`colliding_files`]) has to be read before the first rename, or
/// it is contaminated by the export's own output — but only the *finished* arm
/// is reached with the loop complete, and the whole-plan census went verbatim to
/// all three. So pressing Stop while the first table was still streaming
/// reported
///
/// ```text
/// Export cancelled — no file was finished, so nothing was written to out.
/// 3 existing files were replaced: orders.csv, items.csv, users.csv.
/// ```
///
/// — two flatly contradictory sentences, of which the second is false and is
/// also the **only** disclosure that a folder export destroys anything. It is
/// wrong in the direction that sends a user looking for a backup they do not
/// need.
///
/// Plan order, not `published` order: that is the order the prompt names them in
/// and the order they would have been replaced in.
///
/// **A `.part` counts as destroyed once its table was *attempted*, not once its
/// sibling was published.** The rename that publishes `orders.csv` consumes
/// `orders.csv.part`, so publishing is one way — but the writer truncates the
/// `.part` with `File::create` at the *start* of every attempt, and both the
/// cancel and the failure arms then sweep it. So a table whose retry was stopped
/// or failed destroyed the fragment an earlier run had left, and nothing named
/// it: `published` holds only the tables that finished, so the one file the user
/// might still have wanted back was the one the report was silent about. A
/// fragment whose table the export never reached is still sitting there and is
/// not named, which is unchanged.
///
/// `published` is a subset of `attempted`, so the published-sibling rule is the
/// same rule; it is kept as its own term because a published file is destroyed
/// under its *own* name as well as its fragment's.
pub fn destroyed(colliding: &[String], published: &[String], attempted: &[String]) -> Vec<String> {
    colliding
        .iter()
        .filter(|f| {
            published.iter().any(|p| p == *f)
                || attempted.iter().any(|a| crate::export::part_path(a) == **f)
        })
        .cloned()
        .collect()
}

/// [`FolderVerdict`] for a folder export whose collisions are already in hand.
///
/// Separate from [`colliding_files`] because the caller needs the list for its
/// *report* whichever way the verdict goes, and computing it twice is how the
/// two answers come to disagree.
///
/// **`consented` is the list the user said yes to, not a `bool`.** It was a
/// bool, and the approved re-launch then returned `Write` without looking at the
/// new census at all — while the caller had just recomputed it, calling that
/// "the more correct answer anyway, since the folder may have changed while the
/// question stood". A re-read whose result is discarded is strictly worse than
/// no re-read: it is the one place that *knows* the consented set and the actual
/// set differ. A file that appeared in the folder while the modal stood — an
/// editor autosaving, another export — was destroyed with no prompt naming it,
/// and its only disclosure was the past-tense line in the finished report.
///
/// It needs no outside writer either: `file_plan` re-resolves the chosen tables
/// against a freshly fetched schema, and the collision counter is spent over the
/// *resolved* names, so a table that disappears between the question and the
/// answer can shift a sibling from `orders_2.csv` onto `orders.csv` — a name the
/// prompt never contained.
///
/// So consent covers exactly what it named: anything else is a new question,
/// which is what the invariant this path was written for asks of it — "a
/// destructive modal action guards its own launch, in the same step that
/// launches it".
pub fn folder_verdict(consented: Option<&[String]>, colliding: &[String]) -> FolderVerdict {
    if colliding.is_empty() {
        return FolderVerdict::Write;
    }
    match consented {
        Some(said_yes) if colliding.iter().all(|f| said_yes.iter().any(|y| y == f)) => {
            FolderVerdict::Write
        }
        _ => FolderVerdict::Ask(colliding.to_vec()),
    }
}

/// The question [`FolderVerdict::Ask`] asks, as the confirm modal wants it.
///
/// Here rather than in the view for the reason every other export sentence is:
/// a report with arms (one file, a few, more than fit) is a decision, and not
/// one to make inside a callback the suite cannot reach.
pub fn folder_replace_prompt(folder: &str, replaced: &[String]) -> String {
    const SHOWN: usize = 8;
    let n = replaced.len();
    let named: Vec<&str> = replaced.iter().take(SHOWN).map(String::as_str).collect();
    let list = named.join(", ");
    let more = n.saturating_sub(named.len());
    let tail = if more > 0 {
        format!(", and {more} more")
    } else {
        String::new()
    };
    format!(
        "{n} {} in {folder} {} be replaced, and {} cannot be recovered: {list}{tail}.",
        crate::text::plural(n, "file", "files"),
        crate::text::plural(n, "will", "will"),
        crate::text::plural(n, "it", "they"),
    )
}

/// Plan a **folder** export — the schema tree's `Export ▸ CSV` and its siblings,
/// which write one file per table rather than one file for the set.
///
/// **Not [`plan`] with the options turned down, and the row step is why.** A
/// dump's `SELECT` names its columns through [`dump_columns`], which leaves
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
    // server-assigned columns [`dump_columns`] keeps out of them, and the
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

    /// SQL Server has no `CREATE SCHEMA IF NOT EXISTS`, and a `CREATE SCHEMA`
    /// must be alone in its batch, so the test-and-`EXEC` form is the one that
    /// both restores onto a fresh database and replays onto the one it came
    /// from. `dbo` exists everywhere and is not made.
    #[test]
    fn a_sql_server_schema_is_made_only_when_missing() {
        let out = create_container_sql(
            SqlDialect::MsSql,
            "app",
            &[Some("dbo".into()), Some("sa'les".into()), None],
        );
        assert_eq!(
            out,
            vec!["IF SCHEMA_ID(N'sa''les') IS NULL EXEC(N'CREATE SCHEMA [sa''les]');".to_string()]
        );
        assert_eq!(
            transaction_sql(SqlDialect::MsSql),
            ("BEGIN TRANSACTION;", "COMMIT TRANSACTION;")
        );
        assert_eq!(target_database_sql(SqlDialect::MsSql, "app"), None);
    }

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

    /// **The folder export replaced the user's files with no confirmation**,
    /// after computing the list of what it was about to destroy and using it
    /// only for a post-mortem. A directory picker has no "replace?" — the
    /// single-file export's consent is the save dialog's — and the per-table
    /// names are constructed rather than typed, so aiming an export at a `sql/`
    /// directory holding hand-written `orders.sql` destroyed it with no prompt,
    /// no `.bak` and no undo.
    #[test]
    fn a_folder_export_asks_before_replacing_a_file() {
        let schema = schema_of(vec![table("orders"), table("customers")]);
        let plan = file_plan(
            &schema,
            "shop",
            &all(&schema),
            ExportFormat::Csv,
            SqlDialect::MySql,
        );
        let census = |exists: fn(&str) -> bool| colliding_files(&plan, exists);
        // An empty folder is nothing to ask about.
        assert!(census(|_| false).is_empty());
        assert_eq!(
            folder_verdict(None, &census(|_| false)),
            FolderVerdict::Write
        );
        // One collision is.
        assert_eq!(
            folder_verdict(None, &census(|f| f == "orders.csv")),
            FolderVerdict::Ask(vec!["orders.csv".to_string()])
        );
        // In the plan's order — the order they would be replaced in — each
        // published name followed by the `.part` sibling that is consumed with
        // it.
        let all_four = [
            "orders.csv".to_string(),
            "orders.csv.part".to_string(),
            "customers.csv".to_string(),
            "customers.csv.part".to_string(),
        ];
        assert_eq!(census(|_| true), all_four);
        assert_eq!(
            folder_verdict(None, &census(|_| true)),
            FolderVerdict::Ask(all_four.to_vec())
        );
        // And once the user has said yes, it writes without asking again —
        // otherwise the confirm's Yes cannot get past its own guard.
        let both = census(|_| true);
        assert_eq!(folder_verdict(Some(&both), &both), FolderVerdict::Write);

        // **But only for what they said yes to.** A file that appeared while the
        // modal stood — an editor autosaving, another export, or a sibling
        // shifted onto a free name by a table that went missing — is a new
        // question, not a covered one.
        let said_yes = vec!["orders.csv".to_string()];
        assert_eq!(
            folder_verdict(Some(&said_yes), &both),
            FolderVerdict::Ask(both.clone()),
            "a file the prompt never named was replaced under an old consent"
        );
        // Consent to more than turns up is still consent.
        assert_eq!(folder_verdict(Some(&both), &said_yes), FolderVerdict::Write);
        // And an empty folder needs no consent at all.
        assert_eq!(folder_verdict(None, &[]), FolderVerdict::Write);

        // **And the `.part` siblings, which this export destroys just as
        // surely.** `write_one` opens the fragment with `File::create`, which
        // truncates, and the failure path removes it — so a fragment left by an
        // earlier failed export, which `export_failure_note` has just told the
        // user holds their rows, was replaced by a retry with no prompt naming
        // it. The one file in that directory this app teaches the user to care
        // about was the one the guard could not see.
        assert_eq!(
            census(|f| f == "orders.csv.part"),
            ["orders.csv.part".to_string()]
        );
        assert_eq!(
            folder_verdict(None, &census(|f| f == "orders.csv.part")),
            FolderVerdict::Ask(vec!["orders.csv.part".to_string()])
        );
        // Each published name is followed by its own fragment, so the prompt
        // still reads in the order the export would replace them.
        assert_eq!(
            census(|f| f.starts_with("orders")),
            ["orders.csv".to_string(), "orders.csv.part".to_string()]
        );
    }

    /// **The census is what is at risk; the report is what happened.** Handing
    /// the whole-plan census to the stopped and failed arms told the user that
    /// three files had been replaced in a folder the same sentence had just said
    /// nothing was written to — and that clause is the *only* place a folder
    /// export ever says it destroyed anything.
    #[test]
    fn a_stopped_export_names_only_what_it_actually_replaced() {
        let census = [
            "orders.csv".to_string(),
            "customers.csv".to_string(),
            "items.csv".to_string(),
        ];
        // Stopped before the first table finished — and before it was opened.
        assert!(destroyed(&census, &[], &[]).is_empty());
        // Stopped after the second.
        let two = ["orders.csv".to_string(), "customers.csv".to_string()];
        assert_eq!(destroyed(&census, &two, &two), two);
        // A file the export wrote that was not there before is not a
        // replacement, however far the run got.
        assert!(
            destroyed(
                &[],
                &["orders.csv".to_string()],
                &["orders.csv".to_string()]
            )
            .is_empty()
        );
        // Plan order, not publication order — the order the prompt named them
        // in, and the order they would have gone in.
        let out_of_order = ["items.csv".to_string(), "orders.csv".to_string()];
        assert_eq!(
            destroyed(&census, &out_of_order, &out_of_order),
            ["orders.csv".to_string(), "items.csv".to_string()]
        );

        // **A `.part` goes with its published sibling**, because the rename that
        // publishes `orders.csv` is what consumes `orders.csv.part` — and a
        // fragment whose table the export never reached is still sitting there,
        // so it is not named.
        let with_parts = [
            "orders.csv".to_string(),
            "orders.csv.part".to_string(),
            "items.csv.part".to_string(),
        ];
        let orders = ["orders.csv".to_string()];
        assert_eq!(
            destroyed(&with_parts, &orders, &orders),
            ["orders.csv".to_string(), "orders.csv.part".to_string()],
            "the fragment the rename consumed was not reported"
        );
        assert!(
            destroyed(&with_parts, &[], &[]).is_empty(),
            "a run that opened nothing destroyed nothing"
        );
    }

    /// **The fragment a *stopped retry* destroyed, which nothing named.**
    ///
    /// `write_one` truncates the table's `.part` with `File::create` before it
    /// writes a byte, and the cancel and failure arms then sweep it. So a table
    /// that was begun and did not finish destroyed whatever fragment an earlier
    /// run had left there — the file `export_failure_note` had just told the user
    /// holds their rows — while `published`, which only finished tables reach,
    /// said nothing about it. The report's `replaced` clause is the *only* place
    /// a folder export ever says it destroyed anything.
    #[test]
    fn a_fragment_a_stopped_retry_truncated_is_named_even_though_nothing_published() {
        let census = ["orders.csv.part".to_string()];
        // The run reached `orders` and was stopped inside it: nothing published,
        // and the pre-existing fragment is gone all the same.
        assert_eq!(
            destroyed(&census, &[], &["orders.csv".to_string()]),
            ["orders.csv.part".to_string()],
            "the fragment this run truncated and swept was not reported"
        );
        // A table the run never reached keeps its fragment, and is not named.
        assert!(
            destroyed(&census, &[], &["items.csv".to_string()]).is_empty(),
            "a fragment the export never opened is still sitting there"
        );
        // And the published name is still reported under its own name as well as
        // its fragment's.
        let both = ["orders.csv".to_string(), "orders.csv.part".to_string()];
        let orders = ["orders.csv".to_string()];
        assert_eq!(destroyed(&both, &orders, &orders), both);
    }

    #[test]
    fn the_replace_prompt_names_the_files_and_counts_the_rest() {
        let one = folder_replace_prompt("sql", &["orders.csv".to_string()]);
        assert!(one.contains("1 file"), "{one}");
        assert!(one.contains("orders.csv"), "{one}");
        assert!(one.contains("it cannot be recovered"), "{one}");

        let many: Vec<String> = (0..12).map(|i| format!("t{i}.csv")).collect();
        let msg = folder_replace_prompt("sql", &many);
        assert!(msg.contains("12 files"), "{msg}");
        assert!(msg.contains("and 4 more"), "{msg}");
        assert!(msg.contains("t0.csv"), "{msg}");
        assert!(
            !msg.contains("t11.csv"),
            "the tail is counted, not named: {msg}"
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
    /// `SELECT *`, naming the identity columns `dump_columns` exists to keep
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
        let (order, cycles) = order_tables(&s.tables, &all(&s), SqlDialect::MySql, None);
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
        let (order, cycles) = order_tables(&s.tables, &all(&s), SqlDialect::MySql, None);
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
        let (order, cycles) = order_tables(&s.tables, &all(&s), SqlDialect::MySql, None);
        assert_eq!(names(&s, &order), vec!["employees"]);
        assert!(!cycles, "a self-reference orders fine — it is one table");
    }

    #[test]
    fn a_two_table_cycle_still_dumps_every_table_and_says_so() {
        let s = schema_of(vec![refs(table("a"), "b"), refs(table("b"), "a")]);
        let (order, cycles) = order_tables(&s.tables, &all(&s), SqlDialect::MySql, None);
        assert!(
            cycles,
            "no order satisfies both keys — the caller must know"
        );
        assert_eq!(order.len(), 2, "a cycle must not drop a table");
    }

    #[test]
    fn an_fk_to_a_table_outside_the_selection_does_not_order_it_in() {
        let s = schema_of(vec![refs(table("orders"), "archive"), table("archive")]);
        let (order, cycles) =
            order_tables(&s.tables, &["orders".to_string()], SqlDialect::MySql, None);
        assert_eq!(names(&s, &order), vec!["orders"]);
        assert!(!cycles);
    }

    #[test]
    fn views_come_after_every_base_table() {
        let s = schema_of(vec![view("v_recent"), table("orders"), table("customers")]);
        let (order, _) = order_tables(&s.tables, &all(&s), SqlDialect::MySql, None);
        assert_eq!(names(&s, &order).last().unwrap(), "v_recent");
    }

    #[test]
    fn ties_break_by_name_so_two_dumps_of_one_schema_match() {
        let s = schema_of(vec![table("zebra"), table("apple"), table("mango")]);
        let (order, _) = order_tables(&s.tables, &all(&s), SqlDialect::MySql, None);
        assert_eq!(names(&s, &order), vec!["apple", "mango", "zebra"]);
    }

    /// **A newline inside a name injects a statement into the dump, and it runs
    /// at restore.** A `--` comment ends at the first physical newline and an
    /// identifier may contain one: PostgreSQL 16 creates a table named
    /// `orders\nDROP TABLE customers;` and hands it back from
    /// `information_schema.tables` without complaint (measured). The header
    /// lines interpolated schema-fetched names raw, so the file the user
    /// believes is their backup carried
    ///
    /// ```text
    /// -- orders
    /// DROP TABLE customers;
    /// ```
    ///
    /// — a top-level statement, which runs against whichever database the
    /// restore targets, typically not the hostile one the name came from.
    ///
    /// Everything *executable* in a dump was already guarded; the gap was
    /// exactly the comment lines, which is why the "one identifier quoter"
    /// invariant did not cover it — `ident_sql` doubles a quote character and
    /// says nothing about `\n`, so applying it here would not have helped.
    ///
    /// **Asserted over the emitted script, not over `comment_text`.** A test of
    /// the escaper alone passes against the unfixed tree, since nothing called
    /// it. The property is structural: every line of the file that is not inside
    /// a statement begins with `--`.
    #[test]
    fn a_newline_in_a_name_cannot_open_a_line_of_its_own() {
        let hostile = "orders\nDROP TABLE customers;";
        let mut t = table(hostile);
        t.schema = Some("pub\nDROP TABLE s;".to_string());
        t.columns.push(ColumnInfo {
            name: "seq\nDROP TABLE c;".to_string(),
            type_name: "int".to_string(),
            auto_increment: true,
            ..Default::default()
        });
        let s = schema_of(vec![t]);
        // A ticked name the introspection cannot find, so the "missing" line is
        // written too — it is a fourth interpolation site.
        let chosen = {
            let mut c = all(&s);
            c.push("gone\nDROP TABLE m;".to_string());
            c
        };
        let p = plan(
            &s,
            "shop\nDROP TABLE d;",
            &chosen,
            DumpOptions::default(),
            SqlDialect::MySql,
        );

        // Every line of a `Text` step that is not the step's own SQL must be a
        // comment. A header step is entirely comment lines; a `CREATE`/`DROP`
        // step is entirely SQL. So the check is: no line inside a step that
        // *starts* as a comment may stop being one.
        for step in &p.steps {
            let DumpStep::Text(txt) = step else { continue };
            if !txt.starts_with("--") {
                continue;
            }
            for line in txt.lines() {
                assert!(
                    line.trim_start().starts_with("--") || line.trim().is_empty(),
                    "a comment block grew a statement line: {line:?}\nin step: {txt:?}"
                );
            }
        }
        // And the names are still legible in the file, not merely absent.
        let text = text_of(&p);
        assert!(text.contains("orders DROP TABLE customers;"), "{text}");
    }

    /// The same property one producer further in: the **structure step**.
    ///
    /// The header line above a table goes through `comment_text`; the
    /// `create_ddl` line below it did not, and three of its arms are comments
    /// carrying a server-supplied name — a sequence, a view whose definition
    /// the connection could not read, and a PostgreSQL-shaped trigger emitted
    /// in another dialect. Asserted over the emitted script for the reason the
    /// header's own test states: a test of `comment_text` passes against a tree
    /// that never calls it.
    #[test]
    fn a_comment_only_structure_step_cannot_open_a_line_of_its_own() {
        let mut seq = table("sq\nDROP TABLE customers;");
        seq.is_sequence = true;
        let s = schema_of(vec![seq]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        );

        for step in &p.steps {
            let DumpStep::Text(txt) = step else { continue };
            if !txt.starts_with("--") {
                continue;
            }
            for line in txt.lines() {
                assert!(
                    line.trim_start().starts_with("--") || line.trim().is_empty(),
                    "a comment block grew a statement line: {line:?}\nin step: {txt:?}"
                );
            }
        }
        assert!(
            text_of(&p).contains("sq DROP TABLE customers;"),
            "and the name is still legible, not merely absent"
        );
    }

    /// The unreadable-view arm's step is a comment line **and then** a
    /// `CREATE VIEW` skeleton, so the whole-step check above cannot be used: the
    /// name inside that `CREATE` may hold a newline and is still one quoted
    /// identifier. The comment line above it may not.
    #[test]
    fn an_unreadable_views_comment_line_stays_one_line() {
        let mut unreadable = table("v\nDROP TABLE orders;");
        unreadable.is_view = true;
        unreadable.view_definition = None;
        let ddl = unreadable.create_ddl(SqlDialect::MySql);
        let first = ddl.lines().next().unwrap_or_default();
        assert!(
            first.ends_with("was not available."),
            "the comment line was cut short by the name: {first:?}"
        );
        assert!(
            first.contains("v DROP TABLE orders;"),
            "and the name is still legible: {first:?}"
        );
    }

    /// A PostgreSQL-shaped trigger emitted in another dialect says so on a `--`
    /// line carrying the **unquoted** function name, and that line is followed
    /// by a `;` — so a newline in the name made the rest of it the statement.
    #[test]
    fn a_cross_dialect_trigger_note_stays_one_line() {
        let t = TriggerInfo {
            name: "t1".to_string(),
            table: "orders".to_string(),
            timing: TriggerTiming::Before,
            events: vec![TriggerEvent::Insert],
            action: TriggerAction::Function {
                name: "f\nDROP TABLE customers;".to_string(),
                args: Vec::new(),
            },
            ..Default::default()
        };
        for dialect in [SqlDialect::MySql, SqlDialect::Sqlite] {
            let sql = t.create_sql(dialect);
            for line in sql.lines() {
                assert!(
                    !line.trim_start().starts_with("DROP"),
                    "{dialect:?} note grew a statement line: {line:?}\n{sql}"
                );
            }
            assert!(
                sql.contains("f DROP TABLE customers;"),
                "and the name is still legible: {sql}"
            );
        }

        // **The fifth arm of the same family, and the one the sweep missed.**
        // A MySQL/SQLite trigger carries a `Body` and a PostgreSQL one carries a
        // `Function`, so this note needs a `TriggerInfo` rendered in a dialect
        // other than the one it was read from — exactly the reachability the two
        // arms above have. It interpolated the trigger's own name and its table
        // with no treatment at all, so a name carrying a newline made the second
        // line a top-level statement in whatever script carried it, and
        // `Db::run_script`'s guard deliberately never reads the file.
        let body = TriggerInfo {
            name: "t1\nDROP DATABASE prod;".to_string(),
            table: "orders\nDROP TABLE customers;".to_string(),
            timing: TriggerTiming::Before,
            events: vec![TriggerEvent::Insert],
            action: TriggerAction::Body("SET NEW.a = 1".to_string()),
            ..Default::default()
        };
        let sql = body.create_sql(SqlDialect::Postgres);
        for line in sql.lines() {
            assert!(
                line.trim_start().starts_with("--"),
                "the note grew a line that is not a comment: {line:?}\n{sql}"
            );
        }
        assert!(
            sql.contains("t1 DROP DATABASE prod;"),
            "and the name is still legible: {sql}"
        );
    }

    /// A dump must never name an object it does not create.
    ///
    /// A MariaDB sequence's structure step is a comment — Schemaic reads the
    /// definition from the row, not the catalogue, so it cannot restate one.
    /// The `DROP` above it and the `INSERT` below it were still asking the
    /// two-answer `is_view`, so the file destroyed the sequence and then died
    /// at an `INSERT` into a table nothing had created — taking every later
    /// table's structure and rows with it, since a restore stops at the first
    /// error.
    #[test]
    fn a_sequence_gets_neither_a_drop_nor_a_row_step() {
        let mut seq = table("sq1");
        seq.is_sequence = true;
        seq.columns = vec![ColumnInfo {
            name: "next_not_cached_value".to_string(),
            type_name: "bigint".to_string(),
            ..Default::default()
        }];
        let s = schema_of(vec![seq]);
        let opts = DumpOptions {
            data: true,
            structure: true,
            drop_if_exists: true,
            ..Default::default()
        };
        let p = plan(&s, "shop", &all(&s), opts, SqlDialect::MySql);

        assert!(
            !p.steps.iter().any(|s| matches!(s, DumpStep::Rows { .. })),
            "a sequence's counter row is not data this file can restore"
        );
        let text = text_of(&p);
        assert!(
            !text.contains("DROP TABLE"),
            "the file drops an object it never recreates:\n{text}"
        );
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

    /// **And they name their target the way the file names its own tables.**
    ///
    /// On MySQL and MariaDB `KEY_COLUMN_USAGE.REFERENCED_TABLE_SCHEMA` is the
    /// *database*, so `ForeignKeyInfo::ref_schema` is `Some("shop")` for an
    /// ordinary same-database key — and `fk_clause` hard-qualified the
    /// `REFERENCES` from it while the `ALTER TABLE` above it stayed bare. Edit
    /// the `USE` line, which `target_database_sql`'s own doc calls "the one line
    /// to edit to restore the dump somewhere else", and every table, row and
    /// trigger lands in `shop_copy` while every foreign key points at `shop`.
    /// On a fresh server that is `ERROR 1215` at the very end, after all the
    /// data. On the **same** server — restoring a copy beside the original, the
    /// commonest reason to retarget — it succeeds silently and constrains the
    /// copy against production: an insert into the copy is validated against
    /// live rows, and a production `ON DELETE CASCADE` deletes out of the copy.
    ///
    /// `DumpStep::Rows::insert_database` exists because this exact bug was found
    /// and fixed on the `INSERT` half; the same reasoning never reached here.
    ///
    /// Every foreign-key fixture in this module is built by `refs()` with
    /// `ref_schema: None` — the one shape that cannot show it — so this one
    /// states the database explicitly.
    #[test]
    fn a_foreign_key_inside_the_dump_is_not_pinned_to_the_source_database() {
        let mut orders = refs(table("orders"), "customers");
        orders.foreign_keys[0].ref_schema = Some("shop".to_string());
        let s = schema_of(vec![orders, table("customers")]);
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        ));
        assert!(text.contains("ADD CONSTRAINT"), "{text}");
        assert!(
            !text.contains("`shop`.`customers`"),
            "the restored copy is wired back to the source:\n{text}"
        );
        assert!(text.contains("REFERENCES `customers`"), "{text}");

        // PostgreSQL's `ref_schema` is a **namespace** — part of the object,
        // not its address — so it stays.
        let mut o = refs(table("orders"), "customers");
        o.schema = Some("app".to_string());
        o.foreign_keys[0].ref_schema = Some("app".to_string());
        let mut c = table("customers");
        c.schema = Some("app".to_string());
        let s = schema_of(vec![o, c]);
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        assert!(text.contains("\"app\".\"customers\""), "{text}");
    }

    /// **A key pointing at another database is not a key inside the dump**, and
    /// stripping its qualifier retargets it.
    ///
    /// `fk_targets` compared `fk.ref_schema.or(owner.schema)` against
    /// `cand.schema` — and on MySQL `TableInfo::schema` is always `None`,
    /// because a database *is* its namespace. Both sides unknown, so the
    /// "cannot answer no" arm fired and the key matched on `ref_table` alone: a
    /// cross-database `REFERENCES archive.customers` was classified as in-dump
    /// because a table called `customers` was in the dump, and the strip then
    /// emitted it bare. Restoring that file points the key at the dump's own
    /// `customers` — a different table. If the rows happen to satisfy it the
    /// server creates the constraint **silently**; if they do not, the restore
    /// dies at ERROR 1452 after every row has landed.
    ///
    /// Premise measured on MariaDB 10.11.14: a cross-database InnoDB key is
    /// legal and `information_schema.KEY_COLUMN_USAGE` reports
    /// `REFERENCED_TABLE_SCHEMA = 'zz_archive'` for it.
    #[test]
    fn a_foreign_key_into_another_database_keeps_its_qualifier() {
        let mut orders = refs(table("orders"), "customers");
        orders.foreign_keys[0].ref_schema = Some("archive".to_string());
        let s = schema_of(vec![orders, table("customers")]);
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        ));
        assert!(
            !text.contains("REFERENCES `customers`"),
            "the key was retargeted at the dump's own table:\n{text}"
        );
        // It is a key pointing outside the export, so the file's own header
        // says it was left out — `dropped_fks` is what writes that sentence,
        // and the misclassification is what used to swallow it.
        assert!(
            text.to_lowercase().contains("foreign key"),
            "a key left out of the file says nothing:\n{text}"
        );
        // A same-database key, explicit or bare, is still restated bare.
        let mut same = refs(table("orders"), "customers");
        same.foreign_keys[0].ref_schema = Some("shop".to_string());
        let s = schema_of(vec![same, table("customers")]);
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        ));
        assert!(text.contains("REFERENCES `customers`"), "{text}");
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

    /// **A trigger is created after the rows it would fire on**, on every
    /// engine. Created with its table, an `AFTER INSERT` audit trigger fired
    /// once per restored row — the audit table ended up with its own dumped
    /// rows plus a second copy of each, and a stamping trigger rewrote every
    /// restored value — and the restore reported success. `mysqldump` writes
    /// triggers after the data for the same reason.
    #[test]
    fn triggers_are_created_after_the_rows_they_would_fire_on() {
        for d in [
            SqlDialect::MySql,
            SqlDialect::Postgres,
            SqlDialect::Sqlite,
            SqlDialect::MsSql,
        ] {
            let mut audit = table("audit");
            let mut t = table("orders");
            if d == SqlDialect::MsSql {
                audit.schema = Some("dbo".to_string());
                t.schema = Some("dbo".to_string());
            }
            t.triggers.push(TriggerInfo {
                name: "orders_ai".to_string(),
                schema: t.schema.clone(),
                table: "orders".to_string(),
                timing: TriggerTiming::After,
                events: vec![TriggerEvent::Insert],
                action: TriggerAction::Body("INSERT INTO audit VALUES (1)".to_string()),
                ..Default::default()
            });
            let s = schema_of(vec![t, audit]);
            let file = file_of(&plan(&s, "shop", &all(&s), DumpOptions::default(), d));
            let trigger = pos(&file, "orders_ai");
            assert!(pos(&file, "<<rows orders:") < trigger, "{d:?}: {file}");
            assert!(pos(&file, "<<rows audit:") < trigger, "{d:?}: {file}");
        }
    }

    /// **A MySQL dump with a compound trigger and a routine splits back into
    /// whole statements through the script runner's own splitter.** The
    /// trailing sections open with a `-- Triggers` / `-- Routines and events`
    /// comment step directly above their `DELIMITER $$`, and the splitter took a
    /// directive only after whitespace — so the comment, the directive and the
    /// body's first statement went to the server as one, and the restore
    /// stopped there (ERROR 1064) with every table already replaced. The text
    /// is rendered the way the app's writer does (`"{sql}\n\n"` per step).
    #[test]
    fn a_mysql_dump_with_compound_bodies_splits_back_into_whole_statements() {
        let audit = table("audit");
        let mut t = table("orders");
        t.triggers.push(TriggerInfo {
            name: "orders_ai".to_string(),
            table: "orders".to_string(),
            timing: TriggerTiming::After,
            events: vec![TriggerEvent::Insert],
            action: TriggerAction::Body(
                "BEGIN INSERT INTO audit VALUES (NEW.id); INSERT INTO audit VALUES (NEW.id + 100); END"
                    .to_string(),
            ),
            ..Default::default()
        });
        let mut s = schema_of(vec![t, audit]);
        s.routines
            .push(std::sync::Arc::new(crate::schema::RoutineInfo {
                name: "p_twice".to_string(),
                kind: crate::schema::RoutineKind::Procedure,
                body: "BEGIN SELECT 1; SELECT 2; END".to_string(),
                ..Default::default()
            }));
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        );
        let mut text = String::new();
        for step in &p.steps {
            match step {
                DumpStep::Text(t) => text.push_str(&format!("{t}\n\n")),
                DumpStep::Rows { table, .. } => {
                    text.push_str(&format!("INSERT INTO {table} VALUES (1);\n\n"))
                }
            }
        }
        assert!(text.contains("-- Triggers"), "{text}");
        for block in [7usize, 64, 4096] {
            let mut sp = crate::script::Splitter::new(SqlDialect::MySql);
            let mut stmts = Vec::new();
            for piece in text.as_bytes().chunks(block) {
                stmts.extend(sp.push(piece));
            }
            stmts.extend(sp.finish());
            for st in &stmts {
                assert!(
                    !st.sql
                        .lines()
                        .any(|l| l.trim_start().to_ascii_uppercase().starts_with("DELIMITER")),
                    "a directive reached the server at a {block}-byte block: {:?}",
                    st.sql
                );
            }
            let whole = |needle: &str, tail: &str| {
                stmts
                    .iter()
                    .any(|st| st.sql.contains(needle) && st.sql.contains(tail))
            };
            assert!(
                whole("orders_ai", "NEW.id + 100); END"),
                "the trigger came out in pieces at a {block}-byte block: {stmts:#?}"
            );
            assert!(
                whole("p_twice", "SELECT 2; END"),
                "the routine came out in pieces at a {block}-byte block: {stmts:#?}"
            );
        }
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

    /// **The literal guard is the outermost thing in the file**, because it
    /// decides what every `'…'` after it *means* — including the ones in the
    /// `CREATE TABLE`s that come before the transaction.
    ///
    /// Measured on MariaDB 10.11.14 and MySQL 8.4.11: a value `a'b\c` is
    /// written `'a''b\\c'`, and replayed on a session carrying
    /// `NO_BACKSLASH_ESCAPES` it restores as `a'b\\c` — one character longer
    /// than the row that was dumped, with nothing in the file to say so.
    #[test]
    fn a_mysql_dump_pins_the_mode_its_literals_were_written_for() {
        let s = schema_of(vec![table("orders")]);
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        ));
        let (open, close) = literal_mode_guard_sql(SqlDialect::MySql).expect("mysql has one");
        assert!(file.contains(&open), "no literal guard in:\n{file}");
        assert!(pos(&file, &open) < pos(&file, "SET FOREIGN_KEY_CHECKS = 0;"));
        assert!(pos(&file, &open) < pos(&file, "CREATE TABLE"));
        assert!(pos(&file, "SET FOREIGN_KEY_CHECKS = 1;") < pos(&file, close));
    }

    /// It is not scaffolding the user can turn off: the two checkboxes choose
    /// how the load *behaves*, and this one is about whether the file says what
    /// it means.
    #[test]
    fn the_literal_guard_is_not_one_of_the_optional_scaffolds() {
        let s = schema_of(vec![table("orders")]);
        let opts = DumpOptions {
            wrap_transaction: false,
            disable_fk_checks: false,
            ..Default::default()
        };
        let file = file_of(&plan(&s, "shop", &all(&s), opts, SqlDialect::MySql));
        let (open, close) = literal_mode_guard_sql(SqlDialect::MySql).expect("mysql has one");
        assert!(file.contains(&open));
        assert!(file.contains(close));
    }

    /// SQLite has no backslash escape at all and PostgreSQL has written
    /// standard literals by default since 9.1, so neither file carries a line
    /// that would only be noise in it.
    #[test]
    fn the_other_two_engines_need_no_literal_guard() {
        assert_eq!(literal_mode_guard_sql(SqlDialect::Sqlite), None);
        assert_eq!(literal_mode_guard_sql(SqlDialect::Postgres), None);
        let s = schema_of(vec![table("orders")]);
        for d in [SqlDialect::Postgres, SqlDialect::Sqlite] {
            let file = file_of(&plan(&s, "shop", &all(&s), DumpOptions::default(), d));
            assert!(!file.contains("sql_mode"), "{d:?}:\n{file}");
        }
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

    /// A SQL Server table with an identity key, a `rowversion` and a computed
    /// column — the three columns `is_server_assigned` answers yes for there.
    fn mssql_orders() -> TableInfo {
        let mut t = table("orders");
        t.schema = Some("dbo".to_string());
        t.columns[0].auto_increment = true;
        t.columns[0].identity_always = true;
        t.columns.push(ColumnInfo {
            name: "rv".to_string(),
            type_name: "rowversion".to_string(),
            identity_always: true,
            ..Default::default()
        });
        t.columns.push(server_assigned("total"));
        t
    }

    /// **An identity's values are carried on SQL Server**, which will take an
    /// explicit one under `SET IDENTITY_INSERT … ON` — so the restored rows
    /// keep the keys every foreign key onto them names, rather than being
    /// renumbered. What the server alone writes (a `rowversion`, a computed
    /// column) stays out of the `INSERT` as on every engine, and is the only
    /// thing the header says is lost.
    #[test]
    fn a_sql_server_identity_is_carried_inside_identity_insert() {
        let s = schema_of(vec![mssql_orders(), {
            let mut t = table("plain");
            t.schema = Some("dbo".to_string());
            t
        }]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        );
        assert_eq!(select_of(&p, "orders"), "SELECT [id] FROM [dbo].[orders]");
        let file = file_of(&p);
        let on = pos(&file, "SET IDENTITY_INSERT [dbo].[orders] ON;");
        let rows = pos(&file, "<<rows orders:");
        let off = pos(&file, "SET IDENTITY_INSERT [dbo].[orders] OFF;");
        assert!(on < rows && rows < off, "{file}");
        assert!(
            !file.contains("IDENTITY_INSERT [dbo].[plain]"),
            "a table with no identity needs no switch"
        );
        let header = text_of(&p);
        assert!(
            header.contains("orders.rv") && header.contains("orders.total"),
            "{header}"
        );
        assert!(
            !header.contains("orders.id"),
            "the identity is carried: {header}"
        );
        assert!(!header.contains("renumbered"), "nothing is: {header}");
        // And a structure-only file has no rows to switch it for.
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions {
                data: false,
                ..Default::default()
            },
            SqlDialect::MsSql,
        );
        assert!(!file_of(&p).contains("IDENTITY_INSERT"));
    }

    /// **Every statement closes its batch with `GO` on SQL Server.** `CREATE
    /// VIEW` and `CREATE TRIGGER` must each open a batch of their own, and a
    /// restore — `Db::run_script`, or `sqlcmd` — cuts the file at its `GO`
    /// lines; without them the first view after a table's rows was Msg 111.
    /// A comment needs none, and a script that already ends in one (the
    /// triggers' `client_script`) gets no second.
    ///
    /// **And a comment never shares a module's batch.** SQL Server stores a
    /// module's whole batch as its definition, so a section heading
    /// (`-- Routines and events`, `-- Triggers`, a view's `-- dbo.v`) left open
    /// above a `CREATE` would be the first line of every restored module. It
    /// is not, because every module is scripted under its own `SET ANSI_NULLS`
    /// and `SET QUOTED_IDENTIFIER` batches (`ddl::tsql_settings_scripted`),
    /// which is where the heading lands — pinned here so the wrapper cannot
    /// move without the heading being given a batch of its own.
    #[test]
    fn a_sql_server_dump_closes_every_batch_with_go() {
        let mut t = mssql_orders();
        t.triggers.push(TriggerInfo {
            name: "tr".to_string(),
            schema: Some("dbo".to_string()),
            table: "orders".to_string(),
            timing: TriggerTiming::After,
            events: vec![TriggerEvent::Insert],
            // SQL Server keeps the whole stored statement as the body.
            action: TriggerAction::Body(
                "CREATE TRIGGER dbo.tr ON dbo.orders AFTER INSERT AS SELECT 1".to_string(),
            ),
            ..Default::default()
        });
        let mut v = view("v");
        v.schema = Some("dbo".to_string());
        v.create_sql = Some("CREATE VIEW dbo.v AS SELECT 1 AS id".to_string());
        let mut s = schema_of(vec![t, v]);
        // A routine's `create_sql` is already a runnable script ending in `GO`;
        // the routine section must not wrap it a second time (`GO;` is not a
        // separator, and the restore stopped at it with Msg 102).
        for (name, kind, returns) in [
            ("f_double", crate::schema::RoutineKind::Function, "int"),
            ("p_touch", crate::schema::RoutineKind::Procedure, ""),
        ] {
            s.routines
                .push(std::sync::Arc::new(crate::schema::RoutineInfo {
                    name: name.to_string(),
                    schema: Some("dbo".to_string()),
                    kind,
                    arguments: if returns.is_empty() {
                        String::new()
                    } else {
                        "@x int".to_string()
                    },
                    returns: returns.to_string(),
                    body: "BEGIN\n  RETURN @x * 2;\nEND".to_string(),
                    ..Default::default()
                }));
        }
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        );
        let file = file_of(&p);
        for (i, step) in p.steps.iter().enumerate() {
            match step {
                DumpStep::Text(t)
                    if t.lines()
                        .all(|l| l.trim().is_empty() || l.starts_with("--")) =>
                {
                    assert!(!t.contains("\nGO"), "a comment needs no batch: {t}");
                }
                DumpStep::Text(t) if t.trim() == "GO" => {}
                DumpStep::Text(t) => {
                    assert!(
                        t.trim_end().ends_with("\nGO"),
                        "step {i} left its batch open: {t}"
                    );
                    assert!(!t.contains("GO\nGO") && !t.contains("GO\n\nGO"), "{t}");
                }
                // The rows close their own batches, one per `INSERT`
                // (`render_rows`), so nothing is added after the step.
                DumpStep::Rows { .. } => assert!(
                    !matches!(p.steps.get(i + 1), Some(DumpStep::Text(t)) if t.trim() == "GO"),
                    "an empty batch after the rows: {file}"
                ),
            }
        }
        // Every module opens its own batch, and nothing is in front of its
        // `CREATE` there — not even a comment, which the server would store as
        // the module's first line.
        let mut modules = 0;
        for batch in file.split("\nGO\n") {
            let batch = batch.trim_start();
            if [
                "CREATE VIEW",
                "CREATE FUNCTION",
                "CREATE PROCEDURE",
                "CREATE TRIGGER",
            ]
            .iter()
            .any(|m| batch.contains(m))
            {
                modules += 1;
                assert!(batch.starts_with("CREATE"), "a module's batch: {batch}");
            }
        }
        assert_eq!(
            modules, 4,
            "the view, both routines and the trigger: {file}"
        );
        // Each routine closes its batch once, with a bare `GO`.
        pos(&file, "CREATE FUNCTION [dbo].[f_double]");
        pos(&file, "CREATE PROCEDURE [dbo].[p_touch]");
        assert!(
            file.lines().all(|l| !l.trim().eq_ignore_ascii_case("GO;")),
            "{file}"
        );
        let lines: Vec<&str> = file
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        assert!(
            !lines.windows(2).any(|w| w[0] == "GO" && w[1] == "GO"),
            "an empty batch: {file}"
        );
        // Nothing of the kind on an engine without batches.
        let mut t = table("orders");
        t.columns[0].auto_increment = true;
        let s = schema_of(vec![t]);
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        ));
        assert!(
            !file.contains("\nGO") && !file.contains("IDENTITY_INSERT"),
            "{file}"
        );
    }

    /// **A check or key that was off in the source is put back off, after the
    /// rows.** SQL Server keeps a disabled or untrusted constraint over rows
    /// that violate it; restated as an ordinary one the restore stopped at
    /// those rows (Msg 547), and restated before them an untrusted check —
    /// still enforced for new rows — refused them all the same.
    #[test]
    fn a_sql_server_constraint_that_was_off_is_restated_off_after_the_rows() {
        let mut parent = table("parent");
        parent.schema = Some("dbo".to_string());
        let mut child = refs(table("child"), "parent");
        child.schema = Some("dbo".to_string());
        child.foreign_keys[0].not_enforced = true;
        child.foreign_keys[0].not_validated = true;
        child.check_constraints.push(crate::schema::CheckInfo {
            name: "ck_id".to_string(),
            expression: "[id]>(0)".to_string(),
            enforced: true,
            validated: false,
            inherited: false,
            column_level: false,
        });
        let s = schema_of(vec![parent, child]);
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        ));
        let rows = pos(&file, "<<rows child:");
        let create = pos(&file, "CREATE TABLE [dbo].[child]");
        assert!(!file[create..rows].contains("ck_id"), "{file}");
        let check = pos(
            &file,
            "ALTER TABLE [dbo].[child] WITH NOCHECK ADD CONSTRAINT [ck_id] CHECK ([id]>(0));",
        );
        assert!(rows < check, "{file}");
        let fk = pos(
            &file,
            "ALTER TABLE [dbo].[child] WITH NOCHECK ADD CONSTRAINT [fk_child_parent]",
        );
        assert!(rows < fk, "{file}");
        assert!(
            pos(
                &file,
                "ALTER TABLE [dbo].[child] NOCHECK CONSTRAINT [fk_child_parent];"
            ) > fk,
            "{file}"
        );
    }

    /// **A SQL Server rows step closes a batch after every `INSERT`.** All of a
    /// table's statements shared one `GO` batch, and SQL Server refuses a batch
    /// past 65,536 network packets (256 MB at the default size), so a `sqlcmd`
    /// restore of a large table was dropped mid-file; the app's own restore got
    /// through only because its splitter cut at every `;`. Each statement is at
    /// most `INSERT_BATCH_BYTES` plus one row, so a batch per statement stays
    /// far inside the limit whatever the table's size.
    #[test]
    fn a_sql_server_rows_step_closes_a_batch_after_every_insert() {
        let rs = crate::model::ResultSet::from_rows(
            vec![crate::model::Column {
                name: "id".to_string(),
                type_name: "int".to_string(),
                origin: None,
            }],
            (0..600)
                .map(|i| vec![crate::model::Value::Int(i)])
                .collect(),
        );
        let order: Vec<usize> = (0..600).collect();
        let render = |d: SqlDialect| {
            let mut out = Vec::new();
            let tally = render_rows(
                &mut out,
                &mut crate::export::OneChunk::new(&rs, &order),
                ("shop", Some("dbo"), "t"),
                &[],
                d,
            )
            .unwrap();
            assert_eq!(tally.rows, 600);
            String::from_utf8(out).unwrap()
        };
        let sql = render(SqlDialect::MsSql);
        let inserts = sql.matches("INSERT INTO").count();
        assert_eq!(inserts, 3, "{sql}");
        assert_eq!(sql.matches(";\nGO\n").count(), inserts, "{sql}");
        assert!(sql.trim_end().ends_with("\nGO"), "closed at the end: {sql}");
        // The split a restore makes: one statement per batch.
        let mut splitter = crate::script::Splitter::new(SqlDialect::MsSql);
        let mut batches = splitter.push_str(&sql);
        batches.extend(splitter.finish());
        assert!(
            batches
                .iter()
                .all(|b| b.sql.matches("INSERT INTO").count() <= 1)
        );
        // No batches where the engine has none.
        let sql = render(SqlDialect::MySql);
        assert!(!sql.contains("GO"), "{sql}");
        assert_eq!(sql.matches("INSERT INTO").count(), 3);
    }

    /// **A SQL Server dump carries what a text cell cannot**: bytes — a
    /// `varbinary`, an `image`, and the CLR types `hierarchyid`, `geography`
    /// and `geometry`, whose serialisation keeps a geography's SRID and a
    /// geometry's Z and M — and a `sql_variant`'s base type. The grid's cell
    /// holds a `<n bytes>` placeholder for the first, which went into the file
    /// as `N'<22 bytes>'` (Msg 24114 on restore), and plain text for the second,
    /// so a `date` variant came back an `nvarchar` one. The dump's `SELECT` asks
    /// the server for each as SQL text instead.
    #[test]
    fn a_sql_server_dump_reads_bytes_and_variants_as_literals() {
        let mut t = table("t");
        t.schema = Some("dbo".to_string());
        for (name, ty) in [
            ("h", "hierarchyid"),
            ("g", "geography"),
            ("i", "image"),
            ("b", "varbinary(max)"),
            ("v", "sql_variant"),
            ("s", "nvarchar(10)"),
        ] {
            t.columns.push(ColumnInfo {
                name: name.to_string(),
                type_name: ty.to_string(),
                nullable: true,
                ..Default::default()
            });
        }
        let s = schema_of(vec![t]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        );
        let (select, server) = p
            .steps
            .iter()
            .find_map(|s| match s {
                DumpStep::Rows { select, server, .. } => Some((select.clone(), server.clone())),
                _ => None,
            })
            .expect("a rows step");
        for c in ["h", "g", "i", "b"] {
            assert!(
                select.contains(&format!(
                    "CASE WHEN [{c}] IS NULL THEN NULL ELSE '0x' + \
                     CONVERT(varchar(max), CAST([{c}] AS varbinary(max)), 2) END AS [{c}]"
                )),
                "{select}"
            );
        }
        assert!(
            select.contains("SQL_VARIANT_PROPERTY([v], 'BaseType')"),
            "{select}"
        );
        // A character variant is read whole, by the server's `FOR JSON`:
        // `CAST(v AS nvarchar(…))` stops at 4,000 characters however wide the
        // target, and a `varchar` variant holds up to 8,000.
        assert!(
            select.contains("(SELECT [v] AS x FOR JSON PATH, WITHOUT_ARRAY_WRAPPER)"),
            "{select}"
        );
        assert!(
            select.contains("[id], ") && select.contains(", [s] FROM"),
            "{select}"
        );
        use crate::export::ServerLiteral::{Hex, Variant};
        assert_eq!(
            server,
            vec![
                ("h".to_string(), Hex),
                ("g".to_string(), Hex),
                ("i".to_string(), Hex),
                ("b".to_string(), Hex),
                ("v".to_string(), Variant),
            ]
        );
        // Nothing of the kind on the other engines: their bytes stay withheld.
        let mut t = table("t");
        t.columns.push(ColumnInfo {
            name: "b".to_string(),
            type_name: "varbinary(10)".to_string(),
            ..Default::default()
        });
        let s = schema_of(vec![t]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        );
        assert_eq!(select_of(&p, "t"), "SELECT `id`, `b` FROM `shop`.`t`");
    }

    /// **A SQL Server file whose definitions hold `$(` says how to restore it
    /// with `sqlcmd`.** `sqlcmd` substitutes `$(name)` even inside a string
    /// literal, and a module body or a default is the server's text, which
    /// the file must restate as it is — so where one holds a `$(`, the header
    /// says to run the file with `-x`. The rows need no such word: their
    /// literals hold no `$(` (`export::script_literal`).
    #[test]
    fn a_definition_holding_a_sqlcmd_variable_is_named_in_the_header() {
        let header = |view_sql: &str| {
            let mut t = table("t");
            t.schema = Some("dbo".to_string());
            let mut v = table("v");
            v.schema = Some("dbo".to_string());
            v.is_view = true;
            v.create_sql = Some(view_sql.to_string());
            let s = schema_of(vec![t, v]);
            let p = plan(
                &s,
                "shop",
                &all(&s),
                DumpOptions::default(),
                SqlDialect::MsSql,
            );
            assert!(
                p.steps
                    .iter()
                    .any(|s| matches!(s, DumpStep::Text(t) if t.contains("CREATE VIEW"))),
                "the view is in the file: {:?}",
                p.steps
            );
            match &p.steps[0] {
                DumpStep::Text(h) => h.clone(),
                other => panic!("{other:?}"),
            }
        };
        let h = header("CREATE VIEW [dbo].[v] AS SELECT N'$(HOME)' AS x");
        assert!(h.contains("sqlcmd -x"), "{h}");
        // The sentence itself holds no reference for sqlcmd to trip on.
        assert!(!h.contains("$("), "{h}");
        let h = header("CREATE VIEW [dbo].[v] AS SELECT N'$ (HOME)' AS x");
        assert!(!h.contains("sqlcmd"), "{h}");
    }

    /// **And so does a file whose rows name a `$(`** — `sqlcmd` rewrites a
    /// bracketed name too (`[z$(HOME)t]` created `z/home/mssqlt`, measured on
    /// 2022). A data-only file has no `CREATE TABLE` for the check above to
    /// read; the header's own list of tables is what carries the name to it,
    /// which this pins.
    #[test]
    fn a_data_only_file_whose_table_name_holds_a_sqlcmd_variable_says_so() {
        let mut t = table("z$(HOME)t");
        t.schema = Some("dbo".to_string());
        let s = schema_of(vec![t]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions {
                structure: false,
                ..DumpOptions::default()
            },
            SqlDialect::MsSql,
        );
        assert!(
            p.steps.iter().any(|s| matches!(s, DumpStep::Rows { .. })),
            "{:?}",
            p.steps
        );
        let DumpStep::Text(h) = &p.steps[0] else {
            panic!("{:?}", p.steps[0])
        };
        assert!(h.contains("sqlcmd -x"), "{h}");
    }

    /// **A column typed by an alias is read as its base type.** Its
    /// `type_name` is the alias's own name, `[dbo].[Hash]`, so an alias over
    /// `binary` went into the file as the `NULL` a withheld blob is — which a
    /// `NOT NULL` alias refused on restore (Msg 515) — and one over
    /// `sql_variant` came back an `nvarchar` variant.
    #[test]
    fn a_column_of_an_alias_type_is_read_as_its_base() {
        use crate::export::ServerLiteral::{Hex, Variant};
        use crate::schema::{TsqlObject, TsqlObjectKind};
        let mut t = table("t");
        t.schema = Some("dbo".to_string());
        for (name, ty) in [
            ("h", "[dbo].[Hash]"),
            ("v", "[dbo].[Var]"),
            ("p", "[dbo].[Phone]"),
            ("o", "[other].[Hash]"),
        ] {
            t.columns.push(ColumnInfo {
                name: name.to_string(),
                type_name: ty.to_string(),
                ..Default::default()
            });
        }
        let mut s = schema_of(vec![t]);
        let alias = |schema: &str, name: &str, base: &str| TsqlObject {
            schema: Some(schema.to_string()),
            name: name.to_string(),
            kind: TsqlObjectKind::AliasType {
                base: base.to_string(),
                nullable: false,
            },
        };
        s.tsql_objects = vec![
            alias("dbo", "Hash", "binary(4)"),
            alias("dbo", "Var", "sql_variant"),
            alias("dbo", "Phone", "nvarchar(20)"),
            alias("other", "Hash", "varbinary(max)"),
        ];
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        );
        let server = p
            .steps
            .iter()
            .find_map(|s| match s {
                DumpStep::Rows { server, .. } => Some(server.clone()),
                _ => None,
            })
            .expect("a rows step");
        assert_eq!(
            server,
            vec![
                ("h".to_string(), Hex),
                ("v".to_string(), Variant),
                ("o".to_string(), Hex),
            ]
        );
    }

    /// **The server's text is checked before it is written as SQL.** Bytes
    /// must be `0x` and hex digits and are written bare; a variant is its base
    /// type — checked against the shape a type name has — and its value, which
    /// goes through the one literal quoter. Anything else is `NULL`, and named
    /// as withheld: a value a server supplies never reaches the file unquoted.
    #[test]
    fn server_rendered_literals_are_checked_and_quoted() {
        let text = |s: &str| crate::model::Value::Str(s.to_string());
        let rs = crate::model::ResultSet::from_rows(
            ["id", "b", "v"]
                .iter()
                .map(|n| crate::model::Column {
                    name: n.to_string(),
                    type_name: "nvarchar".to_string(),
                    origin: None,
                })
                .collect(),
            vec![
                vec![
                    crate::model::Value::Int(1),
                    text("0x0102"),
                    text("decimal(5,2)|12.50"),
                ],
                vec![
                    crate::model::Value::Int(2),
                    text("0x"),
                    text("varbinary(2)|0x0A0B"),
                ],
                vec![
                    crate::model::Value::Int(3),
                    crate::model::Value::Null,
                    text(r#"nvarchar(5) COLLATE Latin1_General_CI_AS|{"x":"it's"}"#),
                ],
                vec![
                    crate::model::Value::Int(4),
                    text("0x01); DROP TABLE t; --"),
                    text("int); DROP TABLE t; --|1"),
                ],
                vec![
                    crate::model::Value::Int(5),
                    text("0x0G"),
                    crate::model::Value::Null,
                ],
            ],
        );
        let order: Vec<usize> = (0..5).collect();
        let mut out = Vec::new();
        let tally = render_rows(
            &mut out,
            &mut crate::export::OneChunk::new(&rs, &order),
            ("shop", Some("dbo"), "t"),
            &[
                ("b".to_string(), crate::export::ServerLiteral::Hex),
                ("v".to_string(), crate::export::ServerLiteral::Variant),
            ],
            SqlDialect::MsSql,
        )
        .unwrap();
        let sql = String::from_utf8(out).unwrap();
        assert!(
            sql.contains("(1, 0x0102, CAST(CAST(N'12.50' AS decimal(5,2)) AS sql_variant))"),
            "{sql}"
        );
        assert!(
            sql.contains("(2, 0x, CAST(CAST(0x0A0B AS varbinary(2)) AS sql_variant))"),
            "{sql}"
        );
        assert!(
            sql.contains(
                "(3, NULL, CAST(CAST(N'it''s' COLLATE Latin1_General_CI_AS AS nvarchar(5)) \
                 AS sql_variant))"
            ),
            "{sql}"
        );
        assert!(sql.contains("(4, NULL, NULL)"), "{sql}");
        assert!(sql.contains("(5, NULL, NULL)"), "{sql}");
        assert!(!sql.contains("DROP TABLE"), "{sql}");
        assert_eq!(tally.withheld, vec!["b".to_string(), "v".to_string()]);
    }

    /// **A table the file cannot restate is left out of it and named, not
    /// written as a plain table.** A system-versioned or memory-optimised
    /// table's `CREATE` is a comment, so its `DROP` would destroy what the file
    /// cannot put back and its rows would land in nothing; a graph edge is
    /// restated, but its rows point at node ids the restore assigns afresh, so
    /// they are not carried. Each is said in the header.
    #[test]
    fn a_table_the_file_cannot_restate_is_named_and_left_out() {
        let mk = |name: &str, kind: crate::schema::TsqlTableKind| TableInfo {
            schema: Some("dbo".to_string()),
            tsql_kind: kind,
            ..table(name)
        };
        let s = schema_of(vec![
            mk(
                "hist",
                crate::schema::TsqlTableKind {
                    temporal_type: 2,
                    ..Default::default()
                },
            ),
            mk(
                "node",
                crate::schema::TsqlTableKind {
                    node: true,
                    ..Default::default()
                },
            ),
            mk(
                "edge",
                crate::schema::TsqlTableKind {
                    edge: true,
                    ..Default::default()
                },
            ),
            refs(mk("child", Default::default()), "hist"),
        ]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        );
        let file = file_of(&p);
        assert!(!file.contains("[dbo].[hist]"), "{file}");
        assert!(!file.contains("<<rows hist:"), "{file}");
        let header = text_of(&p);
        assert!(
            header.contains("1 table ticked for export is not in this file")
                && header.contains("dbo.hist (a system-versioned temporal table)"),
            "{header}"
        );
        assert!(
            header.contains("The rows of 1 graph edge table are not in this file"),
            "{header}"
        );
        // The key onto it is one the file cannot restate either.
        assert!(header.contains("1 foreign key is not restated"), "{header}");
        assert!(file.contains("CREATE TABLE [dbo].[node] (\n  [id] int NOT NULL\n) AS NODE;"));
        assert!(file.contains("<<rows node:"), "{file}");
        assert!(file.contains(") AS EDGE;"), "{file}");
        assert!(!file.contains("<<rows edge:"), "{file}");
        assert!(header.contains("dbo.edge"), "{header}");
        assert_eq!(p.tables, 3);
    }

    /// **A SQL Server dump creates the sequences, alias types, XML schema
    /// collections and synonyms its tables lean on.** None was read, so a
    /// table with a `NEXT VALUE FOR` default or an alias-typed column stopped
    /// the restore at its `CREATE TABLE` (Msg 208), and the header said
    /// nothing. They go in before the tables, one a chosen table names is
    /// dropped after them where the file drops up front (a sequence never is),
    /// and a sequence's counter is moved on where the file made it; one in a
    /// namespace the export does not cover is named instead.
    #[test]
    fn a_sql_server_dump_carries_the_objects_its_tables_name() {
        use crate::schema::{TsqlObject, TsqlObjectKind};
        let mut t = table("orders");
        t.schema = Some("dbo".to_string());
        t.columns[0].default = Some("NEXT VALUE FOR [dbo].[seq]".to_string());
        t.columns.push(ColumnInfo {
            name: "phone".to_string(),
            type_name: "[dbo].[Phone]".to_string(),
            ..Default::default()
        });
        t.columns.push(ColumnInfo {
            name: "other".to_string(),
            type_name: "int".to_string(),
            default: Some("NEXT VALUE FOR [Sequences].[OrderID]".to_string()),
            ..Default::default()
        });
        let mut s = schema_of(vec![t]);
        let seq = |schema: &str, name: &str| TsqlObject {
            schema: Some(schema.to_string()),
            name: name.to_string(),
            kind: TsqlObjectKind::Sequence {
                data_type: "int".to_string(),
                start: "1".to_string(),
                increment: "1".to_string(),
                min: "1".to_string(),
                max: "1000".to_string(),
                cycle: false,
                cache: None,
                last_used: Some("7".to_string()),
            },
        };
        s.tsql_objects = vec![
            seq("dbo", "seq"),
            seq("Sequences", "OrderID"),
            TsqlObject {
                schema: Some("dbo".to_string()),
                name: "Phone".to_string(),
                kind: TsqlObjectKind::AliasType {
                    base: "nvarchar(20)".to_string(),
                    nullable: true,
                },
            },
            TsqlObject {
                schema: Some("dbo".to_string()),
                name: "syn".to_string(),
                kind: TsqlObjectKind::Synonym {
                    target: vec!["dbo".to_string(), "orders".to_string()],
                },
            },
        ];
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        );
        let file = file_of(&p);
        let create_table = pos(&file, "CREATE TABLE [dbo].[orders]");
        assert!(
            pos(&file, "CREATE SEQUENCE [dbo].[seq]") < create_table,
            "{file}"
        );
        assert!(
            pos(&file, "CREATE TYPE [dbo].[Phone]") < create_table,
            "{file}"
        );
        assert!(
            pos(&file, "CREATE SYNONYM [dbo].[syn]") < create_table,
            "{file}"
        );
        // A sequence in a namespace with no table of the export's — as
        // WideWorldImporters keeps every key's — is carried, its schema made,
        // but only created where it is missing and never dropped: it is not
        // the file's to own.
        let schema_made = pos(&file, "IF SCHEMA_ID(N'Sequences') IS NULL");
        let outside = pos(
            &file,
            "IF OBJECT_ID(N'[Sequences].[OrderID]', N'SO') IS NULL EXEC(N'CREATE SEQUENCE",
        );
        assert!(schema_made < outside && outside < create_table, "{file}");
        assert!(
            !file.contains("DROP SEQUENCE IF EXISTS [Sequences]"),
            "{file}"
        );
        let drop_table = pos(&file, "DROP TABLE IF EXISTS [dbo].[orders];");
        // A sequence is never dropped — see
        // `a_replay_never_drops_a_sequence_or_an_object_its_tables_do_not_name`.
        assert!(!file.contains("DROP SEQUENCE"), "{file}");
        assert!(
            drop_table < pos(&file, "DROP TYPE IF EXISTS [dbo].[Phone];"),
            "{file}"
        );
        // Each counter goes on from where the source's was, beside its
        // `CREATE` — inside the `EXEC` that makes it only where missing, so a
        // replay that finds it there does not move it again.
        let moved = pos(&file, "@sequence_name = N''[dbo].[seq]'', @range_size = 7,");
        assert!(pos(&file, "CREATE SEQUENCE [dbo].[seq]") < moved, "{file}");
        assert!(moved < create_table, "{file}");
        let moved = pos(
            &file,
            "@sequence_name = N''[Sequences].[OrderID]'', @range_size = 7,",
        );
        assert!(outside < moved && moved < create_table, "{file}");
        let header = text_of(&p);
        assert!(!header.contains("outside this export"), "{header}");
        // Without its other objects, the file says what it leaves out.
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions {
                other_objects: false,
                ..Default::default()
            },
            SqlDialect::MsSql,
        );
        let header = text_of(&p);
        assert!(header.contains("Sequences.OrderID"), "{header}");
        assert!(!header.contains("CREATE SEQUENCE"), "{header}");
    }

    /// **An object outside the export that only a dumped routine names is
    /// carried too.** The routines come with their namespace whatever the
    /// tables name, and an alias type or XML schema collection in `dbo` that
    /// only an `s3` procedure's parameter used was neither carried nor named:
    /// the restore stopped at the procedure (Msg 2715, measured on 2022).
    #[test]
    fn an_object_only_a_dumped_routine_names_is_carried() {
        use crate::schema::{TsqlObject, TsqlObjectKind};
        let mut t = table("t");
        t.schema = Some("s3".to_string());
        let mut s = schema_of(vec![t]);
        let proc = |name: &str, arguments: &str, body: &str| {
            std::sync::Arc::new(crate::schema::RoutineInfo {
                name: name.to_string(),
                schema: Some("s3".to_string()),
                kind: crate::schema::RoutineKind::Procedure,
                language: "SQL".to_string(),
                arguments: arguments.to_string(),
                body: body.to_string(),
                ..Default::default()
            })
        };
        s.routines.push(proc(
            "p",
            "@h [dbo].[Hash], @x xml([dbo].[coll])",
            "SELECT @h",
        ));
        s.routines
            .push(proc("q", "", "SELECT NEXT VALUE FOR [dbo].[ctr]"));
        // Named nowhere, bare in a body: not carried.
        s.routines.push(proc("r", "", "SELECT other FROM t"));
        let obj = |name: &str, kind| TsqlObject {
            schema: Some("dbo".to_string()),
            name: name.to_string(),
            kind,
        };
        s.tsql_objects = vec![
            obj(
                "coll",
                TsqlObjectKind::XmlSchemaCollection {
                    definition: "<xsd:schema/>".to_string(),
                },
            ),
            obj(
                "Hash",
                TsqlObjectKind::AliasType {
                    base: "binary(4)".to_string(),
                    nullable: false,
                },
            ),
            obj(
                "ctr",
                TsqlObjectKind::Sequence {
                    data_type: "int".to_string(),
                    start: "1".to_string(),
                    increment: "1".to_string(),
                    min: "1".to_string(),
                    max: "9".to_string(),
                    cycle: false,
                    cache: None,
                    last_used: None,
                },
            ),
            obj(
                "other",
                TsqlObjectKind::Synonym {
                    target: vec!["x".to_string()],
                },
            ),
        ];
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        ));
        let procedure = pos(&file, "CREATE PROCEDURE [s3].[p]");
        for made in [
            "IF TYPE_ID(N'[dbo].[Hash]') IS NULL EXEC(",
            "WHERE name = N'coll' AND schema_id = SCHEMA_ID(N'dbo')) EXEC(",
            "IF OBJECT_ID(N'[dbo].[ctr]', N'SO') IS NULL EXEC(",
        ] {
            assert!(pos(&file, made) < procedure, "{made}: {file}");
        }
        assert!(!file.contains("[dbo].[other]"), "{file}");
    }

    /// **An alias type's bound default and rule are named, since its `CREATE
    /// TYPE` cannot carry them.** `sp_bindefault`/`sp_bindrule` bindings are
    /// restated nowhere, so a restored column of the type stored `NULL` where
    /// the source stores 7 and took a negative value its rule refused
    /// (measured on 2022), with nothing in the file saying so.
    #[test]
    fn an_alias_types_bound_default_and_rule_are_named_in_the_header() {
        use crate::schema::{TsqlObject, TsqlObjectKind, TsqlTypeBinding};
        let mut t = table("stock");
        t.schema = Some("dbo".to_string());
        t.columns[0].type_name = "[dbo].[Qty]".to_string();
        let mut s = schema_of(vec![t]);
        let alias = |name: &str| TsqlObject {
            schema: Some("dbo".to_string()),
            name: name.to_string(),
            kind: TsqlObjectKind::AliasType {
                base: "int".to_string(),
                nullable: true,
            },
        };
        s.tsql_objects = vec![alias("Qty"), alias("Plain")];
        s.tsql_type_bindings = vec![
            TsqlTypeBinding {
                schema: Some("dbo".to_string()),
                type_name: "Qty".to_string(),
                default: Some("dbo.df_seven".to_string()),
                rule: Some("dbo.rl_pos".to_string()),
            },
            // A type this file does not create says nothing.
            TsqlTypeBinding {
                schema: Some("archive".to_string()),
                type_name: "Old".to_string(),
                default: Some("archive.df".to_string()),
                rule: None,
            },
        ];
        let header = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        ));
        assert!(
            header.contains("1 alias type here carries a bound default or rule")
                && header.contains("dbo.Qty (default dbo.df_seven, rule dbo.rl_pos)"),
            "{header}"
        );
        assert!(!header.contains("archive.Old"), "{header}");
        // Without its other objects the file creates no type at all.
        let header = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions {
                other_objects: false,
                ..Default::default()
            },
            SqlDialect::MsSql,
        ));
        assert!(!header.contains("bound default"), "{header}");
    }

    /// **A replay drops no sequence, and nothing the chosen tables do not
    /// name.** Every object in an exported namespace was owned — dropped up
    /// front and recreated — so replaying an older one-table dump rewound a
    /// sequence other tables draw from, and their next insert took a key
    /// already used (Msg 2627, measured); one an unexported table's default
    /// names refused its drop instead (Msg 3729). A sequence carries state no
    /// script restates, so it is only ever created where it is missing, its
    /// counter moved on inside the same `EXEC`; an object no chosen table
    /// names is created the same way and left standing.
    #[test]
    fn a_replay_never_drops_a_sequence_or_an_object_its_tables_do_not_name() {
        use crate::schema::{TsqlObject, TsqlObjectKind};
        let mut t = table("orders");
        t.schema = Some("dbo".to_string());
        t.columns[0].default = Some("NEXT VALUE FOR [dbo].[seq]".to_string());
        t.columns.push(ColumnInfo {
            name: "phone".to_string(),
            type_name: "[dbo].[Phone]".to_string(),
            ..Default::default()
        });
        let mut s = schema_of(vec![t]);
        let seq = |name: &str| TsqlObject {
            schema: Some("dbo".to_string()),
            name: name.to_string(),
            kind: TsqlObjectKind::Sequence {
                data_type: "int".to_string(),
                start: "1".to_string(),
                increment: "1".to_string(),
                min: "1".to_string(),
                max: "1000".to_string(),
                cycle: false,
                cache: None,
                last_used: Some("2".to_string()),
            },
        };
        let alias = |name: &str| TsqlObject {
            schema: Some("dbo".to_string()),
            name: name.to_string(),
            kind: TsqlObjectKind::AliasType {
                base: "nvarchar(20)".to_string(),
                nullable: true,
            },
        };
        s.tsql_objects = vec![
            seq("seq"),
            seq("ctr"),
            alias("Phone"),
            alias("Other"),
            TsqlObject {
                schema: Some("dbo".to_string()),
                name: "syn".to_string(),
                kind: TsqlObjectKind::Synonym {
                    target: vec!["dbo".to_string(), "orders".to_string()],
                },
            },
        ];
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        ));
        assert!(!file.contains("DROP SEQUENCE"), "{file}");
        assert!(!file.contains("DROP SYNONYM"), "{file}");
        assert!(
            !file.contains("DROP TYPE IF EXISTS [dbo].[Other]"),
            "{file}"
        );
        // The one a chosen table names and a script restates whole is still
        // the file's to replace.
        let drop_phone = pos(&file, "DROP TYPE IF EXISTS [dbo].[Phone];");
        assert!(pos(&file, "DROP TABLE IF EXISTS [dbo].[orders];") < drop_phone);
        assert!(file.contains("\nCREATE TYPE [dbo].[Phone] FROM"), "{file}");
        let create_table = pos(&file, "CREATE TABLE [dbo].[orders]");
        for absent in [
            "IF OBJECT_ID(N'[dbo].[seq]', N'SO') IS NULL EXEC(N'CREATE SEQUENCE",
            "IF OBJECT_ID(N'[dbo].[ctr]', N'SO') IS NULL EXEC(N'CREATE SEQUENCE",
            "IF TYPE_ID(N'[dbo].[Other]') IS NULL EXEC(N'CREATE TYPE",
            "IF OBJECT_ID(N'[dbo].[syn]', N'SN') IS NULL EXEC(N'CREATE SYNONYM",
        ] {
            assert!(pos(&file, absent) < create_table, "{absent}\n{file}");
        }
        // The counter moves on only where the file made the sequence.
        assert!(
            file.contains("@sequence_name = N''[dbo].[seq]'', @range_size = 2,"),
            "{file}"
        );
    }

    /// The statement that stops a replay where something the file cannot put
    /// back is there: SQL Server's `THROW`, its name and message quoted as
    /// literals; nothing on the engines with no such tables.
    #[test]
    fn a_replay_is_stopped_by_name_where_the_object_is_there() {
        assert_eq!(
            refuse_if_present_sql(SqlDialect::MsSql, "[dbo].[it's]", "no 'way'").as_deref(),
            Some("IF OBJECT_ID(N'[dbo].[it''s]') IS NOT NULL THROW 50000, N'no ''way''', 1;")
        );
        for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            assert_eq!(refuse_if_present_sql(d, "t", "why"), None, "{d:?}");
        }
        assert_eq!(refused_note(&[]), "");
        let one = refused_note(&["dbo.knows (a graph edge table)".to_string()]);
        assert!(
            one.contains("already holds this stops before it drops anything")
                && one.contains("put it back as it is: dbo.knows (a graph edge table)."),
            "{one}"
        );
        let two = refused_note(&["a".to_string(), "b".to_string()]);
        assert!(
            two.contains("any of these") && two.ends_with("as they are: a, b."),
            "{two}"
        );
    }

    /// **The *Drop before create* toggle says what it drops.** On an engine
    /// that drops up front it also replaces every routine in the dumped
    /// schemas, used by the tables or not — a replay of an older dump puts
    /// older routine code in place of newer — while the toggle spoke of
    /// tables alone, and so did the file. Both say so now, computed from
    /// `drops_up_front` rather than the engine.
    #[test]
    fn the_drop_toggle_and_the_header_name_the_routines_a_replay_replaces() {
        for d in [
            SqlDialect::MySql,
            SqlDialect::Postgres,
            SqlDialect::Sqlite,
            SqlDialect::MsSql,
        ] {
            let hint = drop_before_create_hint(d);
            assert_eq!(hint.contains("routine"), drops_up_front(d), "{d:?}: {hint}");
            assert!(hint.contains("CREATE"), "{d:?}: {hint}");
        }
        let mut t = table("orders");
        t.schema = Some("dbo".to_string());
        let mut s = schema_of(vec![t]);
        s.routines
            .push(std::sync::Arc::new(crate::schema::RoutineInfo {
                name: "p_report".to_string(),
                schema: Some("dbo".to_string()),
                kind: crate::schema::RoutineKind::Procedure,
                body: "SELECT 1".to_string(),
                ..Default::default()
            }));
        let header = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        ));
        assert!(
            header.contains("replaces 1 routine in these schemas")
                && header.contains("dbo.p_report"),
            "{header}"
        );
        // Without the transaction no routine is dropped, and nothing is said.
        let opts = DumpOptions {
            wrap_transaction: false,
            ..Default::default()
        };
        let header = text_of(&plan(&s, "shop", &all(&s), opts, SqlDialect::MsSql));
        assert!(!header.contains("replaces"), "{header}");
    }

    /// **A replay never drops a graph table.** An edge's rows are not in the
    /// file, so replayed onto its source the edge was dropped and recreated
    /// empty — every relationship gone — and every node came back under a new
    /// node id, so an edge anywhere that pointed at one pointed at nothing;
    /// the run reported success. Neither is dropped now, with or without the
    /// transaction, and the file stops before it drops anything where one is
    /// there. A node with no edge anywhere loses nothing by its new ids.
    #[test]
    fn a_replay_does_not_drop_a_graph_table_and_stops_where_one_is_there() {
        let mk = |name: &str, node: bool, edge: bool| TableInfo {
            schema: Some("dbo".to_string()),
            tsql_kind: crate::schema::TsqlTableKind {
                node,
                edge,
                ..Default::default()
            },
            ..table(name)
        };
        let s = schema_of(vec![
            mk("person", true, false),
            mk("knows", false, true),
            mk("plain", false, false),
        ]);
        for wrap in [true, false] {
            let opts = DumpOptions {
                wrap_transaction: wrap,
                ..Default::default()
            };
            let p = plan(&s, "shop", &all(&s), opts, SqlDialect::MsSql);
            let file = file_of(&p);
            assert!(
                !file.contains("DROP TABLE IF EXISTS [dbo].[knows]"),
                "{file}"
            );
            assert!(
                !file.contains("DROP TABLE IF EXISTS [dbo].[person]"),
                "{file}"
            );
            let first_drop = pos(&file, "DROP TABLE IF EXISTS [dbo].[plain];");
            for obj in ["[dbo].[knows]", "[dbo].[person]"] {
                let stop = pos(
                    &file,
                    &format!("IF OBJECT_ID(N'{obj}') IS NOT NULL THROW 50000,"),
                );
                assert!(stop < first_drop, "{file}");
            }
            // Still created, for a restore into an empty database.
            assert!(file.contains(") AS EDGE;") && file.contains(") AS NODE;"));
            assert_eq!(p.refused.len(), 2, "{:?}", p.refused);
            assert!(
                p.refused
                    .iter()
                    .any(|r| r.starts_with("dbo.knows (a graph edge table")),
                "{:?}",
                p.refused
            );
            assert!(text_of(&p).contains("already holds any of these stops before it drops"));
        }
        // A node with no edge anywhere is an ordinary table to a replay.
        let lone = schema_of(vec![mk("person", true, false)]);
        let p = plan(
            &lone,
            "shop",
            &all(&lone),
            DumpOptions::default(),
            SqlDialect::MsSql,
        );
        assert!(file_of(&p).contains("DROP TABLE IF EXISTS [dbo].[person];"));
        assert!(p.refused.is_empty() && !file_of(&p).contains("THROW"));
        // A file that drops nothing refuses nothing.
        let opts = DumpOptions {
            drop_if_exists: false,
            ..Default::default()
        };
        let p = plan(&s, "shop", &all(&s), opts, SqlDialect::MsSql);
        assert!(p.refused.is_empty() && !file_of(&p).contains("THROW"));
    }

    /// **A replay drops no module it cannot restate whole.** The server keeps
    /// no text for an encrypted procedure, or for an encrypted member of a
    /// numbered group, so the file's "recreation" is a comment and a bare `;`
    /// — and the replay dropped both and reported success; a signed module
    /// comes back unsigned, every call that relied on the certificate failing
    /// on permissions. An encrypted one is left as it is (its `CREATE` is a
    /// comment, so nothing collides); the others stop the replay before it
    /// drops anything; an unreadable member is a comment that names it.
    #[test]
    fn a_replay_leaves_an_encrypted_routine_alone_and_refuses_one_it_cannot_restate() {
        let proc_ = |name: &str, body: &str| crate::schema::RoutineInfo {
            name: name.to_string(),
            schema: Some("dbo".to_string()),
            kind: crate::schema::RoutineKind::Procedure,
            body: body.to_string(),
            ..Default::default()
        };
        let mut t = table("t");
        t.schema = Some("dbo".to_string());
        let mut s = schema_of(vec![t]);
        let mut secret = proc_("secret", "");
        secret.tsql.hidden = true;
        let mut grp = proc_("grp", "SELECT 1");
        grp.tsql.numbered = vec![
            (2, String::new()),
            (3, "CREATE PROCEDURE dbo.grp;3 AS SELECT 3".to_string()),
        ];
        let mut signed = proc_("signed", "SELECT 2");
        signed.tsql.module.signed = true;
        for r in [secret, grp, signed, proc_("ok", "SELECT 4")] {
            s.routines.push(std::sync::Arc::new(r));
        }
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        );
        let file = file_of(&p);
        for kept in ["secret", "grp", "signed"] {
            assert!(
                !file.contains(&format!("DROP PROCEDURE IF EXISTS [dbo].[{kept}]")),
                "{kept}\n{file}"
            );
        }
        let first_drop = pos(&file, "DROP PROCEDURE IF EXISTS [dbo].[ok];");
        for refused in ["grp", "signed"] {
            let stop = pos(
                &file,
                &format!("IF OBJECT_ID(N'[dbo].[{refused}]') IS NOT NULL THROW 50000,"),
            );
            assert!(stop < first_drop, "{file}");
        }
        assert!(
            !file.contains("N'[dbo].[secret]') IS NOT NULL THROW"),
            "{file}"
        );
        assert!(
            file.contains("-- The definition of procedure grp;2 was not available"),
            "{file}"
        );
        assert!(!file.contains("\n;\nGO"), "a bare member: {file}");
        assert_eq!(p.refused.len(), 2, "{:?}", p.refused);
        assert!(
            p.refused.iter().any(|r| r.contains("grp;2")),
            "{:?}",
            p.refused
        );
        assert!(
            p.refused.iter().any(|r| r.contains("signed")),
            "{:?}",
            p.refused
        );
        let header = text_of(&p);
        assert!(
            header.contains("left as it is") && header.contains("dbo.secret"),
            "{header}"
        );
    }

    /// **A replay drops no table carrying a trigger it cannot restate, and no
    /// view the server shows no text for.** Dropping the table takes its
    /// triggers with it: an encrypted one came back as a comment, a signed
    /// one unsigned. An encrypted view's `CREATE` is a comment, so it is left
    /// as it is.
    #[test]
    fn a_replay_refuses_to_drop_a_table_whose_trigger_it_cannot_restate() {
        use crate::schema::{
            TriggerAction, TriggerEvent, TriggerInfo, TriggerLevel, TriggerTiming,
        };
        let trigger = |name: &str, table: &str| TriggerInfo {
            name: name.to_string(),
            schema: Some("dbo".to_string()),
            table: table.to_string(),
            timing: TriggerTiming::After,
            events: vec![TriggerEvent::Insert],
            level: TriggerLevel::Statement,
            action: TriggerAction::Body("SET NOCOUNT ON".to_string()),
            ..Default::default()
        };
        let mk = |name: &str, tr: Option<TriggerInfo>| {
            let mut t = table(name);
            t.schema = Some("dbo".to_string());
            t.triggers.extend(tr);
            t
        };
        let mut hidden = trigger("tr_hidden", "t1");
        hidden.tsql.hidden = true;
        hidden.action = TriggerAction::Body(String::new());
        let mut signed = trigger("tr_signed", "t2");
        signed.tsql.module.signed = true;
        let mut v = view("v_enc");
        v.schema = Some("dbo".to_string());
        v.view_definition = None;
        let mut o = crate::schema::ViewOptions::default();
        o.tsql.hidden = true;
        v.view_options = Some(o);
        let s = schema_of(vec![
            mk("t1", Some(hidden)),
            mk("t2", Some(signed)),
            mk("t3", Some(trigger("tr_ok", "t3"))),
            v,
        ]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        );
        let file = file_of(&p);
        assert!(!file.contains("DROP TABLE IF EXISTS [dbo].[t1]"), "{file}");
        assert!(!file.contains("DROP TABLE IF EXISTS [dbo].[t2]"), "{file}");
        assert!(
            !file.contains("DROP VIEW IF EXISTS [dbo].[v_enc]"),
            "{file}"
        );
        let first_drop = pos(&file, "DROP TABLE IF EXISTS [dbo].[t3];");
        // Asked of the table, which is what the file would drop.
        for t in ["t1", "t2"] {
            let stop = pos(
                &file,
                &format!("IF OBJECT_ID(N'[dbo].[{t}]') IS NOT NULL THROW 50000,"),
            );
            assert!(stop < first_drop, "{file}");
        }
        assert_eq!(p.refused.len(), 2, "{:?}", p.refused);
        assert!(
            p.refused
                .iter()
                .any(|r| r.starts_with("dbo.t2 (") && r.contains("tr_signed")),
            "{:?}",
            p.refused
        );
        assert!(text_of(&p).contains("dbo.v_enc"), "{}", text_of(&p));
    }

    /// **A SQL Server dump says how its dates are written, before any of them.**
    /// `datetime` reads `2026-01-02 …` by the session's language, so a restore
    /// by a `british` login swapped every day and month up to the 12th and
    /// failed on the 13th (Msg 242). Nothing of the kind on the other engines.
    #[test]
    fn a_sql_server_dump_pins_its_date_format_before_any_row() {
        let s = schema_of(vec![mssql_orders()]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        );
        let file = file_of(&p);
        let pin = pos(&file, "SET DATEFORMAT ymd;");
        assert!(pin < pos(&file, "CREATE TABLE"), "{file}");
        assert!(pin < pos(&file, "BEGIN TRANSACTION"), "{file}");
        assert!(pin < pos(&file, "<<rows orders:"), "{file}");
        // And the two settings a filtered or computed column's index needs,
        // which `sqlcmd` without `-I` opens with off (S6.2-L1-03).
        for s in ["SET ANSI_NULLS ON;", "SET QUOTED_IDENTIFIER ON;"] {
            assert!(pos(&file, s) < pos(&file, "CREATE TABLE"), "{s}: {file}");
        }
        for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            let s = schema_of(vec![table("orders")]);
            let file = file_of(&plan(&s, "shop", &all(&s), DumpOptions::default(), d));
            assert!(!file.contains("DATEFORMAT"), "{d:?}: {file}");
            assert!(!file.contains("QUOTED_IDENTIFIER"), "{d:?}: {file}");
        }
    }

    #[test]
    fn every_engine_can_be_dumped_to_a_sql_file() {
        for d in [
            SqlDialect::MySql,
            SqlDialect::Postgres,
            SqlDialect::Sqlite,
            SqlDialect::MsSql,
        ] {
            assert!(supports_dump(d), "{d:?}");
        }
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

    /// **And it is restated whole.** The FK section is where a PostgreSQL dump
    /// puts every key, so it is the path that decided whether a restored copy
    /// behaves like the original. A key declared
    /// `MATCH FULL DEFERRABLE INITIALLY DEFERRED` came back
    /// `MATCH SIMPLE NOT DEFERRABLE`: the first widens what the table accepts
    /// for a partially-NULL composite key, and the second turns a constraint the
    /// application relies on deferring into one checked at statement time — so
    /// inserts the original accepted are refused by a database that "restored
    /// fine".
    #[test]
    fn a_restored_foreign_key_keeps_its_match_and_deferrable_clauses() {
        let mut orders = refs(table("orders"), "customers");
        orders.foreign_keys[0].match_type = Some("FULL".to_string());
        orders.foreign_keys[0].deferrable = Some("DEFERRABLE INITIALLY DEFERRED".to_string());
        let s = schema_of(vec![orders, table("customers")]);
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        assert!(text.contains("MATCH FULL"), "{text}");
        assert!(text.contains("DEFERRABLE INITIALLY DEFERRED"), "{text}");
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
            WriteEnd::Failed {
                message: "disk full".to_string(),
                opened: true,
            },
            WriteEnd::Died("panic".to_string()),
        ] {
            assert!(
                matches!(
                    dump_verdict(ReadEnd::Cancelled, write.clone()),
                    DumpVerdict::Cancelled { .. }
                ),
                "{write:?}"
            );
        }

        // **And `partial` is the writer's answer here too**, not a constant —
        // the same correction `partial` got on the `Failed` arms, which this one
        // did not follow. A cancel that arrives before `File::create` leaves no
        // fragment, and the note pointed at one regardless.
        assert_eq!(
            dump_verdict(
                ReadEnd::Cancelled,
                WriteEnd::Failed {
                    message: "permission denied".to_string(),
                    opened: false,
                }
            ),
            DumpVerdict::Cancelled { partial: false }
        );
        assert_eq!(
            dump_verdict(
                ReadEnd::Cancelled,
                WriteEnd::Failed {
                    message: "disk full".to_string(),
                    opened: true,
                }
            ),
            DumpVerdict::Cancelled { partial: true }
        );
        // A writer that finished its stream, or died holding the file, had one.
        assert_eq!(
            dump_verdict(ReadEnd::Cancelled, WriteEnd::Wrote),
            DumpVerdict::Cancelled { partial: true }
        );
    }

    /// **The sentence follows the fact**, and it is the dump's own sentence
    /// rather than the result export's.
    ///
    /// A cancel during the schema read has created nothing — `fetch_schema`
    /// takes the token because it is the longest phase, and returns before the
    /// writer is spawned — so pointing at `shop.sql.part` sends the user to look
    /// for a file that is not there, on an arm they are unlikely to check twice.
    /// And `export_cancel_note` is not the answer either: it ends "the rows that
    /// were written are in …", which describes a result export and not a file of
    /// `CREATE TABLE`s that may hold no rows at all.
    #[test]
    fn a_cancelled_dump_names_a_fragment_only_when_there_is_one() {
        let with = cancel_note("shop.sql", true);
        assert!(with.contains("shop.sql.part"), "{with}");
        assert!(with.contains("was not changed"), "{with}");
        // Not the result export's wording: this file is not only rows.
        assert!(!with.contains("the rows that were written"), "{with}");

        let without = cancel_note("shop.sql", false);
        assert!(
            !without.contains(".part"),
            "a cancel that wrote nothing points at a fragment: {without}"
        );
        assert!(without.contains("was not changed"), "{without}");
        // The suffix comes from the one function that knows it, in the half that
        // has one.
        assert!(
            with.contains(&crate::export::part_path("shop.sql")),
            "{with}"
        );
    }

    /// **`partial` is a fact about the disk, not a constant.**
    ///
    /// It was hardcoded `true` for every failure while its own doc says it
    /// "means a `.part` fragment is on disk and worth naming". `File::create` is
    /// the writer's first statement, so a read-only folder, a full volume or a
    /// share that has just dropped fails before any fragment exists — and the
    /// note then read *"shop.sql was not changed; the rows that were written are
    /// in shop.sql.part"* about a file that had never been opened, sending the
    /// user to look for something that is not there.
    #[test]
    fn a_failure_before_the_part_was_opened_does_not_name_it() {
        assert_eq!(
            dump_verdict(
                ReadEnd::Clean,
                WriteEnd::Failed {
                    message: "Export failed: Access is denied. (os error 5)".to_string(),
                    opened: false,
                }
            ),
            DumpVerdict::Failed {
                message: "Export failed: Access is denied. (os error 5)".to_string(),
                partial: false,
            }
        );
        // And a failure *after* it still names it — otherwise "don't name the
        // fragment" is just a way of never naming one.
        assert_eq!(
            dump_verdict(
                ReadEnd::Clean,
                WriteEnd::Failed {
                    message: "Export failed: disk full".to_string(),
                    opened: true,
                }
            ),
            DumpVerdict::Failed {
                message: "Export failed: disk full".to_string(),
                partial: true,
            }
        );
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
                WriteEnd::Failed {
                    message: "Export failed: disk full".to_string(),
                    opened: true,
                }
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

    /// **"This database has no tables." was printed about a database nobody had
    /// managed to read.** The picker branched on `(listing, names.is_empty())`
    /// and had no third state, so an unreachable server or an account that
    /// cannot see the catalog produced the reassuring sentence — four lines
    /// above the footer's connection error, and it is the panel the user is
    /// reading. Exactly `ProbeSummary::CutOffBeforeFirst`'s bug, in the sibling
    /// modal that never got the fix.
    #[test]
    fn a_listing_that_failed_is_not_an_empty_database() {
        assert_eq!(picker_body(Listing::Failed, 0), PickerBody::Unreadable);
        assert_eq!(picker_body(Listing::Done, 0), PickerBody::NoTables);
    }

    /// The other three states are unchanged, so the classifier cannot pass by
    /// calling everything unreadable.
    #[test]
    fn the_picker_reads_then_offers_what_came_back() {
        assert_eq!(picker_body(Listing::Reading, 0), PickerBody::Reading);
        assert_eq!(
            picker_body(Listing::Reading, 9),
            PickerBody::Reading,
            "a stale list from the previous open is not what this modal is showing"
        );
        assert_eq!(picker_body(Listing::Done, 1), PickerBody::Tables);
        assert_eq!(picker_body(Listing::Done, 400), PickerBody::Tables);
    }

    /// Names beat the failure: it cannot arise today — the error arm leaves the
    /// list at the empty `Vec` the open reset it to — and offering them would
    /// still be the right answer if a partial read ever landed.
    #[test]
    fn a_failed_listing_that_still_has_names_offers_them() {
        assert_eq!(picker_body(Listing::Failed, 3), PickerBody::Tables);
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

    /// **A group's leader carries `PRECEDES <successor>`, and a file is read top
    /// to bottom.** The catalogue anchors the leader forwards because that is
    /// the right answer for the caller that *replaces* one trigger inside a
    /// group that already exists; a dump replays the whole set into nothing, so
    /// the first `CREATE TRIGGER` named a trigger the file had not created yet
    /// and both servers refused it — MySQL 8.4.11 `ERROR 3011`, MariaDB
    /// 10.11.14 `ERROR 4031`, *"Referenced trigger … does not exist"* — after
    /// the `DROP TABLE` above it had already run against the target. The dump
    /// itself reported success.
    ///
    /// Two triggers in one timing/event group is the ordinary case: it is *why*
    /// anyone writes `FOLLOWS`.
    #[test]
    fn a_dumped_trigger_group_names_nothing_the_file_has_not_created_yet() {
        use crate::schema::TriggerOrder;
        let trg = |name: &str, order: Option<TriggerOrder>| TriggerInfo {
            name: name.to_string(),
            table: "orders".to_string(),
            timing: TriggerTiming::Before,
            events: vec![TriggerEvent::Insert],
            action: TriggerAction::Body("SET NEW.id = 1".to_string()),
            order,
            ..Default::default()
        };
        let mut t = table("orders");
        t.triggers = vec![
            trg("t_a", Some(TriggerOrder::Precedes("t_b".to_string()))),
            trg("t_b", Some(TriggerOrder::Follows("t_a".to_string()))),
        ];
        let s = schema_of(vec![t]);
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MySql,
        ));
        let a_at = file
            .find("TRIGGER `t_a`")
            .unwrap_or_else(|| panic!("{file}"));
        let b_at = file
            .find("TRIGGER `t_b`")
            .unwrap_or_else(|| panic!("{file}"));
        assert!(a_at < b_at, "{file}");
        assert!(
            !file.contains("PRECEDES"),
            "the leader forward-references t_b:\n{file}"
        );
        // And the chain the file does carry rebuilds the order on its own.
        assert!(file.contains("FOLLOWS `t_a`"), "{file}");
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
        // The point here is the ` CASCADE`, and that the `DROP` names the table
        // its `CREATE` is about to make — `public` included, as every executed
        // name is: a bare one resolves through `search_path`, whose stock first
        // entry is `"$user"`, and dropped a schema named after the restoring
        // login's own `orders`.
        assert!(
            text.contains(r#"DROP TABLE IF EXISTS "public"."orders" CASCADE;"#),
            "{text}"
        );
        assert!(
            text.contains(r#"CREATE TABLE "public"."orders" ("#),
            "{text}"
        );
    }

    /// **SQL Server has neither a session FK switch nor `DROP … CASCADE`**, so
    /// a default dump replayed onto its source stopped at the first `DROP
    /// TABLE` of a referenced table (Msg 3726) — the tables are created
    /// parents first, so the child's key still stood. There the destructive
    /// half is a section of its own, before any `CREATE`: the keys between the
    /// dumped tables first (each only if it is there), then everything the
    /// file recreates in the reverse of its creation order — every view and
    /// table children-first, a routine nothing in the file calls ahead of
    /// them all.
    #[test]
    fn a_sql_server_dump_drops_everything_it_recreates_before_creating_any_of_it() {
        let mut parent = table("parent");
        parent.schema = Some("dbo".to_string());
        let mut child = refs(table("child"), "parent");
        child.schema = Some("dbo".to_string());
        let mut v = view("v");
        v.schema = Some("dbo".to_string());
        v.create_sql = Some("CREATE VIEW dbo.v AS SELECT 1 AS id".to_string());
        let mut s = schema_of(vec![child, parent, v]);
        s.routines
            .push(std::sync::Arc::new(crate::schema::RoutineInfo {
                name: "f".to_string(),
                schema: Some("dbo".to_string()),
                kind: crate::schema::RoutineKind::Function,
                arguments: "@x int".to_string(),
                returns: "int".to_string(),
                body: "BEGIN RETURN @x; END".to_string(),
                ..Default::default()
            }));
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        ));
        let drop_fk = pos(
            &file,
            "IF OBJECT_ID(N'[dbo].[fk_child_parent]', N'F') IS NOT NULL \
             ALTER TABLE [dbo].[child] DROP CONSTRAINT [fk_child_parent];",
        );
        let drop_view = pos(&file, "DROP VIEW IF EXISTS [dbo].[v];");
        let drop_child = pos(&file, "DROP TABLE IF EXISTS [dbo].[child];");
        let drop_parent = pos(&file, "DROP TABLE IF EXISTS [dbo].[parent];");
        let drop_fn = pos(&file, "DROP FUNCTION IF EXISTS [dbo].[f];");
        // `f` is called by nothing in the file, so it is created last and
        // dropped first.
        assert!(drop_fk < drop_fn && drop_fn < drop_view, "{file}");
        assert!(drop_view < drop_child && drop_child < drop_parent, "{file}");
        let first_create = pos(&file, "CREATE ");
        assert!(drop_parent < first_create, "{file}");
        assert_eq!(file.matches("DROP TABLE IF EXISTS").count(), 2, "{file}");
        // The engines with a switch or a CASCADE keep the drop beside its
        // `CREATE`.
        for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            let s = schema_of(vec![refs(table("child"), "parent"), table("parent")]);
            let file = file_of(&plan(&s, "shop", &all(&s), DumpOptions::default(), d));
            assert!(!file.contains("DROP CONSTRAINT"), "{d:?}: {file}");
            assert!(!file.contains("DROP FUNCTION"), "{d:?}: {file}");
        }
    }

    /// **Nothing is dropped up front without *One transaction*.** The section
    /// puts every `DROP` ahead of every `CREATE`, so a replay that failed at
    /// one of them — a key from a table outside the export (Msg 3726) — left
    /// every object dropped ahead of it gone and never recreated; only the
    /// transaction made the section safe to fail in. Without it each table and
    /// view is dropped beside its own `CREATE`, as on the other engines, and
    /// no routine or key is dropped at all.
    #[test]
    fn without_one_transaction_a_sql_server_file_drops_each_table_beside_its_create() {
        let mut parent = table("parent");
        parent.schema = Some("dbo".to_string());
        let mut child = refs(table("child"), "parent");
        child.schema = Some("dbo".to_string());
        let mut v = view("v");
        v.schema = Some("dbo".to_string());
        v.create_sql = Some("CREATE VIEW dbo.v AS SELECT 1 AS id".to_string());
        let mut s = schema_of(vec![child, parent, v]);
        s.routines
            .push(std::sync::Arc::new(crate::schema::RoutineInfo {
                name: "f".to_string(),
                schema: Some("dbo".to_string()),
                kind: crate::schema::RoutineKind::Function,
                arguments: "@x int".to_string(),
                returns: "int".to_string(),
                body: "BEGIN RETURN @x; END".to_string(),
                ..Default::default()
            }));
        let opts = DumpOptions {
            wrap_transaction: false,
            ..Default::default()
        };
        let file = file_of(&plan(&s, "shop", &all(&s), opts, SqlDialect::MsSql));
        assert!(!file.contains("-- Dropped first"), "{file}");
        assert!(!file.contains("DROP FUNCTION"), "{file}");
        assert!(!file.contains("DROP CONSTRAINT"), "{file}");
        let create_parent = pos(&file, "CREATE TABLE [dbo].[parent]");
        let drop_child = pos(&file, "DROP TABLE IF EXISTS [dbo].[child];");
        assert!(pos(&file, "DROP TABLE IF EXISTS [dbo].[parent];") < create_parent);
        assert!(create_parent < drop_child, "{file}");
        assert!(
            drop_child < pos(&file, "CREATE TABLE [dbo].[child]"),
            "{file}"
        );
        assert!(
            pos(&file, "DROP VIEW IF EXISTS [dbo].[v];") < pos(&file, "CREATE VIEW [dbo].[v]"),
            "{file}"
        );
    }

    /// **The up-front section drops in the mirror of the file's creation
    /// order.** A schema-bound function holds the tables it reads, so it has to
    /// go before them, and a table whose column calls it holds the function, so
    /// it has to go after that table: the routines dropped as one block after
    /// every table met the first edge (Msg 3729 on replay), and no single place
    /// for the block meets both. Each function is dropped at the mirror of the
    /// slot it is created in, and one nothing in the file calls — created
    /// last — goes first.
    #[test]
    fn a_sql_server_dump_drops_in_the_reverse_of_its_creation_order() {
        let f = |name: &str, body: &str| {
            std::sync::Arc::new(crate::schema::RoutineInfo {
                name: name.to_string(),
                schema: Some("dbo".to_string()),
                kind: crate::schema::RoutineKind::Function,
                returns: "int".to_string(),
                body: body.to_string(),
                ..Default::default()
            })
        };
        let mut t1 = table("t1");
        t1.schema = Some("dbo".to_string());
        let mut t2 = table("t2");
        t2.schema = Some("dbo".to_string());
        t2.columns.push(ColumnInfo {
            name: "c".to_string(),
            generated: Some("[dbo].[f_cnt]()".to_string()),
            ..Default::default()
        });
        let mut s = schema_of(vec![t1, t2]);
        s.routines.push(f(
            "f_cnt",
            "BEGIN RETURN (SELECT COUNT(*) FROM dbo.t1); END",
        ));
        s.routines
            .push(f("f_sb", "BEGIN RETURN (SELECT COUNT(*) FROM dbo.t1); END"));
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        ));
        let drop_t1 = pos(&file, "DROP TABLE IF EXISTS [dbo].[t1];");
        let drop_t2 = pos(&file, "DROP TABLE IF EXISTS [dbo].[t2];");
        let drop_cnt = pos(&file, "DROP FUNCTION IF EXISTS [dbo].[f_cnt];");
        let drop_sb = pos(&file, "DROP FUNCTION IF EXISTS [dbo].[f_sb];");
        assert!(drop_t2 < drop_cnt && drop_cnt < drop_t1, "{file}");
        assert!(drop_sb < drop_t2, "{file}");
        // The creation order it mirrors.
        let create_cnt = pos(&file, "CREATE FUNCTION [dbo].[f_cnt]");
        assert!(pos(&file, "CREATE TABLE [dbo].[t1]") < create_cnt, "{file}");
        assert!(create_cnt < pos(&file, "CREATE TABLE [dbo].[t2]"), "{file}");
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
        let (order, cycles) = order_tables(&s.tables, &all(&s), SqlDialect::MySql, None);
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
        let (order, _) = order_tables(&s.tables, &all(&s), SqlDialect::MySql, None);
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

    /// **A function a table or view calls is created before it.** SQL Server
    /// resolves a function at `CREATE TABLE`/`CREATE VIEW` time, so a computed
    /// column, a check, a default or a view calling one the file created later
    /// stopped the restore (Msg 4121); PostgreSQL resolves a default's and a
    /// view's function the same way. The function still comes after the tables
    /// it reads, and one nothing calls stays in the trailing section.
    #[test]
    fn a_function_a_table_or_view_calls_is_created_before_it() {
        let f = |name: &str, body: &str, schema: &str| {
            std::sync::Arc::new(crate::schema::RoutineInfo {
                name: name.to_string(),
                schema: Some(schema.to_string()),
                kind: crate::schema::RoutineKind::Function,
                language: "sql".to_string(),
                arguments: "@x int".to_string(),
                returns: "int".to_string(),
                body: body.to_string(),
                ..Default::default()
            })
        };
        // SQL Server: a computed column, a check and a view each call one.
        let mut base = table("base");
        base.schema = Some("dbo".to_string());
        let mut calc = table("calc");
        calc.schema = Some("dbo".to_string());
        calc.columns.push(ColumnInfo {
            name: "twice".to_string(),
            generated: Some("[dbo].[f_double]([id])".to_string()),
            ..Default::default()
        });
        calc.check_constraints.push(crate::schema::CheckInfo {
            name: "ck".to_string(),
            expression: "[dbo].[f_ok]([id])=(1)".to_string(),
            enforced: true,
            validated: true,
            inherited: false,
            column_level: false,
        });
        let mut v = view("v");
        v.schema = Some("dbo".to_string());
        v.create_sql =
            Some("CREATE VIEW dbo.v AS SELECT dbo.f_view(id) AS n FROM dbo.base".to_string());
        let mut s = schema_of(vec![base, calc, v]);
        s.routines
            .push(f("f_double", "BEGIN RETURN @x * 2; END", "dbo"));
        s.routines.push(f("f_ok", "BEGIN RETURN 1; END", "dbo"));
        // Reads `base`, so it waits for it, and the view waits for it.
        s.routines.push(f(
            "f_view",
            "BEGIN RETURN (SELECT COUNT(*) FROM dbo.base WHERE id = @x); END",
            "dbo",
        ));
        s.routines.push(f(
            "f_unused",
            "BEGIN RETURN (SELECT COUNT(*) FROM dbo.calc); END",
            "dbo",
        ));
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        ));
        let calc_at = pos(&file, "CREATE TABLE [dbo].[calc]");
        assert!(
            pos(&file, "CREATE FUNCTION [dbo].[f_double]") < calc_at,
            "{file}"
        );
        assert!(
            pos(&file, "CREATE FUNCTION [dbo].[f_ok]") < calc_at,
            "{file}"
        );
        let f_view = pos(&file, "CREATE FUNCTION [dbo].[f_view]");
        assert!(pos(&file, "CREATE TABLE [dbo].[base]") < f_view, "{file}");
        // Rebuilt under the catalogue's name, not restated as stored.
        assert!(f_view < pos(&file, "CREATE VIEW [dbo].[v]"), "{file}");
        assert!(
            pos(&file, "-- Routines and events") < pos(&file, "CREATE FUNCTION [dbo].[f_unused]"),
            "{file}"
        );

        // PostgreSQL: a table's default calls one that reads another table.
        let mut a = table("a_src");
        a.schema = Some("public".to_string());
        let mut b = table("b_user");
        b.schema = Some("public".to_string());
        b.columns[0].default = Some("next_code()".to_string());
        let mut s = schema_of(vec![a, b]);
        s.routines
            .push(f("next_code", "SELECT count(*) + 1 FROM a_src", "public"));
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        let func = pos(&file, "CREATE FUNCTION \"public\".\"next_code\"");
        assert!(
            pos(&file, "CREATE TABLE \"public\".\"a_src\"") < func,
            "{file}"
        );
        assert!(
            func < pos(&file, "CREATE TABLE \"public\".\"b_user\""),
            "{file}"
        );
    }

    /// **What reads a table the file leaves out is left out with it, and
    /// named.** A temporal table is not in the file; a view over it stopped
    /// the restore at its `CREATE VIEW` (Msg 208, measured on 2022) and *One
    /// transaction* rolled the whole file back — while the modal reported
    /// "Wrote 2 tables." So the view goes, the view over the view, and the
    /// inline function bound to it — each named in the header and on the
    /// plan — and a function that only reads it when it runs stays.
    #[test]
    fn what_reads_a_table_left_out_of_the_file_is_left_out_with_it() {
        let dbo = |mut t: TableInfo| {
            t.schema = Some("dbo".to_string());
            t
        };
        let mut emp = dbo(table("emp"));
        emp.tsql_kind.temporal_type = 2;
        let mut s = schema_of(vec![
            emp,
            dbo(table("dept")),
            // `sales.emp` is another table; a view over it stays.
            {
                let mut t = table("emp");
                t.schema = Some("sales".to_string());
                t
            },
            tsql_view(
                "emp_v",
                "CREATE VIEW dbo.emp_v AS SELECT id FROM [dbo].[emp]",
            ),
            tsql_view(
                "emp_vv",
                "CREATE VIEW dbo.emp_vv AS SELECT id FROM dbo.emp_v",
            ),
            tsql_view(
                "sales_v",
                "CREATE VIEW dbo.sales_v AS SELECT id FROM sales.emp",
            ),
            tsql_view(
                "dept_v",
                "CREATE VIEW dbo.dept_v AS SELECT id FROM dbo.dept",
            ),
            tsql_view(
                "v_tvf",
                "CREATE VIEW dbo.v_tvf AS SELECT id FROM dbo.tvf_emp()",
            ),
        ]);
        s.routines.push(tsql_function(
            "tvf_emp",
            "TABLE",
            "RETURN SELECT id FROM emp",
        ));
        s.routines.push(tsql_function(
            "f_count",
            "int",
            "BEGIN RETURN (SELECT COUNT(*) FROM dbo.emp); END",
        ));
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        );
        let file = file_of(&p);
        for gone in ["emp_v]", "emp_vv]", "v_tvf]", "tvf_emp]"] {
            assert!(
                !file.contains(&format!("CREATE VIEW [dbo].[{gone}")),
                "{file}"
            );
            assert!(
                !file.contains(&format!("CREATE FUNCTION [dbo].[{gone}")),
                "{file}"
            );
        }
        for kept in [
            "CREATE VIEW [dbo].[dept_v]",
            "CREATE VIEW [dbo].[sales_v]",
            "CREATE FUNCTION [dbo].[f_count]",
            "CREATE TABLE [sales].[emp]",
        ] {
            assert!(file.contains(kept), "{kept}: {file}");
        }
        assert_eq!(
            p.left_out,
            vec![
                "dbo.emp (a system-versioned temporal table)",
                "dbo.emp_v (a view reading dbo.emp, which is not in this file)",
                "dbo.emp_vv (a view reading dbo.emp_v, which is not in this file)",
                "dbo.tvf_emp (a function bound to dbo.emp, which is not in this file)",
                "dbo.v_tvf (a view reading dbo.tvf_emp, which is not in this file)",
            ],
        );
        assert!(
            file.contains("4 objects are left out with it")
                && file.contains("dbo.emp_vv (a view reading dbo.emp_v"),
            "{file}"
        );
        assert_eq!(p.tables, 4, "the tables and views the file holds");
    }

    /// **A ledger table, its history and its ledger view are left out and
    /// named**, as a temporal table is: written plain, the copy took updates
    /// and deletes with no trace and the restore reported success.
    #[test]
    fn a_ledger_table_its_history_and_its_view_are_left_out_and_named() {
        let dbo = |mut t: TableInfo, ledger_type: u8| {
            t.schema = Some("dbo".to_string());
            t.tsql_kind.ledger_type = ledger_type;
            t
        };
        let mut lv = tsql_view(
            "led_Ledger",
            "CREATE VIEW [dbo].[led_Ledger] AS SELECT 1 AS n",
        );
        lv.tsql_kind.ledger_view = true;
        let s = schema_of(vec![
            dbo(table("led"), 2),
            dbo(table("app_only"), 3),
            dbo(table("MSSQL_LedgerHistoryFor_1"), 1),
            dbo(table("plain"), 0),
            lv,
        ]);
        let p = plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        );
        let file = file_of(&p);
        assert!(file.contains("CREATE TABLE [dbo].[plain]"), "{file}");
        for name in [
            "led]",
            "app_only]",
            "MSSQL_LedgerHistoryFor_1]",
            "led_Ledger]",
        ] {
            assert!(
                !file.contains(&format!("CREATE TABLE [dbo].[{name}")),
                "{file}"
            );
            assert!(
                !file.contains(&format!("CREATE VIEW [dbo].[{name}")),
                "{file}"
            );
            assert!(
                !file.contains(&format!("<<rows {}", name.trim_end_matches(']'))),
                "{file}"
            );
        }
        for what in [
            "dbo.led (an updatable ledger table)",
            "dbo.app_only (an append-only ledger table)",
            "dbo.MSSQL_LedgerHistoryFor_1 (the history table of a ledger table)",
            "dbo.led_Ledger (the ledger view of a ledger table)",
        ] {
            assert!(
                p.left_out.iter().any(|l| l == what),
                "{what}: {:?}",
                p.left_out
            );
            assert!(file.contains(what), "{what}: {file}");
        }
        assert_eq!(p.tables, 1);
    }

    /// **A PostgreSQL check or key added `NOT VALID` goes back so, after the
    /// rows.** Inside `CREATE TABLE` PostgreSQL ignores the clause and
    /// validates the check, so the dump's own rows that violate it failed
    /// the restore (measured on 16), and the transaction rolled it all back;
    /// the key was read as an ordinary one, and its closing `ADD CONSTRAINT`
    /// validated the orphans the source had kept.
    #[test]
    fn a_postgres_not_valid_check_and_key_go_back_not_valid_after_the_rows() {
        let mut parent = table("parent");
        parent.schema = Some("public".to_string());
        let mut t = refs(table("t"), "parent");
        t.schema = Some("public".to_string());
        t.foreign_keys[0].not_validated = true;
        t.check_constraints = vec![
            crate::schema::CheckInfo {
                name: "c_pos".to_string(),
                expression: "(id > 0)".to_string(),
                validated: false,
                ..Default::default()
            },
            crate::schema::CheckInfo {
                name: "c_ok".to_string(),
                expression: "(id < 100)".to_string(),
                ..Default::default()
            },
        ];
        let s = schema_of(vec![parent, t]);
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        let create = pos(&file, "CREATE TABLE \"public\".\"t\"");
        let rows = pos(&file, "<<rows t:");
        let table_ddl = &file[create..rows];
        assert!(table_ddl.contains("CONSTRAINT \"c_ok\" CHECK"), "{file}");
        assert!(!table_ddl.contains("c_pos"), "{file}");
        let held = pos(&file, "ADD CONSTRAINT \"c_pos\" CHECK ((id > 0)) NOT VALID");
        assert!(rows < held, "{file}");
        assert!(file.contains("\"parent\" (\"id\") NOT VALID;"), "{file}");
        // Copy DDL has no rows to wait for, and keeps the check inline.
        assert!(
            s.tables[1]
                .create_ddl(SqlDialect::Postgres)
                .contains("CONSTRAINT \"c_pos\" CHECK ((id > 0)) NOT VALID"),
        );
        // The engines whose inline form holds keep it there.
        for d in [SqlDialect::MySql, SqlDialect::Sqlite] {
            assert!(!crate::schema::unvalidated_check_waits_for_rows(d));
            assert!(!crate::schema::writes_not_valid(d));
        }
        assert!(crate::schema::unvalidated_check_waits_for_rows(
            SqlDialect::MsSql
        ));
        assert!(!crate::schema::writes_not_valid(SqlDialect::MsSql));
    }

    /// **The modal names what the file left out, and one space separates its
    /// sentences.** The report was "Wrote 2 tables." over a file a table
    /// short, and its `format!` put a space after the tally whatever
    /// followed, so a quiet dump read "Wrote 2 tables. " and one with a
    /// missing table had two spaces in it.
    #[test]
    fn the_done_note_says_what_the_file_is_short_of_in_single_spaced_sentences() {
        let none: [String; 0] = [];
        assert_eq!(
            done_note(2, None, &none, &none, &none, &none),
            "Wrote 2 tables."
        );
        assert_eq!(
            done_note(1, Some(""), &none, &none, &none, &none),
            "Wrote 1 table."
        );
        let note = done_note(
            2,
            Some("Binary column [b] was exported as NULL."),
            &["dbo.gone".to_string()],
            &["dbo.emp (a system-versioned temporal table)".to_string()],
            &["dbo.knows".to_string()],
            &["dbo.p (signed, and no script can carry the signature)".to_string()],
        );
        assert!(!note.contains("  ") && !note.ends_with(' '), "{note}");
        for part in [
            "Wrote 2 tables. Binary column [b] was exported as NULL.",
            "1 ticked table not found and is not in the file: dbo.gone.",
            "1 object the export asked for is not in the file: dbo.emp (a system-versioned \
             temporal table).",
            "The rows of graph edge table dbo.knows are not in the file; it is created empty.",
            "A replay onto a database that already holds this stops before it drops anything",
        ] {
            assert!(note.contains(part), "{part}: {note}");
        }
        let missing_only = done_note(3, None, &["a".to_string()], &none, &none, &none);
        assert_eq!(
            missing_only,
            "Wrote 3 tables. 1 ticked table not found and is not in the file: a."
        );
    }

    /// A SQL Server function in `dbo`, for the ordering tests below.
    fn tsql_function(
        name: &str,
        returns: &str,
        body: &str,
    ) -> std::sync::Arc<crate::schema::RoutineInfo> {
        std::sync::Arc::new(crate::schema::RoutineInfo {
            name: name.to_string(),
            schema: Some("dbo".to_string()),
            kind: crate::schema::RoutineKind::Function,
            language: "SQL".to_string(),
            returns: returns.to_string(),
            body: body.to_string(),
            ..Default::default()
        })
    }

    /// `view(name)` in `dbo`, its stored statement `create`.
    fn tsql_view(name: &str, create: &str) -> TableInfo {
        let mut v = view(name);
        v.schema = Some("dbo".to_string());
        v.create_sql = Some(create.to_string());
        v
    }

    /// **Tables, views and the functions they call are ordered as one
    /// graph.** The table and view order was fixed first, by foreign keys,
    /// view-to-view mentions and name, and a called function was fitted in
    /// afterwards — on its caller's side where the two could not both hold.
    /// So a view reaching another view only through a function came out
    /// ahead of it, and the inline function between them, which binds at
    /// `CREATE`, stopped the restore (Msg 208, measured on 2022 and 2025).
    /// The names sort the caller first, so only a walk over the function's
    /// edges can produce this file.
    #[test]
    fn a_view_reached_through_a_function_comes_after_what_the_function_reads() {
        let mut t = table("t");
        t.schema = Some("dbo".to_string());
        let mut s = schema_of(vec![
            t,
            tsql_view("v_a", "CREATE VIEW dbo.v_a AS SELECT n FROM dbo.tvf_b()"),
            tsql_view("v_z", "CREATE VIEW dbo.v_z AS SELECT id AS n FROM dbo.t"),
        ]);
        s.routines.push(tsql_function(
            "tvf_b",
            "TABLE",
            "RETURN SELECT n FROM dbo.v_z",
        ));
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        ));
        let (v_z, tvf, v_a) = (
            pos(&file, "CREATE VIEW [dbo].[v_z]"),
            pos(&file, "CREATE FUNCTION [dbo].[tvf_b]"),
            pos(&file, "CREATE VIEW [dbo].[v_a]"),
        );
        assert!(v_z < tvf && tvf < v_a, "{file}");
        // And a replay drops them in the mirror of that order.
        let drop =
            |kw: &str, name: &str| pos(&file, &format!("DROP {kw} IF EXISTS [dbo].[{name}]"));
        assert!(
            drop("VIEW", "v_a") < drop("FUNCTION", "tvf_b")
                && drop("FUNCTION", "tvf_b") < drop("VIEW", "v_z"),
            "{file}"
        );
    }

    /// **And a table waits for the table its check's function reads.** The
    /// function itself may come first (deferred name resolution allows it),
    /// but the check runs it against every restored row, so `a_orders`' rows
    /// stopped the restore while `z_customers`, which the function counts,
    /// was created after them (Msg 208).
    #[test]
    fn a_table_whose_check_calls_a_function_comes_after_what_the_function_reads() {
        let mut customers = table("z_customers");
        customers.schema = Some("dbo".to_string());
        let mut orders = table("a_orders");
        orders.schema = Some("dbo".to_string());
        orders.check_constraints.push(crate::schema::CheckInfo {
            name: "ck_known".to_string(),
            expression: "[dbo].[f_known]([id])=(1)".to_string(),
            ..Default::default()
        });
        let mut s = schema_of(vec![orders, customers]);
        s.routines.push(tsql_function(
            "f_known",
            "int",
            "BEGIN RETURN (SELECT COUNT(*) FROM dbo.z_customers WHERE id = @id); END",
        ));
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        ));
        let (customers, f, orders) = (
            pos(&file, "CREATE TABLE [dbo].[z_customers]"),
            pos(&file, "CREATE FUNCTION [dbo].[f_known]"),
            pos(&file, "CREATE TABLE [dbo].[a_orders]"),
        );
        assert!(customers < f && f < orders, "{file}");
        assert!(pos(&file, "<<rows z_customers") < orders, "{file}");
    }

    /// **Where nothing calls a function, the order is the one it always
    /// was** — every table first by its keys and name, every view after
    /// them — so two dumps of an unchanged schema stay byte-identical. And a
    /// function reading a table whose own column calls it is still the one
    /// cycle there is: the caller wins, being the statement that would fail.
    #[test]
    fn the_one_sort_keeps_the_old_order_and_breaks_a_real_cycle_at_the_function() {
        let dbo = |mut t: TableInfo| {
            t.schema = Some("dbo".to_string());
            t
        };
        let mut s = schema_of(vec![
            dbo(refs(table("b_child"), "c_parent")),
            dbo(table("c_parent")),
            dbo(table("a_other")),
            tsql_view("v_two", "CREATE VIEW dbo.v_two AS SELECT id FROM dbo.v_one"),
            tsql_view(
                "v_one",
                "CREATE VIEW dbo.v_one AS SELECT id FROM dbo.a_other",
            ),
        ]);
        // A function nothing calls, which reads every table: it stays in the
        // trailing section and moves nothing.
        s.routines.push(tsql_function(
            "f_all",
            "int",
            "BEGIN RETURN (SELECT COUNT(*) FROM dbo.a_other, dbo.b_child, dbo.v_two); END",
        ));
        let plain = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        ));
        let at: Vec<usize> = [
            "TABLE [dbo].[a_other]",
            "TABLE [dbo].[c_parent]",
            "TABLE [dbo].[b_child]",
            "VIEW [dbo].[v_one]",
            "VIEW [dbo].[v_two]",
            "-- Routines and events",
            "FUNCTION [dbo].[f_all]",
        ]
        .iter()
        .map(|n| match n.strip_prefix("-- ") {
            Some(_) => pos(&plain, n),
            None => pos(&plain, &format!("CREATE {n}")),
        })
        .collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "{plain}");

        // `calc`'s column calls `f_count`, which reads `calc`.
        let mut calc = dbo(table("calc"));
        calc.columns.push(ColumnInfo {
            name: "n".to_string(),
            generated: Some("[dbo].[f_count]()".to_string()),
            ..Default::default()
        });
        s.tables.push(calc);
        s.routines.push(tsql_function(
            "f_count",
            "int",
            "BEGIN RETURN (SELECT COUNT(*) FROM dbo.calc); END",
        ));
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::MsSql,
        ));
        assert!(
            pos(&file, "CREATE FUNCTION [dbo].[f_count]") < pos(&file, "CREATE TABLE [dbo].[calc]"),
            "{file}"
        );
        // The rest keeps its order around them.
        let rest: Vec<usize> = ["a_other", "c_parent", "b_child"]
            .iter()
            .map(|n| pos(&file, &format!("CREATE TABLE [dbo].[{n}]")))
            .collect();
        assert!(rest.windows(2).all(|w| w[0] < w[1]), "{file}");
    }

    /// **And after the routines they call.**
    ///
    /// The table→routine edge was ordered; the routine→routine edge was not.
    /// `objects_where` is a `filter().cloned()` over the catalogue vector — no
    /// sort, no walk — so two `LANGUAGE sql` functions came out in whatever
    /// order the catalogue held them, and with the caller first the restore
    /// stops at `ERROR: function b_base() does not exist` *after* the file's
    /// `DROP TABLE`s have run against the target. The same
    /// `check_function_bodies` fact the table ordering already rests on.
    ///
    /// The names are chosen so catalogue order and name order both put the
    /// caller first: only a dependency walk can produce the right file.
    #[test]
    fn a_routine_comes_after_the_routine_it_calls() {
        let mut t = table("orders");
        t.schema = Some("public".to_string());
        let mut s = schema_of(vec![t]);
        for (name, body) in [("a_total", "SELECT b_base()"), ("b_base", "SELECT 1")] {
            s.routines
                .push(std::sync::Arc::new(crate::schema::RoutineInfo {
                    name: name.to_string(),
                    schema: Some("public".to_string()),
                    kind: crate::schema::RoutineKind::Function,
                    language: "sql".to_string(),
                    body: body.to_string(),
                    ..Default::default()
                }));
        }
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        let callee = file.find("b_base").expect("the callee's CREATE");
        let caller = file.find("a_total").expect("the caller's CREATE");
        assert!(callee < caller, "the caller was emitted first:\n{file}");
    }

    /// **A type the file does not create is named in the header, the way a
    /// dropped foreign key is.**
    ///
    /// `plan` emits only objects in the chosen tables' namespaces, so a `public`
    /// enum used by a `sales` column is absent — and the `CREATE TABLE` that
    /// follows declares the column with it, so a restore onto a fresh server
    /// stops at `ERROR: type "order_status" does not exist` before any data
    /// lands. Strictly louder than the dropped-key case that *did* have a
    /// sentence: that is a constraint the restore survives without.
    #[test]
    fn a_type_left_outside_the_export_is_named_in_the_header() {
        let mut t = table("orders");
        t.schema = Some("sales".to_string());
        t.columns[0].type_name = "order_status".to_string();
        let mut s = schema_of(vec![t]);
        s.enums.push(crate::schema::EnumInfo {
            name: "order_status".to_string(),
            schema: Some("public".to_string()),
            values: vec!["new".to_string(), "paid".to_string()],
            ..Default::default()
        });
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        assert!(
            file.contains("public.order_status"),
            "the header says nothing about the type it left behind:\n{file}"
        );
        // The file really does not create it — the sentence is about a gap, not
        // a change of what is emitted.
        assert!(!file.contains("CREATE TYPE"), "{file}");

        // **And a data-only dump is not that gap.** The sentence says "the
        // CREATE TABLE statements name it", and a data-only file has none: it
        // is `INSERT`s, and a restore into an existing database needs the type
        // no more and no less than the target already has. Telling the user to
        // go and satisfy a dependency the file does not have is the same shape
        // as the `.part` that was never written. Its sibling sentence one block
        // up is gated by construction, because `dropped_fks` is only
        // incremented inside the structure step; this one was computed above it.
        let data_only = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions {
                structure: false,
                data: true,
                ..DumpOptions::default()
            },
            SqlDialect::Postgres,
        ));
        assert!(
            !data_only.contains("outside this export"),
            "a file with no CREATE TABLE claims its CREATE TABLE statements name \
             a type:\n{data_only}"
        );
        // The premise, so this cannot pass by the dump being empty: the file has
        // a row step for the table and no `CREATE TABLE` for it.
        assert!(data_only.contains("rows orders"), "{data_only}");
        assert!(!data_only.contains("CREATE TABLE"), "{data_only}");
    }

    /// **…and it does not name an object the file emits a local twin of.**
    ///
    /// The census matched `o.name()` bare against the column's type and default,
    /// with the namespace added afterwards for the sentence only — so with
    /// `sales.status` and `archive.status` both in the catalogue, a `sales` table
    /// whose column is typed `status` reported `archive.status` as an outside
    /// dependency. An unqualified type resolves through the search path, and this
    /// file creates `sales.status`, so the mention is satisfied.
    ///
    /// **The direction matters.** Over-reporting costs a false line in a header;
    /// under-reporting lets a restore stop on a type the file never mentioned.
    /// So the suppression is narrow: only a *bare* mention, and only when this
    /// dump itself emits an object of that name and kind. A qualified mention
    /// still names the object it qualifies, and a bare mention with no local twin
    /// is still reported.
    #[test]
    fn an_outside_object_shadowed_by_a_local_one_is_not_a_dependency() {
        let mut t = table("orders");
        t.schema = Some("sales".to_string());
        t.columns[0].type_name = "status".to_string();
        let mut s = schema_of(vec![t]);
        // The one the file will create…
        s.enums.push(crate::schema::EnumInfo {
            name: "status".to_string(),
            schema: Some("sales".to_string()),
            values: vec!["new".to_string()],
            ..Default::default()
        });
        // …and a same-named one in a namespace this dump has no business in.
        s.enums.push(crate::schema::EnumInfo {
            name: "status".to_string(),
            schema: Some("archive".to_string()),
            values: vec!["old".to_string()],
            ..Default::default()
        });
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        assert!(
            file.contains("CREATE TYPE"),
            "the premise: the file emits the local twin\n{file}"
        );
        assert!(
            !file.contains("archive.status"),
            "the header names an object the file emits a local twin of:\n{file}"
        );

        // And the narrowness, in both directions. A *qualified* mention of the
        // out-of-namespace object is still a dependency…
        let mut t2 = table("orders");
        t2.schema = Some("sales".to_string());
        t2.columns[0].type_name = "archive.status".to_string();
        let mut s2 = schema_of(vec![t2]);
        s2.enums = s.enums.clone();
        let file2 = file_of(&plan(
            &s2,
            "shop",
            &all(&s2),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        assert!(
            file2.contains("archive.status"),
            "a qualified mention names the object it qualifies:\n{file2}"
        );

        // …and a bare mention with no local twin is still reported, which is the
        // case this must not have broken.
        let mut t3 = table("orders");
        t3.schema = Some("sales".to_string());
        t3.columns[0].type_name = "status".to_string();
        let mut s3 = schema_of(vec![t3]);
        s3.enums.push(crate::schema::EnumInfo {
            name: "status".to_string(),
            schema: Some("archive".to_string()),
            values: vec!["old".to_string()],
            ..Default::default()
        });
        let file3 = file_of(&plan(
            &s3,
            "shop",
            &all(&s3),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        assert!(
            file3.contains("archive.status"),
            "a bare mention with nothing local to satisfy it is still a gap:\n{file3}"
        );
    }

    /// And it is a *whole identifier* match, on the two places a column names
    /// one: the declared type and the default expression.
    #[test]
    fn the_outside_dependency_census_matches_whole_identifiers_only() {
        assert!(names_identifier("order_status", "order_status"));
        assert!(names_identifier("public.order_status", "order_status"));
        assert!(names_identifier(
            "nextval('public.order_seq'::regclass)",
            "order_seq"
        ));
        assert!(!names_identifier("order_status_v2", "order_status"));
        assert!(!names_identifier("my_order_status", "order_status"));
        assert!(!names_identifier("anything", ""));
    }

    /// And a name only *mentioned* is not an edge — the same rule the view walk
    /// keeps, asked of the routine bodies.
    #[test]
    fn a_routine_name_that_is_only_mentioned_is_not_a_dependency() {
        let mut t = table("orders");
        t.schema = Some("public".to_string());
        let mut s = schema_of(vec![t]);
        for (name, body) in [
            (
                "a_first",
                "-- unrelated to z_other\nSELECT 'z_other', z_other_backup FROM orders",
            ),
            ("z_other", "SELECT 1"),
        ] {
            s.routines
                .push(std::sync::Arc::new(crate::schema::RoutineInfo {
                    name: name.to_string(),
                    schema: Some("public".to_string()),
                    kind: crate::schema::RoutineKind::Function,
                    language: "sql".to_string(),
                    body: body.to_string(),
                    ..Default::default()
                }));
        }
        let file = file_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Postgres,
        ));
        // No edge, so the catalogue order stands.
        let first = file.find("a_first").expect("the first routine");
        let other = file
            .find("FUNCTION z_other")
            .or_else(|| file.find("z_other()"));
        assert!(
            other.is_none_or(|at| first < at),
            "a mention reordered the file:\n{file}"
        );
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
        // The table is named with its namespace, `public` included: a bare
        // name resolves through `search_path`, so a schema named after the
        // login holding an `orders` had *its* counter set from *its* rows.
        assert!(
            file.contains(
                "pg_get_serial_sequence('\"public\".\"orders\"', 'id') AS s, \
                 (SELECT MAX(\"id\") FROM \"public\".\"orders\")"
            ),
            "{file}"
        );
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

    /// A key carried **inside** a verbatim `CREATE TABLE` still points outside
    /// the export, and the header has to say so — the one engine where the
    /// restore succeeds is the one where nothing warned.
    ///
    /// SQLite writes its keys inside the table's own DDL, so `needs_fk_section`
    /// is false and `plan` used to `continue` before the accounting, leaving
    /// `dropped_fks` at 0 and the header silent. Replayed against SQLite
    /// 3.53.2 the load is clean — `PRAGMA foreign_keys = ON` does not validate
    /// existing rows — and every later write to the table fails with
    /// *no such table: main.customers*.
    #[test]
    fn a_verbatim_key_pointing_out_of_the_export_is_still_reported() {
        let mut orders = refs(table("orders"), "customers");
        orders.create_sql = Some(
            "CREATE TABLE \"orders\" (id INTEGER, cust INTEGER REFERENCES customers(id))"
                .to_string(),
        );
        let s = schema_of(vec![orders]);
        let text = text_of(&plan(
            &s,
            "shop",
            &all(&s),
            DumpOptions::default(),
            SqlDialect::Sqlite,
        ));
        assert!(
            text.contains("outside this export"),
            "the header says nothing about a key that will dangle: {text}"
        );
        // And it is *not* the restatable engines' sentence: nothing was
        // dropped here, so promising that the constraint is gone would be the
        // opposite lie.
        assert!(!text.contains("not restated"), "{text}");
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
