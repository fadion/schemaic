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
//!
//! **`mssql-on-azure` runs the same tests against Azure SQL Database**, opt-in
//! ([`endpoint::opt_in_leg_enabled`]) and in place of the local server when
//! named. There a `CREATE DATABASE` is a new *billable* database outside the
//! free offer, so the leg takes one existing database instead —
//! `SCHEMAIC_IT_MSSQL_AZURE_HOST` / `_DATABASE`, the Entra leg's — signs in
//! through the Azure CLI, gives it to one test at a time, and empties it before
//! and after each ([`WIPE`]). What needs a second database or the server
//! itself says so and asserts nothing ([`azure_cannot`]).

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

/// Is the leg pointed at Azure SQL Database (`mssql-on-azure`)?
fn on_azure() -> bool {
    endpoint::opt_in_leg_enabled("mssql-on-azure")
}

/// The one database `mssql-on-azure` works in.
fn azure_database() -> String {
    std::env::var("SCHEMAIC_IT_MSSQL_AZURE_DATABASE").unwrap_or("schemaic_it".into())
}

/// The wipe empties a whole database, so it is refused anything but the
/// scratch names: `schemaic_it` itself, or the live tier's `schemaic_it_*`.
fn assert_azure_scratch(name: &str) {
    assert!(
        name == "schemaic_it" || name.starts_with(PREFIX),
        "mssql-on-azure only ever empties schemaic_it or {PREFIX}*; refusing {name:?}"
    );
}

/// On Azure, skip a test the one shared database cannot host, loudly.
fn azure_cannot(why: &str) -> bool {
    if on_azure() {
        endpoint::note_leg_no_op("mssql-on-azure", why);
        return true;
    }
    false
}

/// Empties the database it runs in: every user object, schema, type, role and
/// database user, and the database-scoped settings a test turns on. A pass
/// tries each drop and swallows its failure, and passes repeat while anything
/// is left — dependency order found by trying rather than worked out — then
/// the batch fails naming what survived.
const WIPE: &str = "SET NOCOUNT ON;
DECLARE @todo TABLE (n int IDENTITY PRIMARY KEY, s nvarchar(max));
DECLARE @pass int = 0, @n int, @s nvarchar(max), @left nvarchar(max);
WHILE 1 = 1
BEGIN
    SET @pass += 1;
    DELETE @todo;
    INSERT @todo (s) SELECT s FROM (
        SELECT 0 AS k, N'ALTER TABLE ' + QUOTENAME(SCHEMA_NAME(schema_id)) + N'.' + QUOTENAME(name)
            + N' SET (SYSTEM_VERSIONING = OFF)' AS s FROM sys.tables WHERE temporal_type = 2
        UNION ALL SELECT 1, N'ALTER TABLE ' + QUOTENAME(OBJECT_SCHEMA_NAME(parent_object_id)) + N'.'
            + QUOTENAME(OBJECT_NAME(parent_object_id)) + N' DROP CONSTRAINT ' + QUOTENAME(name)
            FROM sys.foreign_keys
        UNION ALL SELECT 2, N'DROP TRIGGER ' + QUOTENAME(name) + N' ON DATABASE'
            FROM sys.triggers WHERE parent_class = 0
        UNION ALL SELECT CASE o.type WHEN 'U' THEN 4 ELSE 3 END,
            N'DROP ' + CASE o.type WHEN 'V' THEN N'VIEW' WHEN 'P' THEN N'PROCEDURE'
                WHEN 'U' THEN N'TABLE' WHEN 'SO' THEN N'SEQUENCE' WHEN 'SN' THEN N'SYNONYM'
                ELSE N'FUNCTION' END
            + N' ' + QUOTENAME(SCHEMA_NAME(o.schema_id)) + N'.' + QUOTENAME(o.name)
            FROM sys.objects o
            WHERE o.is_ms_shipped = 0 AND o.type IN ('V','P','U','SO','SN','FN','IF','TF','FS','FT')
        UNION ALL SELECT 5, N'DROP TYPE ' + QUOTENAME(SCHEMA_NAME(schema_id)) + N'.' + QUOTENAME(name)
            FROM sys.types WHERE is_user_defined = 1
        UNION ALL SELECT 6, N'DROP XML SCHEMA COLLECTION ' + QUOTENAME(SCHEMA_NAME(schema_id)) + N'.'
            + QUOTENAME(name) FROM sys.xml_schema_collections WHERE schema_id <> SCHEMA_ID('sys')
        UNION ALL SELECT 7, N'DROP SCHEMA ' + QUOTENAME(name)
            FROM sys.schemas WHERE schema_id > 4 AND schema_id < 16384
        UNION ALL SELECT 8, N'DROP USER ' + QUOTENAME(name)
            FROM sys.database_principals WHERE principal_id > 4 AND type <> 'R'
        UNION ALL SELECT 9, N'DROP ROLE ' + QUOTENAME(name)
            FROM sys.database_principals WHERE type = 'R' AND is_fixed_role = 0 AND principal_id > 0
    ) todo ORDER BY k;
    IF NOT EXISTS (SELECT 1 FROM @todo) BREAK;
    IF @pass > 10
    BEGIN
        SET @left = (SELECT STRING_AGG(s, N'; ') FROM @todo);
        THROW 50000, @left, 1;
    END
    SET @n = 0;
    WHILE 1 = 1
    BEGIN
        SELECT TOP (1) @n = n, @s = s FROM @todo WHERE n > @n ORDER BY n;
        IF @@ROWCOUNT = 0 BREAK;
        BEGIN TRY EXEC (@s); END TRY BEGIN CATCH END CATCH;
    END
END
IF EXISTS (SELECT 1 FROM sys.database_scoped_configurations
           WHERE name = 'PREVIEW_FEATURES' AND CONVERT(nvarchar(20), value) IN (N'1', N'ON'))
    BEGIN TRY ALTER DATABASE SCOPED CONFIGURATION SET PREVIEW_FEATURES = OFF; END TRY
    BEGIN CATCH END CATCH;";

/// The one database's turn: a test holds it for as long as its [`Scratch`]
/// lives. A tokio mutex, as it neither poisons when a test panics nor makes
/// the scratch `!Send`.
static AZURE_TURN: std::sync::LazyLock<Arc<tokio::sync::Mutex<()>>> =
    std::sync::LazyLock::new(Default::default);

/// Azure SQL Database, signed in as the Azure CLI's user, attached to no
/// database — which there is `master`.
fn azure_base_db() -> Db {
    use schemaic_core::connection::{AuthMode, SslMode};
    let host = std::env::var("SCHEMAIC_IT_MSSQL_AZURE_HOST")
        .expect("SCHEMAIC_IT_MSSQL_AZURE_HOST names the Azure SQL server for mssql-on-azure");
    Db::connect(
        &sign_in_connection(host, 1433, "", AuthMode::AzureCli, SslMode::VerifyFull),
        None,
    )
}

/// The server, attached to no database.
fn base_db() -> Db {
    if on_azure() {
        return azure_base_db();
    }
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
    /// On Azure, this test's turn at the one database, released only after
    /// [`Drop`] has emptied it.
    _turn: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl Scratch {
    async fn create(test: &str) -> Scratch {
        if on_azure() {
            return Scratch::azure().await;
        }
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
        Scratch {
            name,
            db,
            _turn: None,
        }
    }

    /// `mssql-on-azure`'s scratch: the one database, emptied first — a run
    /// stopped mid-test leaves its tables behind. **A test holds at most one**:
    /// a second waits for the turn its own first holds, which hangs the test
    /// and every one queued behind it.
    async fn azure() -> Scratch {
        let name = azure_database();
        assert_azure_scratch(&name);
        let turn = AZURE_TURN.clone().lock_owned().await;
        let db = azure_base_db().with_database(Some(&name));
        db.fetch_query(Some(&name), WIPE, 1, CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("could not empty {name}: {e}"));
        Scratch {
            name,
            db,
            _turn: Some(turn),
        }
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
        if self._turn.is_some() {
            assert_azure_scratch(&name);
            let db = self.db.clone();
            let emptied = std::thread::spawn(move || {
                tokio::runtime::Runtime::new()
                    .expect("a runtime for the teardown")
                    .block_on(db.fetch_query(Some(&name), WIPE, 1, CancellationToken::new()))
            })
            .join();
            if !matches!(emptied, Ok(Ok(_))) && !std::thread::panicking() {
                panic!("could not empty {}: {emptied:?}", self.name);
            }
            return;
        }
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
    if on_azure() || endpoint::leg_enabled("mssql") {
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

/// **A database in maintenance is still listed for a login that may open
/// it.** The listing left out every `RESTRICTED_USER` database and every
/// `SINGLE_USER` one, so the database an administrator had just put in
/// maintenance vanished from their tree — while opening it by name worked.
/// A login holding `CONNECT ANY DATABASE` (`sa` here) now sees both; the
/// single-user one whether or not a session holds it, since reading it says
/// which — and the load leaves that one unread until the user asks.
#[tokio::test(flavor = "multi_thread")]
async fn a_database_in_maintenance_is_listed_for_a_login_that_may_open_it() {
    if !enabled() || azure_cannot("creates no databases of its own, so the maintenance listing") {
        return;
    }
    let restricted = Scratch::create("restricted").await;
    let single = Scratch::create("single").await;
    let base = base_db();
    let alter = |name: &str, mode: &str| {
        let base = base.clone();
        let sql = format!("ALTER DATABASE [{name}] SET {mode} WITH ROLLBACK IMMEDIATE");
        async move {
            base.fetch_query(None, &sql, 1, CancellationToken::new())
                .await
                .unwrap_or_else(|e| panic!("{e}\n{sql}"));
        }
    };
    alter(&restricted.name, "RESTRICTED_USER").await;
    alter(&single.name, "SINGLE_USER").await;
    let names = base.fetch_databases().await;
    let listed = base.list_databases().await;
    // Back to normal before anything can fail, so the drops are ordinary.
    alter(&restricted.name, "MULTI_USER").await;
    alter(&single.name, "MULTI_USER").await;
    let names = names.expect("a database list");
    for scratch in [&restricted, &single] {
        assert!(names.contains(&scratch.name), "{}: {names:?}", scratch.name);
    }
    // **Listed, but not read unasked.** The schema load connects into every
    // database it reads, and a connection into a single-user one takes the
    // slot the administrator just reserved: their `RESTORE` failed with Msg
    // 924 behind it. The listing says which is which, and the load's own
    // decision leaves the single-user one out, even as the active database.
    let listed = listed.expect("a database list");
    let flag = |name: &str| {
        listed
            .iter()
            .find(|d| d.name == name)
            .unwrap_or_else(|| panic!("{name}: {listed:?}"))
            .single_user
    };
    assert!(flag(&single.name), "{listed:?}");
    assert!(!flag(&restricted.name), "{listed:?}");
    let none = std::collections::HashSet::new();
    let reads = schemaic_core::schema::unasked_reads(&listed, Some(&single.name), &none, &none);
    assert!(
        reads.iter().all(|&i| listed[i].name != single.name),
        "a load reads {}: {listed:?}",
        single.name
    );
    assert!(reads.iter().any(|&i| listed[i].name == restricted.name));
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

/// A cursor's plan is the server's `<StmtCursor>`, and it is read as a
/// statement of its own: one declared in the batch has a plan (it read as
/// "no plan"), and one a procedure opens is that procedure's, after its first
/// `SELECT` — not drawn under the `EXECUTE PROC` line that called it.
#[tokio::test(flavor = "multi_thread")]
async fn a_cursors_plan_is_read_as_its_own_statement() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("plancur").await;
    s.exec("CREATE TABLE dbo.t (id int PRIMARY KEY, v int); INSERT dbo.t VALUES (1, 1), (2, 2)")
        .await;
    s.exec(
        "CREATE PROCEDURE dbo.pc AS BEGIN SELECT id FROM dbo.t WHERE id = 2; \
         DECLARE @i int; DECLARE c CURSOR LOCAL FOR SELECT id FROM dbo.t WHERE v > 0; \
         OPEN c; FETCH NEXT FROM c INTO @i; CLOSE c; DEALLOCATE c; END",
    )
    .await;
    let explain = |sql: &'static str| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            db.explain(Some(&name), sql, false, false, CancellationToken::new())
                .await
                .map(|rs| schemaic_core::plan::QueryPlan::from_result(&rs))
                .expect("an estimated plan")
        }
    };
    let ops = |plan: &schemaic_core::plan::QueryPlan| {
        plan.rows.iter().map(|r| r[0].clone()).collect::<Vec<_>>()
    };

    let plan = explain(
        "DECLARE c CURSOR FOR SELECT id FROM dbo.t WHERE v > 1; OPEN c; \
         FETCH NEXT FROM c; CLOSE c; DEALLOCATE c;",
    )
    .await;
    assert!(
        ops(&plan)
            .iter()
            .any(|o| o.contains("Clustered Index Scan")),
        "{plan:?}"
    );

    let plan = explain("EXEC dbo.pc").await;
    let ops = ops(&plan);
    let heads: Vec<&str> = ops
        .iter()
        .filter(|o| o.starts_with("Statement"))
        .map(|o| o.as_str())
        .collect();
    assert_eq!(
        heads,
        [
            "Statement 1: SELECT in procedure dbo.pc",
            "Statement 2: DECLARE CURSOR in procedure dbo.pc",
        ],
        "{plan:?}"
    );
}

/// Take a dump of every table in `src` and render it as the app's writer
/// does: text steps verbatim, each rows step streamed from `src` through the
/// export renderer.
async fn dump_file(src: &Scratch, opts: schemaic_core::dump::DumpOptions) -> String {
    Box::pin(dump_file_of(src, opts, |_| true)).await
}

/// [`dump_file`] of the tables whose display name `pick` takes.
async fn dump_file_of(
    src: &Scratch,
    opts: schemaic_core::dump::DumpOptions,
    pick: impl Fn(&str) -> bool,
) -> String {
    use schemaic_core::dump::{DumpStep, plan};
    let schema = Box::pin(src.db.fetch_schema(&src.name, CancellationToken::new()))
        .await
        .expect("the schema");
    let chosen: Vec<String> = schema
        .tables
        .iter()
        .map(|t| schemaic_core::schema::display_name(t.schema.as_deref(), &t.name))
        .filter(|n| pick(n))
        .collect();
    let dump = plan(&schema, &src.name, &chosen, opts, MS);
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
                server,
            } => {
                let rs = Box::pin(src.db.fetch_query(
                    Some(&database),
                    &select,
                    10_000,
                    CancellationToken::new(),
                ))
                .await
                .expect("the rows");
                let order: Vec<usize> = (0..rs.row_count()).collect();
                let mut out = Vec::new();
                // The app's writer's renderer, batch separators and all.
                schemaic_core::dump::render_rows(
                    &mut out,
                    &mut schemaic_core::export::OneChunk::new(&rs, &order),
                    (&insert_database, schema.as_deref(), &table),
                    &server,
                    MS,
                )
                .unwrap();
                file.push_str(&String::from_utf8(out).unwrap());
                file.push('\n');
            }
        }
    }
    file
}

/// Run `file` into `dst` the way the app's Run file does: cut by the script
/// splitter, drained by `run_script` on one connection.
async fn restore_file(dst: &Scratch, file: &str) -> schemaic_core::script::ExecEnd {
    let mut splitter = schemaic_core::script::Splitter::new(MS);
    let mut stmts = splitter.push_str(file);
    stmts.extend(splitter.finish());
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let feed = tokio::spawn(async move {
        for s in stmts {
            if tx.send(s).await.is_err() {
                break;
            }
        }
    });
    let (end, _) = Box::pin(dst.db.run_script(&dst.name, rx, CancellationToken::new())).await;
    feed.await.unwrap();
    end
}

/// [`restore_file`] the way `sqlcmd` restores: the file cut at its `GO` lines
/// alone, and each batch — everything between two of them, comments included
/// — sent whole on one connection.
async fn restore_batches(dst: &Scratch, file: &str) -> schemaic_core::script::ExecEnd {
    let mut batches: Vec<schemaic_core::script::Statement> = Vec::new();
    let (mut batch, mut start) = (String::new(), 1u64);
    for (n, l) in file.lines().enumerate() {
        if l.trim().eq_ignore_ascii_case("GO") {
            if !batch.trim().is_empty() {
                batches.push(schemaic_core::script::Statement {
                    sql: std::mem::take(&mut batch),
                    line: start,
                    offset: 0,
                });
            }
            batch.clear();
            start = n as u64 + 2;
        } else {
            batch.push_str(l);
            batch.push('\n');
        }
    }
    if !batch.trim().is_empty() {
        batches.push(schemaic_core::script::Statement {
            sql: batch,
            line: start,
            offset: 0,
        });
    }
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let feed = tokio::spawn(async move {
        for s in batches {
            if tx.send(s).await.is_err() {
                break;
            }
        }
    });
    let (end, _) = Box::pin(dst.db.run_script(&dst.name, rx, CancellationToken::new())).await;
    feed.await.unwrap();
    end
}

/// **A dump restores into an empty database as the one it was taken from**:
/// `core::dump`'s file, its rows rendered by the export renderer as the app's
/// writer renders them, cut at its `GO` lines by the script splitter and run by
/// `run_script`. The identity keeps its gaps (so the foreign key onto it still
/// holds) and counts on past them, the view and trigger open batches of their
/// own, and the rowversion and computed column are the server's again.
///
/// **Run on a stack of its own** ([`on_a_large_stack`]): a debug build's
/// poll frame grows with every `await` in the body, and this fixture sat at
/// the 2 MB test-thread limit, one more `await` from overflowing.
#[test]
fn a_dump_restores_into_an_empty_database() {
    on_a_large_stack(dump_restores_into_an_empty_database);
}

/// Run `body` to the end on a thread with a 16 MB stack and a multi-thread
/// runtime of its own — what `#[tokio::test(flavor = "multi_thread")]` gives
/// a test, but on a thread whose stack a fixture this size cannot outgrow.
/// A panic in it is the test's.
fn on_a_large_stack<F, Fut>(body: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()>,
{
    let run = std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("a runtime")
                .block_on(body());
        })
        .expect("a thread for the test");
    if let Err(panic) = run.join() {
        std::panic::resume_unwind(panic);
    }
}

/// [`a_dump_restores_into_an_empty_database`]'s body.
async fn dump_restores_into_an_empty_database() {
    use schemaic_core::dump::DumpOptions;
    if !enabled() || azure_cannot("restores into a second database, which it has not got") {
        return;
    }
    let src = Scratch::create("dumpsrc").await;
    src.exec(
        "CREATE TABLE dbo.customers (id int IDENTITY(1,1) PRIMARY KEY, name nvarchar(50), \
           seen datetime2(3), born date, paid decimal(10,2), rv rowversion, \
           twice AS (paid * 2), dt datetime, sdt smalldatetime)",
    )
    .await;
    src.exec(
        "CREATE TABLE dbo.orders (id int IDENTITY(1,1) PRIMARY KEY, \
           customer_id int NOT NULL REFERENCES dbo.customers(id), note nvarchar(max))",
    )
    .await;
    // An audit trigger, live before the rows: created before the restored
    // rows, it fired once per row and doubled the audit table.
    src.exec("CREATE TABLE dbo.order_audit (order_id int NOT NULL)")
        .await;
    src.exec(
        "CREATE TRIGGER dbo.tr_orders ON dbo.orders AFTER INSERT AS \
         SET NOCOUNT ON; INSERT dbo.order_audit (order_id) SELECT id FROM inserted;",
    )
    .await;
    src.exec(
        "INSERT dbo.customers (name, seen, born, paid, dt, sdt) VALUES \
           (N'Ann', '2026-01-02 03:04:05.678', '1990-05-06', 12.50, \
            '2026-01-02T03:04:05.677', '2026-01-02T03:04:00'), \
           (N'gone', NULL, NULL, NULL, NULL, NULL), \
           (N'Zoë ''q''', NULL, NULL, 0.01, '2026-03-25T10:00:00', '2026-03-25T10:00:00'); \
         DELETE dbo.customers WHERE id = 2; \
         INSERT dbo.orders (customer_id, note) VALUES (3, N'first'), (1, NULL)",
    )
    .await;
    src.exec("CREATE VIEW dbo.v_orders AS SELECT o.id, c.name FROM dbo.orders o JOIN dbo.customers c ON c.id = o.customer_id")
        .await;
    // Constraints switched off over rows that violate them: a disabled check
    // and key, and a check re-enabled without validating (untrusted). Written
    // as ordinary ones, the restore stopped at those rows (Msg 547).
    src.exec(
        "CREATE TABLE dbo.checked (id int PRIMARY KEY, a int, customer_id int, \
           CONSTRAINT ck_pos CHECK (a > 0), CONSTRAINT ck_small CHECK (a < 100), \
           CONSTRAINT fk_checked FOREIGN KEY (customer_id) REFERENCES dbo.customers(id)); \
         ALTER TABLE dbo.checked NOCHECK CONSTRAINT ck_pos; \
         ALTER TABLE dbo.checked NOCHECK CONSTRAINT ck_small; \
         ALTER TABLE dbo.checked NOCHECK CONSTRAINT fk_checked; \
         INSERT dbo.checked VALUES (1, -1, 999), (2, 150, NULL); \
         ALTER TABLE dbo.checked CHECK CONSTRAINT ck_small;",
    )
    .await;
    // Floats a plain decimal cannot carry: T-SQL reads a literal with no
    // exponent as a `numeric`, which stops at 38 digits (Msg 1007).
    src.exec(
        "CREATE TABLE dbo.floats (id int PRIMARY KEY, f float, r real); \
         INSERT dbo.floats VALUES (1, 1e300, 3.4028235e38), (2, -1e-40, 1e-40), \
           (3, 1.2345678901234567e-30, 0.5)",
    )
    .await;
    // A routine of each kind: the file used to close each one `GO;` + `GO`,
    // and the restore stopped at the first with Msg 102.
    src.exec("CREATE FUNCTION dbo.f_double (@x int) RETURNS int AS BEGIN RETURN @x * 2; END")
        .await;
    src.exec("CREATE PROCEDURE dbo.p_count AS SELECT COUNT(*) FROM dbo.orders;")
        .await;
    // A computed column, a check, a default and a view that call a function:
    // SQL Server resolves it at CREATE TABLE/VIEW, and the file created every
    // routine after the tables (Msg 4121).
    src.exec(
        "CREATE TABLE dbo.calc (id int PRIMARY KEY, dbl AS dbo.f_double(id), \
           n int NOT NULL CONSTRAINT df_calc_n DEFAULT (dbo.f_double(4)), \
           CONSTRAINT ck_calc CHECK (dbo.f_double(id) > 0)); \
         INSERT dbo.calc (id) VALUES (1), (2);",
    )
    .await;
    src.exec("CREATE VIEW dbo.v_double AS SELECT dbo.f_double(id) AS d FROM dbo.customers")
        .await;
    // What a text cell cannot carry: bytes (the CLR types among them, a
    // geography off the default SRID and a geometry with Z and M, the
    // hierarchy's root and an empty value, whose serialisations are empty)
    // and a variant of each base type, which must come back as that type.
    src.exec(
        "CREATE TABLE dbo.blobs (id int PRIMARY KEY, h hierarchyid NOT NULL, g geography, \
           gm geometry, i image, b varbinary(max), e varbinary(10) NOT NULL, v sql_variant); \
         INSERT dbo.blobs VALUES (1, '/1/2/', \
           geography::STGeomFromText('POINT(-122.34 47.65)', 4269), \
           geometry::STGeomFromText('LINESTRING(0 0 1 2, 1 1 3 4)', 27700), 0x0102, 0xDEADBEEF, 0x, \
           CAST('2026-01-02' AS date)); \
         INSERT dbo.blobs (id, h, e, v) VALUES (2, '/', 0x01, CAST(12.50 AS decimal(5,2))); \
         INSERT dbo.blobs (id, h, e, v) VALUES (3, '/3/', 0x02, \
           CAST('2026-01-02T03:04:05.677' AS datetime)); \
         INSERT dbo.blobs (id, h, e, v) VALUES (4, '/4/', 0x03, N'it''s é'); \
         INSERT dbo.blobs (id, h, e, v) VALUES (5, '/5/', 0x04, CAST(0x0A0B AS varbinary(4))); \
         INSERT dbo.blobs (id, h, e, v) VALUES (6, '/6/', 0x05, CAST(1e300 AS float)); \
         INSERT dbo.blobs (id, h, e, v) VALUES (7, '/7/', 0x06, CAST(12.3456 AS money)); \
         INSERT dbo.blobs (id, h, e, v) VALUES (8, '/8/', 0x07, CAST('03:04:05.678' AS time(3))); \
         INSERT dbo.blobs (id, h, e, v) VALUES (9, '/9/', 0x08, \
           CAST('2026-01-02 03:04:05.1234567 +02:00' AS datetimeoffset(7))); \
         INSERT dbo.blobs (id, h, e, v) VALUES (10, '/10/', 0x09, CAST(1 AS bit)); \
         INSERT dbo.blobs (id, h, e, v) VALUES (11, '/11/', 0x0A, CAST(0.1 AS real)); \
         INSERT dbo.blobs (id, h, e, v) VALUES (12, '/12/', 0x0B, \
           CAST('ab' AS char(4)) COLLATE Latin1_General_CS_AS); \
         INSERT dbo.blobs (id, h, e, v) VALUES (13, '/13/', 0x0C, \
           CAST('6F9619FF-8B86-D011-B42D-00C04FC964FF' AS uniqueidentifier)); \
         INSERT dbo.blobs (id, h, e, v) VALUES (14, '/14/', 0x0D, 42); \
         INSERT dbo.blobs (id, h, e, v) VALUES (15, '/15/', 0x0E, NULL);",
    )
    .await;
    // A graph node and edge, read as plain tables with their internal columns
    // (the copy refused the node's rows, Msg 515), and a system-versioned
    // table, which no CREATE from the model can restate.
    src.exec(
        "CREATE TABLE dbo.person (id int PRIMARY KEY, name nvarchar(20)) AS NODE; \
         INSERT dbo.person (id, name) VALUES (1, N'a'), (2, N'b'); \
         CREATE TABLE dbo.knows (since int) AS EDGE; \
         INSERT dbo.knows ($from_id, $to_id, since) \
           SELECT p1.$node_id, p2.$node_id, 2020 FROM dbo.person p1, dbo.person p2 \
           WHERE p1.id = 1 AND p2.id = 2; \
         CREATE TABLE dbo.versioned (id int PRIMARY KEY, v int, \
           vf datetime2 GENERATED ALWAYS AS ROW START HIDDEN NOT NULL, \
           vt datetime2 GENERATED ALWAYS AS ROW END HIDDEN NOT NULL, \
           PERIOD FOR SYSTEM_TIME (vf, vt)) \
           WITH (SYSTEM_VERSIONING = ON (HISTORY_TABLE = dbo.versioned_history));",
    )
    .await;
    // What a table leans on that is not a table: a sequence behind a default —
    // one in a schema holding no table, as WideWorldImporters keeps all of its
    // — an alias type, a typed `xml` column's schema collection, and a
    // synonym a view reads through. None was read, so the restore stopped at
    // the first CREATE TABLE naming one (Msg 208).
    src.exec("CREATE SCHEMA Sequences").await;
    src.exec(
        "CREATE SEQUENCE Sequences.OrderID AS int START WITH 1000 INCREMENT BY 5 CACHE 10; \
         CREATE SEQUENCE dbo.seq AS bigint START WITH 1 NO CACHE; \
         CREATE TYPE dbo.Phone FROM nvarchar(20) NOT NULL; \
         CREATE XML SCHEMA COLLECTION dbo.coll AS N'<xsd:schema xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\"><xsd:element name=\"a\" type=\"xsd:int\"/></xsd:schema>';",
    )
    .await;
    src.exec(
        "CREATE TABLE dbo.leaning (id bigint NOT NULL DEFAULT (NEXT VALUE FOR dbo.seq) PRIMARY KEY, \
           code int NOT NULL DEFAULT (NEXT VALUE FOR Sequences.OrderID), phone dbo.Phone, \
           doc xml(dbo.coll)); \
         INSERT dbo.leaning (phone, doc) VALUES (N'555-0100', N'<a>1</a>'), (N'555-0101', NULL); \
         CREATE SYNONYM dbo.syn_leaning FOR dbo.leaning;",
    )
    .await;
    src.exec("CREATE VIEW dbo.v_leaning AS SELECT id FROM dbo.syn_leaning")
        .await;
    // More rows than one `INSERT` carries, so the table's rows are several
    // statements — each closing its own `GO` batch.
    src.exec(
        "CREATE TABLE dbo.many (id int PRIMARY KEY, s nvarchar(40) NOT NULL); \
         INSERT dbo.many (id, s) SELECT TOP (600) \
           ROW_NUMBER() OVER (ORDER BY (SELECT NULL)), N'row' \
           FROM sys.all_objects a CROSS JOIN sys.all_objects b;",
    )
    .await;

    let file = Box::pin(dump_file(&src, DumpOptions::default())).await;
    // For a hand check through `sqlcmd`: the file as the app would write it.
    if let Ok(path) = std::env::var("SCHEMAIC_IT_KEEP_DUMP") {
        std::fs::write(path, &file).expect("the kept dump");
    }
    assert!(
        file.contains("SET IDENTITY_INSERT [dbo].[customers] ON;"),
        "{file}"
    );
    // Every `INSERT` closes its own batch: none outgrows the server's limit
    // however large the table, and a restore does not lean on a `;` split.
    assert!(
        file.matches("INSERT INTO [dbo].[many]").count() > 1,
        "{file}"
    );
    assert!(
        file.split("\nGO\n")
            .all(|batch| batch.matches("INSERT INTO").count() <= 1),
        "{file}"
    );
    // The graph tables as what they are, their internal columns the server's;
    // the temporal pair named and left out.
    assert!(
        file.contains(
            "CREATE TABLE [dbo].[person] (\n  [id] int NOT NULL,\n  [name] nvarchar(20) NULL,"
        ),
        "{file}"
    );
    assert!(
        file.contains(") AS NODE;") && file.contains(") AS EDGE;"),
        "{file}"
    );
    assert!(
        !file.contains("graph_id") && !file.contains("$node_id"),
        "{file}"
    );
    assert!(
        file.contains("dbo.versioned (a system-versioned temporal table)")
            && file.contains("dbo.versioned_history (the history table"),
        "{file}"
    );
    assert!(!file.contains("CREATE TABLE [dbo].[versioned"), "{file}");

    let dst = Scratch::create("dumpdst").await;
    // Restored by a session whose language reads a date day-first, as a
    // `british` (or German, French…) login's does: `datetime` reads
    // `2026-01-02 …` as the 1st of February there unless the file says how
    // its dates are written.
    let end = Box::pin(restore_file(
        &dst,
        &format!("SET LANGUAGE british;\nGO\n\n{file}"),
    ))
    .await;
    assert!(
        matches!(end, schemaic_core::script::ExecEnd::Done),
        "{end:?}\n{file}"
    );

    // Everything the copy must agree with the source on, as one string per
    // side — one query, not one `await` per fact: a debug build's poll frame
    // grows with every await in the test, and this fixture keeps growing.
    let facts = "SELECT STRING_AGG(CAST(f AS nvarchar(max)), NCHAR(10)) WITHIN GROUP (ORDER BY f) FROM ( \
        SELECT CONCAT('customers|', id, '|', name, '|', CONVERT(varchar(30), seen, 121), '|', born, \
               '|', paid, '|', twice, '|', CONVERT(varchar(30), dt, 121), '|', \
               CONVERT(varchar(30), sdt, 120)) AS f FROM dbo.customers \
        UNION ALL SELECT CONCAT('floats|', id, '|', CONVERT(varchar(40), f, 3), '|', \
               CONVERT(varchar(40), r, 3)) FROM dbo.floats \
        UNION ALL SELECT CONCAT('checked|', id, '|', a, '|', customer_id) FROM dbo.checked \
        UNION ALL SELECT CONCAT('audit|', COUNT(*)) FROM dbo.order_audit \
        UNION ALL SELECT CONCAT('v_orders|', COUNT(*)) FROM dbo.v_orders \
        UNION ALL SELECT CONCAT('constraint|', name, ':', is_disabled, is_not_trusted) \
               FROM sys.check_constraints \
        UNION ALL SELECT CONCAT('constraint|', name, ':', is_disabled, is_not_trusted) \
               FROM sys.foreign_keys \
        UNION ALL SELECT CONCAT('trigger|', name) FROM sys.triggers \
        UNION ALL SELECT CONCAT('procedure|', name) FROM sys.procedures \
        UNION ALL SELECT CONCAT('f_double|', dbo.f_double(21)) \
        UNION ALL SELECT CONCAT('calc|', id, '|', dbl, '|', n) FROM dbo.calc \
        UNION ALL SELECT CONCAT('many|', COUNT(*), '|', SUM(id)) FROM dbo.many \
        UNION ALL SELECT CONCAT('leaning|', id, '|', code, '|', phone, '|', \
               CAST(doc AS nvarchar(max))) FROM dbo.leaning \
        UNION ALL SELECT CONCAT('v_leaning|', COUNT(*)) FROM dbo.v_leaning \
        UNION ALL SELECT CONCAT('sequence|', SCHEMA_NAME(schema_id), '.', name, '|', \
               CAST(start_value AS nvarchar(40)), '|', CAST(increment AS nvarchar(40)), '|', \
               is_cached, '|', cache_size, '|next ', \
               CASE WHEN last_used_value IS NULL THEN CAST(current_value AS bigint) \
                    ELSE CAST(last_used_value AS bigint) + CAST(increment AS bigint) END) \
               FROM sys.sequences \
        UNION ALL SELECT CONCAT('column|', c.name, '|', TYPE_NAME(c.user_type_id), '|', \
               xc.name, '|', c.is_nullable) FROM sys.columns c \
               LEFT JOIN sys.xml_schema_collections xc ON xc.xml_collection_id = c.xml_collection_id \
                    AND c.xml_collection_id > 0 \
               WHERE c.object_id = OBJECT_ID('dbo.leaning') \
        UNION ALL SELECT CONCAT('synonym|', name, '|', base_object_name) FROM sys.synonyms \
        UNION ALL SELECT CONCAT('blobs|', id, '|', h.ToString(), '|', \
               CONVERT(varchar(max), CAST(h AS varbinary(max)), 1), '|', \
               CONVERT(varchar(max), CAST(g AS varbinary(max)), 1), '|', g.STSrid, '|', \
               CONVERT(varchar(max), CAST(gm AS varbinary(max)), 1), '|', \
               CONVERT(varchar(max), CAST(i AS varbinary(max)), 1), '|', \
               CONVERT(varchar(max), b, 1), '|', DATALENGTH(e), CONVERT(varchar(max), e, 1), '|', \
               CAST(SQL_VARIANT_PROPERTY(v, 'BaseType') AS sysname), '|', \
               CAST(SQL_VARIANT_PROPERTY(v, 'Precision') AS int), '|', \
               CAST(SQL_VARIANT_PROPERTY(v, 'Scale') AS int), '|', \
               CAST(SQL_VARIANT_PROPERTY(v, 'MaxLength') AS int), '|', \
               CAST(SQL_VARIANT_PROPERTY(v, 'Collation') AS sysname), '|', \
               CASE WHEN CAST(SQL_VARIANT_PROPERTY(v, 'BaseType') AS sysname) IN ('float', 'real') \
                    THEN CONVERT(nvarchar(64), CAST(v AS float), 3) \
                    WHEN CAST(SQL_VARIANT_PROPERTY(v, 'BaseType') AS sysname) = 'datetime' \
                    THEN CONVERT(nvarchar(64), CAST(v AS datetime), 126) \
                    WHEN CAST(SQL_VARIANT_PROPERTY(v, 'BaseType') AS sysname) = 'money' \
                    THEN CONVERT(nvarchar(64), CAST(v AS money), 2) \
                    WHEN CAST(SQL_VARIANT_PROPERTY(v, 'BaseType') AS sysname) = 'varbinary' \
                    THEN CONVERT(nvarchar(64), CAST(v AS varbinary(10)), 1) \
                    WHEN CAST(SQL_VARIANT_PROPERTY(v, 'BaseType') AS sysname) = 'date' \
                    THEN CONVERT(nvarchar(64), CAST(v AS date), 23) \
                    WHEN CAST(SQL_VARIANT_PROPERTY(v, 'BaseType') AS sysname) = 'time' \
                    THEN CAST(CAST(v AS time(7)) AS nvarchar(64)) \
                    WHEN CAST(SQL_VARIANT_PROPERTY(v, 'BaseType') AS sysname) = 'datetimeoffset' \
                    THEN CAST(CAST(v AS datetimeoffset(7)) AS nvarchar(64)) \
                    ELSE CAST(v AS nvarchar(4000)) END) FROM dbo.blobs \
        UNION ALL SELECT CONCAT('v_double|', SUM(d)) FROM dbo.v_double \
      ) x";
    let (want, got) = (src.scalar(facts).await, dst.scalar(facts).await);
    let differ = |want: &str, got: &str| {
        let only = |a: &str, b: &str| -> Vec<String> {
            a.lines()
                .filter(|l| !b.lines().any(|m| m == *l))
                .map(String::from)
                .collect()
        };
        format!(
            "the copy differs\n  only in the source: {:#?}\n  only in the copy: {:#?}",
            only(want, got),
            only(got, want)
        )
    };
    assert!(got == want, "{}", differ(&want, &got));
    // The node is a node again with its rows; the edge is an edge, created
    // empty; the temporal table is not there to be mistaken for a copy.
    let graph = "SELECT CONCAT((SELECT COUNT(*) FROM sys.tables WHERE name = 'person' AND is_node = 1), \
                 '|', (SELECT STRING_AGG(CONCAT(id, name), ',') WITHIN GROUP (ORDER BY id) FROM dbo.person), \
                 '|', (SELECT COUNT(*) FROM sys.tables WHERE name = 'knows' AND is_edge = 1), \
                 '|', (SELECT COUNT(*) FROM dbo.knows), '|', OBJECT_ID('dbo.versioned'))";
    assert_eq!(dst.scalar(graph).await, "1|1a,2b|1|0|");
    for fact in [
        "customers|3|Zoë 'q'|",
        // The audit holds what the source's did: the trigger fired on none of
        // the restored rows.
        "audit|2",
        "v_orders|2",
        // Each constraint is back in the state it was in, over the same rows.
        "constraint|ck_pos:11",
        "constraint|ck_small:01",
        "constraint|fk_checked:11",
        "checked|1|-1|999",
        "trigger|tr_orders",
        "procedure|p_count",
        "f_double|42",
        "calc|2|4|8",
        "v_double|8",
        "many|600|180300",
        // The geography off its default SRID, and the geometry's Z and M.
        "blobs|1|/1/2/|0x5B40|0xAD100000010C",
        "|4269|0x346C00000117",
        "|0x0102|0xDEADBEEF|00x|date|10|0|3||2026-01-02",
        // The hierarchy's root, whose serialisation is empty.
        "blobs|2|/|||||||10x01|decimal|5|2|5||12.50",
        "|datetime|23|3|8||2026-01-02T03:04:05.677",
        "|nvarchar|0|0|8000|SQL_Latin1_General_CP1_CI_AS|it's é",
        "|varbinary|0|0|4||0x0A0B",
        "|float|53|0|8||1.0000000000000001e+300",
        "|money|19|4|8||12.3456",
        "|time|12|3|4||03:04:05.6780000",
        "|datetimeoffset|34|7|10||2026-01-02 03:04:05.1234567 +02:00",
        "|real|24|0|4||1.0000000149011612e-001",
        "|char|0|0|4|Latin1_General_CS_AS|ab  ",
        "|uniqueidentifier|0|0|16||6F9619FF-8B86-D011-B42D-00C04FC964FF",
        "blobs|15|/15/|0xBE||||||10x0E||||||",
        "leaning|1|1000|555-0100|<a>1</a>",
        "leaning|2|1005|555-0101|",
        "v_leaning|2",
        // Each counter goes on from where the source's was.
        "sequence|dbo.seq|1|1|0||next 3",
        "sequence|Sequences.OrderID|1000|5|1|10|next 1010",
        "column|phone|Phone||0",
        "column|doc|xml|coll|1",
        "synonym|syn_leaning|[dbo].[leaning]",
    ] {
        assert!(want.contains(fact), "{fact} in {want}");
    }
    // Replayed onto a database that already holds all of it — the way a dump
    // is most often tested. It stopped at the first referenced table's `DROP`
    // (Msg 3726), the child's key still standing. All of it but the graph
    // tables, which the file refuses to replace
    // (`a_replay_refuses_to_replace_a_graph_table`): dropped ahead of it in the
    // same run, not in an await of their own, for the poll frame's sake.
    let again = Box::pin(restore_file(
        &dst,
        &format!("DROP TABLE dbo.knows;\nDROP TABLE dbo.person;\nGO\n\n{file}"),
    ))
    .await;
    assert!(
        matches!(again, schemaic_core::script::ExecEnd::Done),
        "{again:?}\n{file}"
    );
    let again = dst.scalar(facts).await;
    assert!(again == want, "{}", differ(&want, &again));
    // The identity counts on past the highest key the file carried.
    dst.exec("INSERT dbo.customers (name) VALUES (N'next')")
        .await;
    assert_eq!(dst.scalar("SELECT MAX(id) FROM dbo.customers").await, "4");
}

/// **A character `sql_variant` restores byte for byte, under its own
/// collation, however long.** The scratch databases take the server's
/// `SQL_Latin1_General_CP1_CI_AS`, so a Greek, Japanese or UTF-8 variant is
/// one whose code page is not the restoring database's: the file converted
/// it under the database's and restored `Oµ??a`. And one past 4,000
/// characters was read through `nvarchar(4000)` and restored cut, with
/// `Done`.
#[tokio::test(flavor = "multi_thread")]
async fn a_dump_restores_character_variants_byte_for_byte() {
    use schemaic_core::dump::DumpOptions;
    if !enabled() || azure_cannot("restores into a second database, which it has not got") {
        return;
    }
    let src = Scratch::create("varsrc").await;
    src.exec(
        "CREATE TABLE dbo.variants (id int PRIMARY KEY, v sql_variant); \
         INSERT dbo.variants VALUES (1, CAST(N'Ωμέγα' COLLATE Greek_CI_AS AS varchar(10))); \
         INSERT dbo.variants VALUES (2, CAST(N'価格 1200円' COLLATE Japanese_CI_AS AS varchar(20))); \
         INSERT dbo.variants VALUES \
           (3, CAST(REPLICATE(CAST('x' AS varchar(max)), 5000) AS varchar(6000))); \
         INSERT dbo.variants VALUES (4, CAST(REPLICATE(CAST(N'Ωμ' AS nvarchar(max)), 2600) \
           + N'END' COLLATE Greek_CS_AS AS varchar(8000))); \
         INSERT dbo.variants VALUES (5, CAST(N'a' + NCHAR(1) + N'\"\\/' + NCHAR(13) + NCHAR(10) \
           COLLATE Latin1_General_BIN2 AS char(8))); \
         INSERT dbo.variants VALUES \
           (6, CAST(N'Ωμέγα 😀' COLLATE Latin1_General_100_CI_AS_SC_UTF8 AS varchar(30))); \
         INSERT dbo.variants VALUES (7, CAST(N'日本' COLLATE Japanese_CI_AS AS nchar(4))); \
         INSERT dbo.variants VALUES (8, CAST(N'' COLLATE Greek_CI_AS AS varchar(5))); \
         INSERT dbo.variants VALUES (9, CAST(N'it''s é' AS nvarchar(20)));",
    )
    .await;
    let facts = "SELECT STRING_AGG(CAST(CONCAT(id, '|', \
           CAST(SQL_VARIANT_PROPERTY(v, 'BaseType') AS sysname), '|', \
           CAST(SQL_VARIANT_PROPERTY(v, 'MaxLength') AS int), '|', \
           CAST(SQL_VARIANT_PROPERTY(v, 'Collation') AS sysname), '|', \
           DATALENGTH(v), '|', CONVERT(varchar(max), CAST(v AS varbinary(8000)), 1)) \
           AS nvarchar(max)), NCHAR(10)) WITHIN GROUP (ORDER BY id) FROM dbo.variants";
    let want = src.scalar(facts).await;
    // The source holds what it was meant to: the Greek bytes, not `?`s, and
    // the long ones whole.
    for fact in [
        "1|varchar|10|Greek_CI_AS|5|0xD9ECDDE3E1",
        "3|varchar|6000|SQL_Latin1_General_CP1_CI_AS|5000|0x7878",
        "4|varchar|8000|Greek_CS_AS|5203|0xD9EC",
    ] {
        assert!(want.contains(fact), "{fact} in {want}");
    }
    let file = Box::pin(dump_file(&src, DumpOptions::default())).await;
    let dst = Scratch::create("vardst").await;
    let end = Box::pin(restore_file(&dst, &file)).await;
    assert!(
        matches!(end, schemaic_core::script::ExecEnd::Done),
        "{end:?}\n{file}"
    );
    let got = dst.scalar(facts).await;
    assert_eq!(got, want, "\n{file}");
}

/// **A column typed by an alias over bytes or a variant restores as its
/// base would.** The alias's name was all the dump saw, so a `NOT NULL`
/// alias over `binary` was written `NULL` and the restore stopped (Msg 515),
/// and a variant through an alias came back an `nvarchar` one.
#[tokio::test(flavor = "multi_thread")]
async fn a_dump_restores_alias_typed_bytes_and_variants() {
    use schemaic_core::dump::DumpOptions;
    if !enabled() || azure_cannot("restores into a second database, which it has not got") {
        return;
    }
    let src = Scratch::create("aliassrc").await;
    src.exec(
        "CREATE TYPE dbo.Hash FROM binary(4) NOT NULL; \
         CREATE TYPE dbo.Blob FROM varbinary(max) NULL; \
         CREATE TYPE dbo.Var FROM sql_variant NULL;",
    )
    .await;
    src.exec(
        "CREATE TABLE dbo.aliased (id int PRIMARY KEY, h dbo.Hash, b dbo.Blob, v dbo.Var); \
         INSERT dbo.aliased VALUES (1, 0xDEADBEEF, 0x0102, CAST('2026-01-02' AS date)); \
         INSERT dbo.aliased VALUES (2, 0x00000000, NULL, CAST(N'Ωμέγα' COLLATE Greek_CI_AS AS varchar(10)));",
    )
    .await;
    let facts = "SELECT STRING_AGG(CAST(CONCAT(id, '|', CONVERT(varchar(20), h, 1), '|', \
           CONVERT(varchar(20), b, 1), '|', \
           CAST(SQL_VARIANT_PROPERTY(v, 'BaseType') AS sysname), '|', \
           CAST(SQL_VARIANT_PROPERTY(v, 'Collation') AS sysname), '|', \
           CONVERT(varchar(40), CAST(v AS varbinary(40)), 1)) AS nvarchar(max)), NCHAR(10)) \
           WITHIN GROUP (ORDER BY id) FROM dbo.aliased";
    let want = src.scalar(facts).await;
    assert!(want.contains("1|0xDEADBEEF|0x0102|date|"), "{want}");
    let file = Box::pin(dump_file(&src, DumpOptions::default())).await;
    let dst = Scratch::create("aliasdst").await;
    let end = Box::pin(restore_file(&dst, &file)).await;
    assert!(
        matches!(end, schemaic_core::script::ExecEnd::Done),
        "{end:?}\n{file}"
    );
    assert_eq!(dst.scalar(facts).await, want, "\n{file}");
}

/// **A dump's comments are no part of the modules it restores.** SQL Server
/// stores a module's whole batch as its definition, and a comment-only step
/// closes no batch, so a section heading (`-- Routines and events`,
/// `-- Triggers`, a view's `-- dbo.v_c`) sits in whichever batch follows it.
/// That is a module's `SET ANSI_NULLS` batch, never its `CREATE`
/// (`ddl::tsql_settings_scripted`), so every restored definition opens with
/// `CREATE`; this pins it against the real server, as the unit test
/// `a_sql_server_dump_closes_every_batch_with_go` pins the file's shape.
///
/// **Restored as `sqlcmd` restores**, a batch at a time, every line between
/// two `GO`s sent as it is ([`restore_batches`]): the app's own Run file cuts
/// the batches into statements and drops a comment-only one, so it would
/// never send a heading at all.
#[tokio::test(flavor = "multi_thread")]
async fn a_dumps_comments_stay_out_of_the_modules_it_restores() {
    use schemaic_core::dump::DumpOptions;
    if !enabled() || azure_cannot("restores into a second database, which it has not got") {
        return;
    }
    let src = Scratch::create("cmtsrc").await;
    src.exec("CREATE TABLE dbo.c (id int PRIMARY KEY)").await;
    src.exec("CREATE TRIGGER dbo.tr_c ON dbo.c AFTER INSERT AS SET NOCOUNT ON;")
        .await;
    src.exec("CREATE VIEW dbo.v_c AS SELECT id FROM dbo.c")
        .await;
    src.exec("CREATE PROCEDURE dbo.p_c AS SELECT COUNT(*) FROM dbo.c;")
        .await;
    src.exec("CREATE FUNCTION dbo.f_c (@x int) RETURNS int AS BEGIN RETURN @x; END")
        .await;
    let file = Box::pin(dump_file(&src, DumpOptions::default())).await;
    let dst = Scratch::create("cmtdst").await;
    let end = Box::pin(restore_batches(&dst, &file)).await;
    assert!(
        matches!(end, schemaic_core::script::ExecEnd::Done),
        "{end:?}\n{file}"
    );
    for name in ["dbo.tr_c", "dbo.v_c", "dbo.p_c", "dbo.f_c"] {
        let def = dst
            .scalar(&format!("SELECT OBJECT_DEFINITION(OBJECT_ID(N'{name}'))"))
            .await;
        assert!(
            def.trim_start().starts_with("CREATE"),
            "{name} restored as:\n{def}\n---\n{file}"
        );
    }
}

/// **A replay stops before it drops anything where a graph table is there.**
/// The file cannot put one back as it is — an edge's rows are not in it, and
/// a node's would come back under new node ids — and it dropped them: replayed
/// onto the copy it had restored, every relationship was gone, with the run
/// reporting success.
#[tokio::test(flavor = "multi_thread")]
async fn a_replay_refuses_to_replace_a_graph_table() {
    use schemaic_core::dump::DumpOptions;
    if !enabled() || azure_cannot("restores into a second database, which it has not got") {
        return;
    }
    let src = Scratch::create("dump_graphsrc").await;
    src.exec(
        "CREATE TABLE dbo.plain (id int PRIMARY KEY); INSERT dbo.plain VALUES (1); \
         CREATE TABLE dbo.person (id int PRIMARY KEY) AS NODE; \
         INSERT dbo.person (id) VALUES (1), (2); \
         CREATE TABLE dbo.knows (since int) AS EDGE;",
    )
    .await;
    let file = Box::pin(dump_file(&src, DumpOptions::default())).await;
    let dst = Scratch::create("dump_graphdst").await;
    let end = Box::pin(restore_file(&dst, &file)).await;
    assert!(
        matches!(end, schemaic_core::script::ExecEnd::Done),
        "{end:?}\n{file}"
    );
    dst.exec(
        "INSERT dbo.knows ($from_id, $to_id, since) \
         SELECT p1.$node_id, p2.$node_id, 2021 FROM dbo.person p1, dbo.person p2 \
         WHERE p1.id = 2 AND p2.id = 1; \
         INSERT dbo.plain VALUES (2);",
    )
    .await;
    let end = Box::pin(restore_file(&dst, &file)).await;
    assert!(
        matches!(&end, schemaic_core::script::ExecEnd::Failed { message, .. }
            if message.contains("this file cannot put dbo.")
                && message.contains("before dropping anything")),
        "{end:?}\n{file}"
    );
    // The edge, the nodes it joins and the plain table are as they were.
    assert_eq!(
        dst.scalar(
            "SELECT CONCAT((SELECT COUNT(*) FROM dbo.knows k JOIN dbo.person a \
             ON k.$from_id = a.$node_id JOIN dbo.person b ON k.$to_id = b.$node_id \
             WHERE a.id = 2 AND b.id = 1), '|', (SELECT COUNT(*) FROM dbo.plain))"
        )
        .await,
        "1|2"
    );
}

/// **Without *One transaction*, a replay that fails at a `DROP` has dropped
/// only what it already put back.** A key from a table outside the export
/// refuses its target's drop (Msg 3726); with every `DROP` up front, the
/// tables and the view ahead of it were gone by then, and every `CREATE` was
/// below the failure.
#[tokio::test(flavor = "multi_thread")]
async fn a_replay_without_its_transaction_fails_before_dropping_what_it_cannot_put_back() {
    use schemaic_core::dump::DumpOptions;
    if !enabled() {
        return;
    }
    let s = Scratch::create("dump_notx").await;
    s.exec(
        "CREATE TABLE dbo.customers (id int PRIMARY KEY); INSERT dbo.customers VALUES (1); \
         CREATE TABLE dbo.products (id int PRIMARY KEY); INSERT dbo.products VALUES (1); \
         CREATE TABLE dbo.orders (id int PRIMARY KEY, c int REFERENCES dbo.customers(id));",
    )
    .await;
    s.exec("CREATE VIEW dbo.v_c AS SELECT id FROM dbo.customers")
        .await;
    let opts = DumpOptions {
        wrap_transaction: false,
        ..Default::default()
    };
    let file = Box::pin(dump_file_of(&s, opts, |n| n != "dbo.orders")).await;
    let end = Box::pin(restore_file(&s, &file)).await;
    assert!(
        matches!(&end, schemaic_core::script::ExecEnd::Failed { message, .. } if message.contains("3726")),
        "{end:?}\n{file}"
    );
    assert_eq!(
        s.scalar(
            "SELECT CONCAT(OBJECT_ID('dbo.products', 'U') / OBJECT_ID('dbo.products', 'U'), \
             OBJECT_ID('dbo.v_c', 'V') / OBJECT_ID('dbo.v_c', 'V'), \
             (SELECT COUNT(*) FROM dbo.customers))"
        )
        .await,
        "111",
        "{file}"
    );
}

/// **A replay drops a schema-bound function between the tables it sits
/// between.** It holds the table it reads (Msg 3729 on that table's drop) and
/// is held by the table whose column calls it, so the drops have to mirror
/// the creation order; one block of routine drops after every table stopped
/// the replay.
#[tokio::test(flavor = "multi_thread")]
async fn a_replay_drops_a_schema_bound_function_between_its_tables() {
    use schemaic_core::dump::DumpOptions;
    if !enabled() || azure_cannot("restores into a second database, which it has not got") {
        return;
    }
    let src = Scratch::create("dump_sbsrc").await;
    src.exec("CREATE TABLE dbo.t1 (id int PRIMARY KEY); INSERT dbo.t1 VALUES (1), (2)")
        .await;
    src.exec(
        "CREATE FUNCTION dbo.f_cnt () RETURNS int WITH SCHEMABINDING AS \
         BEGIN RETURN (SELECT COUNT(*) FROM dbo.t1) END",
    )
    .await;
    src.exec(
        "CREATE FUNCTION dbo.f_sb () RETURNS TABLE WITH SCHEMABINDING AS \
         RETURN SELECT id FROM dbo.t1",
    )
    .await;
    src.exec(
        "CREATE TABLE dbo.t2 (id int PRIMARY KEY, c AS dbo.f_cnt()); INSERT dbo.t2 (id) VALUES (7)",
    )
    .await;
    let file = Box::pin(dump_file(&src, DumpOptions::default())).await;
    let dst = Scratch::create("dump_sbdst").await;
    for round in ["restore", "replay"] {
        let end = Box::pin(restore_file(&dst, &file)).await;
        assert!(
            matches!(end, schemaic_core::script::ExecEnd::Done),
            "{round}: {end:?}\n{file}"
        );
        assert_eq!(
            dst.scalar(
                "SELECT CONCAT((SELECT COUNT(*) FROM dbo.t1), '|', \
                 (SELECT MAX(c) FROM dbo.t2), '|', (SELECT COUNT(*) FROM dbo.f_sb()))"
            )
            .await,
            "2|2|2",
            "{round}"
        );
    }
}

/// **A replay drops no module it cannot restate whole.** The server keeps no
/// text for an encrypted procedure or an encrypted member of a numbered
/// group, and no script carries a signature: replayed onto its source, the
/// file dropped each and put back a comment, a bare `;` or an unsigned copy,
/// and reported success. The encrypted procedure is now left as it is and
/// the replay goes through; the group and the signed modules stop it before
/// it drops anything.
#[tokio::test(flavor = "multi_thread")]
async fn a_replay_drops_no_module_it_cannot_restate_whole() {
    use schemaic_core::dump::DumpOptions;
    if !enabled() || azure_cannot("Azure SQL Database has no numbered procedures") {
        return;
    }
    let s = Scratch::create("dump_modules").await;
    s.exec("CREATE TABLE dbo.t (id int PRIMARY KEY); INSERT dbo.t VALUES (1)")
        .await;
    s.exec("CREATE PROCEDURE dbo.secret WITH ENCRYPTION AS SELECT 42 AS n")
        .await;
    s.exec("CREATE PROCEDURE dbo.ok AS SELECT 4 AS n").await;
    let present = "SELECT CONCAT(OBJECT_ID('dbo.secret') / OBJECT_ID('dbo.secret'), \
                   (SELECT COUNT(*) FROM sys.numbered_procedures), \
                   (SELECT COUNT(*) FROM sys.crypt_properties WHERE class = 1), \
                   (SELECT COUNT(*) FROM dbo.t))";
    let file = Box::pin(dump_file(&s, DumpOptions::default())).await;
    let end = Box::pin(restore_file(&s, &file)).await;
    assert!(
        matches!(end, schemaic_core::script::ExecEnd::Done),
        "{end:?}\n{file}"
    );
    assert_eq!(s.scalar(present).await, "1001", "{file}");

    let refused = |end: &schemaic_core::script::ExecEnd, what: &str| {
        matches!(end, schemaic_core::script::ExecEnd::Failed { message, .. }
            if message.contains(what) && message.contains("before dropping anything"))
    };
    s.exec("CREATE PROCEDURE dbo.grp AS SELECT 1 AS n").await;
    s.exec("CREATE PROCEDURE dbo.grp;2 WITH ENCRYPTION AS SELECT 2 AS n")
        .await;
    let file = Box::pin(dump_file(&s, DumpOptions::default())).await;
    assert!(file.contains("procedure grp;2 was not available"), "{file}");
    let end = Box::pin(restore_file(&s, &file)).await;
    assert!(refused(&end, "grp;2"), "{end:?}\n{file}");
    assert_eq!(s.scalar(present).await, "1101", "{file}");
    s.exec("DROP PROCEDURE dbo.grp").await;

    s.exec("CREATE TRIGGER dbo.tr_signed ON dbo.t AFTER INSERT AS SET NOCOUNT ON")
        .await;
    s.exec(
        "CREATE CERTIFICATE zz_dump_cert ENCRYPTION BY PASSWORD = 'Pa55word!!zz' \
         WITH SUBJECT = 'schemaic test'",
    )
    .await;
    for name in ["ok", "tr_signed"] {
        s.exec(&format!(
            "ADD SIGNATURE TO dbo.{name} BY CERTIFICATE zz_dump_cert WITH PASSWORD = 'Pa55word!!zz'"
        ))
        .await;
    }
    let file = Box::pin(dump_file(&s, DumpOptions::default())).await;
    // And the file says the signature is not in it.
    assert!(
        file.contains("-- NOTE: procedure [dbo].[ok] is signed, and the signature is not in"),
        "{file}"
    );
    assert!(
        file.contains("-- NOTE: trigger [dbo].[tr_signed] is signed, and the signature is not in"),
        "{file}"
    );
    let end = Box::pin(restore_file(&s, &file)).await;
    assert!(refused(&end, "signed"), "{end:?}\n{file}");
    assert_eq!(s.scalar(present).await, "1021", "{file}");
}

/// **A replay rewinds no sequence.** One in an exported schema that no chosen
/// table names — here drawn from by application code for a table left out of
/// the export — was dropped and recreated at the dump's position, so the next
/// key it handed out was one already used (Msg 2627).
#[tokio::test(flavor = "multi_thread")]
async fn a_replay_leaves_a_sequence_where_it_stands() {
    use schemaic_core::dump::DumpOptions;
    if !enabled() {
        return;
    }
    let s = Scratch::create("dump_seq").await;
    s.exec("CREATE SCHEMA s2").await;
    s.exec(
        "CREATE SEQUENCE s2.ctr AS int START WITH 1; \
         CREATE TABLE s2.invoices (id int PRIMARY KEY); \
         INSERT s2.invoices VALUES (NEXT VALUE FOR s2.ctr), (NEXT VALUE FOR s2.ctr);",
    )
    .await;
    s.exec("CREATE VIEW s2.v AS SELECT 1 AS one").await;
    let file = Box::pin(dump_file_of(&s, DumpOptions::default(), |n| n == "s2.v")).await;
    s.exec("INSERT s2.invoices VALUES (NEXT VALUE FOR s2.ctr), (NEXT VALUE FOR s2.ctr)")
        .await;
    let end = Box::pin(restore_file(&s, &file)).await;
    assert!(
        matches!(end, schemaic_core::script::ExecEnd::Done),
        "{end:?}\n{file}"
    );
    s.exec("INSERT s2.invoices VALUES (NEXT VALUE FOR s2.ctr)")
        .await;
    assert_eq!(
        s.scalar("SELECT CONCAT(COUNT(*), '|', MAX(id)) FROM s2.invoices")
            .await,
        "5|5",
        "{file}"
    );
}

/// **What reads a table the dump leaves out is left out with it.** A
/// system-versioned table is not in the file, and a view over it stopped the
/// restore into an empty database at its `CREATE VIEW` (Msg 208), the
/// transaction rolling back everything — so the view, and the inline function
/// bound to it, go too, named in the header; the rest restores.
#[tokio::test(flavor = "multi_thread")]
async fn a_dump_leaves_out_what_reads_a_table_it_cannot_restate() {
    use schemaic_core::dump::DumpOptions;
    if !enabled() || azure_cannot("restores into a second database, which it has not got") {
        return;
    }
    let src = Scratch::create("dump_tempsrc").await;
    src.exec(
        "CREATE TABLE dbo.dept (id int PRIMARY KEY); INSERT dbo.dept VALUES (1), (2); \
         CREATE TABLE dbo.emp (id int PRIMARY KEY, v int, \
           vf datetime2 GENERATED ALWAYS AS ROW START HIDDEN NOT NULL, \
           vt datetime2 GENERATED ALWAYS AS ROW END HIDDEN NOT NULL, \
           PERIOD FOR SYSTEM_TIME (vf, vt)) \
           WITH (SYSTEM_VERSIONING = ON (HISTORY_TABLE = dbo.emp_history)); \
         INSERT dbo.emp (id, v) VALUES (1, 10);",
    )
    .await;
    src.exec("CREATE VIEW dbo.emp_v AS SELECT id, v FROM dbo.emp")
        .await;
    src.exec("CREATE VIEW dbo.dept_v AS SELECT id FROM dbo.dept")
        .await;
    src.exec("CREATE FUNCTION dbo.tvf_emp () RETURNS TABLE AS RETURN SELECT id FROM dbo.emp")
        .await;
    let file = Box::pin(dump_file(&src, DumpOptions::default())).await;
    assert!(
        file.contains("dbo.emp_v (a view reading dbo.emp, which is not in this file)")
            && file.contains("dbo.tvf_emp (a function bound to dbo.emp"),
        "{file}"
    );
    let dst = Scratch::create("dump_tempdst").await;
    let end = Box::pin(restore_file(&dst, &file)).await;
    assert!(
        matches!(end, schemaic_core::script::ExecEnd::Done),
        "{end:?}\n{file}"
    );
    assert_eq!(
        dst.scalar(
            "SELECT CONCAT((SELECT COUNT(*) FROM dbo.dept_v), '|', OBJECT_ID('dbo.emp'), '|', \
             OBJECT_ID('dbo.emp_v'), '|', OBJECT_ID('dbo.tvf_emp'))"
        )
        .await,
        "2|||"
    );
}

/// **A ledger table is not dumped as a plain one.** An updatable ledger
/// table came back with its ledger columns as ordinary ones, its history as
/// a plain table and its ledger view as a plain view, and an append-only one
/// accepted `DELETE` — tamper-evidence gone, with the restore reporting
/// success. Each is left out and named; the rest restores.
#[tokio::test(flavor = "multi_thread")]
async fn a_dump_leaves_a_ledger_table_out_and_names_it() {
    use schemaic_core::dump::DumpOptions;
    if !enabled() || azure_cannot("restores into a second database, which it has not got") {
        return;
    }
    let src = Scratch::create("dump_ledgersrc").await;
    src.exec(
        "CREATE TABLE dbo.plain (id int PRIMARY KEY); INSERT dbo.plain VALUES (1); \
         CREATE TABLE dbo.led (id int PRIMARY KEY, v int) \
           WITH (SYSTEM_VERSIONING = ON, LEDGER = ON); \
         INSERT dbo.led VALUES (1, 10); UPDATE dbo.led SET v = 11; \
         CREATE TABLE dbo.app_only (id int PRIMARY KEY) WITH (LEDGER = ON (APPEND_ONLY = ON)); \
         INSERT dbo.app_only VALUES (1);",
    )
    .await;
    let file = Box::pin(dump_file(&src, DumpOptions::default())).await;
    for what in [
        "dbo.led (an updatable ledger table)",
        "dbo.app_only (an append-only ledger table)",
        "(the history table of a ledger table)",
        "dbo.led_Ledger (the ledger view of a ledger table)",
        "dbo.app_only_Ledger (the ledger view of a ledger table)",
    ] {
        assert!(file.contains(what), "{what}\n{file}");
    }
    assert!(
        !file.contains("ledger_start_transaction_id") && !file.contains("CREATE VIEW"),
        "{file}"
    );
    let dst = Scratch::create("dump_ledgerdst").await;
    let end = Box::pin(restore_file(&dst, &file)).await;
    assert!(
        matches!(end, schemaic_core::script::ExecEnd::Done),
        "{end:?}\n{file}"
    );
    assert_eq!(
        dst.scalar(
            "SELECT CONCAT((SELECT COUNT(*) FROM dbo.plain), '|', \
             (SELECT COUNT(*) FROM sys.objects WHERE is_ms_shipped = 0 AND type IN ('U', 'V')))"
        )
        .await,
        "1|1"
    );
}

/// **What only a dumped routine names is carried.** A `dbo` alias type that
/// only an `s3` procedure's parameter used was neither carried nor named in a
/// dump of `s3.t`, and the restore into an empty database stopped at the
/// procedure (Msg 2715).
#[tokio::test(flavor = "multi_thread")]
async fn a_dump_carries_a_type_only_its_routines_name() {
    use schemaic_core::dump::DumpOptions;
    if !enabled() || azure_cannot("restores into a second database, which it has not got") {
        return;
    }
    let src = Scratch::create("dump_rtypesrc").await;
    src.exec("CREATE SCHEMA s3").await;
    src.exec(
        "CREATE TYPE dbo.Hash FROM binary(4) NOT NULL; \
         CREATE TABLE s3.t (id int PRIMARY KEY); INSERT s3.t VALUES (1);",
    )
    .await;
    src.exec("CREATE PROCEDURE s3.p @h dbo.Hash AS SELECT @h AS h")
        .await;
    let file = Box::pin(dump_file_of(&src, DumpOptions::default(), |n| n == "s3.t")).await;
    let dst = Scratch::create("dump_rtypedst").await;
    let end = Box::pin(restore_file(&dst, &file)).await;
    assert!(
        matches!(end, schemaic_core::script::ExecEnd::Done),
        "{end:?}\n{file}"
    );
    assert_eq!(
        dst.scalar(
            "SELECT CONCAT(TYPE_NAME(TYPE_ID('dbo.Hash')), '|', OBJECT_ID('s3.p') / OBJECT_ID('s3.p'))"
        )
        .await,
        "Hash|1"
    );
}

/// **An alias type's bound default and rule are named in the dump.** No
/// `CREATE TYPE` carries a `sp_bindefault`/`sp_bindrule` binding, so the
/// restored column of the type stored `NULL` where the source stores 7 and
/// took a negative value, with nothing in the file saying so.
#[tokio::test(flavor = "multi_thread")]
async fn a_dump_names_an_alias_types_bound_default_and_rule() {
    use schemaic_core::dump::DumpOptions;
    if !enabled() {
        return;
    }
    let src = Scratch::create("dump_bound").await;
    src.exec("CREATE TYPE dbo.Qty FROM int NULL").await;
    src.exec("CREATE DEFAULT dbo.df_seven AS 7").await;
    src.exec("CREATE RULE dbo.rl_pos AS @v >= 0").await;
    src.exec(
        "EXEC sp_bindefault N'dbo.df_seven', N'dbo.Qty'; \
         EXEC sp_bindrule N'dbo.rl_pos', N'dbo.Qty'; \
         CREATE TABLE dbo.stock (id int PRIMARY KEY, q dbo.Qty);",
    )
    .await;
    let file = Box::pin(dump_file(&src, DumpOptions::default())).await;
    assert!(
        file.contains("1 alias type here carries a bound default or rule")
            && file.contains("dbo.Qty (default dbo.df_seven, rule dbo.rl_pos)"),
        "{file}"
    );
}

/// **A default or rule bound to a column is named in its script.** No
/// `CREATE TABLE` carries a `sp_bindefault`/`sp_bindrule` binding, and the
/// reader took defaults from `sys.default_constraints` alone, so Copy DDL and
/// the dump restated the column with neither and said nothing. A binding the
/// column has from its alias type is the type's, which the header names.
#[tokio::test(flavor = "multi_thread")]
async fn a_columns_bound_default_and_rule_are_named_in_its_script() {
    use schemaic_core::dump::DumpOptions;
    if !enabled() || azure_cannot("restores into a second database, which it has not got") {
        return;
    }
    let src = Scratch::create("col_bound").await;
    src.exec("CREATE TYPE dbo.Qty FROM int NULL").await;
    src.exec("CREATE DEFAULT dbo.df_zero AS 0").await;
    src.exec("CREATE RULE dbo.rl_small AS @v < 100").await;
    src.exec(
        "EXEC sp_bindefault N'dbo.df_zero', N'dbo.Qty'; \
         CREATE TABLE dbo.t (id int PRIMARY KEY, a int NULL, b int NULL, q dbo.Qty NULL); \
         EXEC sp_bindefault N'dbo.df_zero', N'dbo.t.a'; \
         EXEC sp_bindrule N'dbo.rl_small', N'dbo.t.a'; \
         EXEC sp_bindrule N'dbo.rl_small', N'dbo.t.b';",
    )
    .await;
    let t = read_table(&src, "t").await;
    let bound: Vec<_> = t
        .tsql_bindings
        .iter()
        .map(|b| (b.column.as_str(), b.default.as_deref(), b.rule.as_deref()))
        .collect();
    assert_eq!(
        bound,
        [
            ("a", Some("dbo.df_zero"), Some("dbo.rl_small")),
            ("b", None, Some("dbo.rl_small"))
        ],
        "q's default is its type's"
    );
    let ddl = t.create_ddl(MS);
    for note in [
        "-- a: default dbo.df_zero and rule dbo.rl_small are bound to it",
        "-- b: rule dbo.rl_small is bound to it (sp_bindrule)",
    ] {
        assert!(ddl.contains(note), "{note}\n{ddl}");
    }
    assert!(!ddl.contains("-- q:"), "{ddl}");
    // The dump carries the same notes, and they restore as the comments they
    // are.
    let file = Box::pin(dump_file(&src, DumpOptions::default())).await;
    assert!(file.contains("-- a: default dbo.df_zero"), "{file}");
    let dst = Scratch::create("col_bound_dst").await;
    let end = Box::pin(restore_file(&dst, &file)).await;
    assert!(
        matches!(end, schemaic_core::script::ExecEnd::Done),
        "{end:?}\n{file}"
    );
}

/// **A synonym written `db..object` comes back as written.** `PARSENAME`'s
/// missing schema part was dropped, so the copy's synonym read `[db].[t]` —
/// schema `db` in the restoring database — and pointed at nothing (Msg 208).
/// Synonyms bind late, so the target database need not exist.
#[tokio::test(flavor = "multi_thread")]
async fn a_dump_keeps_a_synonyms_empty_schema_part() {
    use schemaic_core::dump::DumpOptions;
    if !enabled() || azure_cannot("restores into a second database, which it has not got") {
        return;
    }
    let src = Scratch::create("dump_synsrc").await;
    src.exec(
        "CREATE TABLE dbo.t (id int PRIMARY KEY); \
         CREATE SYNONYM dbo.syn_dd FOR zz_nowhere_db..t; \
         CREATE SYNONYM dbo.syn_full FOR zz_nowhere_db.sales.t;",
    )
    .await;
    // In the dumped schema, so the dump carries them.
    let file = Box::pin(dump_file(&src, DumpOptions::default())).await;
    let dst = Scratch::create("dump_syndst").await;
    let end = Box::pin(restore_file(&dst, &file)).await;
    assert!(
        matches!(end, schemaic_core::script::ExecEnd::Done),
        "{end:?}\n{file}"
    );
    let targets = "SELECT STRING_AGG(CONCAT(name, '=', base_object_name), ',') \
                   WITHIN GROUP (ORDER BY name) FROM sys.synonyms";
    assert_eq!(
        dst.scalar(targets).await,
        "syn_dd=[zz_nowhere_db]..[t],syn_full=[zz_nowhere_db].[sales].[t]",
        "{file}"
    );
}

/// **Tables, views and the functions they call are created in one order.**
/// A view reaching another view only through an inline function, and a table
/// whose check calls a function counting another table, were written
/// caller-first — the names sort them so — and the restore into an empty
/// database stopped at the function (Msg 208) and at the checked rows (Msg
/// 208).
#[tokio::test(flavor = "multi_thread")]
async fn a_dump_orders_tables_and_views_through_the_functions_they_call() {
    use schemaic_core::dump::DumpOptions;
    if !enabled() || azure_cannot("restores into a second database, which it has not got") {
        return;
    }
    let src = Scratch::create("dump_fnordsrc").await;
    src.exec("CREATE TABLE dbo.t (id int PRIMARY KEY); INSERT dbo.t VALUES (1), (2), (3)")
        .await;
    src.exec("CREATE VIEW dbo.v_z AS SELECT id AS n FROM dbo.t")
        .await;
    src.exec("CREATE FUNCTION dbo.tvf_b () RETURNS TABLE AS RETURN SELECT n FROM dbo.v_z")
        .await;
    src.exec("CREATE VIEW dbo.v_a AS SELECT n FROM dbo.tvf_b()")
        .await;
    src.exec(
        "CREATE TABLE dbo.z_customers (id int PRIMARY KEY); INSERT dbo.z_customers VALUES (7), (8)",
    )
    .await;
    src.exec(
        "CREATE FUNCTION dbo.f_known (@id int) RETURNS int AS \
         BEGIN RETURN (SELECT COUNT(*) FROM dbo.z_customers WHERE id = @id); END",
    )
    .await;
    src.exec(
        "CREATE TABLE dbo.a_orders (id int PRIMARY KEY, customer_id int, \
           CONSTRAINT ck_known CHECK (dbo.f_known(customer_id) = 1)); \
         INSERT dbo.a_orders VALUES (1, 7), (2, 8);",
    )
    .await;
    let file = Box::pin(dump_file(&src, DumpOptions::default())).await;
    let dst = Scratch::create("dump_fnorddst").await;
    for round in ["restore", "replay"] {
        let end = Box::pin(restore_file(&dst, &file)).await;
        assert!(
            matches!(end, schemaic_core::script::ExecEnd::Done),
            "{round}: {end:?}\n{file}"
        );
        assert_eq!(
            dst.scalar(
                "SELECT CONCAT((SELECT SUM(n) FROM dbo.v_a), '|', \
                 (SELECT COUNT(*) FROM dbo.a_orders))"
            )
            .await,
            "6|2",
            "{round}"
        );
    }
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
    assert!(err.to_string().contains("empty or blank value"), "{err}");
    assert_eq!(
        s.scalar("SELECT qty FROM dbo.t").await,
        "5",
        "an alias type is its base type"
    );
    // A space converts just as `''` does.
    commit(
        &s,
        GridWrite {
            updates: vec![row_edit(
                &s,
                "t",
                &[("qty", txt("  "))],
                &[("id", Value::Int(1))],
            )],
            ..Default::default()
        },
    )
    .await
    .expect_err("a value of spaces is refused too");
    assert_eq!(s.scalar("SELECT qty FROM dbo.t").await, "5");
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

/// **A database is created with its collation and dropped**, each plan through
/// `run_server_ddl` — attached to `master`, outside a transaction, which T-SQL
/// requires of `CREATE DATABASE` — with the plan exactly as the emitter writes
/// it, the collation a bare name.
#[tokio::test(flavor = "multi_thread")]
async fn a_database_is_created_with_its_collation_and_dropped() {
    use schemaic_core::ddl::{self, Change, DatabaseDraft};
    if !enabled() || azure_cannot("creating a database would be a new billable one") {
        return;
    }
    let name = format!("{PREFIX}{}_mssql_created", std::process::id());
    assert_scratch_name(&name);
    let base = base_db();
    let create = ddl::server_level(
        &name,
        MS,
        Change::CreateDatabase(Box::new(DatabaseDraft {
            collation: Some("Latin1_General_100_CI_AS_SC_UTF8".into()),
            ..DatabaseDraft::blank(&name)
        })),
    )
    .emit();
    base.run_server_ddl(Some(&name), &create, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}\n{}", create.join("\n")));
    let collation = base
        .fetch_query(
            None,
            &format!("SELECT collation_name FROM sys.databases WHERE name = N'{name}'"),
            1,
            CancellationToken::new(),
        )
        .await
        .expect("the catalogue");
    let drop = ddl::server_level(&name, MS, Change::DropDatabase { name: name.clone() }).emit();
    let dropped = base
        .run_server_ddl(Some(&name), &drop, CancellationToken::new())
        .await;
    assert_eq!(
        collation.cell(0, 0).map(|c| c.display().to_string()),
        Some("Latin1_General_100_CI_AS_SC_UTF8".to_string())
    );
    dropped.unwrap_or_else(|e| panic!("{e}\n{}", drop.join("\n")));
    let left = base
        .fetch_query(
            None,
            &format!("SELECT COUNT(*) FROM sys.databases WHERE name = N'{name}'"),
            1,
            CancellationToken::new(),
        )
        .await
        .expect("the catalogue");
    assert_eq!(
        left.cell(0, 0).map(|c| c.display().to_string()),
        Some("0".to_string())
    );
}

/// **A namespace is created and dropped in the plan's transaction**, and
/// `CREATE SCHEMA` — which T-SQL wants first in its batch — runs as the
/// emitter writes it, ahead of a table in the same plan. A drop of one still
/// holding that table is the server's refusal, not a cascade, and leaves both.
#[tokio::test(flavor = "multi_thread")]
async fn a_namespace_is_created_with_its_table_and_dropped_only_when_empty() {
    use schemaic_core::ddl::{self, Change};
    use schemaic_core::schema::{ColumnInfo, TableInfo};
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_schema").await;
    let table = TableInfo {
        name: "orders".into(),
        schema: Some("sales".into()),
        columns: vec![ColumnInfo {
            name: "id".into(),
            type_name: "int".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut plan = ddl::single(
        "orders",
        Some("sales"),
        MS,
        Change::CreateTable(Box::new(ddl::TableDraft::from_table(&table))),
    );
    plan.changes.insert(
        0,
        Change::CreateSchema {
            name: "sales".into(),
            owner: None,
        },
    );
    let stmts = plan.emit();
    s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}\n{}", stmts.join("\n")));
    let in_sales = "SELECT COUNT(*) FROM sys.tables WHERE SCHEMA_NAME(schema_id) = N'sales'";
    assert_eq!(
        s.scalar(in_sales).await,
        "1",
        "the table is in the new schema"
    );

    let drop = |name: &str| {
        ddl::server_level(
            name,
            MS,
            Change::DropSchema {
                name: name.to_string(),
            },
        )
        .emit()
    };
    let refused =
        s.db.run_ddl(&s.name, &drop("sales"), CancellationToken::new())
            .await;
    assert!(refused.is_err(), "a schema holding a table was dropped");
    assert_eq!(s.scalar(in_sales).await, "1", "nothing cascaded");

    s.exec("DROP TABLE sales.orders").await;
    let stmts = drop("sales");
    s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}\n{}", stmts.join("\n")));
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM sys.schemas WHERE name = N'sales'")
            .await,
        "0"
    );
}

/// **The editor's checker and the server agree on T-SQL's edges**
/// (S7.2-L1-01 … L1-08). Each procedure body below is one the checker reads
/// as clean — a `SET NOCOUNT ON` before a write, a `GRANT` before an `IF`, the
/// constructs SQL Server's own modules hold that sqlparser's grammar lacks —
/// and each must compile here; each batch after it is one the checker flags
/// and the server must refuse. Either half failing means a rule in
/// `intel::diagnostics` stands on a claim about the server that is not true
/// of this version.
#[tokio::test(flavor = "multi_thread")]
async fn the_t_sql_the_checker_passes_compiles_and_what_it_flags_is_refused() {
    use schemaic_core::intel::{Catalog, Severity, diagnostics};
    if !enabled() {
        return;
    }
    let s = Scratch::create("checker_edges").await;
    s.exec(
        "CREATE TABLE dbo.employees (id int NOT NULL PRIMARY KEY, name nvarchar(max), \
         salary int, dept_id int); CREATE TABLE dbo.departments (id int, name nvarchar(50))",
    )
    .await;
    s.exec(
        "CREATE FUNCTION dbo.f2 (@a nvarchar(10), @b int = 1) RETURNS int AS BEGIN RETURN 1 END",
    )
    .await;
    let catalog = Catalog::build(&[], None);
    let errors = |sql: &str| {
        diagnostics(sql, &catalog, MS)
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.message)
            .collect::<Vec<_>>()
    };
    let bodies = [
        "SET NOCOUNT ON\nUPDATE dbo.employees SET name = N'x' WHERE id = 1",
        "SET XACT_ABORT ON\nDELETE TOP (10) FROM dbo.employees",
        "GRANT SELECT ON dbo.employees TO public\nIF @@ERROR <> 0 PRINT 1",
        "IF 1 = 1 BEGIN PRINT 1 END ELSE BEGIN PRINT 2 END",
        "BEGIN TRY SELECT 1 END TRY\n-- a comment\nBEGIN CATCH THROW END CATCH",
        "DECLARE @x int = 1\nIF @x = 1 lbl: ELSE PRINT 2",
        "SELECT PARSE('1.5' AS decimal(10,2)) AS p",
        "DECLARE @i int = 1\nSET @i += 1\nSELECT @i += 1",
        "CREATE TABLE #t (id int NOT NULL PRIMARY KEY CLUSTERED, b int UNIQUE NONCLUSTERED, )",
        "DECLARE @t TABLE (a int PRIMARY KEY CLUSTERED)",
        "ALTER TABLE dbo.employees NOCHECK CONSTRAINT ALL",
        "SELECT e.id FROM dbo.employees e INNER HASH JOIN dbo.departments d ON d.id = e.dept_id",
        "UPDATE dbo.employees SET name.WRITE(N'x', NULL, NULL) WHERE id = 1",
        "SELECT COUNT(*) FROM dbo.employees WITH (NOWAIT NOLOCK)",
        "SELECT id AS N'job_id' FROM dbo.employees",
        "WITH XMLNAMESPACES (N'urn:x' AS x) SELECT 1 AS a",
        "IF 1 < = 2 PRINT 1",
        "SELECT 0X00, {fn LCASE(N'A')}",
        "SELECT * FROM ::fn_listextendedproperty(NULL, NULL, NULL, NULL, NULL, NULL, NULL)",
        "DECLARE @r int\nSELECT @r = dbo.f2(N'x', DEFAULT)",
        "GRANT SELECT ON OBJECT::dbo.employees TO public",
        "DROP SYNONYM dbo.nope",
    ];
    for (n, body) in bodies.iter().enumerate() {
        let sql = format!("CREATE PROCEDURE dbo.p{n} AS\nBEGIN\n{body}\nEND");
        assert_eq!(errors(&sql), Vec::<String>::new(), "the checker: {sql}");
        s.try_exec(&sql)
            .await
            .unwrap_or_else(|e| panic!("the server refused what the checker passes: {e}\n{sql}"));
    }
    let refused = [
        "ELSE SELECT 1;\nSELECT 2;",
        "BEGIN SELECT 1;\nSELECT 2;",
        "BEGIN TRY SELECT 1 END TRY;\nBEGIN CATCH PRINT 1 END CATCH",
        "SELECT 1\nEND\nSELECT 2;",
        "SELECT 1\nTHROW 50000, 'x', 1;",
        "MERGE dbo.employees AS t USING dbo.departments AS s ON t.id = s.id \
         WHEN MATCHED THEN DELETE\nSELECT 1;",
        "SELECT id FROM dbo.employees WHERE id *= 1;",
        "SELECT * FROM dbo.employees option;",
        "DECLARE @t TABLE (a int, );",
        "SELECT e.id FROM dbo.employees e HASH JOIN dbo.departments d ON d.id = e.dept_id;",
    ];
    for sql in refused {
        assert!(!errors(sql).is_empty(), "the checker passes: {sql}");
        assert!(
            s.try_exec(sql).await.is_err(),
            "the server ran what the checker flags: {sql}"
        );
    }
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

/// Run a script — Copy DDL's text, a dump — as a restore does: cut at its
/// `GO` lines by the app's own splitter and fed to `Db::run_script`,
/// panicking unless it runs to the end.
async fn replay(s: &Scratch, script: &str) {
    let mut splitter = schemaic_core::script::Splitter::new(MS);
    // A file ends in a newline; a copied script's last `GO` may not.
    let mut stmts = splitter.push_str(&format!("{script}\n"));
    stmts.extend(splitter.finish());
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let feed = tokio::spawn(async move {
        for st in stmts {
            if tx.send(st).await.is_err() {
                break;
            }
        }
    });
    let (end, _) = s.db.run_script(&s.name, rx, CancellationToken::new()).await;
    feed.await.unwrap();
    assert!(
        matches!(end, schemaic_core::script::ExecEnd::Done),
        "{end:?}\n{script}"
    );
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
        // Through the locked stderr handle: libtest shows an `eprintln!` only
        // for a failing test, which made this skip a silent green.
        endpoint::note_leg_no_op(
            "mssql",
            "has no AdventureWorksLT here, so the sample round trip",
        );
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

/// **A real schema reads, and every table round-trips** — a database someone
/// uses rather than a sample, named by `SCHEMAIC_IT_MSSQL_REAL_DATABASE` on
/// whichever leg runs (on Azure its own database beside `schemaic_it`).
/// **Read-only, and guarded so**: the name may not be a scratch one, which the
/// leg's wipe would empty, and nothing here writes — the schema, the account
/// list and each table's rebuild *plan*, never run. What the reading cannot
/// rebuild or edit is printed rather than failed on, since a real schema is
/// entitled to features Schemaic does not restate; a panic, a failed read or
/// a table that is not its own draft is a failure.
#[tokio::test(flavor = "multi_thread")]
async fn a_real_schema_reads_and_every_table_round_trips() {
    use schemaic_core::ddl::TableDraft;
    use std::io::Write as _;
    if !enabled() {
        return;
    }
    let Ok(database) = std::env::var("SCHEMAIC_IT_MSSQL_REAL_DATABASE") else {
        endpoint::note_leg_no_op(
            if on_azure() {
                "mssql-on-azure"
            } else {
                "mssql"
            },
            "names no SCHEMAIC_IT_MSSQL_REAL_DATABASE, so the real-schema read",
        );
        return;
    };
    assert!(
        !database.starts_with(PREFIX) && !database.starts_with("schemaic_it"),
        "{database:?} is a scratch name — the wipe empties those, so a real schema is never one"
    );
    let db = base_db().with_database(Some(&database));
    let schema = db
        .fetch_schema(&database, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("reading {database}: {e}"));
    assert!(!schema.tables.is_empty(), "{database} has no tables");
    db.fetch_principals(Some(&database))
        .await
        .unwrap_or_else(|e| panic!("the accounts of {database}: {e}"));
    let mut notes: Vec<String> = Vec::new();
    for t in schema.tables.iter().filter(|t| !t.is_view) {
        let who = format!("{}.{}", t.schema.as_deref().unwrap_or(""), t.name);
        let own = TableDraft::from_table(t);
        let cs = schemaic_core::ddl::diff(t, &own, MS);
        assert!(cs.changes.is_empty(), "{who}: {:#?}", cs.changes);
        if t.columns.len() > 1 {
            let mut moved = own.clone();
            let last = moved.columns.pop().unwrap();
            moved.columns.insert(0, last);
            let cs = schemaic_core::ddl::diff(t, &moved, MS);
            let _ = cs.emit();
            notes.extend(cs.unsupported().into_iter().map(|r| format!("{who}: {r}")));
        }
        notes.extend(
            t.triggers
                .iter()
                .filter(|tr| !tr.is_editable())
                .map(|tr| format!("{who}: trigger {} is not editable", tr.name)),
        );
    }
    notes.extend(
        schema
            .routines
            .iter()
            .filter(|r| !r.is_editable())
            .map(|r| format!("routine {} is not editable", r.name)),
    );
    let _ = writeln!(
        std::io::stderr().lock(),
        "live: {database}: {} tables read and round-tripped; {} notes{}",
        schema.tables.iter().filter(|t| !t.is_view).count(),
        notes.len(),
        notes.iter().map(|n| format!("\n  {n}")).collect::<String>()
    );
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
    // Materialised: `ALTER VIEW` drops both of these, schema binding or not.
    s.exec("CREATE UNIQUE CLUSTERED INDEX cix ON dbo.v (a)")
        .await;
    s.exec("CREATE NONCLUSTERED INDEX nix ON dbo.v (b)").await;
    let indexes = |name: &'static str| {
        let s = &s;
        async move {
            s.scalar(&format!(
                "SELECT STRING_AGG(name, ',') WITHIN GROUP (ORDER BY name) FROM sys.indexes \
                 WHERE object_id = OBJECT_ID('dbo.{name}') AND index_id > 0"
            ))
            .await
        }
    };
    let v = read_table(&s, "v").await;
    let o = v.view_options.clone().expect("options");
    assert_eq!(o.column_list.as_deref(), Some("a, b"));
    assert_eq!(o.attributes, ["SCHEMABINDING"]);
    assert_eq!(o.tsql.indexes.len(), 2, "{:?}", o.tsql.indexes);
    assert!(v.indexes.is_empty(), "a view's indexes are not a table's");

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
    assert_eq!(indexes("v").await, "cix,nix", "the indexes built again");
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
    assert_eq!(indexes("w").await, "cix,nix", "the indexes on the new name");

    // Its script builds them after it, and replays.
    let ddl = read_table(&s, "w").await.create_ddl(MS);
    replay(
        &s,
        &format!("DROP VIEW IF EXISTS [dbo].[w];\nGO\n{ddl}\nGO"),
    )
    .await;
    assert_eq!(indexes("w").await, "cix,nix", "the script restored them");
}

/// **A view is edited only when its header was read.** `v$as` is one name —
/// read as `v` and the header's `AS`, the editor opened on `AS SELECT …` and
/// Apply emitted `AS AS` — so it reads whole and an edit applies, schema
/// binding kept. An encrypted view shows no text, and is listed and not
/// editable rather than opened on an empty body.
#[tokio::test(flavor = "multi_thread")]
async fn a_view_is_edited_only_when_its_header_was_read() {
    use schemaic_core::ddl::{ViewDraft, diff_view, view_is_editable};
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_view_read").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY); INSERT dbo.t VALUES (1), (2)")
        .await;
    s.exec("CREATE VIEW dbo.v$as WITH SCHEMABINDING AS SELECT id FROM dbo.t")
        .await;
    s.exec("CREATE VIEW dbo.v_enc WITH ENCRYPTION AS SELECT id FROM dbo.t")
        .await;

    let v = read_table(&s, "v$as").await;
    let o = v.view_options.clone().expect("options");
    assert_eq!(o.attributes, ["SCHEMABINDING"]);
    assert_eq!(v.view_definition.as_deref(), Some("SELECT id FROM dbo.t"));
    assert!(view_is_editable(&v));
    let mut d = ViewDraft::from_table(&v).unwrap();
    d.select = "SELECT id FROM dbo.t WHERE id > 1".into();
    let stmts = diff_view(&v, &d, MS).emit();
    s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
    assert_eq!(s.scalar("SELECT COUNT(*) FROM dbo.[v$as]").await, "1");
    assert_eq!(
        s.scalar("SELECT OBJECTPROPERTY(OBJECT_ID('dbo.[v$as]'), 'IsSchemaBound')")
            .await,
        "1"
    );

    let enc = read_table(&s, "v_enc").await;
    assert!(enc.view_options.as_ref().is_some_and(|o| o.tsql.hidden));
    assert!(!view_is_editable(&enc));
    assert!(!ViewDraft::from_table(&enc).unwrap().validate(MS).is_empty());
}

/// **A view's script names the view the catalogue has**, not the one its
/// stored text says. `sp_rename` leaves `sys.sql_modules` reading `CREATE
/// VIEW dbo.v1`, and a user whose default schema is `sales` who writes
/// `CREATE VIEW v` is stored unqualified; restated verbatim, a dump's
/// `DROP VIEW IF EXISTS [dbo].[v2]` then created `v1` in its place. Each
/// script, replayed as the dump writes it, puts back the view it is for.
#[tokio::test(flavor = "multi_thread")]
async fn a_views_script_names_the_view_the_catalogue_has() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_view_script").await;
    s.exec("CREATE VIEW dbo.v1 AS SELECT 1 AS a").await;
    s.exec("EXEC sp_rename 'dbo.v1', 'v2'").await;
    s.exec("CREATE SCHEMA sales").await;
    s.exec(
        "CREATE USER zz_mod_sales WITHOUT LOGIN WITH DEFAULT_SCHEMA = sales; \
         GRANT CREATE VIEW TO zz_mod_sales; GRANT ALTER ON SCHEMA::sales TO zz_mod_sales",
    )
    .await;
    s.exec("EXECUTE AS USER = 'zz_mod_sales'; EXEC ('CREATE VIEW v AS SELECT 2 AS a'); REVERT")
        .await;
    let schema =
        s.db.fetch_schema(&s.name, CancellationToken::new())
            .await
            .expect("the schema");
    let find = |ns: &str, n: &str| {
        schema
            .tables
            .iter()
            .find(|t| t.schema.as_deref() == Some(ns) && t.name == n)
            .unwrap_or_else(|| panic!("{ns}.{n}"))
            .clone()
    };
    let v2 = find("dbo", "v2");
    assert!(
        v2.create_sql.as_deref().unwrap_or_default().contains("v1"),
        "the stored text"
    );
    let sales_v = find("sales", "v");
    for (t, qname, want) in [(v2, "[dbo].[v2]", "1"), (sales_v, "[sales].[v]", "2")] {
        let ddl = t.create_ddl(MS);
        assert!(ddl.contains(&format!("CREATE VIEW {qname}")), "{ddl}");
        replay(&s, &format!("DROP VIEW IF EXISTS {qname};\nGO\n{ddl}\nGO")).await;
        assert_eq!(s.scalar(&format!("SELECT a FROM {qname}")).await, want);
    }
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM sys.views WHERE name IN ('v1')")
            .await,
        "0"
    );
    assert_eq!(
        s.scalar(
            "SELECT COUNT(*) FROM sys.views WHERE SCHEMA_NAME(schema_id) = 'dbo' AND name = 'v'"
        )
        .await,
        "0"
    );
}

/// **A routine is altered in place and keeps what the alter would reset**:
/// its parameters' defaults (which live only in the text), its `WITH`
/// options, its grant and its comment. A rename is a drop and a create that
/// restates the comment; a scalar function turned table-valued, which `CREATE
/// OR ALTER` refuses, is dropped and created; an encrypted routine is listed,
/// hidden and not editable. Each read back diffs to nothing against its own
/// draft.
#[tokio::test(flavor = "multi_thread")]
async fn a_routine_is_altered_in_place_and_keeps_what_the_alter_resets() {
    use schemaic_core::ddl::{RoutineDraft, diff_routine};
    use schemaic_core::schema::{ExecuteAs, RoutineInfo, RoutineKind, TsqlRoutineOption};
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_routine").await;
    s.exec(
        "create procedure dbo.p @a int = 5, @b nvarchar(10) = N'x, y' \
         with recompile, execute as owner as select @a + len(@b)",
    )
    .await;
    s.exec("GRANT EXECUTE ON dbo.p TO public").await;
    s.exec(
        "EXEC sp_addextendedproperty @name = N'MS_Description', @value = N'adds', \
         @level0type = N'SCHEMA', @level0name = N'dbo', @level1type = N'PROCEDURE', @level1name = N'p'",
    )
    .await;
    s.exec("CREATE FUNCTION dbo.f (@x int) RETURNS int BEGIN RETURN @x * 2 END")
        .await;
    s.exec("CREATE PROCEDURE dbo.hid WITH ENCRYPTION AS SELECT 1")
        .await;

    let read = |s: &Scratch| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            db.fetch_schema(&name, CancellationToken::new())
                .await
                .expect("the schema")
                .routines
        }
    };
    let find = |all: &[std::sync::Arc<RoutineInfo>], n: &str| {
        all.iter()
            .find(|r| r.name == n)
            .unwrap_or_else(|| panic!("{n}"))
            .as_ref()
            .clone()
    };
    let apply = |stmts: Vec<String>| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            db.run_ddl(&name, &stmts, CancellationToken::new())
                .await
                .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
        }
    };

    let all = read(&s).await;
    let p = find(&all, "p");
    assert_eq!(p.arguments, "@a int = 5, @b nvarchar(10) = N'x, y'");
    assert_eq!(
        p.tsql.options,
        [
            TsqlRoutineOption::Recompile,
            TsqlRoutineOption::ExecuteAs(ExecuteAs::Owner)
        ]
    );
    assert_eq!(p.comment.as_deref(), Some("adds"));
    assert!(find(&all, "hid").tsql.hidden && !find(&all, "hid").is_editable());
    for r in &all {
        assert!(
            diff_routine(r, &RoutineDraft::from_info(r), MS).is_empty(),
            "the round-trip gate: {}",
            r.name
        );
    }

    // An edit in place: the body and a parameter.
    let mut d = RoutineDraft::from_info(&p);
    d.info.arguments = "@a int = 7, @b nvarchar(10) = N'x, y'".into();
    d.info.body = "select @a * 10 + len(@b)".into();
    let stmts = diff_routine(&p, &d, MS).emit();
    assert!(!stmts.iter().any(|x| x.starts_with("DROP")), "{stmts:#?}");
    apply(stmts).await;
    assert_eq!(s.scalar("EXEC dbo.p").await, "74", "the default kept");
    assert_eq!(
        s.scalar(
            "SELECT COUNT(*) FROM sys.database_permissions WHERE major_id = OBJECT_ID('dbo.p')"
        )
        .await,
        "1",
        "the grant kept"
    );
    let p2 = find(&read(&s).await, "p");
    assert_eq!(p2.tsql.options, p.tsql.options, "the options restated");
    assert_eq!(p2.comment.as_deref(), Some("adds"));

    // A rename: dropped and created, the comment set again.
    let mut d = RoutineDraft::from_info(&p2);
    d.info.name = "q".into();
    apply(diff_routine(&p2, &d, MS).emit()).await;
    let all = read(&s).await;
    assert!(!all.iter().any(|r| r.name == "p"));
    assert_eq!(find(&all, "q").comment.as_deref(), Some("adds"));
    assert_eq!(
        s.scalar("EXEC dbo.q @a = 1").await,
        "14",
        "1 * 10 + len(N'x, y')"
    );

    // A scalar made table-valued, which only a drop and a create can do.
    let f = find(&all, "f");
    let mut d = RoutineDraft::from_info(&f);
    d.info.returns = "TABLE".into();
    d.info.body = "RETURN (SELECT @x * 3 AS v)".into();
    apply(diff_routine(&f, &d, MS).emit()).await;
    assert_eq!(s.scalar("SELECT v FROM dbo.f(2)").await, "6");

    // A new one, from the editor's own starting point.
    let mut d = RoutineDraft::blank(RoutineKind::Procedure, "fresh", Some("dbo".into()), MS);
    assert!(d.validate(MS).is_empty());
    d.info.arguments = "@n int".into();
    d.info.body = "BEGIN SELECT @n + 1; END".into();
    apply(schemaic_core::ddl::create_routine(&d, MS).emit()).await;
    assert_eq!(s.scalar("EXEC dbo.fresh @n = 41").await, "42");
    let all = read(&s).await;
    let fresh = find(&all, "fresh");
    assert!(diff_routine(&fresh, &RoutineDraft::from_info(&fresh), MS).is_empty());
}

/// **A module's header comments survive an edit.** SSMS's template block
/// before `CREATE`, and a comment between the header's parts, are stored in
/// `sys.sql_modules`; the rebuild dropped them from every edit, and from a
/// trigger's and a routine's Copy DDL and dump. Each reads back with them,
/// diffs to nothing against its own draft, and keeps them through an edit.
#[tokio::test(flavor = "multi_thread")]
async fn a_modules_header_comments_survive_an_edit() {
    use schemaic_core::ddl::{
        RoutineDraft, TriggerSetDraft, ViewDraft, diff_routine, diff_triggers, diff_view,
    };
    use schemaic_core::schema::TriggerAction;
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_header_comments").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY); CREATE TABLE dbo.audit (id int)")
        .await;
    let template = "-- =============================================\n\
                    -- Author:\t\tJane\n\
                    -- =============================================\n";
    s.exec(&format!(
        "{template}CREATE TRIGGER dbo.tr_audit\n   ON  dbo.t\n   AFTER INSERT /* why: audit */\nAS\n\
         BEGIN\n  SET NOCOUNT ON; INSERT dbo.audit SELECT id FROM inserted;\nEND"
    ))
    .await;
    s.exec("/* header comment */ CREATE VIEW dbo.v_c AS SELECT id FROM dbo.t")
        .await;
    s.exec(&format!(
        "{template}CREATE PROCEDURE dbo.p AS SELECT 1 AS n"
    ))
    .await;
    let stored = |name: &'static str| {
        let s = &s;
        async move {
            s.scalar(&format!(
                "SELECT definition FROM sys.sql_modules WHERE object_id = OBJECT_ID('dbo.{name}')"
            ))
            .await
        }
    };
    let apply = |stmts: Vec<String>| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            db.run_ddl(&name, &stmts, CancellationToken::new())
                .await
                .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
        }
    };

    let t = read_table(&s, "t").await;
    let tr = t.triggers[0].clone();
    assert!(
        tr.tsql.module.header_comments.contains("-- Author:"),
        "{tr:?}"
    );
    assert!(tr.create_sql(MS).contains("/* why: audit */"));
    let mut set = TriggerSetDraft::from_table(&t);
    assert!(diff_triggers(&t.triggers, &set, MS).is_empty());
    set.triggers[0].info.action = TriggerAction::Body(
        "BEGIN\n  SET NOCOUNT ON; INSERT dbo.audit SELECT -id FROM inserted;\nEND".into(),
    );
    apply(diff_triggers(&t.triggers, &set, MS).emit()).await;
    let after = stored("tr_audit").await;
    assert!(
        after.contains("-- Author:") && after.contains("/* why: audit */"),
        "{after}"
    );
    assert!(after.contains("SELECT -id"), "{after}");

    let v = read_table(&s, "v_c").await;
    let mut d = ViewDraft::from_table(&v).unwrap();
    assert!(diff_view(&v, &d, MS).is_empty());
    d.select = "SELECT id FROM dbo.t WHERE id > 0".into();
    apply(diff_view(&v, &d, MS).emit()).await;
    let after = stored("v_c").await;
    assert!(
        after.contains("/* header comment */") && after.contains("id > 0"),
        "{after}"
    );

    let p =
        s.db.fetch_schema(&s.name, CancellationToken::new())
            .await
            .unwrap()
            .routines
            .iter()
            .find(|r| r.name == "p")
            .unwrap()
            .as_ref()
            .clone();
    assert!(
        schemaic_core::schema::ObjectItem::Routine(Arc::new(p.clone()))
            .create_sql(MS)
            .contains("-- Author:")
    );
    let mut d = RoutineDraft::from_info(&p);
    assert!(diff_routine(&p, &d, MS).is_empty());
    d.info.body = "SELECT 2 AS n".into();
    apply(diff_routine(&p, &d, MS).emit()).await;
    let after = stored("p").await;
    assert!(
        after.contains("-- Author:") && after.contains("SELECT 2"),
        "{after}"
    );
}

/// **A comment where a header's parts meet survives the rebuild.** A view's
/// column list typed one column a line, each with a `--` note, is stored as
/// typed; the rebuild closed it with `) AS` on the note's line, inside the
/// comment, so its edit, its Copy DDL and a dump of it all failed with Msg
/// 156. A comment in front of a procedure's parenthesised parameters was
/// dropped by every rebuild. Each is edited, and its script replayed after a
/// real drop, and is still what it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_comment_where_a_modules_header_parts_meet_survives_its_rebuild() {
    use schemaic_core::ddl::{RoutineDraft, ViewDraft, diff_routine, diff_view};
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_header_line_comments").await;
    s.exec(
        "CREATE VIEW dbo.v_lc (\n  id, -- the key\n  name -- display\n) AS SELECT 1 AS a, 2 AS b",
    )
    .await;
    let apply = |stmts: Vec<String>| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            db.run_ddl(&name, &stmts, CancellationToken::new())
                .await
                .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
        }
    };

    let v = read_table(&s, "v_lc").await;
    let mut d = ViewDraft::from_table(&v).unwrap();
    assert!(diff_view(&v, &d, MS).is_empty());
    d.select = "SELECT 1 AS a, 3 AS b".into();
    apply(diff_view(&v, &d, MS).emit()).await;
    assert_eq!(s.scalar("SELECT name FROM dbo.v_lc").await, "3");
    let ddl = read_table(&s, "v_lc").await.create_ddl(MS);
    replay(&s, &format!("DROP VIEW dbo.v_lc;\nGO\n{ddl}\nGO")).await;
    assert_eq!(s.scalar("SELECT name FROM dbo.v_lc").await, "3", "{ddl}");

    // A comment in front of a procedure's parenthesised parameters, which
    // the list is written back without.
    s.exec("CREATE PROCEDURE dbo.p_doc /* doc */ (@a int) AS SELECT @a AS a")
        .await;
    let routine = |name: &'static str| {
        let s = &s;
        async move {
            s.db.fetch_schema(&s.name, CancellationToken::new())
                .await
                .unwrap()
                .routines
                .iter()
                .find(|r| r.name == name)
                .unwrap()
                .as_ref()
                .clone()
        }
    };
    let stored = |name: &'static str| {
        let s = &s;
        async move {
            s.scalar(&format!(
                "SELECT definition FROM sys.sql_modules WHERE object_id = OBJECT_ID('dbo.{name}')"
            ))
            .await
        }
    };
    let p = routine("p_doc").await;
    let mut d = RoutineDraft::from_info(&p);
    assert!(diff_routine(&p, &d, MS).is_empty());
    d.info.body = "SELECT @a + 1 AS a".into();
    apply(diff_routine(&p, &d, MS).emit()).await;
    let after = stored("p_doc").await;
    assert!(
        after.contains("/* doc */") && after.contains("@a + 1"),
        "{after}"
    );
    let ddl =
        schemaic_core::schema::ObjectItem::Routine(Arc::new(routine("p_doc").await)).create_sql(MS);
    replay(&s, &format!("DROP PROCEDURE dbo.p_doc;\nGO\n{ddl}")).await;
    assert!(stored("p_doc").await.contains("/* doc */"), "{ddl}");
    assert_eq!(s.scalar("EXEC dbo.p_doc @a = 1").await, "2");

    // A comment in front of `TABLE`: still an inline function, so taking the
    // comment out is an alter in place, not a drop and a re-create.
    s.exec(
        "CREATE FUNCTION dbo.f_rows (@a int) RETURNS /* rows */ TABLE AS RETURN (SELECT @a AS a)",
    )
    .await;
    let f = routine("f_rows").await;
    assert_eq!(
        f.tsql_shape(),
        schemaic_core::schema::TsqlShape::InlineTable
    );
    let mut d = RoutineDraft::from_info(&f);
    d.info.returns = "TABLE".into();
    let stmts = diff_routine(&f, &d, MS).emit();
    assert!(!stmts.iter().any(|x| x.starts_with("DROP")), "{stmts:#?}");
    apply(stmts).await;
    assert_eq!(s.scalar("SELECT a FROM dbo.f_rows(4)").await, "4");
}

/// **A natively compiled module refuses `RECOMPILE` and `RETURNS NULL ON
/// NULL INPUT`** (Msg 10794), and the routine form offered both: the
/// validator passed, the preview showed the statement and Apply failed at
/// the server. Here the validator refuses each, and the server, given the
/// plan anyway, refuses it too — while the options the validator lets
/// through (`CALLED ON NULL INPUT`) apply.
#[tokio::test(flavor = "multi_thread")]
async fn a_natively_compiled_module_is_refused_what_the_server_refuses() {
    use schemaic_core::ddl::{RoutineDraft, diff_routine};
    use schemaic_core::schema::TsqlRoutineOption as O;
    if !enabled() || azure_cannot("adds a memory-optimised filegroup to the database") {
        return;
    }
    let s = Scratch::create("ddl_native").await;
    memory_optimised_filegroup(&s).await;
    s.exec(&format!(
        "CREATE PROCEDURE dbo.np WITH NATIVE_COMPILATION, SCHEMABINDING AS {ATOMIC} SELECT 1 AS x END"
    ))
    .await;
    s.exec(&format!(
        "CREATE FUNCTION dbo.nf (@a int) RETURNS int WITH NATIVE_COMPILATION, SCHEMABINDING AS \
         {ATOMIC} RETURN @a END"
    ))
    .await;
    let all =
        s.db.fetch_schema(&s.name, CancellationToken::new())
            .await
            .unwrap()
            .routines;
    let find = |n: &str| all.iter().find(|r| r.name == n).unwrap().as_ref().clone();
    for (name, add, refused) in [
        ("np", O::Recompile, true),
        ("nf", O::ReturnsNullOnNullInput, true),
        ("nf", O::CalledOnNullInput, false),
    ] {
        let r = find(name);
        assert!(r.tsql.has_option(&O::NativeCompilation), "{r:?}");
        let mut d = RoutineDraft::from_info(&r);
        d.info.tsql.options.push(add.clone());
        let errs = d.validate(MS);
        assert_eq!(!errs.is_empty(), refused, "{name} {add:?}: {errs:?}");
        // Run outside a transaction: inside one, any DDL on a natively
        // compiled module is Msg 12331, which would hide the answer asked.
        let stmts = diff_routine(&r, &d, MS).emit();
        assert_eq!(stmts.len(), 1, "{stmts:#?}");
        let ran = s.try_exec(&stmts[0]).await;
        assert_eq!(
            ran.is_err(),
            refused,
            "{name} {add:?}: {:?}\n{stmts:#?}",
            ran.err()
        );
        if let Err(e) = ran {
            assert!(e.to_string().contains("10794"), "{e}");
        }
    }
}

/// The `BEGIN ATOMIC` header every natively compiled module's body opens with.
const ATOMIC: &str =
    "BEGIN ATOMIC WITH (TRANSACTION ISOLATION LEVEL = SNAPSHOT, LANGUAGE = N'us_english')";

/// Give the scratch database the memory-optimised filegroup a natively
/// compiled module or a memory-optimised table needs. Its file goes in the
/// instance's default data directory, named for the database, so two legs
/// running at once do not share one.
async fn memory_optimised_filegroup(s: &Scratch) {
    base_db()
        .fetch_query(
            None,
            &format!(
                "ALTER DATABASE [{n}] ADD FILEGROUP mo CONTAINS MEMORY_OPTIMIZED_DATA; \
                 DECLARE @path nvarchar(400) = CONCAT(CAST(SERVERPROPERTY('InstanceDefaultDataPath') \
                 AS nvarchar(300)), N'{n}_mo'); \
                 EXEC (N'ALTER DATABASE [{n}] ADD FILE (NAME = N''mo'', FILENAME = N''' + @path \
                 + N''') TO FILEGROUP mo')",
                n = s.name
            ),
            1,
            CancellationToken::new(),
        )
        .await
        .expect("a memory-optimised filegroup");
}

/// **A plan touching a natively compiled module applies.** SQL Server answers
/// every `CREATE`, `ALTER` and `DROP` of one inside a user transaction with
/// Msg 12331, and `Db::run_ddl` wrapped every plan in one — so no edit, create
/// or drop of such a procedure or trigger could be applied from here at all.
/// Each plan below says it cannot run whole, takes the runner that does not
/// wrap it, and lands.
#[tokio::test(flavor = "multi_thread")]
async fn a_natively_compiled_module_is_edited_created_and_dropped_through_its_plan() {
    use schemaic_core::ddl::{
        ChangeSet, RoutineDraft, TriggerSetDraft, create_routine, diff_routine, diff_triggers,
        drop_routine, drop_trigger,
    };
    use schemaic_core::schema::TriggerAction;
    if !enabled() || azure_cannot("adds a memory-optimised filegroup to the database") {
        return;
    }
    let s = Scratch::create("ddl_native_plan").await;
    memory_optimised_filegroup(&s).await;
    s.exec(
        "CREATE TABLE dbo.m (id int NOT NULL PRIMARY KEY NONCLUSTERED) \
         WITH (MEMORY_OPTIMIZED = ON, DURABILITY = SCHEMA_ONLY); \
         CREATE TABLE dbo.a (id int NOT NULL PRIMARY KEY NONCLUSTERED) \
         WITH (MEMORY_OPTIMIZED = ON, DURABILITY = SCHEMA_ONLY)",
    )
    .await;
    s.exec(&format!(
        "CREATE PROCEDURE dbo.np WITH NATIVE_COMPILATION, SCHEMABINDING AS {ATOMIC} SELECT 1 AS x END"
    ))
    .await;
    s.exec(&format!(
        "CREATE TRIGGER dbo.tm ON dbo.m WITH NATIVE_COMPILATION, SCHEMABINDING AFTER INSERT AS \
         {ATOMIC} INSERT dbo.a (id) SELECT id FROM inserted END"
    ))
    .await;
    let apply = |cs: ChangeSet| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            let stmts = cs.emit();
            assert!(!cs.runs_whole(), "{stmts:#?}");
            db.run_ddl_piecewise(&name, &stmts, CancellationToken::new())
                .await
                .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
        }
    };
    let routine = |name: &'static str| {
        let s = &s;
        async move {
            s.db.fetch_schema(&s.name, CancellationToken::new())
                .await
                .unwrap()
                .routines
                .iter()
                .find(|r| r.name == name)
                .unwrap_or_else(|| panic!("no routine {name}"))
                .as_ref()
                .clone()
        }
    };

    // An edit in place.
    let np = routine("np").await;
    let mut d = RoutineDraft::from_info(&np);
    d.info.body = d.info.body.replace("SELECT 1 AS x", "SELECT 2 AS x");
    apply(diff_routine(&np, &d, MS)).await;
    assert_eq!(s.scalar("EXEC dbo.np").await, "2");

    // A create, from the edited one under another name.
    let mut d = RoutineDraft::from_info(&routine("np").await);
    d.original = None;
    d.info.name = "np2".into();
    apply(create_routine(&d, MS)).await;
    assert_eq!(s.scalar("EXEC dbo.np2").await, "2");

    // A drop.
    apply(drop_routine(&routine("np2").await, MS)).await;
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM sys.procedures WHERE name = 'np2'")
            .await,
        "0"
    );

    // A natively compiled trigger: an edit, then a drop.
    let t = read_table(&s, "m").await;
    assert!(t.triggers[0].tsql.native_compilation, "{t:?}");
    let mut set = TriggerSetDraft::from_table(&t);
    let TriggerAction::Body(body) = &set.triggers[0].info.action else {
        panic!("{t:?}");
    };
    set.triggers[0].info.action =
        TriggerAction::Body(body.replace("SELECT id FROM", "SELECT id + 10 FROM"));
    apply(diff_triggers(&t.triggers, &set, MS)).await;
    s.exec("INSERT dbo.m (id) VALUES (1)").await;
    assert_eq!(s.scalar("SELECT MAX(id) FROM dbo.a").await, "11");
    let t = read_table(&s, "m").await;
    apply(drop_trigger(&t.triggers[0], MS)).await;
    assert_eq!(s.scalar("SELECT COUNT(*) FROM sys.triggers").await, "0");
}

/// **A module created under `ANSI_NULLS OFF` or `QUOTED_IDENTIFIER OFF`
/// keeps it through an edit.** `CREATE` has no clause for either; the module
/// takes the session's, and Schemaic's is an ANSI-defaults one, so an edit
/// re-filed each under ON and changed what it does: the view's `d = NULL`
/// stopped matching, the trigger's `"x"` literal became a column. Each is
/// read with its settings, edited, and still has them — and still does what
/// it did.
#[tokio::test(flavor = "multi_thread")]
async fn a_module_keeps_its_creation_settings_through_an_edit() {
    use schemaic_core::ddl::{
        RoutineDraft, TriggerSetDraft, ViewDraft, diff_routine, diff_triggers, diff_view,
    };
    use schemaic_core::schema::TriggerAction;
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_module_settings").await;
    s.exec(
        "CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY, d int NULL); \
         CREATE TABLE dbo.audit (note varchar(20)); INSERT dbo.t VALUES (1, NULL)",
    )
    .await;
    // `EXEC` compiles its batch under the settings the `SET`s just made.
    s.exec(
        "SET ANSI_NULLS OFF; EXEC ('CREATE VIEW dbo.v_an AS SELECT id FROM dbo.t WHERE d = NULL')",
    )
    .await;
    s.exec(
        "SET QUOTED_IDENTIFIER OFF; EXEC ('CREATE TRIGGER dbo.tr_qi ON dbo.t AFTER UPDATE AS \
         INSERT dbo.audit VALUES (\"first\")')",
    )
    .await;
    s.exec(
        "SET ANSI_NULLS OFF; SET QUOTED_IDENTIFIER OFF; \
         EXEC ('CREATE PROCEDURE dbo.p_an AS SELECT COUNT(*) FROM dbo.t WHERE d = NULL')",
    )
    .await;
    let settings = |name: &'static str| {
        let s = &s;
        async move {
            s.scalar(&format!(
                "SELECT CONCAT(uses_ansi_nulls, uses_quoted_identifier) FROM sys.sql_modules \
                 WHERE object_id = OBJECT_ID('dbo.{name}')"
            ))
            .await
        }
    };
    let apply = |stmts: Vec<String>| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            db.run_ddl(&name, &stmts, CancellationToken::new())
                .await
                .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
        }
    };

    let v = read_table(&s, "v_an").await;
    assert!(v.view_options.as_ref().unwrap().tsql.module.ansi_nulls_off);
    let mut d = ViewDraft::from_table(&v).unwrap();
    assert!(diff_view(&v, &d, MS).is_empty());
    d.select = "SELECT id, d FROM dbo.t WHERE d = NULL".into();
    apply(diff_view(&v, &d, MS).emit()).await;
    assert_eq!(settings("v_an").await, "01");
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM dbo.v_an").await,
        "1",
        "d = NULL still matches"
    );

    let t = read_table(&s, "t").await;
    assert!(t.triggers[0].tsql.module.quoted_identifier_off);
    let mut set = TriggerSetDraft::from_table(&t);
    assert!(diff_triggers(&t.triggers, &set, MS).is_empty());
    set.triggers[0].info.action =
        TriggerAction::Body("INSERT dbo.audit VALUES (\"second\")".into());
    apply(diff_triggers(&t.triggers, &set, MS).emit()).await;
    assert_eq!(settings("tr_qi").await, "10");
    s.exec("UPDATE dbo.t SET d = d").await;
    assert_eq!(s.scalar("SELECT note FROM dbo.audit").await, "second");

    let p =
        s.db.fetch_schema(&s.name, CancellationToken::new())
            .await
            .unwrap()
            .routines
            .iter()
            .find(|r| r.name == "p_an")
            .unwrap()
            .as_ref()
            .clone();
    assert!(p.tsql.module.ansi_nulls_off && p.tsql.module.quoted_identifier_off);
    let mut d = RoutineDraft::from_info(&p);
    assert!(diff_routine(&p, &d, MS).is_empty());
    d.info.body = "SELECT COUNT(*) + 10 FROM dbo.t WHERE d = NULL".into();
    apply(diff_routine(&p, &d, MS).emit()).await;
    assert_eq!(settings("p_an").await, "00");
    assert_eq!(s.scalar("EXEC dbo.p_an").await, "11");

    // And a script of each puts it back as it was.
    let ddl = read_table(&s, "v_an").await.create_ddl(MS);
    replay(&s, &format!("DROP VIEW dbo.v_an;\nGO\n{ddl}\nGO")).await;
    assert_eq!(settings("v_an").await, "01");
}

/// Compare `target` with `source`, apply the whole plan to `target`, and
/// compare again — returning the plan applied and the second comparison.
async fn sync_modules(
    target: &Scratch,
    source: &Scratch,
) -> (
    schemaic_core::compare::SchemaPlan,
    schemaic_core::compare::SchemaComparison,
) {
    use schemaic_core::compare::SchemaComparison;
    let read = |s: &Scratch| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            db.fetch_schema(&name, CancellationToken::new())
                .await
                .expect("the schema")
        }
    };
    let (t, src) = (read(target).await, read(source).await);
    let plan = SchemaComparison::of(&t, &src, MS).plan(|_| true);
    assert!(plan.unsupported().is_empty(), "{:?}", plan.unsupported());
    let stmts = plan.emit();
    target
        .db
        .run_ddl(&target.name, &stmts, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
    let again = SchemaComparison::of(&read(target).await, &read(source).await, MS);
    (plan, again)
}

/// **A comparison into an empty database applies.** A target holding no
/// objects reads no namespaces, so the plan opened with "Create schema dbo" —
/// a change SQL Server's plans refuse — and the whole plan was withheld. `dbo`
/// is in every database; synced, the target holds the source's table and a
/// second comparison finds nothing to do.
#[tokio::test(flavor = "multi_thread")]
async fn a_comparison_into_an_empty_database_does_not_create_dbo() {
    if !enabled() {
        return;
    }
    let target = Scratch::create("cmp_empty_target").await;
    let source = Scratch::create("cmp_empty_source").await;
    source
        .exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY, d int NULL)")
        .await;
    let (plan, again) = sync_modules(&target, &source).await;
    assert!(
        plan.emit().iter().all(|s| !s.contains("CREATE SCHEMA")),
        "{:#?}",
        plan.emit()
    );
    assert_eq!(
        target
            .scalar("SELECT COUNT(*) FROM sys.tables WHERE name = 't'")
            .await,
        "1"
    );
    assert_eq!(again.differences().count(), 0);
}

/// **A comparison creates a missing view whole** — under the settings the
/// source's was created with, and with its indexes. The plan was a bare
/// `CREATE VIEW`, so a view written under `ANSI_NULLS OFF` arrived ON (its
/// `d = NULL` matching nothing) and an indexed one unmaterialised, both seen
/// only on the next comparison. Synced, each does on the target what it does
/// on the source, and a second comparison finds nothing to do.
#[tokio::test(flavor = "multi_thread")]
async fn a_comparison_creates_a_missing_view_with_its_settings_and_indexes() {
    if !enabled() {
        return;
    }
    let target = Scratch::create("cmp_view_target").await;
    let source = Scratch::create("cmp_view_source").await;
    for s in [&target, &source] {
        s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY, d int NULL); INSERT dbo.t VALUES (1, NULL)")
            .await;
    }
    source
        .exec("SET ANSI_NULLS OFF; EXEC ('CREATE VIEW dbo.v_an AS SELECT id FROM dbo.t WHERE d = NULL')")
        .await;
    source
        .exec("CREATE VIEW dbo.v_ix WITH SCHEMABINDING AS SELECT id, d FROM dbo.t")
        .await;
    source
        .exec("CREATE UNIQUE CLUSTERED INDEX cix ON dbo.v_ix (id)")
        .await;
    let (_, again) = sync_modules(&target, &source).await;
    assert_eq!(target.scalar("SELECT COUNT(*) FROM dbo.v_an").await, "1");
    assert_eq!(
        target
            .scalar("SELECT COUNT(*) FROM sys.indexes WHERE object_id = OBJECT_ID('dbo.v_ix') AND name = 'cix'")
            .await,
        "1"
    );
    let left: Vec<String> = again.differences().map(|e| e.key()).collect();
    assert!(left.is_empty(), "{left:?}");
}

/// **An indexed view's edit refuses what its indexes cannot be built again
/// with, and names the statistics it drops.** The indexes come back from a
/// model with no fill factor, compression or description, so a `PAGE`
/// compressed index came back uncompressed while the preview said it was
/// built again: the plan's guard now refuses it, nothing changed. And
/// `ALTER VIEW` takes a view's hand-made statistics, which nothing restates:
/// the sentence says so, and the edit does take them.
#[tokio::test(flavor = "multi_thread")]
async fn an_indexed_views_edit_refuses_what_its_indexes_cannot_carry() {
    use schemaic_core::ddl::{ViewDraft, diff_view};
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_view_ix_guard").await;
    s.exec(
        "CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY, d int NULL); INSERT dbo.t VALUES (1, 2)",
    )
    .await;
    for v in ["v_pg", "v_st"] {
        s.exec(&format!(
            "CREATE VIEW dbo.{v} WITH SCHEMABINDING AS SELECT id, d FROM dbo.t"
        ))
        .await;
    }
    s.exec("CREATE UNIQUE CLUSTERED INDEX cix ON dbo.v_pg (id) WITH (DATA_COMPRESSION = PAGE)")
        .await;
    s.exec("CREATE UNIQUE CLUSTERED INDEX cix ON dbo.v_st (id)")
        .await;
    s.exec("CREATE STATISTICS st_d ON dbo.v_st (d)").await;
    let edit = |v: &schemaic_core::schema::TableInfo| {
        let mut d = ViewDraft::from_table(v).unwrap();
        d.select = "SELECT id, d FROM dbo.t WHERE id > 0".into();
        diff_view(v, &d, MS)
    };

    let v = read_table(&s, "v_pg").await;
    let stmts = edit(&v).emit();
    let err =
        s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
            .await
            .expect_err("a compressed index is refused");
    assert!(err.to_string().contains("cix"), "{err}");
    assert_eq!(
        s.scalar(
            "SELECT p.data_compression_desc FROM sys.partitions p \
             WHERE p.object_id = OBJECT_ID('dbo.v_pg') AND p.index_id = 1"
        )
        .await,
        "PAGE",
        "nothing changed"
    );

    let v = read_table(&s, "v_st").await;
    let plan = edit(&v);
    assert!(
        plan.destructive()
            .join(" ")
            .contains("Statistics created by hand"),
        "{:?}",
        plan.destructive()
    );
    let stmts = plan.emit();
    s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
    assert_eq!(
        s.scalar(
            "SELECT STRING_AGG(name, ',') FROM sys.stats WHERE object_id = OBJECT_ID('dbo.v_st')"
        )
        .await,
        "cix",
        "the index built again, the hand-made statistics gone as the sentence says"
    );
}

/// **A comparison does not count a signature as a difference, and says when
/// its plan strips one.** A certificate is one database's, so the signed
/// procedure and trigger on the target and their unsigned twins on the
/// source were reported Differing, planned as alters that strip the target's
/// signature and cannot sign, and reported again after Apply. Here they
/// compare the same; once a body differs, the plan says it strips both
/// signatures, and applying it does.
#[tokio::test(flavor = "multi_thread")]
async fn a_comparison_leaves_a_signature_out_of_the_difference() {
    use schemaic_core::compare::SchemaComparison;
    if !enabled() || azure_cannot("signs with a certificate the shared database may not allow") {
        return;
    }
    let target = Scratch::create("cmp_signed_target").await;
    let source = Scratch::create("cmp_signed_source").await;
    for s in [&target, &source] {
        s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY)")
            .await;
        s.exec("CREATE PROCEDURE dbo.p AS SELECT 1 AS s").await;
        s.exec("CREATE TRIGGER dbo.tr ON dbo.t AFTER INSERT AS SET NOCOUNT ON")
            .await;
    }
    target
        .exec(
            "CREATE CERTIFICATE zz_cmp_cert ENCRYPTION BY PASSWORD = 'Pa55word!!zz' \
             WITH SUBJECT = 'schemaic test'",
        )
        .await;
    for name in ["p", "tr"] {
        target
            .exec(&format!(
                "ADD SIGNATURE TO dbo.{name} BY CERTIFICATE zz_cmp_cert WITH PASSWORD = 'Pa55word!!zz'"
            ))
            .await;
    }
    let read = |s: &Scratch| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            db.fetch_schema(&name, CancellationToken::new())
                .await
                .unwrap()
        }
    };
    let c = SchemaComparison::of(&read(&target).await, &read(&source).await, MS);
    let differing: Vec<String> = c.differences().map(|e| e.key()).collect();
    assert!(differing.is_empty(), "{differing:?}");

    source
        .exec("CREATE OR ALTER PROCEDURE dbo.p AS SELECT 2 AS s")
        .await;
    source
        .exec("CREATE OR ALTER TRIGGER dbo.tr ON dbo.t AFTER INSERT AS SET NOCOUNT OFF")
        .await;
    let (plan, again) = sync_modules(&target, &source).await;
    let risks = plan.destructive().join(" ");
    assert!(
        risks.contains("Procedure p is signed") && risks.contains("Trigger tr is signed"),
        "{risks}"
    );
    assert_eq!(
        target
            .scalar("SELECT COUNT(*) FROM sys.crypt_properties WHERE class = 1")
            .await,
        "0",
        "the plan strips both, as it says"
    );
    let left: Vec<String> = again.differences().map(|e| e.key()).collect();
    assert!(left.is_empty(), "{left:?}");
}

/// **A comparison discloses a module the source would not show, rather than
/// planning it.** An encrypted view was planned as `CREATE VIEW v AS ;` and
/// an encrypted procedure as a comment that "succeeded" creating nothing.
/// Whether the target lacks them or holds readable ones, each is now left
/// out of the plan and named in its omitted list.
#[tokio::test(flavor = "multi_thread")]
async fn a_comparison_discloses_the_modules_the_source_would_not_show() {
    use schemaic_core::compare::SchemaComparison;
    if !enabled() {
        return;
    }
    let target = Scratch::create("cmp_enc_target").await;
    let source = Scratch::create("cmp_enc_source").await;
    for s in [&target, &source] {
        s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY)")
            .await;
    }
    source
        .exec("CREATE VIEW dbo.v_enc WITH ENCRYPTION AS SELECT id FROM dbo.t")
        .await;
    source
        .exec("CREATE PROCEDURE dbo.p_enc WITH ENCRYPTION AS SELECT 1 AS n")
        .await;
    let read = |s: &Scratch| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            db.fetch_schema(&name, CancellationToken::new())
                .await
                .unwrap()
        }
    };
    for round in ["absent", "readable"] {
        let c = SchemaComparison::of(&read(&target).await, &read(&source).await, MS);
        let plan = c.plan(|_| true);
        let sql = plan.emit();
        assert!(
            !sql.iter()
                .any(|s| s.contains("v_enc") || s.contains("p_enc")),
            "{round}: {sql:#?}"
        );
        for name in ["v_enc", "p_enc"] {
            assert!(
                plan.omitted.iter().any(|o| o.contains(name)),
                "{round}: {:?}",
                plan.omitted
            );
        }
        if round == "absent" {
            target
                .exec("CREATE VIEW dbo.v_enc AS SELECT id FROM dbo.t")
                .await;
            target
                .exec("CREATE PROCEDURE dbo.p_enc AS SELECT 2 AS n")
                .await;
        }
    }
}

/// **A comparison creates a numbered group whole, and does not count its
/// members as a difference it cannot plan.** A group only the source held
/// was created without `grp;2`; a target missing that member was Differing
/// over a no-op alter of the head, again after every Apply. Synced, the
/// target has the member, and a target whose group lacks one is not
/// offered a plan for it.
#[tokio::test(flavor = "multi_thread")]
async fn a_comparison_creates_a_numbered_group_whole() {
    use schemaic_core::compare::SchemaComparison;
    if !enabled() || azure_cannot("Azure SQL Database has no numbered procedures") {
        return;
    }
    let target = Scratch::create("cmp_grp_target").await;
    let source = Scratch::create("cmp_grp_source").await;
    // Something besides the group on each side, so the second round's target
    // is not empty.
    for s in [&target, &source] {
        s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY)")
            .await;
    }
    source
        .exec("CREATE PROCEDURE dbo.grp AS SELECT 1 AS n")
        .await;
    source
        .exec("CREATE PROCEDURE dbo.grp;2 AS SELECT 2 AS n")
        .await;
    let (_, again) = sync_modules(&target, &source).await;
    assert_eq!(target.scalar("EXEC dbo.grp;2").await, "2");
    let left: Vec<String> = again.differences().map(|e| e.key()).collect();
    assert!(left.is_empty(), "{left:?}");

    source
        .exec("CREATE PROCEDURE dbo.grp;3 AS SELECT 3 AS n")
        .await;
    let read = |s: &Scratch| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            db.fetch_schema(&name, CancellationToken::new())
                .await
                .unwrap()
        }
    };
    let c = SchemaComparison::of(&read(&target).await, &read(&source).await, MS);
    let e = c.entries.iter().find(|e| e.name == "grp").expect("grp");
    assert!(!e.status.is_difference() && e.uncertain, "{:?}", e.status);
    assert!(e.right_ddl.contains("grp;3"), "{}", e.right_ddl);
}

/// **A group whose member the source encrypts is disclosed, not created
/// without it.** The member arrived as a comment in the plan, so the group
/// was created head-only and the Apply reported a success. Now the plan
/// leaves the group out and names it, and the target is left without one.
#[tokio::test(flavor = "multi_thread")]
async fn a_comparison_discloses_a_group_whose_member_the_source_encrypts() {
    use schemaic_core::compare::SchemaComparison;
    if !enabled() || azure_cannot("Azure SQL Database has no numbered procedures") {
        return;
    }
    let target = Scratch::create("cmp_grp_enc_target").await;
    let source = Scratch::create("cmp_grp_enc_source").await;
    source
        .exec("CREATE PROCEDURE dbo.ge AS SELECT 1 AS n")
        .await;
    source
        .exec("CREATE PROCEDURE dbo.ge;2 WITH ENCRYPTION AS SELECT 2 AS n")
        .await;
    let read = |s: &Scratch| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            db.fetch_schema(&name, CancellationToken::new())
                .await
                .unwrap()
        }
    };
    let src = read(&source).await;
    let ge = src.routines.iter().find(|r| r.name == "ge").expect("ge");
    assert_eq!(ge.tsql.numbered.len(), 1, "{:?}", ge.tsql);
    let plan = SchemaComparison::of(&read(&target).await, &src, MS).plan(|_| true);
    assert!(plan.emit().is_empty(), "{:#?}", plan.emit());
    assert!(
        plan.omitted.iter().any(|o| o.contains("ge")),
        "{:?}",
        plan.omitted
    );
}

/// **A comparison's view alter names the target's indexes it drops.** The
/// risk read the source's view's indexes, so a target's `cix` — which
/// `ALTER VIEW` drops — went with an empty risk list. Synced, the plan names
/// it, the target's view matches the source's (unindexed), and a second
/// comparison finds nothing to do.
#[tokio::test(flavor = "multi_thread")]
async fn a_comparisons_view_alter_names_the_targets_indexes() {
    if !enabled() {
        return;
    }
    let target = Scratch::create("cmp_ix_target").await;
    let source = Scratch::create("cmp_ix_source").await;
    for s in [&target, &source] {
        s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY, d int NULL)")
            .await;
    }
    target
        .exec("CREATE VIEW dbo.vi WITH SCHEMABINDING AS SELECT id, d FROM dbo.t")
        .await;
    target
        .exec("CREATE UNIQUE CLUSTERED INDEX cix ON dbo.vi (id)")
        .await;
    source
        .exec("CREATE VIEW dbo.vi WITH SCHEMABINDING AS SELECT id, d FROM dbo.t WHERE id > 0")
        .await;
    let (plan, again) = sync_modules(&target, &source).await;
    let risks = plan.destructive().join(" ");
    assert!(risks.contains("cix"), "{risks}");
    assert_eq!(
        target
            .scalar("SELECT COUNT(*) FROM sys.indexes WHERE object_id = OBJECT_ID('dbo.vi') AND index_id > 0")
            .await,
        "0"
    );
    let left: Vec<String> = again.differences().map(|e| e.key()).collect();
    assert!(left.is_empty(), "{left:?}");
}

/// **A module's script restores it at the defaults through a session that
/// has them off** — `sqlcmd`'s, whose `QUOTED_IDENTIFIER` is OFF unless given
/// `-I`. The scripts stated a setting only when the module had it OFF, so
/// through such a session every module at the defaults came back OFF: a
/// view's `"id"` became the string `'id'`, and an indexed view's index was
/// refused (Msg 1935). Replayed after a session that switched both off, each
/// script puts its module back `11`, doing what it did.
#[tokio::test(flavor = "multi_thread")]
async fn a_modules_script_restores_its_settings_through_a_session_that_has_them_off() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_script_settings").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY); INSERT dbo.t VALUES (7)")
        .await;
    s.exec("CREATE VIEW dbo.v_on AS SELECT \"id\" AS a FROM dbo.t")
        .await;
    s.exec("CREATE VIEW dbo.v_ix WITH SCHEMABINDING AS SELECT id FROM dbo.t")
        .await;
    s.exec("CREATE UNIQUE CLUSTERED INDEX cix ON dbo.v_ix (id)")
        .await;
    s.exec("CREATE PROCEDURE dbo.p_on AS SELECT \"id\" AS a FROM dbo.t")
        .await;
    let settings = |name: &'static str| {
        let s = &s;
        async move {
            s.scalar(&format!(
                "SELECT CONCAT(uses_ansi_nulls, uses_quoted_identifier) FROM sys.sql_modules \
                 WHERE object_id = OBJECT_ID('dbo.{name}')"
            ))
            .await
        }
    };
    let off = "SET QUOTED_IDENTIFIER OFF;\nGO\nSET ANSI_NULLS OFF;\nGO\n";
    for name in ["v_on", "v_ix"] {
        let ddl = read_table(&s, name).await.create_ddl(MS);
        replay(&s, &format!("{off}DROP VIEW dbo.{name};\nGO\n{ddl}\nGO")).await;
        assert_eq!(settings(name).await, "11", "{ddl}");
    }
    assert_eq!(s.scalar("SELECT a FROM dbo.v_on").await, "7");
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM sys.indexes WHERE object_id = OBJECT_ID('dbo.v_ix') AND name = 'cix'")
            .await,
        "1",
        "the index was built again"
    );
    let p =
        s.db.fetch_schema(&s.name, CancellationToken::new())
            .await
            .unwrap()
            .routines
            .iter()
            .find(|r| r.name == "p_on")
            .unwrap()
            .as_ref()
            .clone();
    let ddl = schemaic_core::schema::ObjectItem::Routine(Arc::new(p)).create_sql(MS);
    replay(&s, &format!("{off}DROP PROCEDURE dbo.p_on;\nGO\n{ddl}")).await;
    assert_eq!(settings("p_on").await, "11", "{ddl}");
    assert_eq!(s.scalar("EXEC dbo.p_on").await, "7");
}

/// **A trigger renamed by case alone is renamed, and only once.** Taken as an
/// in-place alter, a case-sensitive database got a second trigger `TR` beside
/// `tr` and every insert fired both; a case-insensitive one kept `tr`. Here,
/// under each collation, the plan leaves exactly one trigger, named `TR`,
/// writing one row per insert.
#[tokio::test(flavor = "multi_thread")]
async fn a_trigger_renamed_by_case_is_renamed_once() {
    use schemaic_core::ddl::{TriggerSetDraft, diff_triggers};
    if !enabled() || azure_cannot("changes the database's collation") {
        return;
    }
    for collation in ["Latin1_General_CS_AS", "Latin1_General_CI_AS"] {
        let s = Scratch::create("ddl_trigger_case").await;
        base_db()
            .fetch_query(
                None,
                &format!("ALTER DATABASE [{}] COLLATE {collation}", s.name),
                1,
                CancellationToken::new(),
            )
            .await
            .unwrap_or_else(|e| panic!("{collation}: {e}"));
        s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY); CREATE TABLE dbo.audit (id int)")
            .await;
        s.exec("CREATE TRIGGER dbo.tr ON dbo.t AFTER INSERT AS INSERT dbo.audit SELECT id FROM inserted")
            .await;
        let t = read_table(&s, "t").await;
        let mut set = TriggerSetDraft::from_table(&t);
        set.triggers[0].info.name = "TR".into();
        let stmts = diff_triggers(&t.triggers, &set, MS).emit();
        s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{collation}: {e}\n{stmts:#?}"));
        assert_eq!(
            s.scalar("SELECT STRING_AGG(name, ',') FROM sys.triggers")
                .await,
            "TR",
            "{collation}"
        );
        s.exec("INSERT dbo.t VALUES (1)").await;
        assert_eq!(
            s.scalar("SELECT COUNT(*) FROM dbo.audit").await,
            "1",
            "{collation}"
        );
    }
}

/// **A signed module's edit says it strips the signature.** Any `CREATE OR
/// ALTER` drops `ADD SIGNATURE`, which cannot be restated without the
/// certificate's key, and the preview said only "Redefines p". Both the
/// procedure and the trigger are read as signed, their plans name the loss,
/// and — the reason the sentence is there — the applied edit really does
/// leave them unsigned.
#[tokio::test(flavor = "multi_thread")]
async fn a_signed_modules_edit_says_it_strips_the_signature() {
    use schemaic_core::ddl::{RoutineDraft, TriggerSetDraft, diff_routine, diff_triggers};
    use schemaic_core::schema::TriggerAction;
    if !enabled() || azure_cannot("signs with a certificate the shared database may not allow") {
        return;
    }
    let s = Scratch::create("ddl_signed").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY)")
        .await;
    s.exec("CREATE PROCEDURE dbo.p_signed AS SELECT 1 AS s")
        .await;
    s.exec("CREATE TRIGGER dbo.tr_signed ON dbo.t AFTER INSERT AS SET NOCOUNT ON")
        .await;
    s.exec(
        "CREATE CERTIFICATE zz_mod_cert ENCRYPTION BY PASSWORD = 'Pa55word!!zz' \
         WITH SUBJECT = 'schemaic test'",
    )
    .await;
    for name in ["p_signed", "tr_signed"] {
        s.exec(&format!(
            "ADD SIGNATURE TO dbo.{name} BY CERTIFICATE zz_mod_cert WITH PASSWORD = 'Pa55word!!zz'"
        ))
        .await;
    }
    let signatures = "SELECT COUNT(*) FROM sys.crypt_properties WHERE class = 1";
    assert_eq!(s.scalar(signatures).await, "2");

    let p =
        s.db.fetch_schema(&s.name, CancellationToken::new())
            .await
            .unwrap()
            .routines
            .iter()
            .find(|r| r.name == "p_signed")
            .unwrap()
            .as_ref()
            .clone();
    assert!(p.tsql.module.signed);
    let mut d = RoutineDraft::from_info(&p);
    d.info.body = "SELECT 2 AS s".into();
    let plan = diff_routine(&p, &d, MS);
    assert!(
        plan.destructive().join(" ").contains("signature"),
        "{:?}",
        plan.destructive()
    );
    s.db.run_ddl(&s.name, &plan.emit(), CancellationToken::new())
        .await
        .unwrap();

    let t = read_table(&s, "t").await;
    assert!(t.triggers[0].tsql.module.signed);
    let mut set = TriggerSetDraft::from_table(&t);
    set.triggers[0].info.action = TriggerAction::Body("SET NOCOUNT OFF".into());
    let plan = diff_triggers(&t.triggers, &set, MS);
    assert!(
        plan.destructive().join(" ").contains("signature"),
        "{:?}",
        plan.destructive()
    );
    s.db.run_ddl(&s.name, &plan.emit(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        s.scalar(signatures).await,
        "0",
        "what the sentence warned of"
    );
}

/// **A parameter's own `AS` is not the header's.** `@a AS int` and a
/// parameter named `@as` are T-SQL the server takes; read with the list cut
/// at the first `AS`, the first lost its type to the body and the second
/// rebuilt into Msg 137. Each reads whole, and an edit to its body applies.
#[tokio::test(flavor = "multi_thread")]
async fn a_parameter_declared_with_as_reads_and_rebuilds_whole() {
    use schemaic_core::ddl::{RoutineDraft, diff_routine};
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_param_as").await;
    s.exec("CREATE PROCEDURE dbo.p_as @a AS int = 5 WITH EXECUTE AS OWNER AS SELECT @a AS v")
        .await;
    s.exec("CREATE PROCEDURE dbo.p_kw @as int = 1 AS SELECT @as AS v")
        .await;
    let all =
        s.db.fetch_schema(&s.name, CancellationToken::new())
            .await
            .expect("the schema")
            .routines;
    for (name, args, call, want) in [
        ("p_as", "@a AS int = 5", "EXEC dbo.p_as", "50"),
        ("p_kw", "@as int = 1", "EXEC dbo.p_kw", "10"),
    ] {
        let r = all.iter().find(|r| r.name == name).expect(name);
        assert_eq!(r.arguments, args, "{name}");
        assert!(r.is_editable(), "{name}");
        let mut d = RoutineDraft::from_info(r);
        d.info.body = d.info.body.replace("AS v", "* 10 AS v");
        let stmts = diff_routine(r, &d, MS).emit();
        s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
        assert_eq!(s.scalar(call).await, want, "{name}");
    }
    assert_eq!(
        s.scalar(
            "SELECT COUNT(*) FROM sys.sql_modules WHERE object_id = OBJECT_ID('dbo.p_as') \
             AND execute_as_principal_id = -2"
        )
        .await,
        "1",
        "EXECUTE AS OWNER kept"
    );
}

/// **A function whose parameter list ends in a `--` comment rebuilds into a
/// statement the server takes.** Written back on one line, the comment
/// swallowed the list's `)` (Msg 102) on every edit, Copy DDL and dump.
#[tokio::test(flavor = "multi_thread")]
async fn a_parameter_list_ending_in_a_comment_rebuilds_and_replays() {
    use schemaic_core::ddl::{RoutineDraft, diff_routine};
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_param_comment").await;
    s.exec(
        "CREATE FUNCTION dbo.f (\n  @a int -- the input\n)\nRETURNS int\nAS\nBEGIN RETURN @a END",
    )
    .await;
    let f = || {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            db.fetch_schema(&name, CancellationToken::new())
                .await
                .expect("the schema")
                .routines
                .iter()
                .find(|r| r.name == "f")
                .expect("f")
                .as_ref()
                .clone()
        }
    };
    let cur = f().await;
    assert!(cur.is_editable());
    let mut d = RoutineDraft::from_info(&cur);
    d.info.body = "BEGIN RETURN @a * 2 END".into();
    let stmts = diff_routine(&cur, &d, MS).emit();
    s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
    assert_eq!(s.scalar("SELECT dbo.f(21)").await, "42");

    let ddl = schemaic_core::schema::ObjectItem::Routine(Arc::new(f().await)).create_sql(MS);
    s.exec("DROP FUNCTION dbo.f").await;
    replay(&s, &ddl).await;
    assert_eq!(s.scalar("SELECT dbo.f(4)").await, "8");
}

/// **A numbered procedure group survives every plan the editor can build for
/// its head.** `grp` is listed once and its text holds no `;`, so only
/// `sys.numbered_procedures` shows `grp;2`: it is read, an edit in place keeps
/// it, a rename — a drop and a create, which would take the group — is
/// refused before it runs, and Copy DDL of the head scripts the member.
#[tokio::test(flavor = "multi_thread")]
async fn a_numbered_procedure_group_survives_every_plan_for_its_head() {
    use schemaic_core::ddl::{RoutineDraft, diff_routine};
    if !enabled() || azure_cannot("Azure SQL Database has no numbered procedures") {
        return;
    }
    let s = Scratch::create("ddl_numbered").await;
    s.exec("CREATE PROCEDURE dbo.grp AS SELECT 1 AS n").await;
    s.exec("CREATE PROCEDURE dbo.grp;2 AS SELECT 2 AS n").await;
    let head = || {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            db.fetch_schema(&name, CancellationToken::new())
                .await
                .expect("the schema")
                .routines
                .iter()
                .find(|r| r.name == "grp")
                .expect("grp")
                .as_ref()
                .clone()
        }
    };
    let members = "SELECT COUNT(*) FROM sys.numbered_procedures";
    let grp = head().await;
    assert_eq!(grp.tsql.numbered.len(), 1, "{:?}", grp.tsql);
    assert_eq!(grp.tsql.numbered[0].0, 2);
    assert!(grp.tsql.numbered[0].1.contains("grp;2"));
    assert!(grp.is_editable());
    assert!(diff_routine(&grp, &RoutineDraft::from_info(&grp), MS).is_empty());

    let mut d = RoutineDraft::from_info(&grp);
    d.info.body = "SELECT 11 AS n".into();
    let plan = diff_routine(&grp, &d, MS);
    assert!(plan.unsupported().is_empty(), "{:?}", plan.unsupported());
    let stmts = plan.emit();
    s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
    assert_eq!(s.scalar("EXEC dbo.grp").await, "11");
    assert_eq!(s.scalar(members).await, "1", "the edit in place kept grp;2");

    let grp = head().await;
    let mut d = RoutineDraft::from_info(&grp);
    d.info.name = "grp_renamed".into();
    assert!(!d.validate(MS).is_empty());
    let plan = diff_routine(&grp, &d, MS);
    assert!(
        plan.unsupported().iter().any(|w| w.contains("grp;2")),
        "{:?}",
        plan.unsupported()
    );

    let ddl = schemaic_core::schema::ObjectItem::Routine(Arc::new(grp)).create_sql(MS);
    assert!(ddl.contains("grp;2 AS SELECT 2 AS n"), "{ddl}");
    // And the script is one the server takes back: dropped, then replayed.
    s.exec("DROP PROCEDURE dbo.grp").await;
    assert_eq!(s.scalar(members).await, "0", "the drop took the group");
    replay(&s, &ddl).await;
    assert_eq!(s.scalar(members).await, "1", "the script put grp;2 back");
    assert_eq!(s.scalar("EXEC dbo.grp;2").await, "2");
}

/// **A numbered group's script creates its members under the group's
/// settings.** A group created under `QUOTED_IDENTIFIER OFF` has `"member"`
/// strings in its members; scripted after the wrapper had put the setting
/// back `ON`, the member was created reading a column, and the restore
/// stopped at Msg 207. Dropped and replayed, each still returns its string.
#[tokio::test(flavor = "multi_thread")]
async fn a_numbered_groups_script_keeps_its_settings_for_its_members() {
    if !enabled() || azure_cannot("Azure SQL Database has no numbered procedures") {
        return;
    }
    let s = Scratch::create("ddl_numbered_qi").await;
    s.exec(
        "SET QUOTED_IDENTIFIER OFF; EXEC ('CREATE PROCEDURE dbo.qgrp AS SELECT \"head\" AS n'); \
         EXEC ('CREATE PROCEDURE dbo.qgrp;2 AS SELECT \"member\" AS n')",
    )
    .await;
    let grp =
        s.db.fetch_schema(&s.name, CancellationToken::new())
            .await
            .unwrap()
            .routines
            .iter()
            .find(|r| r.name == "qgrp")
            .unwrap()
            .as_ref()
            .clone();
    assert!(grp.tsql.module.quoted_identifier_off, "{:?}", grp.tsql);
    let ddl = schemaic_core::schema::ObjectItem::Routine(Arc::new(grp)).create_sql(MS);
    s.exec("DROP PROCEDURE dbo.qgrp").await;
    replay(&s, &ddl).await;
    assert_eq!(s.scalar("EXEC dbo.qgrp").await, "head", "{ddl}");
    assert_eq!(s.scalar("EXEC dbo.qgrp;2").await, "member", "{ddl}");
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

/// **The editor's trigger controls reach the server.** Two triggers swap
/// `First` and `Last` on `INSERT` in one plan — which needs every rank set
/// after every alter, since an event holds one of each (Msg 15130) — one
/// takes `EXECUTE AS` a user, and an event switched off takes its rank with
/// it. Each state reads back as written, and diffs to nothing.
#[tokio::test(flavor = "multi_thread")]
async fn the_trigger_controls_rank_and_execute_as_on_the_server() {
    use FiringRank::{First, Last};
    use TriggerEvent::{Insert, Update};
    use schemaic_core::ddl::{TriggerHost, TriggerSetDraft, diff_triggers};
    use schemaic_core::schema::{ExecuteAs, FiringRank, TriggerEvent};
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_trigger_rank").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY)")
        .await;
    s.exec("CREATE USER app_runner WITHOUT LOGIN").await;
    s.exec("CREATE TRIGGER dbo.a ON dbo.t AFTER INSERT, UPDATE AS SET NOCOUNT ON")
        .await;
    s.exec("CREATE TRIGGER dbo.b ON dbo.t AFTER INSERT AS SET NOCOUNT ON")
        .await;
    s.exec(
        "EXEC sp_settriggerorder @triggername = N'dbo.a', @order = N'First', @stmttype = N'INSERT'; \
         EXEC sp_settriggerorder @triggername = N'dbo.a', @order = N'Last', @stmttype = N'UPDATE'; \
         EXEC sp_settriggerorder @triggername = N'dbo.b', @order = N'Last', @stmttype = N'INSERT'",
    )
    .await;
    let apply = |d: TriggerSetDraft, current: Vec<schemaic_core::schema::TriggerInfo>| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            let errs = d.validate(&current, MS, TriggerHost::Table);
            assert!(errs.is_empty(), "{errs:?}");
            let stmts = diff_triggers(&current, &d, MS).emit();
            db.run_ddl(&name, &stmts, CancellationToken::new())
                .await
                .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
        }
    };
    let rank_of = |t: &schemaic_core::schema::TableInfo, n: &str| {
        t.triggers
            .iter()
            .find(|x| x.name == n)
            .unwrap_or_else(|| panic!("{n}"))
            .tsql
            .clone()
    };

    let t = read_table(&s, "t").await;
    assert_eq!(rank_of(&t, "a").rank, [(Insert, First), (Update, Last)]);
    assert_eq!(rank_of(&t, "b").rank, [(Insert, Last)]);

    // Swap First and Last on INSERT, and run `a` as a user.
    let mut d = TriggerSetDraft::from_table(&t);
    for x in &mut d.triggers {
        match x.info.name.as_str() {
            "a" => {
                x.info.tsql.set_rank(Insert, Some(Last));
                x.info.tsql.execute_as = ExecuteAs::parse_field("app_runner");
            }
            _ => x.info.tsql.set_rank(Insert, Some(First)),
        }
    }
    apply(d, t.triggers.clone()).await;
    let t = read_table(&s, "t").await;
    assert_eq!(rank_of(&t, "a").rank, [(Insert, Last), (Update, Last)]);
    assert_eq!(rank_of(&t, "b").rank, [(Insert, First)]);
    assert_eq!(
        rank_of(&t, "a").execute_as,
        Some(ExecuteAs::User("app_runner".into()))
    );
    assert!(
        diff_triggers(&t.triggers, &TriggerSetDraft::from_table(&t), MS)
            .changes
            .is_empty(),
        "the round-trip gate"
    );

    // `a` stops firing on UPDATE, its rank there going with it; `b` drops
    // its rank; `a` runs as its caller again.
    let mut d = TriggerSetDraft::from_table(&t);
    for x in &mut d.triggers {
        match x.info.name.as_str() {
            "a" => {
                x.info.set_event(Update, false);
                x.info.tsql.execute_as = None;
            }
            _ => x.info.tsql.set_rank(Insert, None),
        }
    }
    apply(d, t.triggers.clone()).await;
    let t = read_table(&s, "t").await;
    let a = t.triggers.iter().find(|x| x.name == "a").unwrap();
    assert_eq!(a.events, [Insert]);
    assert_eq!(a.tsql.rank, [(Insert, Last)]);
    assert_eq!(a.tsql.execute_as, None);
    assert!(rank_of(&t, "b").rank.is_empty());
}

/// **The editor's routine controls reach the server**: a scalar function's
/// null-input clause, `INLINE` and `EXECUTE AS`, and a procedure's `EXECUTE
/// AS` a user — each written through the slot the form writes, read back as
/// set, and diffing to nothing. The null-input toggle turned back off
/// restores the `CALLED ON NULL INPUT` the text had stated.
#[tokio::test(flavor = "multi_thread")]
async fn the_routine_controls_reach_the_server() {
    use schemaic_core::ddl::{RoutineDraft, diff_routine};
    use schemaic_core::schema::{ExecuteAs, RoutineInfo, TsqlRoutineOption as O};
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_routine_opts").await;
    s.exec("CREATE USER app_runner WITHOUT LOGIN").await;
    s.exec(
        "CREATE FUNCTION dbo.f (@x int) RETURNS int WITH CALLED ON NULL INPUT \
         AS BEGIN RETURN ISNULL(@x, -1) END",
    )
    .await;
    s.exec("CREATE PROCEDURE dbo.p AS SELECT USER_NAME()").await;
    let read = |n: &'static str| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            db.fetch_schema(&name, CancellationToken::new())
                .await
                .expect("the schema")
                .routines
                .iter()
                .find(|r| r.name == n)
                .unwrap_or_else(|| panic!("{n}"))
                .as_ref()
                .clone()
        }
    };
    let apply = |cur: RoutineInfo, d: RoutineDraft| {
        let db = s.db.clone();
        let name = s.name.clone();
        async move {
            let errs = d.validate(MS);
            assert!(errs.is_empty(), "{errs:?}");
            let stmts = diff_routine(&cur, &d, MS).emit();
            assert!(!stmts.is_empty(), "an edit");
            db.run_ddl(&name, &stmts, CancellationToken::new())
                .await
                .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
        }
    };

    let f = read("f").await;
    assert_eq!(f.tsql.options, [O::CalledOnNullInput]);
    let original = f.tsql.clone();
    let mut d = RoutineDraft::from_info(&f);
    d.info.tsql.set_null_input(true, &original);
    d.info.tsql.set_inline(Some(false));
    d.info.tsql.set_execute_as(Some(ExecuteAs::Owner));
    apply(f.clone(), d).await;
    let f2 = read("f").await;
    assert!(f2.tsql.returns_null_on_null_input());
    assert!(!f2.tsql.has_option(&O::CalledOnNullInput));
    assert_eq!(f2.tsql.inline(), Some(false));
    assert_eq!(f2.tsql.execute_as(), Some(&ExecuteAs::Owner));
    assert_eq!(
        s.scalar("SELECT ISNULL(CAST(dbo.f(NULL) AS varchar(5)), 'null')")
            .await,
        "null",
        "the body no longer runs on NULL"
    );
    assert!(diff_routine(&f2, &RoutineDraft::from_info(&f2), MS).is_empty());

    // Back off: the stated `CALLED ON NULL INPUT` returns; inlining is the
    // server's choice again; the caller's rights.
    let mut d = RoutineDraft::from_info(&f2);
    d.info.tsql.set_null_input(false, &original);
    d.info.tsql.set_inline(None);
    d.info.tsql.set_execute_as(None);
    apply(f2, d).await;
    let f3 = read("f").await;
    assert_eq!(f3.tsql, original, "as it was stored");
    assert_eq!(s.scalar("SELECT dbo.f(NULL)").await, "-1");

    // A procedure run as a user sees that user.
    let p = read("p").await;
    let mut d = RoutineDraft::from_info(&p);
    d.info
        .tsql
        .set_execute_as(ExecuteAs::parse_field("app_runner"));
    apply(p, d).await;
    let p2 = read("p").await;
    assert_eq!(
        p2.tsql.execute_as(),
        Some(&ExecuteAs::User("app_runner".into()))
    );
    assert_eq!(s.scalar("EXEC dbo.p").await, "app_runner");
    assert!(diff_routine(&p2, &RoutineDraft::from_info(&p2), MS).is_empty());
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

/// **Text a `varchar` column's code page cannot hold is refused, not stored
/// as `?`** — through an import and through the grid — while text it can
/// hold goes in, and so does the same text into an `nvarchar`. The code page
/// 437 column is one `encoding_rs` cannot read, so the server is asked.
#[tokio::test(flavor = "multi_thread")]
async fn text_a_varchar_cannot_hold_is_refused_not_stored_as_question_marks() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("import_cp").await;
    s.exec(
        "CREATE TABLE dbo.imp (id int NOT NULL PRIMARY KEY, \
         name varchar(40) COLLATE SQL_Latin1_General_CP1_CI_AS NULL, \
         dos varchar(40) COLLATE SQL_Latin1_General_CP437_CI_AS NULL, \
         wide nvarchar(40) NULL); \
         INSERT dbo.imp (id, name) VALUES (100, 'seed')",
    )
    .await;
    let mut rows = (1..=3).map(|i| {
        Ok(vec![
            Value::Int(i),
            Value::Str(if i == 3 {
                "Ωμέγα".into()
            } else {
                "café".into()
            }),
        ])
    });
    let err = import_into(&s, &["id", "name"], &mut rows, CancellationToken::new())
        .await
        .expect_err("refused");
    assert!(matches!(err, DbError::Refused(_)), "{err:?}");
    assert!(
        err.to_string().contains("Row 3") && err.to_string().contains('Ω'),
        "{err}"
    );
    assert_eq!(s.scalar("SELECT COUNT(*) FROM dbo.imp").await, "1");

    let mut rows = std::iter::once(Ok(vec![Value::Int(1), Value::Str("Ωμέγα".into())]));
    import_into(&s, &["id", "wide"], &mut rows, CancellationToken::new())
        .await
        .expect("an nvarchar holds it");

    let set = |col: &str, v: &str| GridWrite {
        updates: vec![row_edit(
            &s,
            "imp",
            &[(col, txt(v))],
            &[("id", Value::Int(100))],
        )],
        ..Default::default()
    };
    let err = commit(&s, set("name", "日本")).await.expect_err("refused");
    assert!(err.to_string().contains('日'), "{err}");
    let err = commit(&s, set("dos", "Ωμέγα")).await.expect_err("refused");
    assert!(err.to_string().contains("dos"), "{err}");
    assert_eq!(
        s.scalar("SELECT name FROM dbo.imp WHERE id = 100").await,
        "seed"
    );
    commit(&s, set("name", "café")).await.expect("1252 holds é");
    commit(&s, set("dos", "é")).await.expect("437 holds é");
    // Read back as `nvarchar`: the driver decodes no code page 437 text.
    assert_eq!(
        s.scalar("SELECT CAST(dos AS nvarchar(40)) FROM dbo.imp WHERE id = 100")
            .await,
        "é"
    );
}

/// **A double-byte code page is judged by the server, not by a browser's
/// table.** `encoding_rs`'s Shift_JIS, GBK and Big5 are the WHATWG
/// standard's, which encode `¥` as `\`, `−` as Shift_JIS's minus, GB18030's
/// extra characters and the HKSCS ideographs — none of which Windows' 932,
/// 936 and 950 hold: a `¥1200` written into a Japanese `varchar` was stored
/// `\1200`, a Hong Kong name `?`, and the write reported success.
#[tokio::test(flavor = "multi_thread")]
async fn a_double_byte_varchar_refuses_what_its_windows_code_page_lacks() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("dbcs_cp").await;
    s.exec(
        "CREATE TABLE dbo.imp (id int NOT NULL PRIMARY KEY, \
         jp varchar(40) COLLATE Japanese_CI_AS NULL, \
         tw varchar(40) COLLATE Chinese_Taiwan_Stroke_CI_AS NULL, \
         cn varchar(40) COLLATE Chinese_PRC_CI_AS NULL); \
         INSERT dbo.imp (id, jp) VALUES (100, 'seed')",
    )
    .await;
    let set = |col: &str, v: &str| GridWrite {
        updates: vec![row_edit(
            &s,
            "imp",
            &[(col, txt(v))],
            &[("id", Value::Int(100))],
        )],
        ..Default::default()
    };
    for (col, value, lost, cp) in [
        ("jp", "¥1200", '¥', "932"),
        ("jp", "a−b", '−', "932"),
        ("tw", "陳晄", '晄', "950"),
        ("cn", "\u{1E3F}", '\u{1E3F}', "936"),
    ] {
        let err = commit(&s, set(col, value)).await.expect_err(value);
        assert!(matches!(err, DbError::Refused(_)), "{err:?}");
        let msg = err.to_string();
        assert!(msg.contains(lost) && msg.contains(cp), "{value}: {msg}");
    }
    assert_eq!(
        s.scalar("SELECT jp FROM dbo.imp WHERE id = 100").await,
        "seed"
    );
    // What the code pages do hold goes in, and comes back as it went.
    commit(&s, set("jp", "日本語のテキスト"))
        .await
        .expect("932 holds it");
    commit(&s, set("tw", "台灣")).await.expect("950 holds it");
    assert_eq!(
        s.scalar("SELECT jp FROM dbo.imp WHERE id = 100").await,
        "日本語のテキスト"
    );
    assert_eq!(
        s.scalar("SELECT tw FROM dbo.imp WHERE id = 100").await,
        "台灣"
    );
    // An import is asked about in bulk, a question per batch rather than per
    // value, and a refusal still names the file's row.
    // Keyed past the seed row's 100.
    let mut rows = (1..=1500).map(|i| {
        Ok(vec![
            Value::Int(1000 + i),
            Value::Str(if i == 1400 {
                "¥5".into()
            } else {
                format!("行{i}")
            }),
        ])
    });
    let err = import_into(&s, &["id", "jp"], &mut rows, CancellationToken::new())
        .await
        .expect_err("refused");
    assert!(
        err.to_string().contains("Row 1400") && err.to_string().contains('¥'),
        "{err}"
    );
    assert_eq!(s.scalar("SELECT COUNT(*) FROM dbo.imp").await, "1");
    let mut rows = (1..=1500).map(|i| Ok(vec![Value::Int(1000 + i), Value::Str(format!("行{i}"))]));
    import_into(&s, &["id", "jp"], &mut rows, CancellationToken::new())
        .await
        .expect("932 holds every row");
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM dbo.imp WHERE jp = CONCAT(N'行', id - 1000)")
            .await,
        "1500"
    );
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

/// **An identity switched on is a rebuild — withheld where the rebuild would
/// drop an index it does not read whole**, here one with included columns,
/// and the preview says which rather than applying a table without it.
#[tokio::test(flavor = "multi_thread")]
async fn an_identity_toggle_is_withheld_over_an_index_the_rebuild_cannot_restate() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() {
        return;
    }
    let s = Scratch::create("ddl_ident").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL, a int, b int)")
        .await;
    s.exec("CREATE INDEX ix_a ON dbo.t (a) INCLUDE (b)").await;
    let t = read_table(&s, "t").await;
    let mut d = TableDraft::from_table(&t);
    d.columns[0].info.auto_increment = true;
    let cs = schemaic_core::ddl::diff(&t, &d, MS);
    let refused = cs.unsupported();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(refused[0].contains("ix_a"), "{refused:?}");
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

/// Run `script` as Run Everything does — split, then one connection — and
/// answer each piece's outcome.
async fn run_everything(s: &Scratch, script: &str, cap: usize) -> Vec<Result<ResultSet, DbError>> {
    let stmts = schemaic_core::sql::executable_statements(script, MS);
    let mut outcomes = Vec::new();
    s.db.run_batch(
        Some(&s.name),
        &stmts,
        cap,
        CancellationToken::new(),
        |_, r| outcomes.push(r),
    )
    .await;
    outcomes
}

/// **A variable, a block and a `TRY … CATCH` run as SQL Server's own tools
/// run them.** Split at their `;`s, each piece a batch of its own, the
/// variable was undeclared by the second piece (Msg 137) and the block's first
/// piece a syntax error (Msg 102), after the pieces before had committed.
#[tokio::test(flavor = "multi_thread")]
async fn a_batch_scoped_script_runs_whole() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("batch_scope").await;
    s.exec(
        "CREATE TABLE dbo.t (id int PRIMARY KEY, a int NULL); INSERT dbo.t VALUES (1, 0), (2, 0)",
    )
    .await;
    let out = run_everything(
        &s,
        "DECLARE @id int;\nSELECT @id = 5;\nSELECT @id AS v;",
        10,
    )
    .await;
    assert_eq!(out.len(), 1, "{out:?}");
    let rs = out[0].as_ref().expect("the variable is declared");
    assert_eq!(rs.cell(0, 0).unwrap().display().to_string(), "5");

    let out = run_everything(
        &s,
        "IF 1 = 1\nBEGIN\n  UPDATE dbo.t SET a = 1 WHERE id = 1;\n  UPDATE dbo.t SET a = 2 WHERE id = 2;\nEND",
        10,
    )
    .await;
    assert!(out.iter().all(Result::is_ok), "{out:?}");
    assert_eq!(s.scalar("SELECT SUM(a) FROM dbo.t").await, "3");

    let out = run_everything(
        &s,
        "BEGIN TRY SELECT 1/0 AS x; END TRY BEGIN CATCH SELECT ERROR_NUMBER() AS n; END CATCH",
        10,
    )
    .await;
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(out[0].is_ok(), "the error is caught: {out:?}");

    // A label after a statement no `;` closed still holds its `GOTO` in the
    // batch: cut there, `GOTO again` went alone (Msg 133) after the first
    // `UPDATE` had committed.
    let out = run_everything(
        &s,
        "UPDATE dbo.t SET a = a + 1 WHERE id = 1\nagain:\nUPDATE dbo.t SET a = a + 1 WHERE id = 2;\n\
         IF (SELECT a FROM dbo.t WHERE id = 2) < 5 GOTO again;",
        10,
    )
    .await;
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(out[0].is_ok(), "{out:?}");
    assert_eq!(s.scalar("SELECT SUM(a) FROM dbo.t").await, "7");
}

/// **A batch's error is the batch's outcome, whatever it returned first.**
/// The read stopped at a second result set, or at the row cap, and dropped
/// the stream — and the driver raises an error only at the stream's end, so a
/// failed batch was reported a success, and whether its trailing statements
/// ran depended on how big the first result was.
#[tokio::test(flavor = "multi_thread")]
async fn a_batch_reports_its_error_past_its_first_result() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("batch_err").await;
    s.exec(
        "CREATE TABLE dbo.w (id int PRIMARY KEY, v int NOT NULL); INSERT dbo.w VALUES (1, 0); \
         SELECT TOP (5000) ROW_NUMBER() OVER (ORDER BY (SELECT 1)) AS n INTO dbo.big \
         FROM sys.all_columns a CROSS JOIN sys.all_columns b",
    )
    .await;
    let err = s
        .try_exec("SELECT 1 AS a SELECT 1/0 AS b")
        .await
        .expect_err("the divide by zero is the batch's");
    assert!(err.to_string().contains("Divide by zero"), "{err}");

    // A failed first piece stops Run All before the second.
    let out = run_everything(
        &s,
        "SELECT 1 AS a SELECT 1/0 AS b;\nGO\nUPDATE dbo.w SET v = 7",
        10,
    )
    .await;
    assert!(out[0].is_err(), "{out:?}");
    assert!(matches!(out[1], Err(DbError::Cancelled)), "{out:?}");
    assert_eq!(s.scalar("SELECT v FROM dbo.w").await, "0");

    // Past the cap, a trailing statement in the same batch still runs —
    // whatever the size of what came before — and the grid says it is cut.
    let out = run_everything(&s, "SELECT n FROM dbo.big UPDATE dbo.w SET v = 55", 10).await;
    let rs = out[0].as_ref().expect("the batch ran");
    assert!(rs.truncated && rs.row_count() == 10, "{rs:?}");
    assert_eq!(s.scalar("SELECT v FROM dbo.w").await, "55");
    let out = run_everything(
        &s,
        "SELECT 1 AS a SELECT n FROM dbo.big UPDATE dbo.w SET v = v + 100",
        10,
    )
    .await;
    assert!(out[0].is_ok(), "{out:?}");
    assert_eq!(s.scalar("SELECT v FROM dbo.w").await, "155");

    // A statement's error in a batch the server runs on past it says so.
    let err = s
        .try_exec("INSERT dbo.w VALUES (1, 5)\nUPDATE dbo.w SET v = 9")
        .await
        .expect_err("the duplicate key");
    assert!(err.to_string().contains("may have run"), "{err}");
    assert_eq!(s.scalar("SELECT v FROM dbo.w").await, "9");
}

/// **Past the row cap, a lone write and a procedure call are read to their
/// end.** The read stopped at the cap for any lone statement and dropped the
/// connection, and the server aborted what it was still running: an `UPDATE
/// … OUTPUT` larger than the network buffers was reported a success over a
/// truncated grid with no row changed, and an error a procedure raised after
/// its first result was never reported — on a Manual tab the driver's
/// resynchronisation discarded it at the next request.
#[tokio::test(flavor = "multi_thread")]
async fn a_write_or_a_procedure_past_the_row_cap_is_read_to_its_end() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("cap_drain").await;
    s.exec(
        "SELECT TOP (100000) ROW_NUMBER() OVER (ORDER BY (SELECT 1)) AS n, \
         CAST(0 AS int) AS v, CAST(REPLICATE('x', 200) AS varchar(200)) AS pad INTO dbo.big \
         FROM sys.all_columns a CROSS JOIN sys.all_columns b",
    )
    .await;
    let capped = |sql: &'static str| {
        s.db.fetch_query(Some(&s.name), sql, 10, CancellationToken::new())
    };
    let rs = capped("UPDATE dbo.big SET v = 1 OUTPUT inserted.n, inserted.pad")
        .await
        .expect("the update ran");
    assert!(rs.truncated && rs.row_count() == 10, "{rs:?}");
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM dbo.big WHERE v = 1").await,
        "100000",
        "every row the grid did not show was updated too"
    );
    let rs = capped("DELETE FROM dbo.big OUTPUT deleted.n, deleted.pad WHERE n > 50000")
        .await
        .expect("the delete ran");
    assert!(rs.truncated, "{rs:?}");
    assert_eq!(s.scalar("SELECT COUNT(*) FROM dbo.big").await, "50000");

    s.exec(
        "CREATE PROCEDURE dbo.report_then_fail AS BEGIN \
           SELECT n, pad FROM dbo.big; THROW 50001, 'the purge failed', 1; END",
    )
    .await;
    let err = capped("EXEC dbo.report_then_fail")
        .await
        .expect_err("the procedure's error is the run's");
    assert!(err.to_string().contains("the purge failed"), "{err}");
    // And on a pinned session, whose next request would have flushed it.
    let session = manual(&s).await;
    let out = session
        .fetch_query("EXEC dbo.report_then_fail", 10, CancellationToken::new())
        .await;
    let err = out.result.expect_err("reported on the pinned session too");
    assert!(err.to_string().contains("the purge failed"), "{err}");
    session.rollback().await.expect("rollback");
    session.close().await;
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
/// the columns, keys, checks and indexes come back the same. On Azure, which
/// has the one database, the original is dropped and rebuilt in place.
#[tokio::test(flavor = "multi_thread")]
async fn a_tables_ddl_rebuilds_the_table_it_was_read_from() {
    if !enabled() {
        return;
    }
    let src = Scratch::create("ddl_src").await;
    let second = if on_azure() {
        None
    } else {
        Some(Scratch::create("ddl_dst").await)
    };
    src.exec(
        "CREATE TABLE dbo.t ( \
           id int IDENTITY(1000,-5) CONSTRAINT pk_t PRIMARY KEY, \
           [odd]]name] nvarchar(40) COLLATE Latin1_General_BIN NULL, \
           balance decimal(10,2) NOT NULL DEFAULT ((0)), \
           doubled AS (balance * 2) PERSISTED, \
           tripled AS (CONVERT(int, balance) * 3) PERSISTED NOT NULL, \
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
    let dst = match &second {
        Some(dst) => dst,
        None => {
            src.exec("DROP TABLE dbo.t").await;
            &src
        }
    };
    for stmt in schemaic_core::sql::executable_statements(&ddl, MS) {
        dst.exec(&stmt).await;
    }
    let copy = read(dst).await;
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
/// only a 195 fails the test — or, for a rowset one, a 208, *invalid object
/// name*, which is how `FROM` answers a name it does not know. It checks the
/// over-listing direction; a builtin the list lacks has no oracle.
///
/// **Version-aware.** On a server older than 2025 — which Azure SQL Database
/// never is, though its version number says 12 — the [`NEWER_THAN_2022`]
/// block is excused — and held to the opposite answer, unknown, so the excuse
/// cannot hide a name that server does have. On 2025 the scratch database
/// turns `PREVIEW_FEATURES` on first, since five of that block parse only
/// behind it.
#[tokio::test(flavor = "multi_thread")]
async fn every_catalogued_builtin_is_one_the_server_knows() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("builtins").await;
    let major: u32 = s
        .scalar("SELECT CAST(SERVERPROPERTY('ProductMajorVersion') AS int)")
        .await
        .parse()
        .expect("a major version");
    // Azure SQL Database (edition 5) reports 12 whatever it runs, and runs
    // the newest engine.
    let edition = s
        .scalar("SELECT CAST(SERVERPROPERTY('EngineEdition') AS int)")
        .await;
    let has_2025 = major >= 17 || edition == "5";
    if has_2025 {
        s.exec("ALTER DATABASE SCOPED CONFIGURATION SET PREVIEW_FEATURES = ON")
            .await;
    }
    // The oracle can see a failure: a name nobody has is a 195 on this path,
    // and a 208 in `FROM`.
    let bogus = s
        .try_exec("SELECT SCHEMAIC_NO_SUCH_FUNCTION()")
        .await
        .expect_err("an unknown function");
    assert!(bogus.to_string().contains("(Msg 195"), "{bogus}");
    let bogus = s
        .try_exec("SELECT * FROM SCHEMAIC_NO_SUCH_FUNCTION()")
        .await
        .expect_err("an unknown rowset function");
    assert!(bogus.to_string().contains("(Msg 208"), "{bogus}");
    const ROWSET: &[&str] = &[
        "AI_GENERATE_CHUNKS",
        "CONTAINSTABLE",
        "FREETEXTTABLE",
        "GENERATE_SERIES",
        "OPENDATASOURCE",
        "OPENJSON",
        "OPENQUERY",
        "OPENROWSET",
        "OPENXML",
        "PREDICT",
        "REGEXP_MATCHES",
        "REGEXP_SPLIT_TO_TABLE",
        "STRING_SPLIT",
    ];
    let mut unknown = Vec::new();
    let mut not_new = Vec::new();
    for f in schemaic_core::mssql_builtins::MSSQL_FUNCTIONS {
        let (call, not_known) = if !f.signature.contains('(') {
            // A niladic name 2022 lacks is a keyword there (`CURRENT_DATE`,
            // Msg 156) or a column it cannot find (207).
            (format!("SELECT {}", f.name), ["(Msg 156", "(Msg 207"])
        } else if ROWSET.contains(&f.name) {
            (
                format!("SELECT * FROM {}()", f.name),
                ["(Msg 208", "(Msg 195"],
            )
        } else {
            (format!("SELECT {}()", f.name), ["(Msg 195", "(Msg 195"])
        };
        let err = s.try_exec(&call).await.err().map(|e| e.to_string());
        let is_unknown = |e: &String| not_known.iter().any(|m| e.contains(m));
        if !has_2025 && NEWER_THAN_2022.contains(&f.name) {
            if !err.as_ref().is_some_and(is_unknown) {
                not_new.push(format!("{}: {err:?}", f.name));
            }
            continue;
        }
        // Only 195/208 fail the known direction: a keyword or a column answer
        // on a niladic name is not a verdict about builtins. `PREDICT` is the
        // one rowset name the 208 cannot judge — with no `MODEL =` inside, both
        // versions read `PREDICT()` as a table, and with one they report a
        // corrupt model (Msg 39051), so it is checked by 195 alone.
        let judged_by_208 = ROWSET.contains(&f.name) && f.name != "PREDICT";
        if let Some(e) = err
            && (e.contains("(Msg 195") || (judged_by_208 && e.contains("(Msg 208")))
        {
            unknown.push(format!("{}: {e}", f.name));
        }
    }
    assert!(unknown.is_empty(), "{unknown:#?}");
    assert!(
        not_new.is_empty(),
        "excused as SQL Server 2025's but known to {major}: {not_new:#?}"
    );
}

/// The names [`schemaic_core::mssql_builtins`] holds that a server before
/// SQL Server 2025 (major version 17) does not — the catalog's last block,
/// name for name. See [`every_catalogued_builtin_is_one_the_server_knows`].
const NEWER_THAN_2022: &[&str] = &[
    "REGEXP_LIKE",
    "REGEXP_REPLACE",
    "REGEXP_SUBSTR",
    "REGEXP_INSTR",
    "REGEXP_COUNT",
    "REGEXP_MATCHES",
    "REGEXP_SPLIT_TO_TABLE",
    "EDIT_DISTANCE",
    "EDIT_DISTANCE_SIMILARITY",
    "JARO_WINKLER_DISTANCE",
    "JARO_WINKLER_SIMILARITY",
    "UNISTR",
    "PRODUCT",
    "CURRENT_DATE",
    "BASE64_ENCODE",
    "BASE64_DECODE",
    "JSON_ARRAYAGG",
    "JSON_OBJECTAGG",
    "JSON_CONTAINS",
    "VECTOR_DISTANCE",
    "VECTOR_NORM",
    "VECTOR_NORMALIZE",
    "VECTORPROPERTY",
    "AI_GENERATE_EMBEDDINGS",
    "AI_GENERATE_CHUNKS",
];

/// **The excuse is the catalog's last block, exactly.** A name added to the
/// 2025 block without this list fails 2022's oracle, and the reverse — a list
/// entry the catalog has dropped — would excuse nothing; both are caught here
/// without a server.
#[test]
fn the_2025_excuse_is_the_catalogs_last_block() {
    let catalog = schemaic_core::mssql_builtins::MSSQL_FUNCTIONS;
    let tail: Vec<&str> = catalog[catalog.len() - NEWER_THAN_2022.len()..]
        .iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(tail, NEWER_THAN_2022);
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

/// **A caught error that dooms the transaction ends it at the batch's end.**
/// Under `XACT_ABORT ON`, a `TRY … CATCH` that catches a duplicate key raises
/// nothing itself and leaves `XACT_STATE()` at -1 — but every statement the
/// session runs is its own batch, and SQL Server rolls back an uncommittable
/// transaction when its batch ends (Msg 3998). So no probe ever sees a doomed
/// transaction after a success: the statement fails, the transaction is gone,
/// and the fold says so.
#[tokio::test(flavor = "multi_thread")]
async fn a_caught_error_that_dooms_the_transaction_rolls_it_back_at_the_batch_end() {
    use schemaic_core::tx::StmtOutcome;
    if !enabled() {
        return;
    }
    let s = Scratch::create("tx_doomed_ok").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY); INSERT dbo.t VALUES (1)")
        .await;
    let session = manual(&s).await;
    assert_eq!(stmt(&session, "SET XACT_ABORT ON").await, StmtOutcome::Ok);
    assert_eq!(
        stmt(&session, "INSERT dbo.t VALUES (2)").await,
        StmtOutcome::Ok
    );
    let out = session
        .fetch_query(
            "BEGIN TRY INSERT dbo.t VALUES (1) END TRY BEGIN CATCH SELECT 'caught' AS c END CATCH",
            10,
            CancellationToken::new(),
        )
        .await;
    assert_eq!(
        out.stmt,
        StmtOutcome::FailedAndRolledBack,
        "{:?}",
        out.result
    );
    let err = out.result.expect_err("the batch-end rollback is an error");
    assert!(err.to_string().contains("3998"), "{err}");
    session.close().await;
    assert_eq!(committed_rows(&s).await, "1");
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

/// **A piece that commits and then fails is not reported rolled back.** From
/// its `DECLARE` the text is one piece, so its `COMMIT` and the failure after
/// it reach the server as one batch, and `@@TRANCOUNT` then reads 0 exactly as
/// after a rollback. The tab said "every statement in it since it began is
/// undone" over a debit another connection could read.
#[tokio::test(flavor = "multi_thread")]
async fn a_piece_that_commits_then_fails_is_not_reported_rolled_back() {
    use schemaic_core::tx::StmtOutcome;
    if !enabled() {
        return;
    }
    let s = Scratch::create("tx_commit_fail").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY, bal int NOT NULL); INSERT dbo.t VALUES (1, 100)")
        .await;
    let pieces = schemaic_core::sql::executable_statements(
        "DECLARE @id int = 1;\nUPDATE dbo.t SET bal = bal - 10 WHERE id = @id;\nCOMMIT;\n\
         SELECT 1/0 AS boom;",
        MS,
    );
    assert_eq!(pieces.len(), 1, "{pieces:#?}");
    let session = manual(&s).await;
    let out = session
        .fetch_query(&pieces[0], 10, CancellationToken::new())
        .await;
    assert_eq!(out.stmt, StmtOutcome::FailedAfterCommit, "{:?}", out.result);
    let err = out.result.expect_err("the divide by zero");
    let shown = schemaic_core::tx::failed_message(&err.to_string(), out.stmt);
    assert!(shown.contains("likely committed"), "{shown}");
    session.close().await;
    assert_eq!(
        s.scalar("SELECT bal FROM dbo.t WHERE id = 1").await,
        "90",
        "the debit is committed"
    );
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

/// **A pinned session writes the table its edit names, in whichever database
/// that is** — not the same-named table in the session's own. A Manual tab
/// pinned to A that read `B.dbo.t` and edited a row of it overwrote `A`'s
/// `dbo.t` instead, and the re-read, on the same unqualified name, showed the
/// typed value over the table the user never touched. The re-read and a
/// binary cell's read name B too.
#[tokio::test(flavor = "multi_thread")]
async fn a_manual_grid_edit_of_another_databases_table_lands_there() {
    use schemaic_core::tx::StmtOutcome;
    if !enabled() || on_azure() {
        return;
    }
    let a = Scratch::create("tx_xdb_a").await;
    let b = Scratch::create("tx_xdb_b").await;
    for s in [&a, &b] {
        s.exec(&format!(
            "CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY, v nvarchar(20) NULL, \
             bin varbinary(10) NULL); \
             INSERT dbo.t VALUES (1, N'{}-orig', 0x{})",
            if s.name == a.name { "a" } else { "b" },
            if s.name == a.name { "AA" } else { "BB" },
        ))
        .await;
    }
    let session = manual(&a).await;
    let out = session
        .commit_writes(
            &GridWrite {
                updates: vec![row_edit(
                    &b,
                    "t",
                    &[("v", txt("EDITED"))],
                    &[("id", Value::Int(1))],
                )],
                ..Default::default()
            },
            CancellationToken::new(),
        )
        .await;
    assert_eq!(out.stmt, StmtOutcome::Ok, "{:?}", out.result);
    let template = RefetchTemplate {
        database: b.name.clone(),
        schema: Some("dbo".into()),
        table: "t".into(),
        columns: vec!["id".into(), "v".into()],
        key_cols: vec![0],
        confirm_cols: vec![],
    };
    let reread = session
        .refetch_rows(
            &template,
            &[RefetchRow {
                data_row: 0,
                key: vec![Value::Int(1)],
            }],
            CancellationToken::new(),
        )
        .await
        .result
        .expect("a re-read");
    assert_eq!(reread[0].1[1].display().to_string(), "EDITED");
    let blob = session
        .fetch_blob(
            &BlobRef {
                database: b.name.clone(),
                schema: Some("dbo".into()),
                table: "t".into(),
                column: "bin".into(),
                key: vec![("id".into(), Value::Int(1))],
            },
            CancellationToken::new(),
        )
        .await
        .result
        .expect("a blob read")
        .expect("a value");
    assert_eq!(blob.bytes, vec![0xBB]);
    session.commit().await.expect("commit");
    session.close().await;
    assert_eq!(b.scalar("SELECT v FROM dbo.t").await, "EDITED");
    assert_eq!(a.scalar("SELECT v FROM dbo.t").await, "a-orig");
}

/// **A batch refused before its own `SAVE` undoes nothing** — no second
/// `ROLLBACK TRANSACTION schemaic_w`, which would land on the previous
/// batch's savepoint and silently take back the user's statements since.
#[tokio::test(flavor = "multi_thread")]
async fn a_grid_batch_refused_before_its_savepoint_keeps_the_work_before_it() {
    use schemaic_core::tx::StmtOutcome;
    if !enabled() {
        return;
    }
    let s = Scratch::create("tx_presave").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY, qty int NULL)")
        .await;
    let session = manual(&s).await;
    let first = session
        .commit_writes(
            &GridWrite {
                inserts: vec![row_insert(&s, "t", &[("id", txt("1")), ("qty", txt("5"))])],
                ..Default::default()
            },
            CancellationToken::new(),
        )
        .await;
    assert_eq!(first.stmt, StmtOutcome::Ok, "{:?}", first.result);
    assert_eq!(
        stmt(&session, "INSERT dbo.t VALUES (2, 6)").await,
        StmtOutcome::Ok
    );
    let refused = session
        .commit_writes(
            &GridWrite {
                updates: vec![row_edit(
                    &s,
                    "t",
                    &[("qty", txt(""))],
                    &[("id", Value::Int(1))],
                )],
                ..Default::default()
            },
            CancellationToken::new(),
        )
        .await;
    assert!(
        matches!(refused.result, Err(DbError::Refused(_))),
        "{:?}",
        refused.result
    );
    assert_ne!(refused.stmt, StmtOutcome::FailedAndRolledBack);
    let seen = session
        .fetch_query("SELECT COUNT(*) FROM dbo.t", 10, CancellationToken::new())
        .await
        .result
        .expect("a count");
    assert_eq!(seen.cell(0, 0).expect("a cell").display().to_string(), "2");
    session.commit().await.expect("commit");
    session.close().await;
    assert_eq!(committed_rows(&s).await, "2");
}

/// **A rolled-back identity insert switches `IDENTITY_INSERT` off again** —
/// a savepoint's rollback leaves session settings alone, and left on it
/// refuses the tab's next ordinary insert into the table (Msg 545).
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_identity_insert_leaves_identity_insert_off() {
    use schemaic_core::tx::StmtOutcome;
    if !enabled() {
        return;
    }
    let s = Scratch::create("tx_ident").await;
    s.exec(
        "CREATE TABLE dbo.t (id int IDENTITY(1,1) PRIMARY KEY, v nvarchar(10) NULL); \
         INSERT dbo.t (v) VALUES (N'one')",
    )
    .await;
    let session = manual(&s).await;
    let dup = session
        .commit_writes(
            &GridWrite {
                inserts: vec![row_insert(&s, "t", &[("id", txt("1")), ("v", txt("dup"))])],
                ..Default::default()
            },
            CancellationToken::new(),
        )
        .await;
    assert_eq!(dup.stmt, StmtOutcome::FailedIsolated, "{:?}", dup.result);
    assert_eq!(
        stmt(&session, "INSERT dbo.t (v) VALUES (N'two')").await,
        StmtOutcome::Ok
    );
    session.commit().await.expect("commit");
    session.close().await;
    assert_eq!(committed_rows(&s).await, "2");
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

/// **A Stop of a batch that has already raised an error keeps the connection
/// in step.** The driver answered such an attention with the aborted batch's
/// own error and left the acknowledgement on the wire, so every later reply
/// belonged to the request before it — and the next statement panicked the
/// run task reading another query's row as the transaction's state.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_after_an_error_leaves_the_manual_session_in_step() {
    use schemaic_core::tx::StmtOutcome;
    if !enabled() {
        return;
    }
    let s = Scratch::create("tx_stop_err").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL PRIMARY KEY)")
        .await;
    let session = manual(&s).await;
    assert_eq!(
        stmt(&session, "INSERT dbo.t VALUES (500)").await,
        StmtOutcome::Ok
    );
    for stalled in [
        "SELECT 1/0 AS x; WAITFOR DELAY '00:00:20'",
        "RAISERROR('boom', 16, 1); WAITFOR DELAY '00:00:20'",
    ] {
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            stop.cancel();
        });
        let out = session.fetch_query(stalled, 100, cancel).await;
        assert!(
            matches!(
                out.stmt,
                StmtOutcome::Cancelled | StmtOutcome::CancelledIsolated
            ),
            "{stalled}: {:?} {:?}",
            out.stmt,
            out.result
        );
        let answer = session
            .fetch_query("SELECT 42 AS answer", 10, CancellationToken::new())
            .await
            .result
            .expect("the next statement's own reply");
        assert_eq!(answer.cell(0, 0).unwrap().display().to_string(), "42");
    }
    session.commit().await.expect("commit");
    session.close().await;
    assert_eq!(committed_rows(&s).await, "1");
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

/// A scratch **login**, dropped when it goes out of scope — a login is the
/// server's, so the scratch database's drop does not take it.
struct ScratchLogin(String);

impl Drop for ScratchLogin {
    fn drop(&mut self) {
        let name = self.0.clone();
        assert_scratch_name(&name);
        let dropped = std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .expect("a runtime for the teardown")
                .block_on(base_db().fetch_query(
                    None,
                    &format!("IF SUSER_ID(N'{name}') IS NOT NULL DROP LOGIN [{name}]"),
                    1,
                    CancellationToken::new(),
                ))
        })
        .join();
        if !matches!(dropped, Ok(Ok(_))) && !std::thread::panicking() {
            panic!("could not drop login {}: {dropped:?}", self.0);
        }
    }
}

/// **A `datetime` key and a `datetime` value mean the same day under every
/// login language.** Under `british` — the default for a German, French,
/// Italian or Spanish installation — `yyyy-mm-dd hh:mm:ss` converts to
/// `datetime` as year-*day*-month, so deleting the 2 January row in the grid
/// deleted 1 February, and the 1-row net passed it (measured on 2022). The
/// re-read keyed the same way, and a typed value went in day-swapped.
#[tokio::test(flavor = "multi_thread")]
async fn a_datetime_key_finds_its_row_under_a_day_first_login() {
    if !enabled() || azure_cannot("creates a login, so the day-first language") {
        return;
    }
    let s = Scratch::create("dmy").await;
    let name = format!("{PREFIX}{}_mssql_brit", std::process::id());
    let _login = ScratchLogin(name.clone());
    base_db()
        .fetch_query(
            None,
            &format!(
                "CREATE LOGIN [{name}] WITH PASSWORD = N'Brit_2026!pass', CHECK_POLICY = OFF, \
                 DEFAULT_LANGUAGE = british"
            ),
            1,
            CancellationToken::new(),
        )
        .await
        .expect("a british login");
    s.exec(&format!(
        "CREATE USER [{name}] FOR LOGIN [{name}]; ALTER ROLE db_owner ADD MEMBER [{name}]; \
         CREATE TABLE dbo.d (at datetime NOT NULL PRIMARY KEY, sm smalldatetime NULL, \
         v nvarchar(10) NULL); \
         INSERT dbo.d VALUES ('20260102', '20260102', N'jan2'), ('20260201', '20260201', N'feb1')"
    ))
    .await;
    let brit = Db::from_parts(
        Engine::MsSql,
        var("HOST", "127.0.0.1"),
        var("PORT", "1433").parse().unwrap(),
        name.clone(),
        "Brit_2026!pass".to_string(),
        s.name.clone(),
    );
    let lang = brit
        .fetch_query(
            Some(&s.name),
            "SELECT @@LANGUAGE",
            1,
            CancellationToken::new(),
        )
        .await
        .expect("signed in");
    assert_eq!(lang.cell(0, 0).unwrap().display().to_string(), "British");
    // The key exactly as the grid read it, through this login.
    let read = brit
        .fetch_query(
            Some(&s.name),
            "SELECT at, sm FROM dbo.d WHERE v = N'jan2'",
            1,
            CancellationToken::new(),
        )
        .await
        .expect("a read");
    let at = read.cell(0, 0).unwrap().display().to_string();
    let sm = read.cell(0, 1).unwrap().display().to_string();
    assert_eq!(at, "2026-01-02 00:00:00.000");
    let key = |at: &str| vec![("at", Value::Str(at.into()))];
    let edit = |set: &[(&str, CellEdit)], at: &str| GridWrite {
        updates: vec![row_edit(&s, "d", set, &key(at))],
        ..Default::default()
    };
    // A typed value, in the grid's own spelling, goes in as that day.
    brit.commit_writes(
        &edit(&[("sm", txt("2026-03-04 10:30"))], &at),
        CancellationToken::new(),
    )
    .await
    .expect("an edit of the jan2 row");
    assert_eq!(
        s.scalar("SELECT CONVERT(char(16), sm, 126) FROM dbo.d WHERE v = N'jan2'")
            .await,
        "2026-03-04T10:30"
    );
    // The re-read finds the row by the same key.
    let reread = brit
        .refetch_rows(
            &RefetchTemplate {
                database: s.name.clone(),
                schema: Some("dbo".into()),
                table: "d".into(),
                columns: vec!["at".into(), "v".into()],
                key_cols: vec![0],
                confirm_cols: vec![],
            },
            &[RefetchRow {
                data_row: 0,
                key: vec![Value::Str(at.clone())],
            }],
            CancellationToken::new(),
        )
        .await
        .expect("a re-read");
    assert_eq!(reread[0].1[1].display().to_string(), "jan2");
    // A smalldatetime key, too.
    assert!(!sm.is_empty());
    brit.commit_writes(
        &GridWrite {
            deletes: vec![row_delete(&s, "d", &key(&at))],
            ..Default::default()
        },
        CancellationToken::new(),
    )
    .await
    .expect("the jan2 row deleted");
    assert_eq!(s.scalar("SELECT v FROM dbo.d").await, "feb1");
    // An imported date is the file's day too.
    let columns = vec!["at".to_string(), "v".to_string()];
    let mut rows = std::iter::once(Ok(vec![
        Value::Str("2026-01-03 08:00".into()),
        Value::Str("jan3".into()),
    ]));
    brit.import_rows(
        schemaic_db::ImportTarget {
            database: &s.name,
            schema: Some("dbo"),
            table: "d",
            columns: &columns,
        },
        &mut rows,
        CancellationToken::new(),
    )
    .await
    .expect("imported");
    assert_eq!(
        s.scalar("SELECT CONVERT(char(16), at, 126) FROM dbo.d WHERE v = N'jan3'")
            .await,
        "2026-01-03T08:00"
    );
}

/// **A copied `INSERT` and a cell filter mean the grid's day under a
/// day-first language.** Both wrote the cell's `2026-01-02 10:30:00.000`,
/// which a `datetime` or `smalldatetime` reads as the 1st of February under
/// `british`: the pasted copy stored February with `1 row affected`, and
/// *Filter by this value* on the 2 January row showed the February one.
#[tokio::test(flavor = "multi_thread")]
async fn a_copied_insert_and_a_cell_filter_keep_their_day_under_a_day_first_language() {
    if !enabled() {
        return;
    }
    let s = Scratch::create("dmycopy").await;
    s.exec(
        "CREATE TABLE dbo.d (id int PRIMARY KEY, at datetime NOT NULL, sm smalldatetime NULL); \
         INSERT dbo.d VALUES (1, '20260102 10:30', '20260102 10:30'), \
           (2, '20260201 10:30', '20260201 10:30'); \
         CREATE TABLE dbo.copy (id int PRIMARY KEY, at datetime NOT NULL, sm smalldatetime NULL);",
    )
    .await;
    let rs = s.exec("SELECT id, at, sm FROM dbo.d WHERE id = 1").await;
    let sql = schemaic_core::export::export_inserts(&rs, &[0], Some(("", Some("dbo"), "copy")), MS);
    s.exec(&format!("SET LANGUAGE british;\n{sql}")).await;
    assert_eq!(
        s.scalar(
            "SELECT CONCAT(CONVERT(char(16), at, 126), '|', CONVERT(char(16), sm, 126)) \
             FROM dbo.copy"
        )
        .await,
        "2026-01-02T10:30|2026-01-02T10:30",
        "{sql}"
    );
    for (ci, col) in [(1, "at"), (2, "sm")] {
        let text = rs.cell(0, ci).unwrap().display().to_string();
        let ty = &rs.columns[ci].type_name;
        let ids = |negate: bool| {
            let cond = schemaic_core::filter::eq_condition(col, ty, Some(&text), negate, MS);
            format!(
                "SET LANGUAGE british; \
                 SELECT CONCAT('ids ', STRING_AGG(id, ',') WITHIN GROUP (ORDER BY id)) \
                 FROM dbo.d WHERE {cond}"
            )
        };
        assert_eq!(
            s.scalar(&ids(false)).await,
            "ids 1",
            "{col}: {}",
            ids(false)
        );
        assert_eq!(s.scalar(&ids(true)).await, "ids 2", "{col}: {}", ids(true));
    }
}

/// **Accounts end to end, both halves.** A login and the user it brings are
/// created in one plan and listed linked; the login signs in to the database
/// through its user; grants at the schema and a role membership read back as
/// the sentences that made them; a reset on the *user* row changes the
/// *login's* password; and the drops leave neither behind.
#[tokio::test(flavor = "multi_thread")]
async fn a_login_and_its_user_are_created_granted_reset_and_dropped() {
    use schemaic_core::ddl::{Change, account, accounts};
    use schemaic_core::users::{
        AccountDraft, GrantLevel, PasswordReset, PrincipalKind, PrivilegeChange, RoleChange,
        companion_user_draft,
    };
    if !enabled()
        || azure_cannot(
            "takes CREATE LOGIN only in master, and the plan runs in the user database, \
             so the login round trip",
        )
    {
        return;
    }
    let s = Scratch::create("accounts").await;
    let name = format!("{PREFIX}{}_mssql_acct", std::process::id());
    let _login = ScratchLogin(name.clone());
    let run = |stmts: Vec<String>| {
        let db = s.db.clone();
        let database = s.name.clone();
        async move {
            db.run_ddl(&database, &stmts, CancellationToken::new())
                .await
                .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
        }
    };
    let signs_in = |password: &'static str| {
        let db = Db::from_parts(
            Engine::MsSql,
            var("HOST", "127.0.0.1"),
            var("PORT", "1433").parse().unwrap(),
            name.clone(),
            password.to_string(),
            s.name.clone(),
        );
        let database = s.name.clone();
        async move {
            db.fetch_query(
                Some(&database),
                "SELECT USER_NAME()",
                1,
                CancellationToken::new(),
            )
            .await
            .ok()
            .and_then(|rs| rs.cell(0, 0).map(|c| c.display().to_string()))
        }
    };

    // One plan: the login, and the user it brings.
    let draft = AccountDraft {
        name: name.clone(),
        kind: PrincipalKind::Login,
        password: "Schemaic_Pw1!".into(),
        also_user: true,
        ..Default::default()
    };
    let user_draft = companion_user_draft(&draft, MS).expect("a user");
    run(accounts(
        &name,
        MS,
        vec![
            Change::CreateAccount(Box::new(draft)),
            Change::CreateAccount(Box::new(user_draft)),
        ],
    )
    .emit())
    .await;
    let list =
        s.db.fetch_principals(Some(&s.name))
            .await
            .expect("the accounts")
            .list;
    let login = list
        .iter()
        .find(|p| p.name == name && p.kind == PrincipalKind::Login)
        .expect("the login")
        .clone();
    let user = list
        .iter()
        .find(|p| p.name == name && p.kind == PrincipalKind::User)
        .expect("the user")
        .clone();
    assert_eq!(user.login.as_deref(), Some(name.as_str()), "linked");
    assert!(list.iter().any(|p| p.name == "db_datareader" && p.system));
    assert_eq!(
        signs_in("Schemaic_Pw1!").await.as_deref(),
        Some(name.as_str())
    );

    // A schema grant and a role, read back as the sentences that made them.
    // The grant is grantable, so the revoke further down is the one T-SQL
    // refuses without `CASCADE`.
    let schema_grant = PrivilegeChange {
        account: user.clone(),
        level: GrantLevel::Schema("dbo".into()),
        privileges: vec!["SELECT".into()],
        with_grant_option: true,
    };
    run(account(
        &name,
        MS,
        Change::GrantPrivileges(Box::new(schema_grant.clone())),
    )
    .emit())
    .await;
    let reader = list
        .iter()
        .find(|p| p.name == "db_datareader")
        .unwrap()
        .clone();
    run(account(
        &name,
        MS,
        Change::GrantRole(Box::new(RoleChange {
            role: reader,
            member: user.clone(),
            with_admin_option: false,
        })),
    )
    .emit())
    .await;
    let grants =
        s.db.fetch_grants(Some(&s.name), &user)
            .await
            .expect("the grants");
    let q = format!("[{name}]");
    for want in [
        format!("GRANT CONNECT ON DATABASE::[{}] TO {q};", s.name),
        format!("GRANT SELECT ON SCHEMA::[dbo] TO {q} WITH GRANT OPTION;"),
        format!("ALTER ROLE [db_datareader] ADD MEMBER {q};"),
    ] {
        assert!(
            grants.statements.contains(&want),
            "{want}\n{:#?}",
            grants.statements
        );
    }

    // Taken back, grant option and all.
    run(account(&name, MS, Change::RevokePrivileges(Box::new(schema_grant))).emit()).await;
    let grants =
        s.db.fetch_grants(Some(&s.name), &user)
            .await
            .expect("the grants");
    assert!(
        !grants
            .statements
            .iter()
            .any(|x| x.contains("ON SCHEMA::[dbo]")),
        "{:#?}",
        grants.statements
    );
    let server = s.db.fetch_grants(None, &login).await.expect("the login's");
    assert!(
        server
            .statements
            .iter()
            .any(|x| x.starts_with("GRANT CONNECT SQL TO")),
        "{:#?}",
        server.statements
    );

    // The login's own half: a server permission and a server role, in one
    // plan run from the scratch database — where T-SQL refuses a server
    // grant unless it is sent to `master` — and taken back the same way.
    let server_grant = PrivilegeChange {
        account: login.clone(),
        level: GrantLevel::Global,
        privileges: vec!["VIEW SERVER STATE".into()],
        with_grant_option: true,
    };
    let server_role = RoleChange {
        role: schemaic_core::users::Principal {
            name: "dbcreator".into(),
            host: None,
            kind: PrincipalKind::Role,
            system: true,
            attributes: Vec::new(),
            role_ambiguous: false,
            login: None,
            database_password: false,
            external_sign_in: false,
        },
        member: login.clone(),
        with_admin_option: false,
    };
    run(accounts(
        &name,
        MS,
        vec![
            Change::GrantPrivileges(Box::new(server_grant.clone())),
            Change::GrantRole(Box::new(server_role.clone())),
        ],
    )
    .emit())
    .await;
    let server = s.db.fetch_grants(None, &login).await.expect("the login's");
    for want in [
        format!("GRANT VIEW SERVER STATE TO {q} WITH GRANT OPTION;"),
        format!("ALTER SERVER ROLE [dbcreator] ADD MEMBER {q};"),
    ] {
        assert!(
            server.statements.contains(&want),
            "{want}\n{:#?}",
            server.statements
        );
    }
    run(accounts(
        &name,
        MS,
        vec![
            Change::RevokePrivileges(Box::new(server_grant)),
            Change::RevokeRole(Box::new(server_role)),
        ],
    )
    .emit())
    .await;
    let server = s.db.fetch_grants(None, &login).await.expect("the login's");
    assert!(
        !server
            .statements
            .iter()
            .any(|x| x.contains("VIEW SERVER STATE") || x.contains("dbcreator")),
        "{:#?}",
        server.statements
    );

    // A reset on the user row is the login's password.
    run(account(
        &name,
        MS,
        Change::SetAccountPassword(Box::new(PasswordReset {
            account: user.clone(),
            password: "Schemaic_Pw2!".into(),
            scram_salt: None,
            password_policy: None,
        })),
    )
    .emit())
    .await;
    assert!(
        signs_in("Schemaic_Pw1!").await.is_none(),
        "the old one no longer works"
    );
    assert_eq!(
        signs_in("Schemaic_Pw2!").await.as_deref(),
        Some(name.as_str())
    );

    // The user, then the login.
    run(account(&name, MS, Change::DropAccount(Box::new(user))).emit()).await;
    run(account(&name, MS, Change::DropAccount(Box::new(login))).emit()).await;
    let list = s.db.fetch_principals(Some(&s.name)).await.unwrap().list;
    assert!(!list.iter().any(|p| p.name == name), "{list:?}");
}

/// **Every class of permission a principal holds is on its list**, not the
/// four the grant form writes. The list read the server's class `SERVER`
/// alone and dropped the rest of the database's, so a login granted
/// `IMPERSONATE ON LOGIN::sa` read as holding `CONNECT SQL`. In the database,
/// permissions on a user, a role, a type and an XML schema collection read
/// back as T-SQL, and **replaying them for another user reproduces the same
/// list** — the sentences are statements the server takes, not a rendering.
/// The server half (a login's `IMPERSONATE ON LOGIN::sa` and `CONNECT ON
/// ENDPOINT::`) is not run on Azure SQL Database, whose logins are `master`'s.
#[tokio::test(flavor = "multi_thread")]
async fn every_class_of_permission_a_principal_holds_is_listed() {
    use schemaic_core::users::PrincipalKind;
    if !enabled() {
        return;
    }
    let s = Scratch::create("perm_classes").await;
    s.exec(
        "CREATE USER grantee WITHOUT LOGIN; CREATE USER twin WITHOUT LOGIN; \
         CREATE USER other WITHOUT LOGIN; CREATE ROLE r; \
         CREATE TABLE dbo.t (id int PRIMARY KEY, v int);",
    )
    .await;
    s.exec("CREATE TYPE dbo.zt FROM int;").await;
    s.exec(
        "CREATE XML SCHEMA COLLECTION dbo.zx AS N'<xsd:schema \
         xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\"><xsd:element name=\"a\" \
         type=\"xsd:int\"/></xsd:schema>';",
    )
    .await;
    s.exec(
        "GRANT IMPERSONATE ON USER::other TO grantee; \
         GRANT ALTER ON ROLE::r TO grantee; \
         GRANT CONTROL ON TYPE::dbo.zt TO grantee; \
         GRANT REFERENCES ON XML SCHEMA COLLECTION::dbo.zx TO grantee WITH GRANT OPTION; \
         GRANT SELECT ON OBJECT::dbo.t (v) TO grantee; \
         DENY DELETE ON OBJECT::dbo.t TO grantee;",
    )
    .await;
    let list =
        s.db.fetch_principals(Some(&s.name))
            .await
            .expect("the accounts")
            .list;
    let user = |name: &str| {
        list.iter()
            .find(|p| p.name == name && p.kind == PrincipalKind::User)
            .unwrap_or_else(|| panic!("{name} in {list:#?}"))
            .clone()
    };
    let grants =
        s.db.fetch_grants(Some(&s.name), &user("grantee"))
            .await
            .expect("the grants");
    for want in [
        "GRANT IMPERSONATE ON USER::[other] TO [grantee];",
        "GRANT ALTER ON ROLE::[r] TO [grantee];",
        "GRANT CONTROL ON TYPE::[dbo].[zt] TO [grantee];",
        "GRANT REFERENCES ON XML SCHEMA COLLECTION::[dbo].[zx] TO [grantee] WITH GRANT OPTION;",
        "GRANT SELECT ON OBJECT::[dbo].[t] ([v]) TO [grantee];",
        "DENY DELETE ON OBJECT::[dbo].[t] TO [grantee];",
    ] {
        assert!(
            grants.statements.iter().any(|s| s == want),
            "{want}\n{:#?}",
            grants.statements
        );
    }
    assert_eq!(grants.note, None, "{:#?}", grants.statements);

    // Replayed for `twin`, the same list comes back.
    let replay: Vec<String> = grants
        .statements
        .iter()
        .map(|s| s.replace("[grantee]", "[twin]"))
        .collect();
    s.exec(&replay.join(" ")).await;
    let twin =
        s.db.fetch_grants(Some(&s.name), &user("twin"))
            .await
            .expect("the twin's grants");
    assert_eq!(twin.statements, replay);

    if azure_cannot("keeps its logins in master, so the server half of the grant listing") {
        return;
    }
    let name = format!("{PREFIX}{}_mssql_perm", std::process::id());
    let target = format!("{PREFIX}{}_mssql_perm2", std::process::id());
    // Dropped in reverse: the grantee first, as the server refuses to drop
    // `target` while a permission on it is recorded as its grant (Msg 15173).
    let _target = ScratchLogin(target.clone());
    let _login = ScratchLogin(name.clone());
    let base = base_db();
    base.fetch_query(
        None,
        &format!(
            "CREATE LOGIN [{name}] WITH PASSWORD = N'Schemaic_Pw1!', CHECK_POLICY = OFF; \
             CREATE LOGIN [{target}] WITH PASSWORD = N'Schemaic_Pw1!', CHECK_POLICY = OFF; \
             GRANT IMPERSONATE ON LOGIN::sa TO [{name}]; \
             GRANT CONTROL ON LOGIN::[{target}] TO [{name}]; \
             GRANT CONNECT ON ENDPOINT::[TSQL Default TCP] TO [{name}]; \
             GRANT VIEW SERVER STATE TO [{name}];"
        ),
        1,
        CancellationToken::new(),
    )
    .await
    .expect("the login and its grants");
    let logins = base.fetch_principals(None).await.expect("the logins").list;
    let login = logins
        .iter()
        .find(|p| p.name == name && p.kind == PrincipalKind::Login)
        .expect("the login")
        .clone();
    let server = base.fetch_grants(None, &login).await.expect("the login's");
    let q = format!("[{name}]");
    for want in [
        format!("GRANT IMPERSONATE ON LOGIN::[sa] TO {q};"),
        format!("GRANT CONTROL ON LOGIN::[{target}] TO {q};"),
        format!("GRANT CONNECT ON ENDPOINT::[TSQL Default TCP] TO {q};"),
        format!("GRANT VIEW SERVER STATE TO {q};"),
        format!("GRANT CONNECT SQL TO {q};"),
    ] {
        assert!(
            server.statements.contains(&want),
            "{want}\n{:#?}",
            server.statements
        );
    }
    assert_eq!(server.note, None, "{:#?}", server.statements);
}

/// **A table-level grant keeps the account's column denials.** The server's
/// own `GRANT SELECT ON OBJECT::dbo.t` deletes a `DENY SELECT` on one of the
/// table's columns — pinned here first, as the reason for the rest — so the
/// grant form's plan reads the denials and issues them again after its grant:
/// the column stays denied and the grant lands. Over a grant option the
/// server refuses the bare re-denial (Msg 4611) and the plan applies nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_table_grant_keeps_the_accounts_column_denials() {
    use schemaic_core::ddl::{Change, account};
    use schemaic_core::users::{GrantLevel, PrincipalKind, PrivilegeChange};
    if !enabled() {
        return;
    }
    let s = Scratch::create("col_deny").await;
    s.exec(
        "CREATE TABLE dbo.t (id int, ssn int, v int); INSERT dbo.t VALUES (1, 2, 3); \
         CREATE USER grantee WITHOUT LOGIN; CREATE USER plain WITHOUT LOGIN; \
         GRANT SELECT ON OBJECT::dbo.t TO grantee; GRANT SELECT ON OBJECT::dbo.t TO plain; \
         DENY SELECT ON OBJECT::dbo.t (ssn) TO grantee; DENY SELECT ON OBJECT::dbo.t (ssn) TO plain; \
         DENY UPDATE ON OBJECT::dbo.t (v) TO grantee;",
    )
    .await;
    let denied = |who: &str| {
        format!(
            "SELECT COUNT(*) FROM sys.database_permissions WHERE class = 1 AND state = 'D' \
             AND minor_id <> 0 AND major_id = OBJECT_ID(N'dbo.t') \
             AND grantee_principal_id = DATABASE_PRINCIPAL_ID(N'{who}')"
        )
    };
    // The server's behaviour, which is the reason: a plain grant lifts it.
    s.exec("GRANT SELECT ON OBJECT::dbo.t TO plain").await;
    assert_eq!(s.scalar(&denied("plain")).await, "0", "the server kept it");

    let list =
        s.db.fetch_principals(Some(&s.name))
            .await
            .expect("the accounts")
            .list;
    let grantee = list
        .iter()
        .find(|p| p.name == "grantee" && p.kind == PrincipalKind::User)
        .expect("the user")
        .clone();
    let grant = |with_grant_option: bool| PrivilegeChange {
        account: grantee.clone(),
        level: GrantLevel::Table {
            qualifier: "dbo".into(),
            name: "t".into(),
        },
        privileges: vec!["SELECT".into(), "INSERT".into(), "UPDATE".into()],
        with_grant_option,
    };
    let plan =
        |c: PrivilegeChange| account("grantee", MS, Change::GrantPrivileges(Box::new(c))).emit();
    s.db.run_ddl(&s.name, &plan(grant(false)), CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let grants =
        s.db.fetch_grants(Some(&s.name), &grantee)
            .await
            .expect("the grants");
    for want in [
        "GRANT INSERT ON OBJECT::[dbo].[t] TO [grantee];",
        "GRANT UPDATE ON OBJECT::[dbo].[t] TO [grantee];",
        "DENY SELECT ON OBJECT::[dbo].[t] ([ssn]) TO [grantee];",
        "DENY UPDATE ON OBJECT::[dbo].[t] ([v]) TO [grantee];",
    ] {
        assert!(
            grants.statements.iter().any(|x| x == want),
            "{want}\n{:#?}",
            grants.statements
        );
    }
    let read = s
        .try_exec("EXECUTE AS USER = 'grantee'; SELECT ssn FROM dbo.t; REVERT;")
        .await;
    assert!(read.is_err(), "the denied column is readable: {read:?}");

    // Grantable: the re-denial needs CASCADE, so nothing is applied.
    let refused =
        s.db.run_ddl(&s.name, &plan(grant(true)), CancellationToken::new())
            .await;
    assert!(refused.is_err(), "a grantable grant over a denial applied");
    assert_eq!(s.scalar(&denied("grantee")).await, "2", "a denial was lost");
    let grants =
        s.db.fetch_grants(Some(&s.name), &grantee)
            .await
            .expect("the grants");
    assert!(
        !grants
            .statements
            .iter()
            .any(|x| x.contains("WITH GRANT OPTION")),
        "{:#?}",
        grants.statements
    );
}

/// A scratch database made **contained** (`CONTAINMENT = PARTIAL`), or `None`
/// — noted as the leg's no-op, `what` naming what is skipped — where the
/// server's `contained database authentication` is off. A server-wide setting
/// this reads rather than changes. Azure SQL Database has no such setting,
/// every database there being contained already.
async fn contained_scratch(prefix: &str, what: &str) -> Option<Scratch> {
    let azure = on_azure();
    let allowed = azure
        || base_db()
            .fetch_query(
                None,
                "SELECT CAST(value_in_use AS int) FROM sys.configurations \
             WHERE name = 'contained database authentication'",
                1,
                CancellationToken::new(),
            )
            .await
            .expect("the setting")
            .cell(0, 0)
            .is_some_and(|c| c.display() == "1");
    if !allowed {
        endpoint::note_leg_no_op(
            "mssql",
            &format!("has contained database authentication off, so {what}"),
        );
        return None;
    }
    let s = Scratch::create(prefix).await;
    if !azure {
        base_db()
            .fetch_query(
                None,
                &format!("ALTER DATABASE [{}] SET CONTAINMENT = PARTIAL", s.name),
                1,
                CancellationToken::new(),
            )
            .await
            .expect("a contained database");
    }
    Some(s)
}

/// **A contained user, end to end.** In a database made contained, the
/// listing says so; a user created with a password of its own is listed with
/// no login and as holding its password, signs in to that database with it,
/// and after a reset on its row signs in with the new one only.
///
/// Needs the server's `contained database authentication` on — a server-wide
/// setting this test reads rather than changes, and reports when it is off.
/// Azure SQL Database has no such setting, every database there being
/// contained already; its sign-ins are left out, as an Entra-only server
/// refuses every password.
#[tokio::test(flavor = "multi_thread")]
async fn a_contained_user_is_created_signs_in_and_is_reset() {
    use schemaic_core::ddl::{Change, account};
    use schemaic_core::users::{AccountDraft, PasswordReset, PrincipalKind};
    // Nothing of the user's name in either: Azure refuses a password that
    // shares part of it (Msg 40632).
    const FIRST: &str = "Tq7#vLp2!xW9";
    const SECOND: &str = "Rm4$kNz8?bJ3";
    if !enabled() {
        return;
    }
    let azure = on_azure();
    let Some(s) = contained_scratch("contained", "the contained user round trip").await else {
        return;
    };
    let name = format!("{PREFIX}{}_mssql_cuser", std::process::id());
    let run = |stmts: Vec<String>| {
        let db = s.db.clone();
        let database = s.name.clone();
        async move {
            db.run_ddl(&database, &stmts, CancellationToken::new())
                .await
                .unwrap_or_else(|e| panic!("{e}\n{stmts:#?}"));
        }
    };
    let signs_in = |password: &'static str| {
        let db = Db::from_parts(
            Engine::MsSql,
            var("HOST", "127.0.0.1"),
            var("PORT", "1433").parse().unwrap(),
            name.clone(),
            password.to_string(),
            s.name.clone(),
        );
        let database = s.name.clone();
        async move {
            db.fetch_query(
                Some(&database),
                "SELECT USER_NAME()",
                1,
                CancellationToken::new(),
            )
            .await
            .ok()
            .and_then(|rs| rs.cell(0, 0).map(|c| c.display().to_string()))
        }
    };

    let listed =
        s.db.fetch_principals(Some(&s.name))
            .await
            .expect("the list");
    assert!(listed.scope.contained, "the listing sees the containment");
    run(account(
        &name,
        MS,
        Change::CreateAccount(Box::new(AccountDraft {
            name: name.clone(),
            kind: PrincipalKind::User,
            password: FIRST.into(),
            ..Default::default()
        })),
    )
    .emit())
    .await;
    let user =
        s.db.fetch_principals(Some(&s.name))
            .await
            .unwrap()
            .list
            .into_iter()
            .find(|p| p.name == name)
            .expect("the user");
    assert_eq!(
        (user.kind, user.login.as_deref(), user.database_password),
        (PrincipalKind::User, None, true)
    );
    if !azure {
        assert_eq!(signs_in(FIRST).await.as_deref(), Some(name.as_str()));
    }

    run(account(
        &name,
        MS,
        Change::SetAccountPassword(Box::new(PasswordReset {
            account: user,
            password: SECOND.into(),
            scram_salt: None,
            password_policy: None,
        })),
    )
    .emit())
    .await;
    if azure {
        // Past libtest's capture, as `endpoint::note_leg_no_op` writes.
        use std::io::Write as _;
        let _ = writeln!(
            std::io::stderr().lock(),
            "live: mssql-on-azure refuses every password — the contained user's sign-ins went unchecked"
        );
        return;
    }
    assert!(signs_in(FIRST).await.is_none(), "the old one");
    assert_eq!(signs_in(SECOND).await.as_deref(), Some(name.as_str()));
}

/// **What the New account form may make is read with the list.** A server
/// keeps its logins and lists them; Azure SQL Database's are `master`'s, so
/// none is listed or offered there, and a connection signed in through Entra
/// may make an Entra user — whose statement the server takes as one, answering
/// a name Entra does not know with the principal it could not find rather than
/// with a syntax error.
#[tokio::test(flavor = "multi_thread")]
async fn the_account_scope_is_read_with_the_list() {
    use schemaic_core::ddl::{Change, account};
    use schemaic_core::users::{AccountDraft, PrincipalKind};
    if !enabled() {
        return;
    }
    let s = Scratch::create("scope").await;
    let listed =
        s.db.fetch_principals(Some(&s.name))
            .await
            .expect("the list");
    let logins = listed
        .list
        .iter()
        .filter(|p| p.kind == PrincipalKind::Login)
        .count();
    if !on_azure() {
        assert!(!listed.scope.logins_elsewhere && !listed.scope.entra_users);
        assert!(logins > 0, "sa at least");
        return;
    }
    assert!(listed.scope.logins_elsewhere, "{:?}", listed.scope);
    assert!(listed.scope.entra_users, "signed in through the Azure CLI");
    assert_eq!(logins, 0, "{:#?}", listed.list);
    assert!(
        listed.note.as_deref().is_some_and(|n| n.contains("master")),
        "{:?}",
        listed.note
    );
    let stmts = account(
        "nobody",
        MS,
        Change::CreateAccount(Box::new(AccountDraft {
            name: "schemaic-it-nobody@invalid.example".into(),
            kind: PrincipalKind::User,
            external: true,
            ..Default::default()
        })),
    )
    .emit();
    let refused =
        s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
            .await
            .expect_err("no such principal");
    assert!(
        refused.to_string().contains("could not be found"),
        "{refused}\n{stmts:#?}"
    );
}

// ── Signing in without a password ───────────────────────────────────────────

/// A saved SQL Server connection signing in as `auth`, as the app builds one.
fn sign_in_connection(
    host: String,
    port: u16,
    database: &str,
    auth: schemaic_core::connection::AuthMode,
    tls: schemaic_core::connection::SslMode,
) -> schemaic_core::connection::Connection {
    use schemaic_core::connection::{Connection, Environment, SshTunnel, Tls};
    Connection {
        id: 1,
        name: "sign-in".into(),
        db_type: "SQL Server".into(),
        host,
        port,
        user: String::new(),
        password: String::new(),
        file: String::new(),
        database: database.into(),
        ssh: SshTunnel::default(),
        tls: Tls {
            mode: tls,
            ..Tls::default()
        },
        color: None,
        prominent_color: false,
        read_only: false,
        cli_access: false,
        environment: Environment::None,
        ai_data: None,
        folder: String::new(),
        auth,
    }
}

/// One scalar on `db`, in `database`.
async fn signed_in_scalar(db: &Db, database: &str, sql: &str) -> String {
    let rs = db
        .fetch_query(Some(database), sql, 10, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}\nstatement: {sql}"));
    rs.cell(0, 0)
        .map(|c| c.display().to_string())
        .unwrap_or_else(|| panic!("{sql} returned no cell"))
}

/// **Windows sign-in is this process's own identity** — SSPI, the form's
/// user and password never sent. The opt-in `mssql-windows` leg, against a
/// Windows SQL Server this process can reach:
/// `SCHEMAIC_IT_MSSQL_WINDOWS_HOST` / `_PORT`, by default `127.0.0.1` /
/// `1435` (a local Developer instance moved off 1433, which WSL's container
/// holds). The login the server sees is the Windows account running the test,
/// authenticated by NTLM or Kerberos rather than as a SQL login.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
async fn a_windows_sign_in_is_this_processs_own_identity() {
    use schemaic_core::connection::{AuthMode, SslMode};
    if !endpoint::opt_in_leg_enabled("mssql-windows") {
        endpoint::note_leg_skipped("mssql-windows");
        return;
    }
    let host = std::env::var("SCHEMAIC_IT_MSSQL_WINDOWS_HOST").unwrap_or("127.0.0.1".into());
    let port = std::env::var("SCHEMAIC_IT_MSSQL_WINDOWS_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(1435);
    // `Prefer`: the local instance's certificate is the one it generated
    // for itself, which only a mode that trusts none accepts.
    let conn = sign_in_connection(host, port, "", AuthMode::Windows, SslMode::Prefer);
    assert_eq!(conn.effective_auth(), AuthMode::Windows);
    let db = Db::connect(&conn, None);
    db.ping(std::time::Duration::from_secs(20))
        .await
        .expect("a Windows sign-in");
    let me = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").expect("USERDOMAIN"),
        std::env::var("USERNAME").expect("USERNAME")
    );
    let login = signed_in_scalar(&db, "master", "SELECT SUSER_SNAME()").await;
    assert!(login.eq_ignore_ascii_case(&me), "{login} is not {me}");
    let scheme = signed_in_scalar(
        &db,
        "master",
        "SELECT auth_scheme FROM sys.dm_exec_connections WHERE session_id = @@SPID",
    )
    .await;
    assert!(
        scheme == "NTLM" || scheme == "KERBEROS",
        "signed in by {scheme}, not by Windows"
    );
    // The same connection with its (empty) password is refused — which is
    // what makes the sign-in above the mode's doing.
    let password = Db::connect(
        &schemaic_core::connection::Connection {
            auth: AuthMode::Password,
            ..conn
        },
        None,
    );
    let refused = password
        .ping(std::time::Duration::from_secs(20))
        .await
        .expect_err("an empty SQL login");
    assert!(refused.to_string().contains("18456"), "{refused}");
}

/// **An Entra sign-in is the Azure CLI's user**, with a token the CLI mints
/// and Schemaic never stores — and the second connection is handed the
/// cached one rather than asking the CLI again. The opt-in `mssql-azure`
/// leg, against an Azure SQL database with Entra sign-in and a signed-in
/// `az`: `SCHEMAIC_IT_MSSQL_AZURE_HOST` (required, no default — it is
/// somebody's own server) and `_DATABASE` (`schemaic_it`). Verified
/// certificates, as Azure's are public.
#[tokio::test(flavor = "multi_thread")]
async fn an_entra_sign_in_is_the_azure_clis_user() {
    use schemaic_core::connection::{AuthMode, SslMode};
    if !endpoint::opt_in_leg_enabled("mssql-azure") {
        endpoint::note_leg_skipped("mssql-azure");
        return;
    }
    let host = std::env::var("SCHEMAIC_IT_MSSQL_AZURE_HOST")
        .expect("SCHEMAIC_IT_MSSQL_AZURE_HOST names the Azure SQL server for mssql-azure");
    let database =
        std::env::var("SCHEMAIC_IT_MSSQL_AZURE_DATABASE").unwrap_or("schemaic_it".into());
    let conn = sign_in_connection(
        host,
        1433,
        &database,
        AuthMode::AzureCli,
        SslMode::VerifyFull,
    );
    let db = Db::connect(&conn, None);
    let login = signed_in_scalar(&db, &database, "SELECT SUSER_SNAME()").await;
    assert!(login.contains('@'), "{login} is not an Entra user");
    // A second connection, on the cached token.
    assert_eq!(
        signed_in_scalar(&db, &database, "SELECT ORIGINAL_LOGIN()").await,
        login
    );
    // A SQL login is refused on an Entra-only server, which is what makes the
    // assertion above one about the token rather than a password.
    let password = Db::connect(
        &sign_in_connection(
            conn.host.clone(),
            1433,
            &database,
            AuthMode::Password,
            SslMode::VerifyFull,
        ),
        None,
    );
    assert!(
        password
            .fetch_query(Some(&database), "SELECT 1", 1, CancellationToken::new())
            .await
            .is_err()
    );
}

/// **An Entra connection left at `Disable` still signs in, over a verified
/// session.** The token is a bearer token for every Azure SQL database the
/// account can reach, so `Connection::tls_plan` raises the plan to
/// `VerifyFull` whatever the picker says — which Azure's public certificate
/// passes with no CA file. The `mssql-azure` leg.
#[tokio::test(flavor = "multi_thread")]
async fn an_entra_connection_left_at_disable_signs_in_verified() {
    use schemaic_core::connection::{AuthMode, SslMode};
    if !endpoint::opt_in_leg_enabled("mssql-azure") {
        endpoint::note_leg_skipped("mssql-azure");
        return;
    }
    let host = std::env::var("SCHEMAIC_IT_MSSQL_AZURE_HOST")
        .expect("SCHEMAIC_IT_MSSQL_AZURE_HOST names the Azure SQL server for mssql-azure");
    let database =
        std::env::var("SCHEMAIC_IT_MSSQL_AZURE_DATABASE").unwrap_or("schemaic_it".into());
    let conn = sign_in_connection(host, 1433, &database, AuthMode::AzureCli, SslMode::Disable);
    let db = Db::connect(&conn, None);
    assert!(
        db.tls_plan().is_some_and(|p| p.verifies_server()),
        "{:?}",
        db.tls_plan()
    );
    let encrypted = signed_in_scalar(
        &db,
        &database,
        "SELECT CAST(encrypt_option AS nvarchar(10)) FROM sys.dm_exec_connections \
         WHERE session_id = @@SPID",
    )
    .await;
    assert_eq!(encrypted, "TRUE");
}

/// **A column moved is a table rebuilt, and everything on the table comes
/// back.** The rows (identity values kept, a new column's default filling the
/// old rows), the key, the check, the unique index, the defaults under their
/// own names, the comments, a disabled trigger, and another table's foreign
/// key with its `ON DELETE` — and the identity carries on from where the old
/// table's stood, not from the highest row left. The round trip after it is
/// empty.
#[tokio::test(flavor = "multi_thread")]
async fn a_moved_column_rebuilds_the_table_and_keeps_what_stood_on_it() {
    use schemaic_core::ddl::{Change, ColumnDraft, TableDraft};
    if !enabled() {
        return;
    }
    let s = Scratch::create("rebuild").await;
    for sql in [
        "CREATE TABLE dbo.parent (id int IDENTITY(1,1) CONSTRAINT pk_parent PRIMARY KEY, \
         code nvarchar(10) NOT NULL CONSTRAINT df_code DEFAULT (N'x'), \
         qty int NULL CONSTRAINT ck_qty CHECK (qty >= 0), note nvarchar(50) NULL)",
        "CREATE UNIQUE INDEX ux_code ON dbo.parent (code)",
        "EXEC sp_addextendedproperty @name = N'MS_Description', @value = N'parents', \
         @level0type = N'SCHEMA', @level0name = N'dbo', @level1type = N'TABLE', @level1name = N'parent'",
        "EXEC sp_addextendedproperty @name = N'MS_Description', @value = N'a note', \
         @level0type = N'SCHEMA', @level0name = N'dbo', @level1type = N'TABLE', \
         @level1name = N'parent', @level2type = N'COLUMN', @level2name = N'note'",
        "CREATE TRIGGER dbo.tr_parent ON dbo.parent AFTER INSERT AS SET NOCOUNT ON",
        "DISABLE TRIGGER dbo.tr_parent ON dbo.parent",
        "INSERT dbo.parent (code, qty, note) VALUES (N'a', 1, N'n1'), (N'b', 2, N'n2'), (N'c', 3, N'n3')",
        "DELETE dbo.parent WHERE id = 3",
        "CREATE TABLE dbo.child (id int PRIMARY KEY, parent_id int \
         CONSTRAINT fk_child_parent REFERENCES dbo.parent (id) ON DELETE CASCADE)",
        "INSERT dbo.child VALUES (1, 2)",
    ] {
        s.exec(sql).await;
    }
    let t = read_table(&s, "parent").await;
    assert_eq!(t.referenced_by.len(), 1, "{:?}", t.referenced_by);
    let mut d = TableDraft::from_table(&t);
    let note = d.columns.remove(3);
    d.columns.insert(0, note);
    d.columns
        .push(ColumnDraft::new(schemaic_core::schema::ColumnInfo {
            name: "added".into(),
            type_name: "int".into(),
            nullable: false,
            default: Some("7".into()),
            ..Default::default()
        }));
    let cs = schemaic_core::ddl::diff(&t, &d, MS);
    assert!(
        matches!(cs.changes.first(), Some(Change::RebuildTable(_))),
        "{:#?}",
        cs.changes
    );
    apply_draft(&s, &t, &d).await;

    let t2 = read_table(&s, "parent").await;
    let names: Vec<&str> = t2.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["note", "id", "code", "qty", "added"]);
    assert_eq!(
        s.scalar(
            "SELECT STRING_AGG(CONCAT(id, ':', code, ':', note, ':', added), ',') \
             WITHIN GROUP (ORDER BY id) FROM dbo.parent"
        )
        .await,
        "1:a:n1:7,2:b:n2:7"
    );
    assert_eq!(
        s.scalar(
            "SELECT STRING_AGG(name, ',') WITHIN GROUP (ORDER BY name) \
             FROM sys.objects WHERE parent_object_id = OBJECT_ID(N'dbo.parent')"
        )
        .await
        .split(',')
        .filter(|n| !n.starts_with("DF__"))
        .collect::<Vec<_>>(),
        ["ck_qty", "df_code", "pk_parent", "tr_parent"]
    );
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM sys.indexes WHERE name = N'ux_code'")
            .await,
        "1"
    );
    assert_eq!(
        s.scalar("SELECT CAST(is_disabled AS int) FROM sys.triggers WHERE name = N'tr_parent'")
            .await,
        "1"
    );
    assert_eq!(t2.comment.as_deref(), Some("parents"));
    assert_eq!(t2.columns[0].comment.as_deref(), Some("a note"));
    assert_eq!(
        s.scalar(
            "SELECT delete_referential_action_desc FROM sys.foreign_keys \
             WHERE name = N'fk_child_parent' AND referenced_object_id = OBJECT_ID(N'dbo.parent')"
        )
        .await,
        "CASCADE"
    );
    s.exec("INSERT dbo.parent (code) VALUES (N'd')").await;
    assert_eq!(
        s.scalar("SELECT id FROM dbo.parent WHERE code = N'd'")
            .await,
        "4",
        "the identity carries on past the deleted row"
    );
    let again = schemaic_core::ddl::diff(&t2, &TableDraft::from_table(&t2), MS);
    assert!(again.changes.is_empty(), "{:#?}", again.changes);
}

/// **An identity switched on is a rebuild too**, keeping the values the rows
/// had and numbering on from the highest.
#[tokio::test(flavor = "multi_thread")]
async fn an_identity_switched_on_keeps_the_rows_values() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() {
        return;
    }
    let s = Scratch::create("rebuild_id").await;
    s.exec("CREATE TABLE dbo.t (id int NOT NULL CONSTRAINT pk_t PRIMARY KEY, v int)")
        .await;
    s.exec("INSERT dbo.t VALUES (5, 50), (9, 90)").await;
    let t = read_table(&s, "t").await;
    let mut d = TableDraft::from_table(&t);
    d.columns[0].info.auto_increment = true;
    apply_draft(&s, &t, &d).await;
    s.exec("INSERT dbo.t (v) VALUES (100)").await;
    assert_eq!(
        s.scalar(
            "SELECT STRING_AGG(CONCAT(id, ':', v), ',') WITHIN GROUP (ORDER BY id) FROM dbo.t"
        )
        .await,
        "5:50,9:90,10:100"
    );
    let t2 = read_table(&s, "t").await;
    assert!(t2.columns[0].auto_increment);
}

/// **A rebuild hands out no identity value the old table already issued** —
/// not when every row was deleted, where the shadow has had no insert and
/// `DBCC CHECKIDENT` makes the reseed value itself the next one, so the id
/// last issued came round again (S3.2-L1-01); and not for a descending
/// identity, which the reseed used to skip. Nor for a table never inserted
/// into, which starts at its seed as before.
#[tokio::test(flavor = "multi_thread")]
async fn a_rebuild_reissues_no_identity_value() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() {
        return;
    }
    let s = Scratch::create("rebuild_reseed").await;
    // (table, identity, rows inserted, rows left, the next id wanted)
    let cases = [
        (
            "emptied",
            "IDENTITY(1,1)",
            "(1, 1), (2, 2), (3, 3)",
            "DELETE dbo.emptied",
            "4",
        ),
        (
            "kept",
            "IDENTITY(1,1)",
            "(1, 1), (2, 2), (3, 3)",
            "DELETE dbo.kept WHERE a = 3",
            "4",
        ),
        (
            "down",
            "IDENTITY(-1,-1)",
            "(1, 1), (2, 2), (3, 3)",
            "DELETE dbo.down WHERE a > 1",
            "-4",
        ),
        ("fresh", "IDENTITY(1,1)", "", "", "1"),
    ];
    for (table, identity, rows, delete, next) in cases {
        s.exec(&format!(
            "CREATE TABLE dbo.{table} (id int {identity} PRIMARY KEY, a int NULL, b int NULL)"
        ))
        .await;
        if !rows.is_empty() {
            s.exec(&format!("INSERT dbo.{table} (a, b) VALUES {rows}"))
                .await;
            s.exec(delete).await;
        }
        let t = read_table(&s, table).await;
        let mut d = TableDraft::from_table(&t);
        d.columns.swap(1, 2);
        apply_draft(&s, &t, &d).await;
        s.exec(&format!("INSERT dbo.{table} (a, b) VALUES (9, 9)"))
            .await;
        assert_eq!(
            s.scalar(&format!("SELECT id FROM dbo.{table} WHERE a = 9"))
                .await,
            next,
            "{table}"
        );
    }
}

/// **The guard stops a rebuild before anything runs** where the table holds
/// what the model does not — here a permission granted on it, which
/// `DROP TABLE` would take — and the table is left as it was.
#[tokio::test(flavor = "multi_thread")]
async fn a_rebuild_is_refused_where_the_table_has_what_it_would_drop() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() {
        return;
    }
    let s = Scratch::create("rebuild_guard").await;
    s.exec("CREATE TABLE dbo.t (a int, b int)").await;
    s.exec("INSERT dbo.t VALUES (1, 2)").await;
    s.exec("GRANT SELECT ON dbo.t TO public").await;
    let t = read_table(&s, "t").await;
    let mut d = TableDraft::from_table(&t);
    d.columns.swap(0, 1);
    let stmts = schemaic_core::ddl::diff(&t, &d, MS).emit();
    let refused =
        s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
            .await
            .expect_err("refused");
    assert!(
        refused
            .to_string()
            .contains("permissions are granted on it"),
        "{refused}"
    );
    let t2 = read_table(&s, "t").await;
    assert_eq!(t2.columns[0].name, "a");
    assert_eq!(s.scalar("SELECT COUNT(*) FROM dbo.t").await, "1");
}

/// **The other rebuilds, at once, under a new name**: an identity switched
/// off, a column turned computed, and a key the table has on itself — which
/// goes down with it and comes back pointed at the new name.
#[tokio::test(flavor = "multi_thread")]
async fn a_rebuild_under_a_new_name_switches_off_an_identity_and_computes_a_column() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() {
        return;
    }
    let s = Scratch::create("rebuild_more").await;
    s.exec(
        "CREATE TABLE dbo.node (id int IDENTITY(1,1) CONSTRAINT pk_node PRIMARY KEY, \
         up int NULL CONSTRAINT fk_node_up REFERENCES dbo.node (id), \
         qty int NOT NULL, twice int NULL)",
    )
    .await;
    s.exec("INSERT dbo.node (up, qty, twice) VALUES (NULL, 2, 0), (1, 3, 0)")
        .await;
    let t = read_table(&s, "node").await;
    let mut d = TableDraft::from_table(&t);
    d.name = "tree".into();
    d.columns[0].info.auto_increment = false;
    d.columns[3].info.generated = Some("[qty]*(2)".into());
    apply_draft(&s, &t, &d).await;
    let t2 = read_table(&s, "tree").await;
    assert!(!t2.columns[0].auto_increment);
    assert!(t2.columns[3].generated.is_some());
    assert_eq!(
        s.scalar("SELECT STRING_AGG(CONCAT(id, ':', up, ':', twice), ',') WITHIN GROUP (ORDER BY id) FROM dbo.tree")
            .await,
        "1::4,2:1:6"
    );
    assert_eq!(
        t2.foreign_keys
            .iter()
            .map(|f| (f.name.as_str(), f.ref_table.as_str()))
            .collect::<Vec<_>>(),
        [("fk_node_up", "tree")]
    );
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM sys.objects WHERE name = N'node'")
            .await,
        "0"
    );
    let again = schemaic_core::ddl::diff(&t2, &TableDraft::from_table(&t2), MS);
    assert!(again.changes.is_empty(), "{:#?}", again.changes);
}

/// The views and the inline function over `dbo.<table>` that select `*` — a
/// view, a view in another schema over that view, an inline function, and a
/// view over a synonym of the table — which SQL Server binds to the table's
/// columns **by position** until they are refreshed. Each is in [`STARS`].
async fn star_dependents(s: &Scratch, table: &str) {
    for sql in [
        format!("CREATE VIEW dbo.{table}_v AS SELECT * FROM dbo.{table}"),
        "CREATE SCHEMA rpt".to_string(),
        format!("CREATE VIEW rpt.{table}_vv AS SELECT * FROM dbo.{table}_v"),
        format!(
            "CREATE FUNCTION dbo.{table}_f() RETURNS TABLE AS RETURN SELECT * FROM dbo.{table}"
        ),
        format!("CREATE SYNONYM dbo.{table}_sy FOR dbo.{table}"),
        format!("CREATE VIEW dbo.{table}_sv AS SELECT * FROM dbo.{table}_sy"),
    ] {
        s.exec(&sql).await;
    }
}

/// What [`star_dependents`] makes over `dbo.<table>`, as each is selected
/// from: `{}` is the table's name.
const STARS: [&str; 4] = ["dbo.{}_v", "rpt.{}_vv", "dbo.{}_f()", "dbo.{}_sv"];

/// [`STARS`] over `table`.
fn stars(table: &str) -> Vec<String> {
    STARS.iter().map(|s| s.replace("{}", table)).collect()
}

/// **A rebuild refreshes what selects `*` from the table.** Moving
/// `credit_limit` before `balance` left a `SELECT *` view bound to the old
/// positions: it showed each column under the other's name, and an `UPDATE`
/// of `balance` through it zeroed `credit_limit` (R3-L5-02).
///
/// **Found where the deprecated catalogue keeps no row** (R3-L5-01,
/// S2-L5-01): the table is first rebuilt the way SSMS's designer does it
/// (`SELECT … INTO`, `DROP TABLE`, `sp_rename`), which leaves
/// `sys.sql_dependencies` empty for every view over it; and a view selects
/// `*` through a synonym, whose row names the synonym. Each was left reading
/// the old positions. (A view in another database is not refreshed, and the
/// preview says so — `tsql_collect_star_dependents` has why.)
#[tokio::test(flavor = "multi_thread")]
async fn a_rebuild_refreshes_the_views_that_select_star_from_it() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() {
        return;
    }
    let s = Scratch::create("rebuild_star").await;
    s.exec("CREATE TABLE dbo.acct (id int PRIMARY KEY, balance int, credit_limit int)")
        .await;
    s.exec("INSERT dbo.acct VALUES (1, 1000, 50)").await;
    star_dependents(&s, "acct").await;
    // Rebuilt outside Schemaic, in the same shape.
    s.exec(
        "SELECT * INTO dbo.acct_tmp FROM dbo.acct; DROP TABLE dbo.acct; \
         EXEC sp_rename N'dbo.acct_tmp', N'acct'; \
         ALTER TABLE dbo.acct ADD CONSTRAINT pk_acct PRIMARY KEY (id)",
    )
    .await;
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM sys.sql_dependencies WHERE referenced_major_id = OBJECT_ID(N'dbo.acct')")
            .await,
        "0",
        "the deprecated catalogue has nothing to find them by"
    );
    let t = read_table(&s, "acct").await;
    let mut d = TableDraft::from_table(&t);
    d.columns.swap(1, 2);
    apply_draft(&s, &t, &d).await;
    for from in stars("acct") {
        assert_eq!(
            s.scalar(&format!(
                "SELECT CONCAT(balance, ':', credit_limit) FROM {from}"
            ))
            .await,
            "1000:50",
            "{from} reads each column under its own name"
        );
    }
    s.exec("UPDATE dbo.acct_v SET balance = 0 WHERE id = 1")
        .await;
    assert_eq!(
        s.scalar("SELECT CONCAT(balance, ':', credit_limit) FROM dbo.acct")
            .await,
        "0:50",
        "the write through the view landed on balance"
    );
    // A rebuild under a new name leaves its dependents naming the old one, as
    // any rename does, and does not fail on refreshing them.
    let t = read_table(&s, "acct").await;
    let mut d = TableDraft::from_table(&t);
    d.columns.swap(1, 2);
    d.name = "acct2".into();
    apply_draft(&s, &t, &d).await;
    assert!(s.try_exec("SELECT * FROM dbo.acct_v").await.is_err());
}

/// **A column move applies in a contained database.** Its catalogue's
/// collation is always `Latin1_General_100_CI_AS_KS_WS_SC` whatever the
/// database's, and the `SELECT *` collector's work table was declared
/// `COLLATE DATABASE_DEFAULT`, so its join with the catalogue was Msg 468 and
/// every rebuild, rebuilt computed column and dropped column there failed.
#[tokio::test(flavor = "multi_thread")]
async fn a_column_move_applies_in_a_contained_database() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() {
        return;
    }
    let Some(s) = contained_scratch("contained_star", "the contained column move").await else {
        return;
    };
    s.exec("CREATE TABLE dbo.acct (id int PRIMARY KEY, balance int, credit_limit int)")
        .await;
    s.exec("INSERT dbo.acct VALUES (1, 1000, 50)").await;
    s.exec("CREATE VIEW dbo.acct_v AS SELECT * FROM dbo.acct")
        .await;
    let t = read_table(&s, "acct").await;
    let mut d = TableDraft::from_table(&t);
    d.columns.swap(1, 2);
    apply_draft(&s, &t, &d).await;
    assert_eq!(
        s.scalar("SELECT CONCAT(balance, ':', credit_limit) FROM dbo.acct_v")
            .await,
        "1000:50",
        "the view was refreshed"
    );
}

/// **Another database's `SELECT *` views are refreshed after the plan
/// commits** (R3-L5-01). A view in another database selecting `*` from the
/// table by a three-part name, one through a synonym there, one over that
/// view, and one over the table's own database's `*` view each kept the old
/// positions after a column move — `UPDATE xv SET balance = 0` landed on
/// `credit_limit` — while the preview said only that they would. The batch
/// the plan carries for after its commit refreshes each, leaves one that
/// names its columns alone, and names the one it could not refresh: in a
/// read-only database.
#[tokio::test(flavor = "multi_thread")]
async fn a_column_move_refreshes_other_databases_views_after_it_commits() {
    use schemaic_core::ddl::{TableDraft, diff, elsewhere_report};
    if !enabled() || azure_cannot("has no cross-database reference to refresh") {
        return;
    }
    let a = Scratch::create("xdb_src").await;
    let b = Scratch::create("xdb_views").await;
    let c = Scratch::create("xdb_ro").await;
    a.exec("CREATE TABLE dbo.acct (id int PRIMARY KEY, balance int, credit_limit int)")
        .await;
    a.exec("INSERT dbo.acct VALUES (1, 1000, 50)").await;
    a.exec("CREATE VIEW dbo.acct_v AS SELECT * FROM dbo.acct")
        .await;
    let src = format!("[{}].dbo", a.name);
    for sql in [
        format!("CREATE VIEW dbo.xv AS SELECT * FROM {src}.acct"),
        format!("CREATE SYNONYM dbo.s_acct FOR {src}.acct"),
        "CREATE VIEW dbo.sv AS SELECT * FROM dbo.s_acct".to_string(),
        "CREATE VIEW dbo.vv AS SELECT * FROM dbo.xv".to_string(),
        format!("CREATE VIEW dbo.over_local AS SELECT * FROM {src}.acct_v"),
        format!("CREATE VIEW dbo.named AS SELECT id, balance FROM {src}.acct"),
    ] {
        b.exec(&sql).await;
    }
    c.exec(&format!("CREATE VIEW dbo.rv AS SELECT * FROM {src}.acct"))
        .await;
    let read_only = |on: bool| {
        let name = c.name.clone();
        async move {
            base_db()
                .fetch_query(
                    None,
                    &format!(
                        "ALTER DATABASE [{name}] SET {} WITH ROLLBACK IMMEDIATE",
                        if on { "READ_ONLY" } else { "READ_WRITE" }
                    ),
                    1,
                    CancellationToken::new(),
                )
                .await
                .expect("the database's mode");
        }
    };
    read_only(true).await;

    let t = read_table(&a, "acct").await;
    let mut d = TableDraft::from_table(&t);
    d.columns.swap(1, 2);
    let batch = diff(&t, &d, MS)
        .refresh_elsewhere()
        .expect("a plan that moves columns refreshes elsewhere");
    apply_draft(&a, &t, &d).await;
    let rows = a.db.refresh_elsewhere(&a.name, &batch).await;
    read_only(false).await;
    let rows = rows.expect("the refresh ran");
    let of = |db: &str| -> Vec<(String, bool)> {
        rows.iter()
            .filter(|r| r.database == db)
            .filter_map(|r| r.module.clone().map(|m| (m, r.error.is_none())))
            .collect()
    };
    let mut refreshed = of(&b.name);
    refreshed.sort();
    assert_eq!(
        refreshed,
        [
            ("[dbo].[over_local]".to_string(), true),
            ("[dbo].[sv]".to_string(), true),
            ("[dbo].[vv]".to_string(), true),
            ("[dbo].[xv]".to_string(), true),
        ],
        "every * view over it, and not the one naming its columns: {rows:?}"
    );
    for view in ["xv", "sv", "vv", "over_local"] {
        assert_eq!(
            b.scalar(&format!(
                "SELECT CONCAT(balance, ':', credit_limit) FROM dbo.{view}"
            ))
            .await,
            "1000:50",
            "{view} reads each column under its own name"
        );
    }
    b.exec("UPDATE dbo.xv SET balance = 0 WHERE id = 1").await;
    assert_eq!(
        a.scalar("SELECT CONCAT(balance, ':', credit_limit) FROM dbo.acct")
            .await,
        "0:50",
        "the write through the other database's view landed on balance"
    );
    assert_eq!(of(&c.name), [("[dbo].[rv]".to_string(), false)], "{rows:?}");
    let report = elsewhere_report("acct", &rows, None).expect("a failure to say");
    assert!(
        report.contains(&format!("{}.[dbo].[rv] (", c.name)) && report.contains("read-only"),
        "{report}"
    );
}

/// **A computed column rebuilt around a rename refreshes what selects `*`.**
/// Dropping `c` and adding it back moved it last, and a `SELECT *` view kept
/// the old positions: it read `x` as `c`, and an `UPDATE` of `x` through it
/// wrote `y` (S3.2-L5-01).
#[tokio::test(flavor = "multi_thread")]
async fn a_rebuilt_computed_column_refreshes_the_views_that_select_star() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() {
        return;
    }
    let s = Scratch::create("computed_star").await;
    s.exec("CREATE TABLE dbo.t4 (id int PRIMARY KEY, a int, c AS (a * 2), x int, y int)")
        .await;
    s.exec("INSERT dbo.t4 (id, a, x, y) VALUES (1, 10, 111, 222)")
        .await;
    star_dependents(&s, "t4").await;
    let t = read_table(&s, "t4").await;
    let mut d = TableDraft::from_table(&t);
    d.columns[1].info.name = "a2".into();
    apply_draft(&s, &t, &d).await;
    for from in stars("t4") {
        assert_eq!(
            s.scalar(&format!(
                "SELECT CONCAT(a2, ':', c, ':', x, ':', y) FROM {from}"
            ))
            .await,
            "10:20:111:222",
            "{from} reads each column under its own name"
        );
    }
    s.exec("UPDATE dbo.t4_v SET x = 999 WHERE id = 1").await;
    assert_eq!(
        s.scalar("SELECT CONCAT(x, ':', y) FROM dbo.t4").await,
        "999:222",
        "the write through the view landed on x"
    );
}

/// **A column dropped and another added in one plan refreshes what selects
/// `*`**: the column count is unchanged, so a view bound by position read the
/// new column's values under the dropped one's name, silently — a view over
/// a synonym of the table included (S2-L5-01), which wrote `c` into `e`.
#[tokio::test(flavor = "multi_thread")]
async fn a_dropped_column_refreshes_the_views_that_select_star() {
    use schemaic_core::ddl::{ColumnDraft, TableDraft};
    if !enabled() {
        return;
    }
    let s = Scratch::create("drop_star").await;
    s.exec("CREATE TABLE dbo.t (id int PRIMARY KEY, a int, b int)")
        .await;
    s.exec("INSERT dbo.t VALUES (1, 10, 20)").await;
    star_dependents(&s, "t").await;
    let t = read_table(&s, "t").await;
    let mut d = TableDraft::from_table(&t);
    d.columns.remove(2);
    d.columns
        .push(ColumnDraft::new(schemaic_core::schema::ColumnInfo {
            name: "e".into(),
            type_name: "int".into(),
            nullable: true,
            default: Some("99".into()),
            ..Default::default()
        }));
    apply_draft(&s, &t, &d).await;
    for from in stars("t") {
        assert_eq!(
            s.scalar(&format!(
                "SELECT STRING_AGG(c.name, ',') WITHIN GROUP (ORDER BY c.column_id) \
                 FROM sys.columns c WHERE c.object_id = OBJECT_ID(N'{}')",
                from.trim_end_matches("()")
            ))
            .await,
            "id,a,e",
            "{from} names the table's columns as they now are"
        );
    }
    assert_eq!(s.scalar("SELECT e FROM dbo.t_v").await, "99");
}

/// Diff `draft` against `current` and run it, expecting the server to refuse
/// it: the refusal's text.
async fn refused_draft(
    s: &Scratch,
    current: &schemaic_core::schema::TableInfo,
    draft: &schemaic_core::ddl::TableDraft,
) -> String {
    let stmts = schemaic_core::ddl::diff(current, draft, MS).emit();
    s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
        .await
        .map(|()| panic!("applied:\n{}", stmts.join("\n")))
        .unwrap_err()
        .to_string()
}

/// **`ALTER COLUMN` is refused over a masked or sparse column** (S3.1-L1-01):
/// it resets both, so making a masked column `NOT NULL`, or retyping a sparse
/// one, left the first readable in the clear and the second dense, with the
/// plan reporting success. Now it stops before it starts, the column keeps
/// both, and a column with neither still changes.
#[tokio::test(flavor = "multi_thread")]
async fn a_masked_or_sparse_column_is_not_altered_in_place() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() {
        return;
    }
    let s = Scratch::create("masked").await;
    s.exec(
        "CREATE TABLE dbo.sp (id int PRIMARY KEY, s int SPARSE NULL, \
         m varchar(100) MASKED WITH (FUNCTION = 'email()') NULL, plain int NULL)",
    )
    .await;
    s.exec("INSERT dbo.sp VALUES (1, 2, 'alice@example.com', 3)")
        .await;
    let t = read_table(&s, "sp").await;
    type Edit = fn(&mut schemaic_core::schema::ColumnInfo);
    let edits: [(&str, Edit); 3] = [
        ("m", |c| c.nullable = false),
        ("m", |c| c.type_name = "varchar(200)".into()),
        ("s", |c| c.type_name = "bigint".into()),
    ];
    for (col, edit) in edits {
        let mut d = TableDraft::from_table(&t);
        edit(
            &mut d
                .columns
                .iter_mut()
                .find(|c| c.info.name == col)
                .unwrap()
                .info,
        );
        let refused = refused_draft(&s, &t, &d).await;
        assert!(
            refused.contains(&format!("column {col} is masked or sparse")),
            "{refused}"
        );
    }
    assert_eq!(
        s.scalar(
            "SELECT CONCAT(SUM(CAST(is_masked AS int)), ':', SUM(CAST(is_sparse AS int))) \
             FROM sys.columns WHERE object_id = OBJECT_ID(N'dbo.sp')"
        )
        .await,
        "1:1"
    );
    let mut d = TableDraft::from_table(&t);
    d.columns[3].info.type_name = "bigint".into();
    apply_draft(&s, &t, &d).await;
    assert_eq!(read_table(&s, "sp").await.columns[3].type_name, "bigint");
}

/// **A dependent a retype takes off is not put back without what the model
/// does not read** (S3.2-L5-02): an index came back without its
/// `IGNORE_DUP_KEY`, fill factor, page locks, compression, disabled state or
/// description, a key without its fill factor, and a foreign key without its
/// `NOT FOR REPLICATION` — each silently. One table per case; each plan is
/// refused naming what it would have re-created, and the column keeps its
/// type. (A disabled, untrusted key is re-added as it was, so it is not
/// refused — [`a_retype_under_a_disabled_or_untrusted_key_keeps_its_state`].)
///
/// **Every arm of the guard, run** (S2-L6-01): a check's `NOT FOR
/// REPLICATION` and description, a key's description, a unique constraint's
/// description and a partition scheme are here as well as the index shapes,
/// since a catalogue predicate that looks right and misses fails open with
/// every SQL-text needle still green — the shape S3.2-L5-03 was.
#[tokio::test(flavor = "multi_thread")]
async fn a_retype_is_refused_where_a_dependent_carries_what_it_would_drop() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() {
        return;
    }
    let s = Scratch::create("recreate_guard").await;
    let cases: [(&str, &str, &str); 13] = [
        (
            "t_dup",
            "CREATE UNIQUE INDEX ux ON dbo.t_dup (a) WITH (IGNORE_DUP_KEY = ON)",
            "index ux",
        ),
        (
            "t_ff",
            "CREATE INDEX ix ON dbo.t_ff (a) WITH (FILLFACTOR = 60)",
            "index ix",
        ),
        (
            "t_locks",
            "CREATE INDEX ix ON dbo.t_locks (a) WITH (ALLOW_PAGE_LOCKS = OFF)",
            "index ix",
        ),
        (
            "t_zip",
            "CREATE INDEX ix ON dbo.t_zip (a) WITH (DATA_COMPRESSION = ROW)",
            "index ix",
        ),
        (
            "t_off",
            "CREATE INDEX ix ON dbo.t_off (a); ALTER INDEX ix ON dbo.t_off DISABLE",
            "index ix",
        ),
        (
            "t_doc",
            "CREATE INDEX ix ON dbo.t_doc (a); \
             EXEC sp_addextendedproperty N'MS_Description', N'by a', N'SCHEMA', N'dbo', \
             N'TABLE', N't_doc', N'INDEX', N'ix'",
            "index ix",
        ),
        (
            "t_fk",
            "ALTER TABLE dbo.t_fk ADD CONSTRAINT fk_b FOREIGN KEY (b) \
             REFERENCES dbo.t_fk (a) NOT FOR REPLICATION",
            "foreign key fk_b",
        ),
        (
            "t_fkdoc",
            "ALTER TABLE dbo.t_fkdoc ADD CONSTRAINT fk_doc FOREIGN KEY (b) \
             REFERENCES dbo.t_fkdoc (a); \
             EXEC sp_addextendedproperty N'MS_Description', N'up', N'SCHEMA', N'dbo', \
             N'TABLE', N't_fkdoc', N'CONSTRAINT', N'fk_doc'",
            "foreign key fk_doc",
        ),
        (
            "t_cknfr",
            "ALTER TABLE dbo.t_cknfr ADD CONSTRAINT ck_nfr CHECK NOT FOR REPLICATION (b > 0)",
            "check ck_nfr",
        ),
        (
            "t_ckdoc",
            "ALTER TABLE dbo.t_ckdoc ADD CONSTRAINT ck_doc CHECK (b > 0); \
             EXEC sp_addextendedproperty N'MS_Description', N'positive', N'SCHEMA', N'dbo', \
             N'TABLE', N't_ckdoc', N'CONSTRAINT', N'ck_doc'",
            "check ck_doc",
        ),
        (
            // A description on the unique *constraint*, which is class 1 on
            // the constraint's id rather than class 7 on the index's.
            "t_uqdoc",
            "ALTER TABLE dbo.t_uqdoc ADD CONSTRAINT uq_doc UNIQUE (b); \
             EXEC sp_addextendedproperty N'MS_Description', N'one each', N'SCHEMA', N'dbo', \
             N'TABLE', N't_uqdoc', N'CONSTRAINT', N'uq_doc'",
            "index uq_doc",
        ),
        (
            "t_part",
            "CREATE PARTITION FUNCTION pf_t_part (int) AS RANGE LEFT FOR VALUES (10); \
             CREATE PARTITION SCHEME ps_t_part AS PARTITION pf_t_part ALL TO ([PRIMARY]); \
             CREATE INDEX ix ON dbo.t_part (a) ON ps_t_part (a)",
            "index ix",
        ),
        (
            "t_pk",
            "ALTER TABLE dbo.t_pk DROP CONSTRAINT pk_t_pk; \
             ALTER TABLE dbo.t_pk ADD CONSTRAINT pk_t_pk PRIMARY KEY (a) WITH (FILLFACTOR = 70)",
            "primary key",
        ),
    ];
    for (table, setup, names) in cases {
        s.exec(&format!(
            "CREATE TABLE dbo.{table} (a int NOT NULL CONSTRAINT pk_{table} PRIMARY KEY NONCLUSTERED, \
             b int NULL)"
        ))
        .await;
        // Both columns retyped, so a key from `b` to `a` still matches; the
        // key on `a` is re-created in every case, and carries nothing but in
        // the one about it.
        s.exec(setup).await;
        let t = read_table(&s, table).await;
        let mut d = TableDraft::from_table(&t);
        d.columns[0].info.type_name = "bigint".into();
        d.columns[1].info.type_name = "bigint".into();
        let refused = refused_draft(&s, &t, &d).await;
        assert!(refused.contains(names), "{table}: {refused}");
        assert_eq!(
            read_table(&s, table).await.columns[0].type_name,
            "int",
            "{table} unchanged"
        );
    }
}

/// **A computed column rebuilt around a rename is refused where it carries
/// what `DROP COLUMN` takes** (S2-L5-03): renaming `salary` drops and re-adds
/// `annual AS (salary * 12)`, and a `DENY SELECT` on `annual` went with it —
/// the plan succeeded and the denied user read the column. A column `GRANT`
/// and an extended property other than its description go the same way (a
/// sensitivity classification cannot stand on a computed column, Msg 16111).
/// One table per case, each refused naming the column and left as it was;
/// one carrying only its description, which the plan puts back, still
/// renames.
#[tokio::test(flavor = "multi_thread")]
async fn a_rebuilt_computed_column_is_refused_where_it_carries_what_the_drop_takes() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() {
        return;
    }
    let s = Scratch::create("computed_guard").await;
    s.exec("CREATE USER u_reader WITHOUT LOGIN").await;
    let cases: [(&str, &str); 4] = [
        (
            "c_deny",
            "GRANT SELECT ON dbo.c_deny TO u_reader; \
             DENY SELECT ON dbo.c_deny (annual) TO u_reader",
        ),
        (
            "c_grant",
            "GRANT SELECT ON dbo.c_grant (annual) TO u_reader",
        ),
        (
            "c_prop",
            "EXEC sp_addextendedproperty N'Owner', N'payroll', N'SCHEMA', N'dbo', \
             N'TABLE', N'c_prop', N'COLUMN', N'annual'",
        ),
        (
            "c_doc",
            "EXEC sp_addextendedproperty N'MS_Description', N'yearly', N'SCHEMA', N'dbo', \
             N'TABLE', N'c_doc', N'COLUMN', N'annual'",
        ),
    ];
    for (table, setup) in cases {
        s.exec(&format!(
            "CREATE TABLE dbo.{table} (id int PRIMARY KEY, salary int, annual AS (salary * 12)); \
             INSERT dbo.{table} (id, salary) VALUES (1, 1000)"
        ))
        .await;
        s.exec(setup).await;
        let t = read_table(&s, table).await;
        let mut d = TableDraft::from_table(&t);
        d.columns[1].info.name = "base_salary".into();
        if table == "c_doc" {
            apply_draft(&s, &t, &d).await;
            assert_eq!(
                read_table(&s, table).await.columns[2].comment.as_deref(),
                Some("yearly")
            );
            continue;
        }
        let refused = refused_draft(&s, &t, &d).await;
        assert!(
            refused.contains("computed column annual would drop its permissions"),
            "{table}: {refused}"
        );
        assert_eq!(
            read_table(&s, table).await.columns[1].name,
            "salary",
            "{table} unchanged"
        );
    }
    assert_eq!(
        s.scalar(
            "SELECT COUNT(*) FROM sys.database_permissions \
             WHERE major_id = OBJECT_ID(N'dbo.c_deny') AND minor_id <> 0"
        )
        .await,
        "1",
        "the DENY stands"
    );
}

/// **A retype under a disabled or untrusted foreign key applies, and the key
/// comes back as it was** (R2-L8-06). The in-place plan re-adds such a key
/// `WITH NOCHECK` and disables it again, yet the guard still refused it
/// saying the re-add "would enable and trust it". Each table keeps a row its
/// key does not hold, which a re-add that validated would refuse.
#[tokio::test(flavor = "multi_thread")]
async fn a_retype_under_a_disabled_or_untrusted_key_keeps_its_state() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() {
        return;
    }
    let s = Scratch::create("nocheck_retype").await;
    for (table, disable, state) in [("t_off", true, "1:1"), ("t_untrusted", false, "0:1")] {
        s.exec(&format!(
            "CREATE TABLE dbo.{table} (a int NOT NULL CONSTRAINT pk_{table} PRIMARY KEY, \
             b int NULL); INSERT dbo.{table} VALUES (1, 5); \
             ALTER TABLE dbo.{table} WITH NOCHECK ADD CONSTRAINT fk_{table} FOREIGN KEY (b) \
             REFERENCES dbo.{table} (a)"
        ))
        .await;
        if disable {
            s.exec(&format!(
                "ALTER TABLE dbo.{table} NOCHECK CONSTRAINT fk_{table}"
            ))
            .await;
        }
        let t = read_table(&s, table).await;
        let mut d = TableDraft::from_table(&t);
        d.columns[0].info.type_name = "bigint".into();
        d.columns[1].info.type_name = "bigint".into();
        apply_draft(&s, &t, &d).await;
        assert_eq!(read_table(&s, table).await.columns[1].type_name, "bigint");
        assert_eq!(
            s.scalar(&format!(
                "SELECT CONCAT(CAST(is_disabled AS int), ':', CAST(is_not_trusted AS int)) \
                 FROM sys.foreign_keys WHERE name = N'fk_{table}'"
            ))
            .await,
            state,
            "{table}: the key keeps its disabled and untrusted states"
        );
        assert_eq!(
            s.scalar(&format!("SELECT CONCAT(a, ':', b) FROM dbo.{table}"))
                .await,
            "1:5"
        );
    }
}

/// **Every arm of the rebuild's guard, run** (S3.2-L6-01). The guard is the
/// rebuild's whole defence against a silent loss, and an arm whose catalogue
/// predicate is wrong fails open — S3.2-L5-03 was exactly one that looked
/// right and missed. One table per arm, the smallest that should trip it;
/// each is read, its columns swapped (a rebuild), and the plan must be
/// refused with that arm's reason and leave the table as it was. `after`
/// runs between the read and the plan, for the arm about a stale reading.
#[tokio::test(flavor = "multi_thread")]
async fn every_arm_of_the_rebuild_guard_refuses_its_table() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() {
        return;
    }
    let s = Scratch::create("guard_arms").await;
    struct Arm {
        table: &'static str,
        setup: &'static [&'static str],
        after: &'static [&'static str],
        says: &'static str,
    }
    let arms = [
        Arm {
            table: "g_perm",
            setup: &["GRANT SELECT ON dbo.g_perm TO public"],
            after: &[],
            says: "permissions are granted on it",
        },
        Arm {
            table: "g_bound",
            setup: &["CREATE VIEW dbo.g_bound_v WITH SCHEMABINDING AS SELECT a FROM dbo.g_bound"],
            after: &[],
            says: "a schema-bound view or function depends on it",
        },
        Arm {
            table: "g_esc",
            setup: &["ALTER TABLE dbo.g_esc SET (LOCK_ESCALATION = DISABLE)"],
            after: &[],
            says: "has a lock escalation setting",
        },
        Arm {
            table: "g_ct",
            setup: &[
                "ALTER DATABASE CURRENT SET CHANGE_TRACKING = ON",
                "ALTER TABLE dbo.g_ct ENABLE CHANGE_TRACKING",
            ],
            after: &[],
            says: "it has change tracking or a full-text index",
        },
        Arm {
            table: "g_zip",
            setup: &["ALTER TABLE dbo.g_zip REBUILD WITH (DATA_COMPRESSION = ROW)"],
            after: &[],
            says: "it is partitioned, compressed or stored off the default filegroup",
        },
        Arm {
            table: "g_part",
            setup: &[
                "CREATE PARTITION FUNCTION g_pf (int) AS RANGE LEFT FOR VALUES (10)",
                "CREATE PARTITION SCHEME g_ps AS PARTITION g_pf ALL TO ([PRIMARY])",
                "CREATE UNIQUE NONCLUSTERED INDEX ux_g_part ON dbo.g_part (id) ON g_ps (id)",
            ],
            after: &[],
            says: "it is partitioned, compressed or stored off the default filegroup",
        },
        Arm {
            table: "g_prop",
            setup: &["EXEC sp_addextendedproperty N'Owner', N'me', \
                      N'SCHEMA', N'dbo', N'TABLE', N'g_prop'"],
            after: &[],
            says: "it carries extended properties other than its comments",
        },
        Arm {
            table: "g_keydoc",
            setup: &[
                "EXEC sp_addextendedproperty N'MS_Description', N'the key', \
                      N'SCHEMA', N'dbo', N'TABLE', N'g_keydoc', N'CONSTRAINT', N'pk_g_keydoc'",
            ],
            after: &[],
            says: "its keys, indexes, checks, defaults or triggers carry extended properties",
        },
        Arm {
            table: "g_ixdoc",
            setup: &[
                "CREATE INDEX ix_g_ixdoc ON dbo.g_ixdoc (a)",
                "EXEC sp_addextendedproperty N'MS_Description', N'by a', \
                 N'SCHEMA', N'dbo', N'TABLE', N'g_ixdoc', N'INDEX', N'ix_g_ixdoc'",
            ],
            after: &[],
            says: "its keys, indexes, checks, defaults or triggers carry extended properties",
        },
        Arm {
            table: "g_trdoc",
            setup: &[
                "CREATE TRIGGER dbo.tr_g_trdoc ON dbo.g_trdoc AFTER INSERT AS SET NOCOUNT ON",
                "EXEC sp_addextendedproperty N'MS_Description', N'audits', \
                 N'SCHEMA', N'dbo', N'TABLE', N'g_trdoc', N'TRIGGER', N'tr_g_trdoc'",
            ],
            after: &[],
            says: "its keys, indexes, checks, defaults or triggers carry extended properties",
        },
        Arm {
            table: "g_ff",
            setup: &["CREATE INDEX ix_g_ff ON dbo.g_ff (a) WITH (FILLFACTOR = 70)"],
            after: &[],
            says: "an index sets a fill factor, padding, IGNORE_DUP_KEY or row or page locks",
        },
        Arm {
            table: "g_off",
            setup: &[
                "CREATE INDEX ix_g_off ON dbo.g_off (a)",
                "ALTER INDEX ix_g_off ON dbo.g_off DISABLE",
            ],
            after: &[],
            says: "or is disabled",
        },
        Arm {
            table: "g_sparse",
            setup: &["ALTER TABLE dbo.g_sparse ADD s int SPARSE NULL"],
            after: &[],
            says: "a column is sparse, a column set, hidden",
        },
        Arm {
            table: "g_fk",
            setup: &[
                "CREATE TABLE dbo.g_fk_p (id int PRIMARY KEY)",
                "ALTER TABLE dbo.g_fk WITH NOCHECK ADD CONSTRAINT fk_g_fk FOREIGN KEY (b) \
                 REFERENCES dbo.g_fk_p (id)",
            ],
            after: &[],
            says: "a foreign key on it or to it is disabled, untrusted or NOT FOR REPLICATION",
        },
        // S2-L5-04: what `DROP TABLE` takes and the arms above did not ask.
        Arm {
            table: "g_owner",
            setup: &[
                "CREATE USER u_g_owner WITHOUT LOGIN",
                "ALTER AUTHORIZATION ON dbo.g_owner TO u_g_owner",
            ],
            after: &[],
            says: "it has an owner of its own",
        },
        Arm {
            table: "g_rule",
            setup: &[
                "CREATE RULE dbo.r_g_pos AS @v > 0",
                "EXEC sp_bindrule N'dbo.r_g_pos', N'dbo.g_rule.a'",
            ],
            after: &[],
            says: "a column has a rule or a default bound to it",
        },
        Arm {
            table: "g_bdef",
            setup: &[
                "CREATE DEFAULT dbo.d_g_zero AS 0",
                "EXEC sp_bindefault N'dbo.d_g_zero', N'dbo.g_bdef.a'",
            ],
            after: &[],
            says: "a column has a rule or a default bound to it",
        },
        Arm {
            table: "g_label",
            setup: &["ADD SENSITIVITY CLASSIFICATION TO dbo.g_label.a \
                      WITH (LABEL = 'Confidential')"],
            after: &[],
            says: "a column carries a sensitivity classification",
        },
        Arm {
            table: "g_inbound",
            setup: &[
                "CREATE TABLE dbo.g_inbound_c (id int PRIMARY KEY, \
                 p int CONSTRAINT fk_g_inbound REFERENCES dbo.g_inbound (id))",
                "EXEC sp_addextendedproperty N'MS_Description', N'its parent', \
                 N'SCHEMA', N'dbo', N'TABLE', N'g_inbound_c', N'CONSTRAINT', N'fk_g_inbound'",
            ],
            after: &[],
            says: "a foreign key another table has on it carries extended properties",
        },
        Arm {
            // Made again as a ledger table, its ledger columns declared rather
            // than hidden — the shape the column arm's `is_hidden` misses.
            table: "g_ledger",
            setup: &[
                "DROP TABLE dbo.g_ledger",
                "CREATE TABLE dbo.g_ledger (id int NOT NULL CONSTRAINT pk_g_ledger PRIMARY KEY, \
                 a int NULL, b int NULL, \
                 s bigint GENERATED ALWAYS AS TRANSACTION_ID START, \
                 e bigint GENERATED ALWAYS AS TRANSACTION_ID END NULL, \
                 ss bigint GENERATED ALWAYS AS SEQUENCE_NUMBER START, \
                 se bigint GENERATED ALWAYS AS SEQUENCE_NUMBER END NULL) \
                 WITH (SYSTEM_VERSIONING = ON, LEDGER = ON)",
                "INSERT dbo.g_ledger (id, a, b) VALUES (1, 2, 3)",
            ],
            after: &[],
            says: "it is a ledger table",
        },
        Arm {
            table: "g_stale",
            setup: &[],
            after: &["CREATE TRIGGER dbo.tr_g_stale ON dbo.g_stale AFTER INSERT AS SET NOCOUNT ON"],
            says: "changed since it was read",
        },
    ];
    for arm in arms {
        let t = arm.table;
        s.exec(&format!(
            "CREATE TABLE dbo.{t} (id int NOT NULL CONSTRAINT pk_{t} PRIMARY KEY, \
             a int NULL, b int NULL); INSERT dbo.{t} (id, a, b) VALUES (1, 2, 3)"
        ))
        .await;
        for sql in arm.setup {
            s.exec(sql).await;
        }
        let current = read_table(&s, t).await;
        for sql in arm.after {
            s.exec(sql).await;
        }
        let mut d = TableDraft::from_table(&current);
        d.columns.swap(1, 2);
        let refused = refused_draft(&s, &current, &d).await;
        assert!(refused.contains(arm.says), "{t}: {refused}");
        assert_eq!(
            s.scalar(&format!(
                "SELECT CONCAT(STRING_AGG(c.name, ',') WITHIN GROUP (ORDER BY c.column_id), \
                 ':', (SELECT COUNT(*) FROM dbo.{t})) FROM sys.columns c \
                 WHERE c.object_id = OBJECT_ID(N'dbo.{t}') AND c.name IN ('id', 'a', 'b')"
            ))
            .await,
            "id,a,b:1",
            "{t} unchanged"
        );
    }
}

/// **A rebuild is refused over a signed trigger** (S2-L5-05): it re-creates
/// each trigger from its text, and the copy came back unsigned — whatever the
/// certificate's user was granted for the trigger stopped applying, with the
/// risk line saying the triggers were put back. The plan is withheld from the
/// reading, and its guard refuses it from the server for a signature added
/// after the table was read; the signature stands.
#[tokio::test(flavor = "multi_thread")]
async fn a_rebuild_is_refused_over_a_signed_trigger() {
    use schemaic_core::ddl::TableDraft;
    if !enabled() || azure_cannot("signs with a certificate the shared database may not allow") {
        return;
    }
    let s = Scratch::create("rebuild_signed").await;
    for sql in [
        "CREATE TABLE dbo.r_sig (id int NOT NULL PRIMARY KEY, a int NULL, b int NULL)",
        "CREATE TRIGGER dbo.tr_sig ON dbo.r_sig AFTER INSERT AS SET NOCOUNT ON",
        "CREATE CERTIFICATE zz_rebuild_cert ENCRYPTION BY PASSWORD = 'Pa55word!!zz' \
         WITH SUBJECT = 'schemaic test'",
    ] {
        s.exec(sql).await;
    }
    let unsigned = read_table(&s, "r_sig").await;
    s.exec(
        "ADD SIGNATURE TO dbo.tr_sig BY CERTIFICATE zz_rebuild_cert \
         WITH PASSWORD = 'Pa55word!!zz'",
    )
    .await;
    let t = read_table(&s, "r_sig").await;
    let mut d = TableDraft::from_table(&t);
    d.columns.swap(1, 2);
    let withheld = schemaic_core::ddl::diff(&t, &d, MS).unsupported();
    assert!(
        withheld
            .iter()
            .any(|w| w.contains("signature on the trigger tr_sig")),
        "{withheld:?}"
    );
    // Read before it was signed, the plan is the guard's to stop.
    let mut d = TableDraft::from_table(&unsigned);
    d.columns.swap(1, 2);
    assert!(
        schemaic_core::ddl::diff(&unsigned, &d, MS)
            .unsupported()
            .is_empty()
    );
    let refused = refused_draft(&s, &unsigned, &d).await;
    assert!(refused.contains("a trigger on it is signed"), "{refused}");
    assert_eq!(
        s.scalar("SELECT COUNT(*) FROM sys.crypt_properties WHERE class = 1")
            .await,
        "1"
    );
}

/// **A designer key on a table outside `dbo` references its own schema's
/// table** (S3.1-L1-02). The picker lists the designed table's schema and
/// sets only the table name; written bare, the key bound to the login's
/// default schema's same-named table — `dbo.customers` here — so orders for
/// `sales` customers were refused and a cascade from `dbo` deleted `sales`
/// orders. Both doors: a key added to an existing table, and one on a new
/// table.
#[tokio::test(flavor = "multi_thread")]
async fn a_designer_key_outside_dbo_references_its_own_schema() {
    use schemaic_core::ddl::{ForeignKeyDraft, TableDraft};
    use schemaic_core::schema::ForeignKeyInfo;
    if !enabled() {
        return;
    }
    let s = Scratch::create("fk_schema").await;
    for sql in [
        "CREATE SCHEMA sales",
        "CREATE TABLE dbo.customers (id int PRIMARY KEY); INSERT dbo.customers VALUES (1), (2)",
        "CREATE TABLE sales.customers (id int PRIMARY KEY); INSERT sales.customers VALUES (7)",
        "CREATE TABLE sales.orders (id int PRIMARY KEY, cust int NULL)",
    ] {
        s.exec(sql).await;
    }
    let key = |name: &str| ForeignKeyInfo {
        name: name.into(),
        columns: vec!["cust".into()],
        ref_table: "customers".into(),
        ref_columns: vec!["id".into()],
        on_delete: Some("CASCADE".into()),
        ..Default::default()
    };
    let schema =
        s.db.fetch_schema(&s.name, CancellationToken::new())
            .await
            .expect("the schema");
    let orders = schema
        .tables
        .iter()
        .find(|t| t.schema.as_deref() == Some("sales") && t.name == "orders")
        .expect("sales.orders")
        .clone();
    let mut d = TableDraft::from_table(&orders);
    d.foreign_keys.push(ForeignKeyDraft::new(key("fk_cust")));
    apply_draft(&s, &orders, &d).await;

    let mut new = TableDraft::from_table(&orders);
    new.name = "orders2".into();
    new.foreign_keys = vec![ForeignKeyDraft::new(key("fk_cust2"))];
    let stmts = schemaic_core::ddl::create(&new, MS).emit();
    s.db.run_ddl(&s.name, &stmts, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{e}\n{}", stmts.join("\n")));

    assert_eq!(
        s.scalar(
            "SELECT STRING_AGG(CONCAT(name, ':', OBJECT_SCHEMA_NAME(referenced_object_id)), ',') \
             WITHIN GROUP (ORDER BY name) FROM sys.foreign_keys"
        )
        .await,
        "fk_cust:sales,fk_cust2:sales"
    );
    // An order for a `sales` customer is taken, and a `dbo` one refused.
    s.exec("INSERT sales.orders VALUES (1, 7)").await;
    assert!(
        s.try_exec("INSERT sales.orders VALUES (2, 1)")
            .await
            .is_err()
    );
}

/// SQL Server's documented reserved words, as its reference lists them.
const DOCUMENTED_RESERVED: &[&str] = &[
    "ADD",
    "ALL",
    "ALTER",
    "AND",
    "ANY",
    "AS",
    "ASC",
    "AUTHORIZATION",
    "BACKUP",
    "BEGIN",
    "BETWEEN",
    "BREAK",
    "BROWSE",
    "BULK",
    "BY",
    "CASCADE",
    "CASE",
    "CHECK",
    "CHECKPOINT",
    "CLOSE",
    "CLUSTERED",
    "COALESCE",
    "COLLATE",
    "COLUMN",
    "COMMIT",
    "COMPUTE",
    "CONSTRAINT",
    "CONTAINS",
    "CONTAINSTABLE",
    "CONTINUE",
    "CONVERT",
    "CREATE",
    "CROSS",
    "CURRENT",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "CURRENT_USER",
    "CURSOR",
    "DATABASE",
    "DBCC",
    "DEALLOCATE",
    "DECLARE",
    "DEFAULT",
    "DELETE",
    "DENY",
    "DESC",
    "DISK",
    "DISTINCT",
    "DISTRIBUTED",
    "DOUBLE",
    "DROP",
    "DUMP",
    "ELSE",
    "END",
    "ERRLVL",
    "ESCAPE",
    "EXCEPT",
    "EXEC",
    "EXECUTE",
    "EXISTS",
    "EXIT",
    "EXTERNAL",
    "FETCH",
    "FILE",
    "FILLFACTOR",
    "FOR",
    "FOREIGN",
    "FREETEXT",
    "FREETEXTTABLE",
    "FROM",
    "FULL",
    "FUNCTION",
    "GOTO",
    "GRANT",
    "GROUP",
    "HAVING",
    "HOLDLOCK",
    "IDENTITY",
    "IDENTITY_INSERT",
    "IDENTITYCOL",
    "IF",
    "IN",
    "INDEX",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTO",
    "IS",
    "JOIN",
    "KEY",
    "KILL",
    "LEFT",
    "LIKE",
    "LINENO",
    "LOAD",
    "MERGE",
    "NATIONAL",
    "NOCHECK",
    "NONCLUSTERED",
    "NOT",
    "NULL",
    "NULLIF",
    "OF",
    "OFF",
    "OFFSETS",
    "ON",
    "OPEN",
    "OPENDATASOURCE",
    "OPENQUERY",
    "OPENROWSET",
    "OPENXML",
    "OPTION",
    "OR",
    "ORDER",
    "OUTER",
    "OVER",
    "PERCENT",
    "PIVOT",
    "PLAN",
    "PRECISION",
    "PRIMARY",
    "PRINT",
    "PROC",
    "PROCEDURE",
    "PUBLIC",
    "RAISERROR",
    "READ",
    "READTEXT",
    "RECONFIGURE",
    "REFERENCES",
    "REPLICATION",
    "RESTORE",
    "RESTRICT",
    "RETURN",
    "REVERT",
    "REVOKE",
    "RIGHT",
    "ROLLBACK",
    "ROWCOUNT",
    "ROWGUIDCOL",
    "RULE",
    "SAVE",
    "SCHEMA",
    "SECURITYAUDIT",
    "SELECT",
    "SEMANTICKEYPHRASETABLE",
    "SEMANTICSIMILARITYDETAILSTABLE",
    "SEMANTICSIMILARITYTABLE",
    "SESSION_USER",
    "SET",
    "SETUSER",
    "SHUTDOWN",
    "SOME",
    "STATISTICS",
    "SYSTEM_USER",
    "TABLE",
    "TABLESAMPLE",
    "TEXTSIZE",
    "THEN",
    "TO",
    "TOP",
    "TRAN",
    "TRANSACTION",
    "TRIGGER",
    "TRUNCATE",
    "TRY_CONVERT",
    "TSEQUAL",
    "UNION",
    "UNIQUE",
    "UNPIVOT",
    "UPDATE",
    "UPDATETEXT",
    "USE",
    "USER",
    "VALUES",
    "VARYING",
    "VIEW",
    "WAITFOR",
    "WHEN",
    "WHERE",
    "WHILE",
    "WITH",
    "WITHIN",
    "WRITETEXT",
];

/// **The editor's reserved list is what the server refuses as an alias.**
/// The documented list holds six words the parser takes anyway — the system
/// views' own `t.prec AS precision` drew a red reserved-alias error — so each
/// documented word's `is_reserved_word` answer is compared with the server's
/// own on `SELECT 1 AS <word>`, and every one of them is still bracketed as a
/// name (`must_quote_ident`), where listing one wrongly costs nothing.
#[tokio::test(flavor = "multi_thread")]
async fn the_reserved_list_is_what_the_server_refuses_as_an_alias() {
    use schemaic_core::intel::{is_reserved_word, must_quote_ident};
    if !enabled() {
        return;
    }
    let s = Scratch::create("reserved").await;
    let mut wrong = Vec::new();
    for w in DOCUMENTED_RESERVED {
        let accepted = s.try_exec(&format!("SELECT 1 AS {w}")).await.is_ok();
        if is_reserved_word(w, MS) == accepted {
            wrong.push(format!(
                "{w}: the server {} it as an alias, we say reserved={}",
                if accepted { "accepts" } else { "refuses" },
                is_reserved_word(w, MS)
            ));
        }
        assert!(must_quote_ident(w, MS), "{w} goes out bare");
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
