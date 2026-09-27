//! SQL Server's live leg, on its own terms.
//!
//! **Outside [`crate::suite`] and the `live_suite!` macro**, because the shared
//! suite writes rows back, applies DDL and pins sessions, and SQL Server answers
//! all three with a refusal until they are written. What is here is what the
//! engine *does* do — connecting, the read path and its values, provenance, the
//! read-only guard, validation, the schema, activity — plus the two catalogues
//! only it has: its builtin functions, and its DMV snippets.
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

use schemaic_core::intel::SqlDialect;
use schemaic_core::model::ResultSet;
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
    // Its DDL is the statement the server stored, not one built around it.
    assert_eq!(
        c.triggers[0].create_sql(MS),
        "CREATE TRIGGER dbo.tr ON dbo.customers AFTER INSERT, DELETE AS SET NOCOUNT ON;"
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
           id int IDENTITY(1,1) CONSTRAINT pk_t PRIMARY KEY, \
           [odd]]name] nvarchar(40) COLLATE Latin1_General_BIN NULL, \
           balance decimal(10,2) NOT NULL DEFAULT ((0)), \
           doubled AS (balance * 2) PERSISTED, \
           code varchar(8) NOT NULL CONSTRAINT uq_code UNIQUE, \
           CONSTRAINT ck_t CHECK (balance >= 0)); \
         CREATE INDEX ix_bal ON dbo.t (balance DESC) WHERE balance > 0;",
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
                    "{} {} null={} pk={} id={} def={:?} gen={:?} {} coll={:?}",
                    c.name,
                    c.type_name,
                    c.nullable,
                    c.primary_key,
                    c.auto_increment,
                    c.default,
                    c.generated,
                    c.generated_stored,
                    c.collation
                )
            })
            .collect()
    };
    assert_eq!(cols(&copy), cols(&original), "{ddl}");
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
