//! A connection's init SQL (`Connection::init_sql`) against the real servers:
//! run on the connection an operation opens, before the operation; under a
//! read-only `Enforce`, not able to lift it; and a statement the server
//! refuses fails the operation rather than leaving it on a session without
//! the setting.

use schemaic_core::intel::SqlDialect;
use schemaic_db::Enforce;
use tokio_util::sync::CancellationToken;

use crate::endpoint::Target;
use crate::scratch::Scratch;

/// A setting each engine reads back, and the value the init sets it to.
fn setting(dialect: SqlDialect) -> (&'static str, &'static str, &'static str) {
    match dialect {
        SqlDialect::Postgres => (
            "SET application_name = 'schemaic_init_probe'",
            "SELECT current_setting('application_name')",
            "schemaic_init_probe",
        ),
        SqlDialect::MySql => (
            "SET time_zone = '+03:00'",
            "SELECT @@session.time_zone",
            "+03:00",
        ),
        SqlDialect::MsSql | SqlDialect::Sqlite => unreachable!("not a leg of this suite"),
    }
}

/// **The setting is in force on the operation's own connection** — a fresh
/// one, since every `Db` method connects anew, so it can only be there if the
/// connect ran it.
pub async fn the_init_sql_is_in_force_on_the_operations_connection(target: &'static Target) {
    let scratch = Scratch::create(target, "init_sql_on").await;
    let (init, read, want) = setting(scratch.dialect());
    let db = scratch.db.clone().with_init_sql(init);
    let rs = db
        .fetch_query(Some(&scratch.database), read, 10, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", target.name));
    assert_eq!(
        rs.cell(0, 0).expect("a value").display().to_string(),
        want,
        "{}: the init SQL was not in force",
        target.name
    );
    scratch.teardown().await;
}

/// **Init SQL runs before the read-only guard, so it cannot lift it** — the
/// setting is in force and a write is still refused on the same enforced
/// session.
pub async fn a_read_only_session_with_init_sql_still_refuses_a_write(target: &'static Target) {
    let scratch = Scratch::create(target, "init_sql_ro").await;
    let t = scratch.qualified("keep");
    scratch
        .exec(&format!(
            "CREATE TABLE {t} (id INTEGER NOT NULL PRIMARY KEY)"
        ))
        .await;
    let (init, read, want) = setting(scratch.dialect());
    let db = scratch.db.clone().with_init_sql(init);
    let fetch = |sql: String| {
        let db = db.clone();
        let database = scratch.database.clone();
        async move {
            db.fetch_query_enforced(
                Some(&database),
                &sql,
                10,
                CancellationToken::new(),
                Enforce::ReadOnly,
            )
            .await
        }
    };
    let rs = fetch(read.to_string()).await.expect("the read runs");
    assert_eq!(rs.cell(0, 0).expect("a value").display().to_string(), want);
    assert!(
        fetch(format!("INSERT INTO {t} (id) VALUES (1)"))
            .await
            .is_err(),
        "{}: a read-only session with init SQL ran a write",
        target.name
    );
    let n = scratch.exec(&format!("SELECT COUNT(*) FROM {t}")).await;
    assert_eq!(n.cell(0, 0).expect("a count").display().to_string(), "0");
    scratch.teardown().await;
}

/// **A statement the server refuses fails the operation**, naming the init
/// SQL — the operation does not go on without a setting the user asked for.
pub async fn a_refused_init_statement_fails_the_operation(target: &'static Target) {
    let scratch = Scratch::create(target, "init_sql_bad").await;
    // A setting `init_sql::check` lets through with a value only the server
    // can refuse — an unknown name is refused before the connect now.
    let bad = match scratch.dialect() {
        SqlDialect::Postgres => "SET statement_timeout = 'banana'",
        SqlDialect::MySql => "SET time_zone = 'Nowhere/Nope'",
        SqlDialect::MsSql | SqlDialect::Sqlite => unreachable!("not a leg of this suite"),
    };
    let db = scratch.db.clone().with_init_sql(bad);
    let err = db
        .fetch_query(
            Some(&scratch.database),
            "SELECT 1",
            10,
            CancellationToken::new(),
        )
        .await
        .expect_err("an operation ran past a refused init statement");
    assert!(
        err.to_string().contains("init SQL"),
        "{}: {err}",
        target.name
    );
    scratch.teardown().await;
}
