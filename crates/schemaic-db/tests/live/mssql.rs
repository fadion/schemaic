//! SQL Server's live leg, on its own terms.
//!
//! **Outside [`crate::suite`] and the `live_suite!` macro**, because the shared
//! suite pins sessions, which SQL Server refuses until Manual mode is written,
//! and its write-back and DDL cases stand on the shared suite's `Target`, which
//! this leg does not have. What is here is what the engine *does* do —
//! connecting, the read path and its values, provenance, the read-only guard,
//! validation, the schema, activity, the grid's write-back, the table designer
//! — plus the two catalogues only it has: its builtin functions, and its DMV
//! snippets.
//!
//! | variable | default |
//! |---|---|
//! | `SCHEMAIC_IT_MSSQL_HOST` / `_PORT` / `_USER` / `_PASSWORD` | `127.0.0.1` / `1433` / `sa` / `Schemaic_2026` |
//!
//! The default password is the local test container's (`docker run … -e
//! MSSQL_SA_PASSWORD=Schemaic_2026 mcr.microsoft.com/mssql/server:2022-latest`)
//! and CI's service container's. `SCHEMAIC_IT_ENGINES=mssql` runs this leg
//! alone.
//!
//! Each test makes its own `schemaic_it_*` database and drops it, under the same
//! name guard as every other leg ([`crate::scratch::assert_scratch_name`]).

use std::sync::Arc;

use schemaic_core::blob::BlobRef;
use schemaic_core::intel::SqlDialect;
use schemaic_core::model::{
    CellEdit, GridWrite, RefetchRow, RefetchTemplate, ResultSet, RowDelete, RowEdit, RowInsert,
    Value,
};
use schemaic_db::{Db, DbError, Enforce, Engine};
use tokio_util::sync::CancellationToken;

use crate::endpoint;
use crate::scratch::{PREFIX, assert_scratch_name};

const MS: SqlDialect = SqlDialect::MsSql;

fn var(field: &str, default: &str) -> String {
    match std::env::var(format!("SCHEMAIC_IT_MSSQL_{field}")) {
        Ok(v) if !v.is_empty() => v,
        _ => default.to_string(),
    }
}

/// The server, attached to no database.
fn base_db() -> Db {
    let port = var("PORT", "1433");
    Db::from_parts(
        Engine::MsSql,
        var("HOST", "127.0.0.1"),
        port.parse()
            .unwrap_or_else(|_| panic!("SCHEMAIC_IT_MSSQL_PORT is not a port number: {port:?}")),
        var("USER", "sa"),
        var("PASSWORD", "Schemaic_2026"),
        String::new(),
    )
}

/// A scratch database for one test, dropped when it goes out of scope —
/// including when the test panics, which is when a leftover is likeliest.
struct Scratch {
    name: String,
    db: Db,
}

impl Scratch {
    async fn create(test: &str) -> Scratch {
        let name = format!("{PREFIX}{}_mssql_{test}", std::process::id());
        assert_scratch_name(&name);
        assert!(name.len() <= 128, "{name:?} is too long a database name");
        let base = base_db();
        base.fetch_query(
            None,
            &format!("CREATE DATABASE [{name}]"),
            1,
            CancellationToken::new(),
        )
        .await
        .unwrap_or_else(|e| panic!("could not create {name}: {e}"));
        let db = base.clone().with_database(Some(&name));
        Scratch { name, db }
    }

    /// Run a batch in the scratch database, panicking on a failure.
    async fn exec(&self, sql: &str) -> ResultSet {
        self.try_exec(sql)
            .await
            .unwrap_or_else(|e| panic!("{e}\nstatement: {sql}"))
    }

    async fn try_exec(&self, sql: &str) -> Result<ResultSet, DbError> {
        self.db
            .fetch_query(Some(&self.name), sql, 10_000, CancellationToken::new())
            .await
    }

    /// The first cell of the first row, as the grid shows it.
    async fn scalar(&self, sql: &str) -> String {
        let rs = self.exec(sql).await;
        rs.cell(0, 0)
            .map(|c| c.display().to_string())
            .unwrap_or_else(|| panic!("{sql} returned no cell"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let name = self.name.clone();
        assert_scratch_name(&name);
        // A drop cannot await; a thread of its own with a runtime of its own,
        // as `scratch::Scratch`'s guard does.
        let dropped = std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .expect("a runtime for the teardown")
                .block_on(base_db().fetch_query(
                    None,
                    &format!(
                        "ALTER DATABASE [{name}] SET SINGLE_USER WITH ROLLBACK IMMEDIATE; \
                         DROP DATABASE [{name}]"
                    ),
                    1,
                    CancellationToken::new(),
                ))
        })
        .join();
        if !matches!(dropped, Ok(Ok(_))) && !std::thread::panicking() {
            panic!("could not drop {}: {dropped:?}", self.name);
        }
    }
}

/// Skip, loudly, when the leg was left out.
fn enabled() -> bool {
    if endpoint::leg_enabled("mssql") {
        return true;
    }
    endpoint::note_leg_skipped("mssql");
    false
}

/// Every type the value renderer has an arm for, with one row whose text is
/// the one SQL Server's own tools print.
const TYPES: &str = "CREATE TABLE dbo.types ( \
    id int IDENTITY(1,1) PRIMARY KEY, \
    n nvarchar(20), v varchar(10), dec decimal(10,2), whole decimal(20,0), r real, f float, \
    dt datetime, sdt smalldatetime, d date, t time(3), dt2 datetime2(7), \
    dto datetimeoffset(0), g uniqueidentifier, b bit, bin varbinary(8), m money, \
    x xml, big bigint, tiny tinyint);
INSERT dbo.types (n, v, dec, whole, r, f, dt, sdt, d, t, dt2, dto, g, b, bin, m, x, big, tiny) \
VALUES (N'Ωμέγα', 'plain', -0.05, 12345678901234567890, 0.1, 2.5, \
    '2026-09-27 12:50:53.997', '2026-09-27 12:51', '0001-01-01', '23:59:59.123', \
    '9999-12-31 23:59:59.9999999', '2026-01-01 10:00:00 -05:30', \
    '37ab5dac-1262-4372-82ba-caad1925cd9a', 1, 0x0102, 12.5, N'<a>1</a>', \
    -9223372036854775808, 255);";

#[tokio::test(flavor = "multi_thread")]
async fn a_ping_and_the_database_list_reach_the_server() {
    if !enabled() {
        return;
    }
    base_db()
        .ping(schemaic_db::PING_TIMEOUT)
        .await
        .expect("the server answers");
    let s = Scratch::create("dblist").await;
    // Not retried: the rest of this leg creates and drops databases in
    // parallel, which the listing reads past (`mssql::DATABASE_LISTING`).
    let names = base_db().fetch_databases().await.expect("a database list");
    assert!(names.contains(&s.name), "{names:?}");
    for system in ["master", "tempdb", "model", "msdb"] {
        assert!(!names.iter().any(|n| n == system), "{system} is plumbing");
    }
}

/// Every value renders as the text SQL Server prints for it — the values at
/// the edges of each type's range included.
#[tokio::test(flavor = "multi_thread")]
async fn every_type_renders_as_sql_server_prints_it() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("types").await;
    s.exec(TYPES).await;
    let rs = s.exec("SELECT * FROM dbo.types").await;
    let shown: Vec<(String, String)> = (0..rs.col_count())
        .map(|c| {
            (
                rs.columns[c].name.clone(),
                rs.cell(0, c)
                    .map(|x| x.display().to_string())
                    .unwrap_or_default(),
            )
        })
        .collect();
    let expected = [
        ("id", "1"),
        ("n", "Ωμέγα"),
        ("v", "plain"),
        ("dec", "-0.05"),
        ("whole", "12345678901234567890"),
        ("r", "0.1"),
        ("f", "2.5"),
        ("dt", "2026-09-27 12:50:53.997"),
        ("sdt", "2026-09-27 12:51:00"),
        ("d", "0001-01-01"),
        ("t", "23:59:59.123"),
        ("dt2", "9999-12-31 23:59:59.9999999"),
        ("dto", "2026-01-01 10:00:00 -05:30"),
        ("g", "37AB5DAC-1262-4372-82BA-CAAD1925CD9A"),
        ("b", "1"),
        ("bin", "<2 bytes>"),
        ("m", "12.5000"),
        ("x", "<a>1</a>"),
        ("big", "-9223372036854775808"),
        ("tiny", "255"),
    ];
    for ((name, got), (want_name, want)) in shown.iter().zip(expected) {
        assert_eq!(name, want_name);
        assert_eq!(got, want, "{name}");
    }
    assert_eq!(shown.len(), expected.len());
    // The declared types in full, from the describe.
    let types: Vec<&str> = rs.columns.iter().map(|c| c.type_name.as_str()).collect();
    assert_eq!(
        &types[1..5],
        [
            "nvarchar(20)",
            "varchar(10)",
            "decimal(10,2)",
            "decimal(20,0)"
        ]
    );
}

/// Each column of a `SELECT *` knows its base table and column, and the key
/// is marked — what makes the grid editable once write-back lands.
#[tokio::test(flavor = "multi_thread")]
async fn a_select_star_carries_each_columns_provenance() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("provenance").await;
    s.exec("CREATE SCHEMA sales").await;
    s.exec("CREATE TABLE sales.orders (id int NOT NULL PRIMARY KEY, [odd]]name] nvarchar(5) NULL)")
        .await;
    let rs = s.exec("SELECT *, id + 1 AS next FROM sales.orders").await;
    let o = |i: usize| rs.columns[i].origin.clone();
    let id = o(0).expect("id comes from a table");
    assert_eq!(
        (
            id.database.as_str(),
            id.schema.as_deref(),
            id.table.as_str(),
            id.column.as_str()
        ),
        (s.name.as_str(), Some("sales"), "orders", "id")
    );
    assert!(id.flags.unique_key && id.flags.not_null);
    assert_eq!(o(1).expect("odd]name too").column, "odd]name");
    assert!(o(2).is_none(), "an expression has no source");
}

/// **The read-only guard is a rollback, and a rollback undoes DDL here.** A
/// `SELECT … INTO` — a table created behind a `SELECT` head — is gone once the
/// enforced read returns, and the text gate refuses it before it gets there.
#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_session_rolls_back_what_a_select_hides() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("readonly").await;
    s.exec("CREATE TABLE dbo.t (a int); INSERT dbo.t VALUES (1), (2)")
        .await;
    let sneak = "SELECT * INTO dbo.copy_of_t FROM dbo.t";
    assert!(
        schemaic_core::sql::read_only_reason(sneak, MS).is_err(),
        "the text gate refuses it first"
    );
    let rs =
        s.db.fetch_query_enforced(
            Some(&s.name),
            sneak,
            10,
            CancellationToken::new(),
            Enforce::ReadOnly,
        )
        .await
        .expect("it ran, inside the guard");
    assert_eq!(rs.affected, Some(2));
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM sys.tables WHERE name = 'copy_of_t'")
            .await,
        "0"
    );
    // And a plain read still reads.
    let rs =
        s.db.fetch_query_enforced(
            Some(&s.name),
            "SELECT a FROM dbo.t ORDER BY a",
            10,
            CancellationToken::new(),
            Enforce::ReadOnly,
        )
        .await
        .expect("a read");
    assert_eq!(rs.row_count(), 2);
    // The same guard over a batch.
    let mut outcomes = Vec::new();
    s.db.run_batch_enforced(
        Some(&s.name),
        &["DELETE FROM dbo.t".to_string()],
        10,
        CancellationToken::new(),
        |_, r| outcomes.push(r.map(|rs| rs.affected)),
        Some(Enforce::ReadOnly),
    )
    .await;
    assert_eq!(outcomes[0].as_ref().ok(), Some(&Some(2)));
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM dbo.t").await,
        "2",
        "rolled back"
    );
}

/// The plan is the server's XML, read into the plan table: the estimated
/// form runs nothing (a `DELETE` planned deletes no row), the measured form
/// counts what actually happened and is rolled back, and on a read-only
/// connection the measured form refuses what the rollback cannot undo.
#[tokio::test(flavor = "multi_thread")]
async fn a_plan_is_read_from_the_servers_showplan_and_changes_nothing() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("plan").await;
    s.exec("CREATE TABLE dbo.t (a int); INSERT dbo.t VALUES (1), (2), (3)")
        .await;
    let explain = |sql: &'static str, analyze: bool, read_only: bool| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            db.explain(
                Some(&name),
                sql,
                analyze,
                read_only,
                CancellationToken::new(),
            )
            .await
            .map(|rs| schemaic_core::plan::QueryPlan::from_result(&rs))
        }
    };

    let plan = explain("DELETE FROM dbo.t WHERE a > 1", false, false)
        .await
        .expect("an estimated plan");
    assert_eq!(plan.columns[0], "Operation", "{plan:?}");
    assert!(
        plan.rows.iter().any(|r| r[0].contains("Table Delete")),
        "{plan:?}"
    );
    assert!(
        plan.warnings
            .iter()
            .any(|w| w.kind == schemaic_core::plan::PlanWarningKind::FullScan),
        "a heap is scanned: {plan:?}"
    );
    assert_eq!(s.scalar("SELECT COUNT(*) FROM dbo.t").await, "3");

    let plan = explain("SELECT a FROM dbo.t WHERE a >= 2", true, true)
        .await
        .expect("a measured plan");
    let actual = plan.columns.iter().position(|c| c == "Actual rows");
    let scan = plan
        .rows
        .iter()
        .find(|r| r[0].contains("Table Scan"))
        .expect("the scan");
    assert_eq!(scan[actual.expect("measured")], "2", "{plan:?}");

    let plan = explain("UPDATE dbo.t SET a = a + 10", true, false)
        .await
        .expect("a measured write");
    assert!(plan.columns.iter().any(|c| c == "Actual rows"), "{plan:?}");
    assert_eq!(
        s.scalar("SELECT MAX(a) FROM dbo.t").await,
        "3",
        "rolled back"
    );

    assert!(matches!(
        explain("DELETE FROM dbo.t", true, true).await,
        Err(DbError::Refused(_))
    ));
}

/// **A dump restores into an empty database as the one it was taken from**:
/// `core::dump`'s file, its rows rendered by the export renderer as the app's
/// writer renders them, cut at its `GO` lines by the script splitter and run by
/// `run_script`. The identity keeps its gaps (so the foreign key onto it still
/// holds) and counts on past them, the view and trigger open batches of their
/// own, and the rowversion and computed column are the server's again.
#[tokio::test(flavor = "multi_thread")]
async fn a_dump_restores_into_an_empty_database() {
    use schemaic_core::dump::{DumpOptions, DumpStep, plan};
    if !enabled() {
        return;
    }
    let src = Scratch::create("dumpsrc").await;
    src.exec(
        "CREATE TABLE dbo.customers (id int IDENTITY(1,1) PRIMARY KEY, name nvarchar(50), \
           seen datetime2(3), born date, paid decimal(10,2), rv rowversion, \
           twice AS (paid * 2))",
    )
    .await;
    src.exec(
        "CREATE TABLE dbo.orders (id int IDENTITY(1,1) PRIMARY KEY, \
           customer_id int NOT NULL REFERENCES dbo.customers(id), note nvarchar(max))",
    )
    .await;
    src.exec(
        "INSERT dbo.customers (name, seen, born, paid) VALUES \
           (N'Ann', '2026-01-02 03:04:05.678', '1990-05-06', 12.50), \
           (N'gone', NULL, NULL, NULL), (N'Zoë ''q''', NULL, NULL, 0.01); \
         DELETE dbo.customers WHERE id = 2; \
         INSERT dbo.orders (customer_id, note) VALUES (3, N'first'), (1, NULL)",
    )
    .await;
    src.exec("CREATE VIEW dbo.v_orders AS SELECT o.id, c.name FROM dbo.orders o JOIN dbo.customers c ON c.id = o.customer_id")
        .await;
    src.exec("CREATE TRIGGER dbo.tr_orders ON dbo.orders AFTER INSERT AS SET NOCOUNT ON")
        .await;

    let schema = src
        .db
        .fetch_schema(&src.name, CancellationToken::new())
        .await
        .expect("the schema");
    let chosen: Vec<String> = schema
        .tables
        .iter()
        .map(|t| schemaic_core::schema::display_name(t.schema.as_deref(), &t.name))
        .collect();
    let dump = plan(&schema, &src.name, &chosen, DumpOptions::default(), MS);
    let mut file = String::new();
    for step in dump.steps {
        match step {
            DumpStep::Text(sql) => {
                file.push_str(&sql);
                file.push_str("\n\n");
            }
            DumpStep::Rows {
                database,
                insert_database,
                schema,
                table,
                select,
            } => {
                let rs = src
                    .db
                    .fetch_query(Some(&database), &select, 10_000, CancellationToken::new())
                    .await
                    .expect("the rows");
                let order: Vec<usize> = (0..rs.row_count()).collect();
                let mut out = Vec::new();
                schemaic_core::export::export_inserts_to(
                    &mut out,
                    &rs,
                    &order,
                    Some((&insert_database, schema.as_deref(), &table)),
                    MS,
                )
                .unwrap();
                file.push_str(&String::from_utf8(out).unwrap());
                file.push('\n');
            }
        }
    }
    assert!(
        file.contains("SET IDENTITY_INSERT [dbo].[customers] ON;"),
        "{file}"
    );

    let dst = Scratch::create("dumpdst").await;
    let mut splitter = schemaic_core::script::Splitter::new(MS);
    let mut stmts = splitter.push_str(&file);
    stmts.extend(splitter.finish());
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let feed = tokio::spawn(async move {
        for s in stmts {
            if tx.send(s).await.is_err() {
                break;
            }
        }
    });
    let (end, _) = dst
        .db
        .run_script(&dst.name, rx, CancellationToken::new())
        .await;
    feed.await.unwrap();
    assert!(
        matches!(end, schemaic_core::script::ExecEnd::Done),
        "{end:?}\n{file}"
    );

    let rows = "SELECT CONCAT(id, '|', name, '|', CONVERT(varchar(30), seen, 121), '|', born, '|', paid, '|', twice) \
                FROM dbo.customers ORDER BY id";
    let both = |s: &Scratch| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            let rs = db
                .fetch_query(Some(&name), rows, 100, CancellationToken::new())
                .await
                .unwrap();
            (0..rs.row_count())
                .map(|r| rs.cell(r, 0).unwrap().display().to_string())
                .collect::<Vec<_>>()
        }
    };
    let restored = both(&dst).await;
    assert_eq!(restored, both(&src).await);
    assert!(restored[1].starts_with("3|Zoë 'q'|"), "{restored:?}");
    assert_eq!(
        dst.scalar("SELECT COUNT(*) FROM sys.foreign_keys").await,
        "1"
    );
    assert_eq!(dst.scalar("SELECT COUNT(*) FROM dbo.v_orders").await, "2");
    assert_eq!(
        dst.scalar("SELECT COUNT(*) FROM sys.triggers WHERE name = 'tr_orders'")
            .await,
        "1"
    );
    // The identity counts on past the highest key the file carried.
    dst.exec("INSERT dbo.customers (name) VALUES (N'next')")
        .await;
    assert_eq!(dst.scalar("SELECT MAX(id) FROM dbo.customers").await, "4");
}

/// Validation compiles without running: a missing table is reported by
/// number, and a `DELETE` that checks clean deleted nothing.
#[tokio::test(flavor = "multi_thread")]
async fn validation_names_a_missing_table_and_runs_nothing() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("prepare").await;
    s.exec("CREATE TABLE dbo.t (a int); INSERT dbo.t VALUES (1)")
        .await;
    let err =
        s.db.prepare_check(Some(&s.name), "SELECT * FROM dbo.nope")
            .await
            .expect_err("a missing table");
    assert!(err.to_string().contains("Msg 208"), "{err}");
    let err =
        s.db.prepare_check(Some(&s.name), "SELECT nocol FROM dbo.t")
            .await
            .expect_err("a missing column");
    assert!(err.to_string().contains("Msg 207"), "{err}");
    s.db.prepare_check(Some(&s.name), "DELETE FROM dbo.t")
        .await
        .expect("a valid statement");
    assert_eq!(s.scalar("SELECT COUNT(*) FROM dbo.t").await, "1");
}

/// A statement's own row count is reported, one statement at a time.
#[tokio::test(flavor = "multi_thread")]
async fn a_write_reports_the_rows_it_touched() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("affected").await;
    s.exec("CREATE TABLE dbo.t (a int)").await;
    assert_eq!(
        s.exec("INSERT dbo.t VALUES (1), (2), (3)").await.affected,
        Some(3)
    );
    assert_eq!(
        s.exec("UPDATE dbo.t SET a = a WHERE a > 1").await.affected,
        Some(2)
    );
    assert_eq!(s.exec("DELETE FROM dbo.t").await.affected, Some(3));
}

// ── Grid write-back ──────────────────────────────────────────────────────────

fn row_edit(s: &Scratch, table: &str, set: &[(&str, CellEdit)], key: &[(&str, Value)]) -> RowEdit {
    RowEdit {
        database: s.name.clone(),
        schema: Some("dbo".into()),
        table: table.into(),
        set: set
            .iter()
            .map(|(c, v)| (c.to_string(), v.clone()))
            .collect(),
        key: key
            .iter()
            .map(|(c, v)| (c.to_string(), v.clone()))
            .collect(),
    }
}

fn row_insert(s: &Scratch, table: &str, cols: &[(&str, CellEdit)]) -> RowInsert {
    RowInsert {
        database: s.name.clone(),
        schema: Some("dbo".into()),
        table: table.into(),
        cols: cols
            .iter()
            .map(|(c, v)| (c.to_string(), v.clone()))
            .collect(),
    }
}

fn row_delete(s: &Scratch, table: &str, key: &[(&str, Value)]) -> RowDelete {
    RowDelete {
        database: s.name.clone(),
        schema: Some("dbo".into()),
        table: table.into(),
        key: key
            .iter()
            .map(|(c, v)| (c.to_string(), v.clone()))
            .collect(),
    }
}

fn txt(s: &str) -> CellEdit {
    CellEdit::Text(s.into())
}

async fn commit(s: &Scratch, write: GridWrite) -> Result<u64, DbError> {
    s.db.commit_writes(&write, CancellationToken::new()).await
}

/// **An edit lands on exactly its row**, a NULL is stored as NULL, bytes as
/// bytes, and what the grid re-reads and opens is what was written.
#[tokio::test(flavor = "multi_thread")]
async fn a_staged_edit_lands_on_its_row_and_reads_back() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("wb_update").await;
    s.exec(
        "CREATE TABLE dbo.t (id int PRIMARY KEY, note nvarchar(20) NULL, qty int NULL, \
         photo varbinary(max) NULL); \
         INSERT dbo.t VALUES (1, N'one', 1, NULL), (2, N'two', 2, NULL)",
    )
    .await;
    let n = commit(
        &s,
        GridWrite {
            updates: vec![row_edit(
                &s,
                "t",
                &[
                    ("note", txt("Ωμέγα")),
                    ("qty", CellEdit::Null),
                    ("photo", CellEdit::Bytes(Arc::from(&[0u8, 1, 2, 255][..]))),
                ],
                &[("id", Value::Int(1))],
            )],
            ..Default::default()
        },
    )
    .await
    .expect("the edit commits");
    assert_eq!(n, 1);
    assert_eq!(
        s.scalar(
            "SELECT CONCAT(note, '|', COALESCE(CAST(qty AS varchar), 'NULL'), '|', \
                  CONVERT(varchar(20), photo, 1)) FROM dbo.t WHERE id = 1"
        )
        .await,
        "Ωμέγα|NULL|0x000102FF"
    );
    assert_eq!(
        s.scalar("SELECT note FROM dbo.t WHERE id = 2").await,
        "two",
        "only its row"
    );

    let template = RefetchTemplate {
        database: s.name.clone(),
        schema: Some("dbo".into()),
        table: "t".into(),
        columns: vec!["id".into(), "note".into(), "qty".into()],
        key_cols: vec![0],
        confirm_cols: vec![],
    };
    let again =
        s.db.refetch_rows(
            &template,
            &[
                RefetchRow {
                    data_row: 0,
                    key: vec![Value::Int(1)],
                },
                RefetchRow {
                    data_row: 5,
                    key: vec![Value::Int(99)],
                },
            ],
            CancellationToken::new(),
        )
        .await
        .expect("the re-read");
    assert_eq!(
        again,
        vec![(
            0,
            vec![Value::Int(1), Value::Str("Ωμέγα".into()), Value::Null]
        )],
        "a vanished row is skipped"
    );

    let blob =
        s.db.fetch_blob(
            &BlobRef {
                database: s.name.clone(),
                schema: Some("dbo".into()),
                table: "t".into(),
                column: "photo".into(),
                key: vec![("id".into(), Value::Int(1))],
            },
            CancellationToken::new(),
        )
        .await
        .expect("the blob read")
        .expect("a value");
    assert_eq!((blob.bytes, blob.len), (vec![0u8, 1, 2, 255], 4));
}

/// Inserts take the defaults they are not given, an explicit identity value
/// goes in through `IDENTITY_INSERT` — which is on only for that statement —
/// and a delete runs before an insert that reuses its key.
#[tokio::test(flavor = "multi_thread")]
async fn inserts_take_defaults_identities_and_a_deleted_key() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("wb_insert").await;
    s.exec(
        "CREATE TABLE dbo.t (id int IDENTITY(1,1) PRIMARY KEY, \
         code nvarchar(5) NOT NULL UNIQUE DEFAULT N'x', qty int NOT NULL DEFAULT 7); \
         INSERT dbo.t (code) VALUES (N'old')",
    )
    .await;
    let n = commit(
        &s,
        GridWrite {
            deletes: vec![row_delete(&s, "t", &[("id", Value::Int(1))])],
            inserts: vec![
                row_insert(&s, "t", &[("code", txt("old"))]),
                row_insert(&s, "t", &[("id", txt("50")), ("code", txt("id50"))]),
                row_insert(&s, "t", &[]),
            ],
            ..Default::default()
        },
    )
    .await
    .expect("the batch commits");
    assert_eq!(n, 4);
    assert_eq!(
        s.scalar(
            "SELECT STRING_AGG(CONCAT(id, ':', code, ':', qty), ',') \
                  WITHIN GROUP (ORDER BY id) FROM dbo.t"
        )
        .await,
        "2:old:7,50:id50:7,51:x:7"
    );
}

/// **The 1-row net, both ways, and a failure part-way through.** A key that
/// matches no row and one that matches two are refused, and so is a value the
/// column cannot take — each after an earlier statement in the batch had
/// already succeeded, and each leaving the table exactly as it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_batch_that_misses_is_rolled_back_whole() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("wb_rollback").await;
    s.exec(
        "CREATE TABLE dbo.t (id int NOT NULL, grp int NOT NULL, qty int NULL); \
         INSERT dbo.t VALUES (1, 1, 10), (2, 1, 20), (3, 2, 30)",
    )
    .await;
    let first = row_edit(&s, "t", &[("qty", txt("99"))], &[("id", Value::Int(3))]);
    let cases = [
        (
            row_edit(&s, "t", &[("qty", txt("0"))], &[("id", Value::Int(42))]),
            "affected 0 rows",
        ),
        (
            row_edit(&s, "t", &[("qty", txt("0"))], &[("grp", Value::Int(1))]),
            "affected 2 rows",
        ),
        (
            row_edit(&s, "t", &[("qty", txt("abc"))], &[("id", Value::Int(1))]),
            "Msg 245",
        ),
    ];
    for (bad, says) in cases {
        let err = commit(
            &s,
            GridWrite {
                updates: vec![first.clone(), bad],
                ..Default::default()
            },
        )
        .await
        .expect_err("the batch fails");
        let msg = err.to_string();
        assert!(msg.contains(says), "{msg}");
        assert!(msg.contains("rolled back all changes"), "{msg}");
        assert!(!msg.contains("query failed: query failed"), "{msg}");
        assert_eq!(
            s.scalar(
                "SELECT STRING_AGG(CAST(qty AS varchar), ',') WITHIN GROUP (ORDER BY id) FROM dbo.t"
            )
            .await,
            "10,20,30",
            "{says}: nothing survives"
        );
    }
    assert_eq!(commit(&s, GridWrite::default()).await.expect("empty"), 0);
}

/// **An empty value is refused where SQL Server would convert it** — a cleared
/// `int` would otherwise store 0 and report success — while an empty string in
/// a text column is a value like any other.
#[tokio::test(flavor = "multi_thread")]
async fn an_empty_value_is_not_written_as_zero() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("wb_blank").await;
    // Two batches: a table cannot name a type its own batch creates.
    s.exec("CREATE TYPE dbo.Qty FROM int NULL").await;
    s.exec(
        "CREATE TABLE dbo.t (id int PRIMARY KEY, qty dbo.Qty, note nvarchar(5) NULL); \
         INSERT dbo.t VALUES (1, 5, N'n')",
    )
    .await;
    let err = commit(
        &s,
        GridWrite {
            updates: vec![row_edit(
                &s,
                "t",
                &[("qty", txt(""))],
                &[("id", Value::Int(1))],
            )],
            ..Default::default()
        },
    )
    .await
    .expect_err("refused");
    assert!(err.to_string().contains("empty value"), "{err}");
    assert_eq!(
        s.scalar("SELECT qty FROM dbo.t").await,
        "5",
        "an alias type is its base type"
    );
    commit(
        &s,
        GridWrite {
            updates: vec![row_edit(
                &s,
                "t",
                &[("note", txt(""))],
                &[("id", Value::Int(1))],
            )],
            ..Default::default()
        },
    )
    .await
    .expect("text takes an empty string");
    assert_eq!(
        s.scalar("SELECT CONCAT('[', note, ']') FROM dbo.t").await,
        "[]"
    );
}

/// **A trigger's own statements do not count as the edit's.** The statement's
/// count is the last one TDS reports; a trigger that writes two audit rows
/// reports its 2 first, and the edit is still 1.
#[tokio::test(flavor = "multi_thread")]
async fn a_trigger_does_not_trip_the_one_row_guard() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("wb_trigger").await;
    s.exec(
        "CREATE TABLE dbo.t (id int PRIMARY KEY, qty int); \
         CREATE TABLE dbo.audit (at int); \
         INSERT dbo.t VALUES (1, 1)",
    )
    .await;
    s.exec(
        "CREATE TRIGGER dbo.t_audit ON dbo.t AFTER UPDATE AS \
         INSERT dbo.audit VALUES (1), (2)",
    )
    .await;
    let n = commit(
        &s,
        GridWrite {
            updates: vec![row_edit(
                &s,
                "t",
                &[("qty", txt("2"))],
                &[("id", Value::Int(1))],
            )],
            ..Default::default()
        },
    )
    .await
    .expect("one row, whatever the trigger did");
    assert_eq!(n, 1);
    assert_eq!(s.scalar("SELECT COUNT(*) FROM dbo.audit").await, "2");
}

/// **Stop during a commit stops it on the server and undoes it.** A trigger
/// holds the edit for five seconds; Stop answers well inside that, the
/// attention ends the statement, and the rollback leaves the row as it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_commit_is_undone() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("wb_cancel").await;
    s.exec("CREATE TABLE dbo.t (id int PRIMARY KEY, qty int); INSERT dbo.t VALUES (1, 1)")
        .await;
    s.exec("CREATE TRIGGER dbo.t_slow ON dbo.t AFTER UPDATE AS WAITFOR DELAY '00:00:05'")
        .await;
    let token = CancellationToken::new();
    let stop = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        stop.cancel();
    });
    let started = std::time::Instant::now();
    let err =
        s.db.commit_writes(
            &GridWrite {
                updates: vec![row_edit(
                    &s,
                    "t",
                    &[("qty", txt("2"))],
                    &[("id", Value::Int(1))],
                )],
                ..Default::default()
            },
            token,
        )
        .await
        .expect_err("stopped");
    assert!(matches!(err, DbError::Cancelled), "{err}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "Stop waited for the trigger: {:?}",
        started.elapsed()
    );
    assert_eq!(s.scalar("SELECT qty FROM dbo.t").await, "1", "undone");
}

// ── Schema changes ───────────────────────────────────────────────────────────

/// **A designed table is created as it was drafted.** The designer's draft,
/// through the emitter and `run_ddl`, read back by introspection: an identity
/// key, a collation, a default, a persisted computed column, a check, a unique
/// index and a cascading foreign key into another schema.
#[tokio::test(flavor = "multi_thread")]
async fn a_designed_table_is_created_as_drafted() {
    use schemaic_core::ddl::{
        self, CheckDraft, ColumnDraft, ForeignKeyDraft, IndexDraft, TableDraft,
    };
    use schemaic_core::schema::{CheckInfo, ColumnInfo, ForeignKeyInfo, IndexColumn, IndexInfo};
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_create").await;
    s.exec("CREATE SCHEMA sales").await;
    s.exec("CREATE TABLE dbo.status (id int PRIMARY KEY)").await;
    let col = |name: &str, ty: &str| {
        ColumnDraft::new(ColumnInfo {
            name: name.into(),
            type_name: ty.into(),
            nullable: true,
            ..Default::default()
        })
    };
    let mut id = col("id", "int");
    id.info.nullable = false;
    id.info.auto_increment = true;
    let mut email = col("email", "nvarchar(255)");
    email.info.nullable = false;
    email.info.collation = Some("Latin1_General_CS_AS".into());
    let mut qty = col("qty", "int");
    qty.info.default = Some("0".into());
    let mut total = col("total", "int");
    total.info.generated = Some("[qty] * 2".into());
    total.info.generated_stored = true;
    let draft = TableDraft {
        name: "people".into(),
        schema: Some("sales".into()),
        columns: vec![id, email, qty, total, col("status_id", "int")],
        primary_key: vec!["id".into()],
        indexes: vec![IndexDraft::new(IndexInfo {
            name: "email_uq".into(),
            columns: vec![IndexColumn::plain("email")],
            unique: true,
            ..Default::default()
        })],
        foreign_keys: vec![ForeignKeyDraft::new(ForeignKeyInfo {
            name: "fk_status".into(),
            columns: vec!["status_id".into()],
            ref_schema: Some("dbo".into()),
            ref_table: "status".into(),
            ref_columns: vec!["id".into()],
            on_delete: Some("CASCADE".into()),
            ..Default::default()
        })],
        check_constraints: vec![CheckDraft::new(CheckInfo {
            name: "qty_pos".into(),
            expression: "[qty] >= 0".into(),
            enforced: true,
            ..Default::default()
        })],
        ..Default::default()
    };
    let stmts = ddl::create(&draft, MS).emit();
    s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}\n{}", stmts.join("\n")));

    let schema =
        s.db.fetch_schema(&s.name, CancellationToken::new())
            .await
            .expect("the schema");
    let t = schema
        .tables
        .iter()
        .find(|t| t.schema.as_deref() == Some("sales") && t.name == "people")
        .expect("sales.people was created");
    let c = |n: &str| t.columns.iter().find(|x| x.name == n).expect(n);
    assert!(c("id").primary_key && c("id").auto_increment && !c("id").nullable);
    assert_eq!(
        c("email").collation.as_deref(),
        Some("Latin1_General_CS_AS")
    );
    assert!(!c("email").nullable && c("qty").nullable);
    assert_eq!(c("qty").default.as_deref(), Some("0"));
    assert!(c("total").generated.is_some() && c("total").generated_stored);
    assert!(t.indexes.iter().any(|i| i.name == "email_uq" && i.unique));
    assert_eq!(t.check_constraints[0].name, "qty_pos");
    let fk = &t.foreign_keys[0];
    assert_eq!(
        (
            fk.name.as_str(),
            fk.ref_schema.as_deref(),
            fk.on_delete.as_deref()
        ),
        ("fk_status", Some("dbo"), Some("CASCADE"))
    );
    // And it is a table the grid can write: the identity fills itself in.
    s.exec("INSERT sales.people (email, status_id) VALUES (N'a@b', NULL)")
        .await;
    assert_eq!(
        s.scalar("SELECT CONCAT(id, ':', qty, ':', total) FROM sales.people")
            .await,
        "1:0:0"
    );
}

/// **A plan that fails part-way leaves nothing behind.** T-SQL's DDL is
/// transactional, so the table the first statement made is gone once the
/// second — an index on a column that is not there — is refused, and the error
/// says so: statement 2, nothing applied.
#[tokio::test(flavor = "multi_thread")]
async fn a_failing_plan_is_rolled_back_whole() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_rollback").await;
    let stmts = vec![
        "CREATE TABLE [dbo].[t] (\n  [id] int NOT NULL,\n  PRIMARY KEY ([id])\n);".to_string(),
        "CREATE INDEX [ix] ON [dbo].[t] ([nope]);".to_string(),
    ];
    let err =
        s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
            .await
            .expect_err("the index is refused");
    assert_eq!((err.at, err.applied), (1, 0), "{err}");
    assert!(err.message.contains("nope"), "{err}");
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM sys.tables WHERE name = 't'")
            .await,
        "0",
        "the table went with it"
    );
}

/// The three drops, each through its own change set, each gone afterwards —
/// and a procedure by its bare name, T-SQL having no overloads to tell apart.
#[tokio::test(flavor = "multi_thread")]
async fn a_table_a_view_and_a_procedure_are_dropped() {
    use schemaic_core::ddl::{self, Change};
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_drop").await;
    s.exec("CREATE TABLE dbo.t (id int)").await;
    s.exec("CREATE VIEW dbo.v AS SELECT id FROM dbo.t").await;
    s.exec("CREATE PROCEDURE dbo.p AS SELECT 1").await;
    let schema =
        s.db.fetch_schema(&s.name, CancellationToken::new())
            .await
            .expect("the schema");
    let p = schema
        .routines
        .iter()
        .find(|r| r.name == "p")
        .expect("the procedure is listed");
    let plans = [
        ddl::drop_routine(p, MS),
        ddl::single(
            "v",
            Some("dbo"),
            MS,
            Change::DropView {
                materialized: false,
            },
        ),
        ddl::single("t", Some("dbo"), MS, Change::DropTable),
    ];
    for plan in plans {
        let stmts = plan.emit();
        s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{e}\n{}", stmts.join("\n")));
    }
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM sys.objects WHERE name IN ('t', 'v', 'p')")
            .await,
        "0"
    );
}

/// The table `name` in `dbo`, as introspection reads it.
async fn read_table(s: &Scratch, name: &str) -> schemaic_core::schema::TableInfo {
    let schema =
        s.db.fetch_schema(&s.name, CancellationToken::new())
            .await
            .expect("the schema");
    schema
        .tables
        .iter()
        .find(|t| t.schema.as_deref() == Some("dbo") && t.name == name)
        .unwrap_or_else(|| panic!("dbo.{name} in {:?}", schema.tables))
        .clone()
}

/// Diff `draft` against `current`, refuse nothing, and apply it.
async fn apply_draft(
    s: &Scratch,
    current: &schemaic_core::schema::TableInfo,
    draft: &schemaic_core::ddl::TableDraft,
) -> Vec<String> {
    let cs = schemaic_core::ddl::diff(current, draft, MS);
    assert!(
        cs.unsupported().is_empty(),
        "withheld: {:?}",
        cs.unsupported()
    );
    let stmts = cs.emit();
    s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}\n{}", stmts.join("\n")));
    stmts
}

/// **The round-trip gate, against the server**: every table of the sample
/// database diffs to nothing against its own draft, so opening the designer
/// on one and applying without an edit is a plan with no statements. Skipped
/// where the sample database is not installed (it is not in CI).
#[tokio::test(flavor = "multi_thread")]
async fn every_sample_table_diffs_to_nothing_against_its_own_draft() {
    if !enabled() {
        return;
    }
    let db = base_db();
    // Asked of the catalogue first, so that an introspection that fails on a
    // machine that has the sample is a failure and not a skip. A query rather
    // than `fetch_databases`, whose ping-length connect bound is short for a
    // leg running thirty tests at once.
    let installed = db
        .fetch_query(
            None,
            "SELECT COUNT(*) FROM sys.databases WHERE name = N'AdventureWorksLT'",
            1,
            CancellationToken::new(),
        )
        .await
        .expect("the catalogue")
        .cell(0, 0)
        .is_some_and(|c| c.display() == "1");
    if !installed {
        eprintln!("AdventureWorksLT is not installed here; skipping the sample round trip");
        return;
    }
    let schema = db
        .fetch_schema("AdventureWorksLT", CancellationToken::new())
        .await
        .expect("the sample's schema");
    assert!(!schema.tables.is_empty());
    for t in &schema.tables {
        let cs = schemaic_core::ddl::diff(t, &schemaic_core::ddl::TableDraft::from_table(t), MS);
        assert!(
            cs.changes.is_empty(),
            "{}.{}: {:?}",
            t.schema.as_deref().unwrap_or(""),
            t.name,
            cs.changes
        );
    }
}

/// **A whole designer edit lands.** One column renamed, retyped, made
/// `NOT NULL` and given a new default — its old default dropped first, which
/// a retype needs — a column with a default dropped, a column added, a check
/// swapped, an index added and the table renamed, in one plan. Read back by
/// introspection, the data kept, and the new default filling a new row.
#[tokio::test(flavor = "multi_thread")]
async fn a_designer_edit_lands_as_drafted() {
    use schemaic_core::ddl::{CheckDraft, ColumnDraft, IndexDraft, TableDraft};
    use schemaic_core::schema::{CheckInfo, ColumnInfo, IndexColumn, IndexInfo};
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_alter").await;
    s.exec(
        "CREATE TABLE dbo.t (id int IDENTITY(1,1) NOT NULL CONSTRAINT pk_t PRIMARY KEY, \
         qty int NULL CONSTRAINT df_qty DEFAULT 0, old int NULL DEFAULT 9, \
         CONSTRAINT ck_qty CHECK (qty >= 0)); \
         INSERT dbo.t (qty) VALUES (5)",
    )
    .await;
    let t = read_table(&s, "t").await;
    let mut d = TableDraft::from_table(&t);
    let qty = d
        .columns
        .iter_mut()
        .find(|c| c.info.name == "qty")
        .expect("qty");
    qty.info.name = "amount".into();
    qty.info.type_name = "bigint".into();
    qty.info.nullable = false;
    qty.info.default = Some("1".into());
    d.columns.retain(|c| c.info.name != "old");
    d.columns.push(ColumnDraft::new(ColumnInfo {
        name: "note".into(),
        type_name: "nvarchar(10)".into(),
        nullable: true,
        ..Default::default()
    }));
    d.check_constraints.clear();
    d.check_constraints.push(CheckDraft::new(CheckInfo {
        name: "ck_amount".into(),
        expression: "[amount] >= (1)".into(),
        enforced: true,
        ..Default::default()
    }));
    d.indexes.push(IndexDraft::new(IndexInfo {
        name: "ix_note".into(),
        columns: vec![IndexColumn::plain("note")],
        ..Default::default()
    }));
    d.name = "t2".into();
    let stmts = apply_draft(&s, &t, &d).await;

    let t2 = read_table(&s, "t2").await;
    let c = |n: &str| t2.columns.iter().find(|x| x.name == n);
    let amount = c("amount").unwrap_or_else(|| panic!("{stmts:#?}"));
    assert_eq!(amount.type_name, "bigint");
    assert!(!amount.nullable);
    assert_eq!(amount.default.as_deref(), Some("1"));
    assert!(c("old").is_none() && c("qty").is_none() && c("note").is_some());
    assert_eq!(t2.check_constraints.len(), 1);
    assert_eq!(t2.check_constraints[0].name, "ck_amount");
    assert!(t2.indexes.iter().any(|i| i.name == "ix_note"));
    assert_eq!(
        s.scalar("SELECT amount FROM dbo.t2 WHERE id = 1").await,
        "5",
        "the data kept"
    );
    s.exec("INSERT dbo.t2 (note) VALUES (N'x')").await;
    assert_eq!(
        s.scalar("SELECT amount FROM dbo.t2 WHERE note = N'x'")
            .await,
        "1"
    );
    // And the table now round-trips as itself.
    let again = schemaic_core::ddl::diff(&t2, &TableDraft::from_table(&t2), MS);
    assert!(again.changes.is_empty(), "{:?}", again.changes);
}

/// **Comments are set, changed and cleared as `MS_Description`**, whether or
/// not one was there — the plan looks it up as it runs — and by the names the
/// plan's renames leave. Read back each time, and round-tripping to nothing.
#[tokio::test(flavor = "multi_thread")]
async fn comments_are_set_changed_and_cleared() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_comment").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY, qty int NULL)")
        .await;
    let t = read_table(&s, "t").await;
    let mut d = TableDraft::from_table(&t);
    d.comment = Some("stock's table".into());
    let qty = d.columns.iter_mut().find(|c| c.info.name == "qty").unwrap();
    qty.info.name = "amount".into();
    qty.info.comment = Some("how many".into());
    d.name = "t2".into();
    let stmts = apply_draft(&s, &t, &d).await;
    let t2 = read_table(&s, "t2").await;
    assert_eq!(t2.comment.as_deref(), Some("stock's table"), "{stmts:#?}");
    let amount = t2.columns.iter().find(|c| c.name == "amount").unwrap();
    assert_eq!(amount.comment.as_deref(), Some("how many"));

    // Changed where one is there, and cleared.
    let mut d = TableDraft::from_table(&t2);
    d.comment = None;
    d.columns[1].info.comment = Some("units".into());
    apply_draft(&s, &t2, &d).await;
    let t3 = read_table(&s, "t2").await;
    assert_eq!(t3.comment, None);
    assert_eq!(t3.columns[1].comment.as_deref(), Some("units"));
    let again = schemaic_core::ddl::diff(&t3, &TableDraft::from_table(&t3), MS);
    assert!(again.changes.is_empty(), "{:?}", again.changes);

    // A new table's are added after it.
    let mut n = TableDraft::from_table(&t3);
    n.name = "fresh".into();
    n.comment = Some("new".into());
    let create = schemaic_core::ddl::single(
        "fresh",
        Some("dbo"),
        MS,
        schemaic_core::ddl::Change::CreateTable(Box::new(n)),
    );
    s.db.run_ddl(&s.name, &create.emit(), CancellationToken::new())
        .await
        .expect("create");
    let fresh = read_table(&s, "fresh").await;
    assert_eq!(fresh.comment.as_deref(), Some("new"));
    assert_eq!(fresh.columns[1].comment.as_deref(), Some("units"));
}

/// **A retype under its dependents lands**: the key, an index, a unique
/// constraint and a check come off, the columns change, and each goes back as
/// it was — the unique constraint as a constraint — with the data kept and the
/// table round-tripping to nothing. The foreign key's column changes only its
/// nullability, which it survives, and it stays on.
#[tokio::test(flavor = "multi_thread")]
async fn a_retype_under_its_dependents_lands() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_deps").await;
    s.exec(
        "CREATE TABLE dbo.p (id bigint NOT NULL PRIMARY KEY); \
         INSERT dbo.p VALUES (7); \
         CREATE TABLE dbo.t (id int NOT NULL CONSTRAINT pk_t PRIMARY KEY, a int NULL, \
           c varchar(10) NULL CONSTRAINT uq_c UNIQUE, f int NULL, \
           k int NULL CONSTRAINT ck_k CHECK (k > 0)); \
         CREATE INDEX ix_a ON dbo.t (a); \
         INSERT dbo.t VALUES (1, 2, 'x', NULL, 3)",
    )
    .await;
    // The foreign key needs matching types, so it is added after the
    // referenced key is bigint and `f` is retyped to it by the plan.
    s.exec(
        "ALTER TABLE dbo.t ALTER COLUMN f bigint NULL; \
            ALTER TABLE dbo.t ADD CONSTRAINT fk_f FOREIGN KEY (f) REFERENCES dbo.p (id)",
    )
    .await;
    let t = read_table(&s, "t").await;
    let mut d = schemaic_core::ddl::TableDraft::from_table(&t);
    for c in d.columns.iter_mut() {
        match c.info.name.as_str() {
            "id" | "a" | "k" => c.info.type_name = "bigint".into(),
            "c" => c.info.type_name = "nvarchar(10)".into(),
            // A nullability change alone, which the foreign key allows.
            "f" => c.info.nullable = false,
            _ => {}
        }
    }
    s.exec("UPDATE dbo.t SET f = 7").await;
    let stmts = apply_draft(&s, &t, &d).await;
    let t2 = read_table(&s, "t").await;
    let ty = |n: &str| {
        t2.columns
            .iter()
            .find(|c| c.name == n)
            .unwrap()
            .type_name
            .clone()
    };
    assert_eq!(
        (ty("id"), ty("a"), ty("c"), ty("k")),
        (
            "bigint".into(),
            "bigint".into(),
            "nvarchar(10)".into(),
            "bigint".into()
        ),
        "{stmts:#?}"
    );
    let uq = t2.indexes.iter().find(|i| i.name == "uq_c").expect("uq_c");
    assert_eq!(uq.constraint.as_deref(), Some("uq_c"), "still a constraint");
    assert!(t2.indexes.iter().any(|i| i.name == "ix_a"));
    assert!(
        t2.indexes
            .iter()
            .any(|i| i.is_primary() && i.constraint.as_deref() == Some("pk_t"))
    );
    assert_eq!(t2.foreign_keys[0].name, "fk_f");
    assert_eq!(t2.check_constraints[0].name, "ck_k");
    // The foreign key was only in the way of a retype, and `f`'s change was
    // not one: it stayed on.
    assert!(!stmts.iter().any(|s| s.contains("[fk_f]")), "{stmts:#?}");
    assert_eq!(s.scalar("SELECT c FROM dbo.t WHERE id = 1").await, "x");
    let again = schemaic_core::ddl::diff(&t2, &schemaic_core::ddl::TableDraft::from_table(&t2), MS);
    assert!(again.changes.is_empty(), "{:?}", again.changes);
}

/// **A column a computed one reads is renamed and retyped**: the persisted
/// computed column, its index and its comment come off and go back reading the
/// new name — at the end of the table, since T-SQL has no reorder — with its
/// values recomputed and the table round-tripping to nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_column_under_a_computed_one_is_renamed_and_retyped() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_computed").await;
    s.exec(
        "CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY, s int NULL, \
           x AS (s * 2) PERSISTED, note int NULL); \
         CREATE INDEX ix_x ON dbo.t (x); \
         EXEC sp_addextendedproperty N'MS_Description', N'doubled', \
           N'SCHEMA', N'dbo', N'TABLE', N't', N'COLUMN', N'x'; \
         INSERT dbo.t (id, s) VALUES (1, 21)",
    )
    .await;
    let t = read_table(&s, "t").await;
    let mut d = schemaic_core::ddl::TableDraft::from_table(&t);
    let col = d.columns.iter_mut().find(|c| c.info.name == "s").unwrap();
    col.info.name = "s2".into();
    col.info.type_name = "bigint".into();
    let stmts = apply_draft(&s, &t, &d).await;
    let t2 = read_table(&s, "t").await;
    let names: Vec<&str> = t2.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "s2", "note", "x"], "{stmts:#?}");
    let x = &t2.columns[3];
    assert_eq!(x.generated.as_deref(), Some("[s2]*(2)"));
    assert!(x.generated_stored);
    assert_eq!(x.comment.as_deref(), Some("doubled"));
    assert!(t2.indexes.iter().any(|i| i.name == "ix_x"));
    assert_eq!(s.scalar("SELECT x FROM dbo.t WHERE id = 1").await, "42");
    let again = schemaic_core::ddl::diff(&t2, &schemaic_core::ddl::TableDraft::from_table(&t2), MS);
    assert!(again.changes.is_empty(), "{:?}", again.changes);
}

/// **A view is altered in place and renamed by re-creating it.** A
/// schema-bound view with a column list and a grant: `CREATE OR ALTER`
/// changes its body keeping all three — T-SQL's `ALTER VIEW` would drop the
/// binding and the names if they were not restated — and a rename builds it
/// again under the new name, whose stored definition then names it.
#[tokio::test(flavor = "multi_thread")]
async fn a_view_is_altered_in_place_and_renamed() {
    use schemaic_core::ddl::{ViewDraft, diff_view};
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_view").await;
    s.exec(
        "CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY, d int); \
         INSERT dbo.t VALUES (1, 0), (2, 5)",
    )
    .await;
    s.exec("CREATE VIEW dbo.v (a, b) WITH SCHEMABINDING AS SELECT id, d FROM dbo.t")
        .await;
    s.exec("GRANT SELECT ON dbo.v TO public").await;
    let v = read_table(&s, "v").await;
    let o = v.view_options.clone().expect("options");
    assert_eq!(o.column_list.as_deref(), Some("a, b"));
    assert_eq!(o.attributes, ["SCHEMABINDING"]);

    let mut d = ViewDraft::from_table(&v).expect("a view");
    d.select = "SELECT id, d FROM dbo.t WHERE d > 0".into();
    let stmts = diff_view(&v, &d, MS).emit();
    s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
    assert_eq!(s.scalar("SELECT COUNT(*) FROM dbo.v").await, "1");
    assert_eq!(s.scalar("SELECT b FROM dbo.v").await, "5", "still named b");
    assert_eq!(
        s.scalar("SELECT OBJECTPROPERTY(OBJECT_ID('dbo.v'), 'IsSchemaBound')")
            .await,
        "1"
    );
    assert_eq!(
        s.scalar(
            "SELECT COUNT(*) FROM sys.database_permissions \
             WHERE major_id = OBJECT_ID('dbo.v') AND permission_name = 'SELECT'"
        )
        .await,
        "1",
        "the grant kept"
    );
    let v2 = read_table(&s, "v").await;
    assert!(diff_view(&v2, &ViewDraft::from_table(&v2).unwrap(), MS).is_empty());

    let mut d = ViewDraft::from_table(&v2).unwrap();
    d.name = "w".into();
    let stmts = diff_view(&v2, &d, MS).emit();
    s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
    let w = read_table(&s, "w").await;
    assert!(
        w.create_sql.as_deref().unwrap_or_default().contains("[w]"),
        "{:?}",
        w.create_sql
    );
    assert_eq!(w.view_options.unwrap().attributes, ["SCHEMABINDING"]);
    assert_eq!(s.scalar("SELECT b FROM dbo.w").await, "5");
}

/// **A trigger is altered in place and keeps what the alter would reset.**
/// Its header options come back through the parts (`EXECUTE AS`, `NOT FOR
/// REPLICATION`), a disabled trigger stays disabled, the `First` rank any
/// `ALTER TRIGGER` drops is set again, and the edited body fires. A rename is
/// a drop and a create; a new `INSTEAD OF` trigger on a view fires in place of
/// the write; an encrypted one is listed, hidden, and not editable. Each read
/// back diffs to nothing against its own draft — the round-trip gate.
#[tokio::test(flavor = "multi_thread")]
async fn a_trigger_is_altered_in_place_and_keeps_what_the_alter_resets() {
    use schemaic_core::ddl::{TriggerDraft, TriggerSetDraft, diff_triggers};
    use schemaic_core::schema::{
        ExecuteAs, FiringRank, TriggerAction, TriggerEnabled, TriggerEvent, TriggerTiming,
    };
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_trigger").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY); CREATE TABLE dbo.log (what nvarchar(20))")
        .await;
    s.exec("CREATE VIEW dbo.v AS SELECT id FROM dbo.t").await;
    s.exec(
        "-- kept by the server, dropped by a rebuild\n\
         create trigger dbo.tr_a on dbo.t with execute as owner after insert, delete \
         not for replication as\nbegin\n  SET NOCOUNT ON;\nend",
    )
    .await;
    s.exec("CREATE TRIGGER dbo.tr_b ON dbo.t AFTER UPDATE AS INSERT dbo.log VALUES (N'b')")
        .await;
    s.exec("CREATE TRIGGER dbo.tr_c ON dbo.t WITH ENCRYPTION AFTER INSERT AS SELECT 1")
        .await;
    s.exec("EXEC sp_settriggerorder @triggername = N'dbo.tr_a', @order = N'First', @stmttype = N'INSERT'")
        .await;
    s.exec("DISABLE TRIGGER dbo.tr_a ON dbo.t").await;

    let t = read_table(&s, "t").await;
    let tr = |t: &schemaic_core::schema::TableInfo, n: &str| {
        t.triggers
            .iter()
            .find(|x| x.name == n)
            .unwrap_or_else(|| panic!("{n} in {:?}", t.triggers))
            .clone()
    };
    let a = tr(&t, "tr_a");
    assert_eq!(
        a.action,
        TriggerAction::Body("begin\n  SET NOCOUNT ON;\nend".into())
    );
    assert_eq!(a.events, [TriggerEvent::Insert, TriggerEvent::Delete]);
    assert_eq!(a.tsql.execute_as, Some(ExecuteAs::Owner));
    assert!(a.tsql.not_for_replication);
    assert_eq!(a.tsql.rank, [(TriggerEvent::Insert, FiringRank::First)]);
    assert_eq!(a.enabled, TriggerEnabled::Disabled);
    assert!(tr(&t, "tr_c").tsql.hidden && !tr(&t, "tr_c").is_editable());
    assert!(
        diff_triggers(&t.triggers, &TriggerSetDraft::from_table(&t), MS)
            .changes
            .is_empty(),
        "the round-trip gate"
    );

    // Edit tr_a's body in place, rename tr_b, keep tr_c untouched.
    let mut d = TriggerSetDraft::from_table(&t);
    for x in &mut d.triggers {
        match x.original.as_deref() {
            Some("tr_a") => {
                x.info.action = TriggerAction::Body("INSERT dbo.log VALUES (N'a')".into())
            }
            Some("tr_b") => x.info.name = "tr_b2".into(),
            _ => {}
        }
    }
    assert!(
        d.validate(&t.triggers, MS, schemaic_core::ddl::TriggerHost::Table)
            .is_empty()
    );
    let stmts = diff_triggers(&t.triggers, &d, MS).emit();
    assert!(
        !stmts
            .iter()
            .any(|x| x.contains("DROP TRIGGER [dbo].[tr_a]")),
        "altered, not dropped: {stmts:#?}"
    );
    s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
    let t2 = read_table(&s, "t").await;
    let a2 = tr(&t2, "tr_a");
    assert_eq!(
        a2.action,
        TriggerAction::Body("INSERT dbo.log VALUES (N'a')".into())
    );
    assert_eq!(a2.tsql.execute_as, Some(ExecuteAs::Owner));
    assert!(a2.tsql.not_for_replication);
    assert_eq!(
        a2.tsql.rank,
        [(TriggerEvent::Insert, FiringRank::First)],
        "rank restored"
    );
    assert_eq!(a2.enabled, TriggerEnabled::Disabled, "still disabled");
    assert!(t2.triggers.iter().any(|x| x.name == "tr_b2"));
    assert!(!t2.triggers.iter().any(|x| x.name == "tr_b"));
    assert!(tr(&t2, "tr_c").tsql.hidden, "the encrypted one untouched");
    assert!(
        diff_triggers(&t2.triggers, &TriggerSetDraft::from_table(&t2), MS)
            .changes
            .is_empty(),
        "the round-trip gate, after the write"
    );
    // The renamed one fires; the disabled one does not, until enabled.
    s.exec("INSERT dbo.t VALUES (1); UPDATE dbo.t SET id = 2")
        .await;
    assert_eq!(
        s.scalar("SELECT STRING_AGG(what, ',') FROM dbo.log").await,
        "b"
    );
    s.exec("ENABLE TRIGGER dbo.tr_a ON dbo.t; INSERT dbo.t VALUES (3)")
        .await;
    assert_eq!(
        s.scalar("SELECT STRING_AGG(what, ',') WITHIN GROUP (ORDER BY what) FROM dbo.log")
            .await,
        "a,b"
    );

    // A new INSTEAD OF trigger on the view, fired by a write to it.
    let v = read_table(&s, "v").await;
    let mut d = TriggerSetDraft::from_table(&v);
    let mut fresh = TriggerDraft::blank("tr_v", "v", Some("dbo".into()));
    fresh.info.timing = TriggerTiming::InsteadOf;
    fresh.info.level = schemaic_core::schema::TriggerLevel::Statement;
    fresh.info.events = vec![TriggerEvent::Insert];
    fresh.info.action = TriggerAction::Body("INSERT dbo.log SELECT N'v' FROM inserted".into());
    d.triggers.push(fresh);
    assert!(
        d.validate(&v.triggers, MS, schemaic_core::ddl::TriggerHost::View)
            .is_empty()
    );
    let stmts = diff_triggers(&v.triggers, &d, MS).emit();
    s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
    s.exec("INSERT dbo.v VALUES (9)").await;
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM dbo.log WHERE what = N'v'")
            .await,
        "1"
    );
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM dbo.t WHERE id = 9").await,
        "0",
        "instead of"
    );
}

/// Import `rows` into `dbo.imp (id, name)` on `s`.
async fn import_into(
    s: &Scratch,
    columns: &[&str],
    rows: &mut (dyn Iterator<Item = Result<Vec<Value>, String>> + Send),
    cancel: CancellationToken,
) -> Result<u64, DbError> {
    let columns: Vec<String> = columns.iter().map(|c| c.to_string()).collect();
    s.db.import_rows(
        schemaic_db::ImportTarget {
            database: &s.name,
            schema: Some("dbo"),
            table: "imp",
            columns: &columns,
        },
        rows,
        cancel,
    )
    .await
}

/// **An import loads every row across batches** — text as `N'…'`, so a
/// non-Latin name survives, a NULL as NULL — and an identity column the file
/// supplies is written under `IDENTITY_INSERT`, which is off again after.
#[tokio::test(flavor = "multi_thread")]
async fn an_import_loads_every_row_across_batches() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("import").await;
    s.exec("CREATE TABLE dbo.imp (id int IDENTITY(1,1) PRIMARY KEY, name nvarchar(40) NULL)")
        .await;
    let n = schemaic_core::import::INSERT_BATCH_ROWS * 2 + 7;
    let mut rows = (1..=n).map(|i| {
        Ok(vec![
            Value::Int(i as i64 * 10),
            if i == 3 {
                Value::Null
            } else {
                Value::Str(format!("Ωμέγα {i}"))
            },
        ])
    });
    let wrote = import_into(&s, &["id", "name"], &mut rows, CancellationToken::new())
        .await
        .expect("imported");
    assert_eq!(wrote, n as u64);
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM dbo.imp").await,
        n.to_string()
    );
    assert_eq!(
        s.scalar("SELECT name FROM dbo.imp WHERE id = 20").await,
        "Ωμέγα 2"
    );
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM dbo.imp WHERE name IS NULL")
            .await,
        "1"
    );
    // Off again: an insert that leaves the identity to the server works.
    s.exec("INSERT dbo.imp (name) VALUES (N'after')").await;
}

/// **A row the server refuses rolls the whole import back**, batches already
/// sent included — a duplicate key past the first batch boundary.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_import_row_rolls_the_whole_import_back() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("import_dup").await;
    s.exec("CREATE TABLE dbo.imp (id int NOT NULL PRIMARY KEY, name nvarchar(40) NULL)")
        .await;
    let collide_at = schemaic_core::import::INSERT_BATCH_ROWS + 10;
    let mut rows = (1..=collide_at + 5).map(|i| {
        let id = if i == collide_at { 1 } else { i as i64 };
        Ok(vec![Value::Int(id), Value::Str(format!("r{i}"))])
    });
    let err = import_into(&s, &["id", "name"], &mut rows, CancellationToken::new())
        .await
        .expect_err("a duplicate key");
    assert!(err.to_string().contains("Msg 2627"), "{err}");
    assert_eq!(s.scalar("SELECT COUNT(*) FROM dbo.imp").await, "0");
}

/// **An empty field bound for a number is refused, not stored as 0** — the
/// import rolls back whole and names the column and the row.
#[tokio::test(flavor = "multi_thread")]
async fn an_imported_blank_number_is_refused_not_stored_as_zero() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("import_blank").await;
    s.exec("CREATE TABLE dbo.imp (id int NOT NULL PRIMARY KEY, qty int NULL)")
        .await;
    let mut rows = (1..=3).map(|i| {
        Ok(vec![
            Value::Int(i),
            Value::Str(if i == 3 { String::new() } else { i.to_string() }),
        ])
    });
    let err = import_into(&s, &["id", "qty"], &mut rows, CancellationToken::new())
        .await
        .expect_err("refused");
    assert!(matches!(err, DbError::Refused(_)), "{err:?}");
    assert!(
        err.to_string().contains("Row 3") && err.to_string().contains("qty"),
        "{err}"
    );
    assert_eq!(s.scalar("SELECT COUNT(*) FROM dbo.imp").await, "0");
}

/// **Stop mid-import rolls back and says so.**
#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_import_rolls_back_and_says_so() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("import_stop").await;
    s.exec("CREATE TABLE dbo.imp (id int NOT NULL PRIMARY KEY, name nvarchar(40) NULL)")
        .await;
    let per = schemaic_core::import::INSERT_BATCH_ROWS;
    let mut rows = (1..=per * 6).map(move |i| {
        if i % per == 0 {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        Ok(vec![Value::Int(i as i64), Value::Str(format!("row {i}"))])
    });
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        stop.cancel();
    });
    let err = import_into(&s, &["id", "name"], &mut rows, cancel)
        .await
        .expect_err("stopped");
    assert!(matches!(err, DbError::Cancelled), "{err:?}");
    assert_eq!(s.scalar("SELECT COUNT(*) FROM dbo.imp").await, "0");
}

/// A primary key widened to two columns: dropped by the constraint name
/// introspection read, and added over both.
#[tokio::test(flavor = "multi_thread")]
async fn a_primary_key_is_replaced() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_pk").await;
    s.exec(
        "CREATE TABLE dbo.t (a int NOT NULL, b int NOT NULL, CONSTRAINT pk_t PRIMARY KEY (a)); \
         INSERT dbo.t VALUES (1, 1), (2, 2)",
    )
    .await;
    let t = read_table(&s, "t").await;
    let mut d = TableDraft::from_table(&t);
    d.primary_key = vec!["a".into(), "b".into()];
    let err = apply_draft_err_free(&s, &t, &d).await;
    assert!(err.is_none(), "{err:?}");
    let t2 = read_table(&s, "t").await;
    let pk = t2
        .indexes
        .iter()
        .find(|i| i.name == "PRIMARY")
        .expect("a key");
    assert_eq!(pk.column_names().collect::<Vec<_>>(), vec!["a", "b"]);
}

/// [`apply_draft`], answering the failure instead of panicking on it.
async fn apply_draft_err_free(
    s: &Scratch,
    current: &schemaic_core::schema::TableInfo,
    draft: &schemaic_core::ddl::TableDraft,
) -> Option<String> {
    let cs = schemaic_core::ddl::diff(current, draft, MS);
    if !cs.unsupported().is_empty() {
        return Some(format!("withheld: {:?}", cs.unsupported()));
    }
    s.db.run_ddl(&s.name, &cs.emit(), CancellationToken::new())
        .await
        .err()
        .map(|e| e.to_string())
}

/// **What the review of the designer found, against the server.** A named
/// default keeps its name across a retype and is untouched by a nullability
/// change; a column an unchanged check names is renamed around the check;
/// keying a nullable column makes it `NOT NULL` first; a nullable column
/// added with a default fills the existing rows; and a disabled check that is
/// edited comes back disabled.
#[tokio::test(flavor = "multi_thread")]
async fn a_designer_edit_keeps_what_it_did_not_change() {
    use schemaic_core::ddl::{ColumnDraft, TableDraft};
    use schemaic_core::schema::ColumnInfo;
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_keep").await;
    s.exec(
        "CREATE TABLE dbo.t (id int NOT NULL CONSTRAINT pk_t PRIMARY KEY, \
         qty int NULL CONSTRAINT df_qty DEFAULT 0, \
         n int NULL CONSTRAINT df_n DEFAULT 1, \
         k int NULL, \
         w int NULL CONSTRAINT ck_w CHECK (w > 0), \
         CONSTRAINT ck_qty CHECK (qty >= 0)); \
         ALTER TABLE dbo.t NOCHECK CONSTRAINT ck_w; \
         INSERT dbo.t (id, qty, n, k) VALUES (1, 5, 2, 10)",
    )
    .await;
    let t = read_table(&s, "t").await;
    let mut d = TableDraft::from_table(&t);
    let col = |d: &mut TableDraft, n: &str| -> usize {
        d.columns.iter().position(|c| c.info.name == n).expect(n)
    };
    // Retype `qty`, and rename it though `ck_qty` still names it.
    let i = col(&mut d, "qty");
    d.columns[i].info.type_name = "bigint".into();
    d.columns[i].info.name = "amount".into();
    // Only nullability for `n`.
    let i = col(&mut d, "n");
    d.columns[i].info.nullable = false;
    // Key `k` too, left nullable in the draft.
    d.primary_key = vec!["id".into(), "k".into()];
    // A disabled check's predicate edited.
    let w = d
        .check_constraints
        .iter_mut()
        .find(|c| c.info.name == "ck_w")
        .expect("ck_w");
    w.info.expression = "[w] > 1".into();
    // A nullable column with a default, added.
    d.columns.push(ColumnDraft::new(ColumnInfo {
        name: "status".into(),
        type_name: "nvarchar(10)".into(),
        nullable: true,
        default: Some("N'new'".into()),
        ..Default::default()
    }));
    let stmts = apply_draft(&s, &t, &d).await;

    let defaults = s
        .scalar(
            "SELECT STRING_AGG(CONCAT(c.name, '=', dc.name), ',') WITHIN GROUP (ORDER BY c.name) \
             FROM sys.default_constraints dc JOIN sys.columns c \
               ON c.object_id = dc.parent_object_id AND c.column_id = dc.parent_column_id \
             WHERE dc.parent_object_id = OBJECT_ID(N'dbo.t') AND c.name IN ('amount', 'n')",
        )
        .await;
    assert_eq!(defaults, "amount=df_qty,n=df_n", "{stmts:#?}");
    assert_eq!(
        s.scalar("SELECT definition FROM sys.check_constraints WHERE name = 'ck_qty'")
            .await,
        "([amount]>=(0))"
    );
    assert_eq!(
        s.scalar("SELECT CAST(is_disabled AS int) FROM sys.check_constraints WHERE name = 'ck_w'")
            .await,
        "1",
        "still disabled"
    );
    assert_eq!(
        s.scalar("SELECT status FROM dbo.t WHERE id = 1").await,
        "new"
    );
    let t2 = read_table(&s, "t").await;
    let pk = t2
        .indexes
        .iter()
        .find(|i| i.name == "PRIMARY")
        .expect("a key");
    assert_eq!(pk.constraint.as_deref(), Some("pk_t"), "kept its name");
    assert_eq!(pk.column_names().collect::<Vec<_>>(), vec!["id", "k"]);
    let again = schemaic_core::ddl::diff(&t2, &TableDraft::from_table(&t2), MS);
    assert!(again.changes.is_empty(), "{:?}", again.changes);
}

/// **A clustered index is edited as one, and a nonclustered key stays one**:
/// `cx` gains a column and comes back `CLUSTERED` rather than leaving the
/// table a heap, and the `NONCLUSTERED` key, rebuilt around a retype, comes
/// back nonclustered rather than taking T-SQL's default — which, with `cx`
/// there, the server would refuse (Msg 1902).
#[tokio::test(flavor = "multi_thread")]
async fn a_clustered_index_and_a_nonclustered_key_keep_their_clustering() {
    use schemaic_core::ddl::TableDraft;
    use schemaic_core::schema::IndexColumn;
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_cx").await;
    s.exec(
        "CREATE TABLE dbo.t (id int NOT NULL CONSTRAINT pk_t PRIMARY KEY NONCLUSTERED, d int); \
         CREATE CLUSTERED INDEX cx ON dbo.t (d)",
    )
    .await;
    let t = read_table(&s, "t").await;
    let ix = |t: &schemaic_core::schema::TableInfo, name: &str| {
        t.indexes
            .iter()
            .find(|i| i.name == name)
            .unwrap_or_else(|| panic!("{name} in {:?}", t.indexes))
            .clone()
    };
    assert_eq!(ix(&t, "cx").clustered, Some(true));
    assert!(!ix(&t, "cx").lossy);
    assert_eq!(ix(&t, "PRIMARY").clustered, Some(false));

    let mut d = TableDraft::from_table(&t);
    d.indexes[0].info.columns.push(IndexColumn::plain("id"));
    d.columns[0].info.type_name = "bigint".into();
    let stmts = apply_draft(&s, &t, &d).await;
    let t2 = read_table(&s, "t").await;
    assert_eq!(ix(&t2, "cx").clustered, Some(true), "{stmts:#?}");
    assert_eq!(ix(&t2, "cx").column_names().count(), 2);
    assert_eq!(ix(&t2, "PRIMARY").clustered, Some(false), "{stmts:#?}");
    let again = schemaic_core::ddl::diff(&t2, &TableDraft::from_table(&t2), MS);
    assert!(again.changes.is_empty(), "{:?}", again.changes);
}

/// **An identity switched on is withheld, not applied** — T-SQL's `ALTER
/// COLUMN` cannot, and the preview says so and keeps Apply closed rather
/// than writing half the edit.
#[tokio::test(flavor = "multi_thread")]
async fn an_identity_toggle_is_withheld() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_ident").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL)").await;
    let t = read_table(&s, "t").await;
    let mut d = TableDraft::from_table(&t);
    d.columns[0].info.auto_increment = true;
    let cs = schemaic_core::ddl::diff(&t, &d, MS);
    assert_eq!(cs.unsupported().len(), 1, "{:?}", cs.unsupported());
    assert!(cs.emit().is_empty(), "{:?}", cs.emit());
}

/// **A key finds its row whatever its type** — as the grid read it back: a
/// `real` by its shortest digits (as an `f64` it is another number), a
/// `datetime2`, a `uniqueidentifier` and a `decimal` by the text SQL Server
/// printed, and a `bit` set from `true`.
#[tokio::test(flavor = "multi_thread")]
async fn keys_of_every_shape_find_their_row() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("wb_keys").await;
    s.exec(
        "CREATE TABLE dbo.t (r real NOT NULL, at datetime2(7) NOT NULL, \
         g uniqueidentifier NOT NULL, d decimal(10,2) NOT NULL, flag bit NULL, \
         CONSTRAINT pk PRIMARY KEY (r, at, g, d)); \
         INSERT dbo.t VALUES (0.1, '2026-09-27 12:50:53.1234567', \
         '37ab5dac-1262-4372-82ba-caad1925cd9a', -0.05, 0)",
    )
    .await;
    // The key values exactly as a read of the table renders them.
    let rs = s.exec("SELECT r, at, g, d FROM dbo.t").await;
    let key: Vec<(String, Value)> = ["r", "at", "g", "d"]
        .iter()
        .enumerate()
        .map(|(i, c)| (c.to_string(), rs.cell(0, i).expect("a cell").to_value()))
        .collect();
    let mut e = row_edit(&s, "t", &[("flag", txt("true"))], &[]);
    e.key = key;
    let n = commit(
        &s,
        GridWrite {
            updates: vec![e],
            ..Default::default()
        },
    )
    .await
    .expect("every key column matched");
    assert_eq!(n, 1);
    assert_eq!(s.scalar("SELECT CAST(flag AS int) FROM dbo.t").await, "1");
}

/// **A routine body keeps its semicolons, and `GO` is never sent.** What the
/// splitter makes of a script is what the server accepts.
#[tokio::test(flavor = "multi_thread")]
async fn a_script_split_at_go_runs_as_sql_servers_tools_run_it() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("go").await;
    let script = "CREATE TABLE dbo.t (a int);\nGO\n\
                  CREATE PROCEDURE dbo.fill AS BEGIN INSERT dbo.t VALUES (1); \
                  INSERT dbo.t VALUES (2); END;\nGO\n\
                  EXEC dbo.fill;\nGO 2\nSELECT COUNT(*) FROM dbo.t";
    let stmts = schemaic_core::sql::executable_statements(script, MS);
    assert_eq!(stmts.len(), 4, "{stmts:?}");
    let mut outcomes = Vec::new();
    s.db.run_batch(
        Some(&s.name),
        &stmts,
        10,
        CancellationToken::new(),
        |_, r| outcomes.push(r),
    )
    .await;
    for (i, o) in outcomes.iter().enumerate() {
        assert!(o.is_ok(), "statement {i}: {o:?}");
    }
    // `GO 2`'s count is accepted and not honoured: the procedure ran once.
    assert_eq!(s.scalar("SELECT COUNT(*) FROM dbo.t").await, "2");
}

/// The schema reads back as it was declared.
#[tokio::test(flavor = "multi_thread")]
async fn introspection_reads_the_schema_as_declared() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("schema").await;
    s.exec("CREATE SCHEMA sales").await;
    s.exec("CREATE TYPE dbo.Phone FROM varchar(20) NULL").await;
    s.exec(
        "CREATE TABLE dbo.customers ( \
           id int IDENTITY(1,1) CONSTRAINT pk_c PRIMARY KEY, \
           name nvarchar(50) NOT NULL, \
           balance decimal(10,2) NOT NULL CONSTRAINT df_b DEFAULT ((0)), \
           doubled AS (balance * 2) PERSISTED, \
           CONSTRAINT ck_b CHECK (balance >= 0)); \
         CREATE TABLE sales.orders ( \
           id bigint NOT NULL PRIMARY KEY, \
           customer int NOT NULL CONSTRAINT fk_o FOREIGN KEY REFERENCES dbo.customers(id) \
             ON DELETE CASCADE, \
           phone dbo.Phone)",
    )
    .await;
    s.exec("CREATE VIEW dbo.v AS SELECT id, name AS [AS] FROM dbo.customers")
        .await;
    s.exec("CREATE TRIGGER dbo.tr ON dbo.customers AFTER INSERT, DELETE AS SET NOCOUNT ON")
        .await;
    s.exec("CREATE FUNCTION dbo.twice (@x int) RETURNS int AS BEGIN RETURN @x * 2 END")
        .await;
    s.exec("CREATE PROCEDURE dbo.purge @before date AS DELETE FROM dbo.customers WHERE 1 = 0")
        .await;
    s.exec("CREATE TABLE dbo.facts (id int, v int); CREATE CLUSTERED COLUMNSTORE INDEX cci ON dbo.facts")
        .await;
    let schema =
        s.db.fetch_schema(&s.name, CancellationToken::new())
            .await
            .expect("the schema");
    let t = |ns: &str, name: &str| {
        schema
            .tables
            .iter()
            .find(|t| t.schema.as_deref() == Some(ns) && t.name == name)
            .unwrap_or_else(|| panic!("{ns}.{name} in {:?}", schema.tables))
    };
    let c = t("dbo", "customers");
    let col = |n: &str| c.columns.iter().find(|x| x.name == n).expect(n);
    assert!(col("id").primary_key && col("id").auto_increment);
    assert_eq!(col("name").type_name, "nvarchar(50)");
    assert_eq!(col("balance").default.as_deref(), Some("0"));
    assert_eq!(col("doubled").generated.as_deref(), Some("[balance]*(2)"));
    assert!(col("doubled").generated_stored);
    assert_eq!(c.check_constraints[0].expression, "[balance]>=(0)");
    let pk = c
        .indexes
        .iter()
        .find(|i| i.name == "PRIMARY")
        .expect("the key");
    assert_eq!(pk.constraint.as_deref(), Some("pk_c"));
    assert_eq!(c.triggers.len(), 1);
    assert_eq!(c.triggers[0].events.len(), 2);
    // Its DDL is rebuilt from the parts the stored statement was read into.
    assert_eq!(
        c.triggers[0].create_sql(MS),
        "CREATE TRIGGER [dbo].[tr] ON [dbo].[customers] AFTER INSERT, DELETE\nAS\nSET NOCOUNT ON"
    );
    let o = t("sales", "orders");
    assert_eq!(o.foreign_keys[0].ref_schema.as_deref(), Some("dbo"));
    assert_eq!(o.foreign_keys[0].on_delete.as_deref(), Some("CASCADE"));
    // An alias type is named in its own schema, not the table's: bare, `Phone`
    // in `sales.orders`' DDL would resolve through the login's default schema.
    let phone = o.columns.iter().find(|c| c.name == "phone").expect("phone");
    assert_eq!(phone.type_name, "[dbo].[Phone]");
    let v = t("dbo", "v");
    assert!(v.is_view);
    assert_eq!(
        v.view_definition.as_deref(),
        Some("SELECT id, name AS [AS] FROM dbo.customers")
    );
    assert_eq!(schema.tables[0].schema.as_deref(), Some("dbo"), "dbo first");
    let f = schema
        .routines
        .iter()
        .find(|r| r.name == "twice")
        .expect("the function");
    assert_eq!(
        (f.arguments.as_str(), f.returns.as_str()),
        ("@x int", "int")
    );
    assert_eq!(f.kind, schemaic_core::schema::RoutineKind::Function);
    // `sys.objects.type` is `char(2)`: a procedure's `P` arrives as `P `.
    let p = schema
        .routines
        .iter()
        .find(|r| r.name == "purge")
        .expect("the procedure");
    assert_eq!(p.kind, schemaic_core::schema::RoutineKind::Procedure);
    assert_eq!(p.arguments, "@before date");
    // A columnstore index has no key columns, and is still an index — one the
    // table's DDL names rather than drops in silence.
    let facts = t("dbo", "facts");
    let cci = facts
        .indexes
        .iter()
        .find(|i| i.name == "cci")
        .unwrap_or_else(|| panic!("the columnstore index: {:?}", facts.indexes));
    assert!(cci.lossy);
    let ddl = facts.create_ddl(MS);
    assert!(ddl.contains("-- Index cci "), "{ddl}");
    // And the list alone agrees with the whole.
    let list = s.db.fetch_table_list(&s.name).await.expect("the list");
    assert_eq!(list.tables.len(), schema.tables.len());
    // A row count past `int` is what `COUNT_BIG` is for; two is enough to see
    // the statement run.
    s.exec("INSERT dbo.customers (name) VALUES (N'a'), (N'b')")
        .await;
    let n =
        s.db.count_rows(&s.name, Some("dbo"), "customers", CancellationToken::new())
            .await
            .expect("a count");
    assert_eq!(n, 2);
}

/// **The DDL *Show DDL* writes builds the table it was read from.** Read a
/// table, emit its `CREATE`, run that in a second database, and read the copy:
/// the columns, keys, checks and indexes come back the same.
#[tokio::test(flavor = "multi_thread")]
async fn a_tables_ddl_rebuilds_the_table_it_was_read_from() {
    if !enabled() {
        return;
    }
    let src = Scratch::create("ddl_src").await;
    let dst = Scratch::create("ddl_dst").await;
    src.exec(
        "CREATE TABLE dbo.t ( \
           id int IDENTITY(1000,-5) CONSTRAINT pk_t PRIMARY KEY, \
           [odd]]name] nvarchar(40) COLLATE Latin1_General_BIN NULL, \
           balance decimal(10,2) NOT NULL DEFAULT ((0)), \
           doubled AS (balance * 2) PERSISTED, \
           code varchar(8) NOT NULL CONSTRAINT uq_code UNIQUE, \
           CONSTRAINT ck_t CHECK (balance >= 0)); \
         CREATE INDEX ix_bal ON dbo.t (balance DESC) WHERE balance > 0; \
         EXEC sp_addextendedproperty N'MS_Description', N'ledger', N'SCHEMA', N'dbo', N'TABLE', N't'; \
         EXEC sp_addextendedproperty N'MS_Description', N'owed', \
           N'SCHEMA', N'dbo', N'TABLE', N't', N'COLUMN', N'balance';",
    )
    .await;
    let read = |s: &Scratch| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            let schema = db
                .fetch_schema(&name, CancellationToken::new())
                .await
                .expect("schema");
            schema
                .tables
                .into_iter()
                .find(|t| t.name == "t")
                .expect("the table")
        }
    };
    let original = read(&src).await;
    assert_eq!(
        original.columns[0].identity_spec,
        Some(("1000".into(), "-5".into()))
    );
    let ddl = original.create_ddl(MS);
    for stmt in schemaic_core::sql::executable_statements(&ddl, MS) {
        dst.exec(&stmt).await;
    }
    let copy = read(&dst).await;
    let cols = |t: &schemaic_core::schema::TableInfo| -> Vec<String> {
        t.columns
            .iter()
            .map(|c| {
                format!(
                    "{} {} null={} pk={} id={} {:?} def={:?} gen={:?} {} coll={:?} {:?}",
                    c.name,
                    c.type_name,
                    c.nullable,
                    c.primary_key,
                    c.auto_increment,
                    c.identity_spec,
                    c.default,
                    c.generated,
                    c.generated_stored,
                    c.collation,
                    c.comment
                )
            })
            .collect()
    };
    assert_eq!(cols(&copy), cols(&original), "{ddl}");
    assert_eq!(copy.comment.as_deref(), Some("ledger"), "{ddl}");
    let idx = |t: &schemaic_core::schema::TableInfo| -> Vec<String> {
        let mut v: Vec<String> = t
            .indexes
            .iter()
            .map(|i| {
                let names: Vec<&str> = i.column_names().collect();
                format!(
                    "{} {names:?} {} {:?} {:?}",
                    i.name, i.unique, i.predicate, i.constraint
                )
            })
            .collect();
        v.sort();
        v
    };
    assert_eq!(idx(&copy), idx(&original), "{ddl}");
    assert_eq!(
        copy.check_constraints[0].expression,
        original.check_constraints[0].expression
    );
}

/// A long statement stops when asked, and the server has stopped it too.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_query_stops_on_the_server() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("cancel").await;
    let cancel = CancellationToken::new();
    let trip = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        trip.cancel();
    });
    let started = std::time::Instant::now();
    let r =
        s.db.fetch_query(
            Some(&s.name),
            "WAITFOR DELAY '00:00:30'; SELECT 1 /* schemaic_cancel_probe */",
            1,
            cancel,
        )
        .await;
    assert!(matches!(r, Err(DbError::Cancelled)), "{r:?}");
    assert!(started.elapsed().as_secs() < 10, "{:?}", started.elapsed());
    let running = s
        .scalar(
            "SELECT COUNT(*) FROM sys.dm_exec_requests r \
             CROSS APPLY sys.dm_exec_sql_text(r.sql_handle) t \
             WHERE t.text LIKE '%schemaic_cancel' + '_probe%' AND r.session_id <> @@SPID",
        )
        .await;
    assert_eq!(running, "0");
}

/// **Stop answers while the statement is still being compiled.** The read
/// describes its result first, and the describe compiles the statement —
/// which waits behind another session's schema lock. Awaited without the
/// cancel beside it, Stop did nothing until the lock let go.
#[tokio::test(flavor = "multi_thread")]
async fn a_query_waiting_to_compile_stops_when_asked() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("cancel_compile").await;
    s.exec("CREATE TABLE dbo.t (a int)").await;
    // Another session holds the table's schema lock for five seconds.
    let holder = {
        let (db, name) = (s.db.clone(), s.name.clone());
        tokio::spawn(async move {
            db.fetch_query(
                Some(&name),
                "BEGIN TRANSACTION; ALTER TABLE dbo.t ADD b int; \
                 WAITFOR DELAY '00:00:05'; ROLLBACK",
                1,
                CancellationToken::new(),
            )
            .await
        })
    };
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let cancel = CancellationToken::new();
    let trip = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        trip.cancel();
    });
    let started = std::time::Instant::now();
    let r =
        s.db.fetch_query(Some(&s.name), "SELECT a FROM dbo.t", 10, cancel)
            .await;
    assert!(matches!(r, Err(DbError::Cancelled)), "{r:?}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "Stop waited for the lock: {:?}",
        started.elapsed()
    );
    let _ = holder.await;
}

/// The activity panel sees the server's other sessions — not the one
/// polling, as on the other engines — and will end one but not cancel its
/// statement: SQL Server has no statement for that.
#[tokio::test(flavor = "multi_thread")]
async fn activity_lists_sessions_and_offers_only_a_kill() {
    if !enabled() {
        return;
    }
    // A session to be seen: a wait on a connection of its own.
    let waiting = tokio::spawn(async {
        base_db()
            .fetch_query(
                None,
                "WAITFOR DELAY '00:00:04' /* schemaic_activity_probe */",
                1,
                CancellationToken::new(),
            )
            .await
    });
    let probe = |s: &schemaic_core::activity::SessionInfo| {
        s.sql
            .as_deref()
            .is_some_and(|q| q.contains("schemaic_activity_probe"))
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let sessions = loop {
        let sessions = base_db().fetch_sessions().await.expect("a session list");
        if sessions.iter().any(probe) || std::time::Instant::now() > deadline {
            break sessions;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    };
    let waiter = sessions
        .iter()
        .find(|s| probe(s))
        .unwrap_or_else(|| panic!("the waiting session is listed: {sessions:?}"));
    // The poll is not: its statement is the activity query itself, the one
    // text that reads `open_transaction_count`.
    assert!(
        !sessions.iter().any(|s| s
            .sql
            .as_deref()
            .is_some_and(|q| q.contains("open_transaction_count"))),
        "{sessions:?}"
    );
    let refused = base_db()
        .kill_session(waiter.id, schemaic_core::activity::KillKind::Query)
        .await;
    assert!(matches!(refused, Err(DbError::Refused(_))), "{refused:?}");
    let _ = waiting.await;
}

/// **Every name in the read-only gate's function allowlist is a SQL Server
/// builtin**, asked of the server the only way it answers: a name that is
/// not one is error 195 when called, and every listed name must answer
/// something else. A misspelt entry is a function nobody can call.
#[tokio::test(flavor = "multi_thread")]
async fn every_allowlisted_function_is_a_builtin() {
    if !enabled() {
        return;
    }
    // A column of the gate's own, spelled nowhere else: if the list moves,
    // this reads the moved list.
    let probe = "SELECT 1";
    assert!(schemaic_core::sql::read_only_reason(probe, MS).is_ok());
    let names = schemaic_core::sql::mssql_read_functions_for_test();
    // Rowset functions and grammar positions answer 195 as a bare call, and
    // are checked where they stand instead.
    const IN_PLACE: &[&str] = &[
        "openjson",
        "string_split",
        "generate_series",
        "binary",
        "datetime2",
        "datetimeoffset",
        "decimal",
        "float",
        "numeric",
        "nvarchar",
        "time",
        "varbinary",
        "varchar",
        "path",
        "raw",
        "root",
    ];
    let mut unknown = Vec::new();
    for name in names.iter().filter(|n| !IN_PLACE.contains(n)) {
        let sql = format!(
            "BEGIN TRY EXEC('SELECT {name}()'); SELECT 0 END TRY \
             BEGIN CATCH SELECT ERROR_NUMBER() END CATCH"
        );
        let rs = base_db()
            .fetch_query(None, &sql, 1, CancellationToken::new())
            .await
            .expect("the probe runs");
        if rs.cell(0, 0).map(|c| c.text().to_string()).as_deref() == Some("195") {
            unknown.push(*name);
        }
    }
    assert!(unknown.is_empty(), "not SQL Server builtins: {unknown:?}");
    for sql in [
        "SELECT * FROM OPENJSON(N'[1]')",
        "SELECT * FROM STRING_SPLIT('a,b', ',')",
        "SELECT * FROM GENERATE_SERIES(1, 2)",
        "SELECT CONVERT(nvarchar(5), 1), CONVERT(varbinary(4), 1), CONVERT(varchar(3), 1), \
         CONVERT(datetime2(3), GETDATE()), CONVERT(decimal(5,2), 1), CONVERT(float(24), 1), \
         CONVERT(time(0), GETDATE()), CONVERT(datetimeoffset(0), GETDATE()), \
         CONVERT(binary(2), 1), CONVERT(numeric(4,1), 1)",
        "SELECT 1 AS a FOR XML PATH('r'), ROOT('x')",
        "SELECT 1 AS a FOR XML RAW('r')",
    ] {
        assert!(
            schemaic_core::sql::read_only_reason(sql, MS).is_ok(),
            "{sql}"
        );
        base_db()
            .fetch_query(None, sql, 10, CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
    for name in schemaic_core::sql::mssql_sys_read_functions_for_test() {
        let rs = base_db()
            .fetch_query(
                None,
                &format!("SELECT OBJECT_ID('sys.{name}')"),
                1,
                CancellationToken::new(),
            )
            .await
            .expect("the probe runs");
        assert!(
            rs.cell(0, 0).is_some_and(|c| !c.is_null()),
            "sys.{name} is not a system object"
        );
    }
}

/// Every snippet shipped for SQL Server runs, and the read-only gate lets it.
#[tokio::test(flavor = "multi_thread")]
async fn every_builtin_snippet_runs() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("snippets").await;
    for snippet in schemaic_core::snippet::builtins(MS) {
        assert!(
            schemaic_core::sql::read_only_reason(&snippet.body, MS).is_ok(),
            "{}: {:?}",
            snippet.name,
            schemaic_core::sql::read_only_reason(&snippet.body, MS)
        );
        s.try_exec(&snippet.body)
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", snippet.name));
    }
}

/// **Every name the SQL Server catalog holds is a builtin this server knows.**
///
/// T-SQL's intrinsics are in no catalog view, so the list is hand-written
/// (`core::mssql_builtins`), and the parser is its oracle: a name T-SQL does
/// not know fails to compile with Msg 195, *is not a recognized built-in
/// function name*, where a builtin called with the wrong arguments fails some
/// other way — and nothing runs either time. So each entry is called with no
/// arguments, in `FROM` for the rowset ones and bare for the niladic ones, and
/// only a 195 fails the test. It checks the over-listing direction; a builtin
/// the list lacks has no oracle.
#[tokio::test(flavor = "multi_thread")]
async fn every_catalogued_builtin_is_one_the_server_knows() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("builtins").await;
    // The oracle can see a failure: a name nobody has is a 195 on this path.
    let bogus = s
        .try_exec("SELECT SCHEMAIC_NO_SUCH_FUNCTION()")
        .await
        .expect_err("an unknown function");
    assert!(bogus.to_string().contains("(Msg 195"), "{bogus}");
    const ROWSET: &[&str] = &[
        "CONTAINSTABLE",
        "FREETEXTTABLE",
        "GENERATE_SERIES",
        "OPENDATASOURCE",
        "OPENJSON",
        "OPENQUERY",
        "OPENROWSET",
        "OPENXML",
        "PREDICT",
        "STRING_SPLIT",
    ];
    let mut unknown = Vec::new();
    for f in schemaic_core::mssql_builtins::MSSQL_FUNCTIONS {
        let call = if !f.signature.contains('(') {
            format!("SELECT {}", f.name)
        } else if ROWSET.contains(&f.name) {
            format!("SELECT * FROM {}()", f.name)
        } else {
            format!("SELECT {}()", f.name)
        };
        if let Err(e) = s.try_exec(&call).await
            && e.to_string().contains("(Msg 195")
        {
            unknown.push(format!("{}: {e}", f.name));
        }
    }
    assert!(unknown.is_empty(), "{unknown:#?}");
}

// ── Manual transaction mode ─────────────────────────────────────────────────

/// A pinned session on `s`'s database, with its lazy transaction opened.
async fn manual(s: &Scratch) -> Arc<schemaic_db::session::Session> {
    let session = schemaic_db::session::Session::open(&s.db, Some(&s.name))
        .await
        .expect("a pinned session");
    session.ensure_tx().await.expect("BEGIN TRANSACTION");
    session
}

/// Rows another connection can see — `READPAST`, since under READ COMMITTED
/// a plain read would wait on the pinned session's locks until it ended.
async fn committed_rows(s: &Scratch) -> String {
    s.scalar("SELECT COUNT(*) FROM dbo.t WITH (READPAST)").await
}

/// One statement on the pinned session, for its outcome alone.
async fn stmt(
    session: &schemaic_db::session::Session,
    sql: &str,
) -> schemaic_core::tx::StmtOutcome {
    session
        .fetch_query(sql, 100, CancellationToken::new())
        .await
        .stmt
}

/// **Nothing commits until Commit, and Rollback undoes it.** The pinned
/// session's writes are invisible outside it until the button, and gone
/// after a rollback; the session reports its own server id (`@@SPID`).
#[tokio::test(flavor = "multi_thread")]
async fn a_manual_session_commits_only_when_told() {
    use schemaic_core::tx::StmtOutcome;
    if !enabled() {
        return;
    }
    let s = Scratch::create("tx_manual").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY)")
        .await;
    let session = manual(&s).await;
    assert!(session.server_id().is_some_and(|id| id > 0));
    assert_eq!(
        stmt(&session, "INSERT dbo.t VALUES (1)").await,
        StmtOutcome::Ok
    );
    assert_eq!(committed_rows(&s).await, "0", "not yet committed");
    session.commit().await.expect("commit");
    assert_eq!(committed_rows(&s).await, "1");

    session.ensure_tx().await.expect("BEGIN");
    assert_eq!(
        stmt(&session, "INSERT dbo.t VALUES (2)").await,
        StmtOutcome::Ok
    );
    session.rollback().await.expect("rollback");
    assert_eq!(committed_rows(&s).await, "1", "the rollback undid it");
    session.close().await;
}

/// **The server, not the text, decides what a T-SQL transaction statement
/// did.** A `COMMIT` inside a nested `BEGIN TRAN` closes nothing; a
/// `ROLLBACK TRANSACTION` to a savepoint keeps the transaction; a plain
/// `COMMIT` closes it, and the next statement gets a fresh `BEGIN`.
#[tokio::test(flavor = "multi_thread")]
async fn t_sql_transaction_statements_are_settled_by_the_server() {
    use schemaic_core::tx::StmtOutcome;
    if !enabled() {
        return;
    }
    let s = Scratch::create("tx_tsql").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY)")
        .await;
    let session = manual(&s).await;
    assert_eq!(
        stmt(&session, "INSERT dbo.t VALUES (1)").await,
        StmtOutcome::Ok
    );
    assert_eq!(stmt(&session, "BEGIN TRAN").await, StmtOutcome::Ok);
    assert_eq!(
        stmt(&session, "COMMIT").await,
        StmtOutcome::Ok,
        "the inner level only"
    );
    assert_eq!(committed_rows(&s).await, "0");
    assert_eq!(stmt(&session, "SAVE TRANSACTION sp").await, StmtOutcome::Ok);
    assert_eq!(
        stmt(&session, "INSERT dbo.t VALUES (2)").await,
        StmtOutcome::Ok
    );
    assert_eq!(
        stmt(&session, "ROLLBACK TRANSACTION sp").await,
        StmtOutcome::Ok,
        "still open"
    );
    assert_eq!(stmt(&session, "COMMIT").await, StmtOutcome::OkAndClosed);
    assert_eq!(
        committed_rows(&s).await,
        "1",
        "row 1 kept, row 2 rolled back"
    );
    // The next statement is in a new transaction, opened by `ensure_tx`.
    session.ensure_tx().await.expect("BEGIN");
    assert_eq!(
        stmt(&session, "INSERT dbo.t VALUES (3)").await,
        StmtOutcome::Ok
    );
    session.rollback().await.expect("rollback");
    assert_eq!(committed_rows(&s).await, "1");
    session.close().await;
}

/// **A failure leaves the transaction open — unless the server ended it.**
/// A duplicate key fails and keeps what came before; with `XACT_ABORT ON`
/// the same error rolls the transaction back, and the session says so.
#[tokio::test(flavor = "multi_thread")]
async fn a_failure_is_folded_as_the_server_left_the_transaction() {
    use schemaic_core::tx::StmtOutcome;
    if !enabled() {
        return;
    }
    let s = Scratch::create("tx_fail").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY)")
        .await;
    let session = manual(&s).await;
    assert_eq!(
        stmt(&session, "INSERT dbo.t VALUES (1)").await,
        StmtOutcome::Ok
    );
    assert_eq!(
        stmt(&session, "INSERT dbo.t VALUES (1)").await,
        StmtOutcome::Failed
    );
    session.commit().await.expect("still committable");
    assert_eq!(committed_rows(&s).await, "1");

    assert_eq!(stmt(&session, "SET XACT_ABORT ON").await, StmtOutcome::Ok);
    session.ensure_tx().await.expect("BEGIN");
    assert_eq!(
        stmt(&session, "INSERT dbo.t VALUES (2)").await,
        StmtOutcome::Ok
    );
    assert_eq!(
        stmt(&session, "INSERT dbo.t VALUES (1)").await,
        StmtOutcome::FailedAndRolledBack
    );
    assert_eq!(
        committed_rows(&s).await,
        "1",
        "row 2 went with the rollback"
    );
    session.close().await;
}

/// **A grid edit on the pinned session is part of the transaction**, and a
/// batch that fails is undone alone: the earlier statement survives, the
/// failure is isolated, and a rollback undoes the lot.
#[tokio::test(flavor = "multi_thread")]
async fn a_grid_edit_in_a_manual_session_is_isolated_and_uncommitted() {
    use schemaic_core::tx::StmtOutcome;
    if !enabled() {
        return;
    }
    let s = Scratch::create("tx_grid").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY, v nvarchar(10) NULL)")
        .await;
    let session = manual(&s).await;
    assert_eq!(
        stmt(&session, "INSERT dbo.t VALUES (1, N'a')").await,
        StmtOutcome::Ok
    );
    let ok = session
        .commit_writes(
            &GridWrite {
                inserts: vec![row_insert(&s, "t", &[("id", txt("2")), ("v", txt("b"))])],
                ..Default::default()
            },
            CancellationToken::new(),
        )
        .await;
    assert_eq!(ok.stmt, StmtOutcome::Ok, "{:?}", ok.result);
    let dup = session
        .commit_writes(
            &GridWrite {
                inserts: vec![
                    row_insert(&s, "t", &[("id", txt("3")), ("v", txt("c"))]),
                    row_insert(&s, "t", &[("id", txt("1")), ("v", txt("x"))]),
                ],
                ..Default::default()
            },
            CancellationToken::new(),
        )
        .await;
    assert_eq!(dup.stmt, StmtOutcome::FailedIsolated, "{:?}", dup.result);
    // Row 3 went with its batch's savepoint; 1 and 2 are still there.
    let seen = session
        .fetch_query("SELECT COUNT(*) FROM dbo.t", 100, CancellationToken::new())
        .await
        .result
        .expect("a count");
    assert_eq!(seen.cell(0, 0).expect("a cell").display().to_string(), "2");
    assert_eq!(committed_rows(&s).await, "0");
    session.rollback().await.expect("rollback");
    assert_eq!(committed_rows(&s).await, "0");
    session.close().await;
}

/// **Stop on a pinned session stops the statement and keeps the
/// transaction** — `XACT_ABORT` off, the attention aborts only the batch —
/// and the connection answers the next statement.
#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_statement_keeps_the_manual_transaction() {
    use schemaic_core::tx::StmtOutcome;
    if !enabled() {
        return;
    }
    let s = Scratch::create("tx_stop").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY)")
        .await;
    let session = manual(&s).await;
    assert_eq!(
        stmt(&session, "INSERT dbo.t VALUES (1)").await,
        StmtOutcome::Ok
    );
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        stop.cancel();
    });
    let out = session
        .fetch_query("WAITFOR DELAY '00:00:05'", 100, cancel)
        .await;
    assert_eq!(out.stmt, StmtOutcome::Cancelled, "{:?}", out.result);
    assert_eq!(
        stmt(&session, "INSERT dbo.t VALUES (2)").await,
        StmtOutcome::Ok
    );
    session.commit().await.expect("commit");
    assert_eq!(committed_rows(&s).await, "2");
    session.close().await;
}

/// A read-only connection is refused a pinned session: SQL Server holds no
/// read-only transaction, so a Manual tab's Commit could keep a hidden write.
#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_connection_is_refused_a_manual_session() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("tx_ro").await;
    let refused =
        schemaic_db::session::Session::open_enforced(&s.db, Some(&s.name), Some(Enforce::ReadOnly))
            .await;
    assert!(
        matches!(refused, Err(DbError::Refused(ref m)) if m.contains("Auto mode")),
        "{:?}",
        refused.err()
    );
}
