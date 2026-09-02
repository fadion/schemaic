//! The four paths that need a connection to *behave*, not just to answer:
//! `.sql` scripts, bulk imports, the pinned manual-transaction session, and
//! cancelling a statement already running.
//!
//! **All four are exceptions to something, and an exception is exactly what a
//! pure test cannot check.** `run_script` and `Session` are the two documented
//! departures from one-connection-per-operation, and the reason in both cases is
//! that their statements are *not* independent: a dump's `SET FOREIGN_KEY_CHECKS`,
//! its own `BEGIN`, a temporary table, an open transaction. Whether the
//! connection really is held is a fact about the server's session, visible from
//! nowhere else. `import_rows` departs from `commit_writes` for throughput and
//! keeps its all-or-nothing promise by a different mechanism. And a cancel is
//! only a cancel if the server stops: a client that returns early while the
//! statement runs on has told the user something untrue about their database.

use std::time::{Duration, Instant};

use schemaic_core::model::Value;
use schemaic_core::script::{ExecEnd, Statement};
use schemaic_db::{DbError, ImportTarget, session::Session};
use tokio_util::sync::CancellationToken;

use crate::endpoint::Target;
use crate::scratch::Scratch;

/// A script runs every statement it is given, in order.
pub async fn a_script_runs_every_statement_in_order(target: &'static Target) {
    let scratch = Scratch::create(target, "script_order").await;
    let t = scratch.qualified("s");

    let (end, ran) = run_script(
        &scratch,
        &[
            format!("CREATE TABLE {t} (id INTEGER NOT NULL PRIMARY KEY, name VARCHAR(16));"),
            format!("INSERT INTO {t} (id, name) VALUES (1, 'first');"),
            format!("INSERT INTO {t} (id, name) VALUES (2, 'second');"),
            format!("UPDATE {t} SET name = 'edited' WHERE id = 1;"),
        ],
    )
    .await;

    assert_eq!(end, ExecEnd::Done, "{}: how the run ended", target.name);
    assert_eq!(ran, 4, "{}: statements run", target.name);
    assert_eq!(
        column(&scratch, &format!("SELECT name FROM {t} ORDER BY id")).await,
        ["edited", "second"],
        "{}: the script's effect, in order",
        target.name
    );

    scratch.teardown().await;
}

/// One connection for the whole file, so session state carries between
/// statements.
///
/// **This is the exception, stated as a test.** A temporary table belongs to the
/// session that made it; if `run_script` opened a connection per statement — as
/// every other `Db` method does — the second statement here would fail with "no
/// such table", and a restore whose dump opens with `SET FOREIGN_KEY_CHECKS = 0`
/// would silently run with them on.
pub async fn a_script_holds_one_connection_so_session_state_carries(target: &'static Target) {
    let scratch = Scratch::create(target, "script_session").await;
    let t = scratch.qualified("s");

    let (end, ran) = run_script(
        &scratch,
        &[
            format!("CREATE TABLE {t} (id INTEGER NOT NULL PRIMARY KEY, name VARCHAR(16));"),
            // TEMPORARY is spelled the same on both, and means the same thing:
            // this table exists for one session and no other.
            "CREATE TEMPORARY TABLE scratch_tmp (id INTEGER, name VARCHAR(16));".to_string(),
            "INSERT INTO scratch_tmp (id, name) VALUES (1, 'carried');".to_string(),
            format!("INSERT INTO {t} (id, name) SELECT id, name FROM scratch_tmp;"),
        ],
    )
    .await;

    assert_eq!(
        end,
        ExecEnd::Done,
        "{}: a temporary table did not survive to the next statement, so the run \
         did not hold one connection",
        target.name
    );
    assert_eq!(ran, 4, "{}: statements run", target.name);
    assert_eq!(
        column(&scratch, &format!("SELECT name FROM {t}")).await,
        ["carried"],
        "{}: what the temporary table carried",
        target.name
    );

    scratch.teardown().await;
}

/// A statement the server refuses stops the run, names its line, and nothing
/// after it executes.
///
/// A script is not transactional unless the file said so, so what already ran
/// stays — and the count of it is the answer to the only question a stopped run
/// leaves.
pub async fn a_refused_statement_stops_the_run_and_names_its_line(target: &'static Target) {
    let scratch = Scratch::create(target, "script_refused").await;
    let t = scratch.qualified("s");

    let (end, ran) = run_script(
        &scratch,
        &[
            format!("CREATE TABLE {t} (id INTEGER NOT NULL PRIMARY KEY, name VARCHAR(16));"),
            format!("INSERT INTO {t} (id, name) VALUES (1, 'kept');"),
            format!("INSERT INTO {t} (id, name) VALUES (2, 'dup', 'too', 'many');"),
            format!("INSERT INTO {t} (id, name) VALUES (3, 'never');"),
        ],
    )
    .await;

    match end {
        ExecEnd::Failed { line, .. } => assert_eq!(
            line, 3,
            "{}: the failure should name the statement's own line",
            target.name
        ),
        other => panic!("{}: expected a refusal, got {other:?}", target.name),
    }
    assert_eq!(
        ran, 2,
        "{}: statements that ran before the refusal",
        target.name
    );
    assert_eq!(
        column(&scratch, &format!("SELECT name FROM {t} ORDER BY id")).await,
        ["kept"],
        "{}: the run continued past its failure",
        target.name
    );

    scratch.teardown().await;
}

/// A script with nothing in it finishes, having run nothing.
pub async fn an_empty_script_finishes_having_run_nothing(target: &'static Target) {
    let scratch = Scratch::create(target, "script_empty").await;

    let (end, ran) = run_script(&scratch, &[]).await;

    assert_eq!(end, ExecEnd::Done, "{}: how the run ended", target.name);
    assert_eq!(ran, 0, "{}: statements run", target.name);

    scratch.teardown().await;
}

/// An import loads every row it was given.
pub async fn an_import_loads_every_row(target: &'static Target) {
    let scratch = Scratch::create(target, "import").await;
    seed_import_table(&scratch).await;

    let mut rows = (1..=50)
        .map(|i| Ok(vec![Value::Int(i), Value::Str(format!("row {i}"))]))
        .collect::<Vec<_>>()
        .into_iter();

    let loaded = import(&scratch, &mut rows)
        .await
        .unwrap_or_else(|e| panic!("{}: the import failed: {e}", target.name));

    assert_eq!(loaded, 50, "{}: rows loaded", target.name);
    assert_eq!(
        count(&scratch).await,
        "50",
        "{}: rows in the table",
        target.name
    );

    scratch.teardown().await;
}

/// A reader that fails partway rolls the whole import back.
///
/// The rows before the error were already sent to the server in an earlier
/// batch, which is the point: the promise is all-or-nothing across batches, not
/// within one.
pub async fn a_reader_error_rolls_the_whole_import_back(target: &'static Target) {
    let scratch = Scratch::create(target, "import_reader").await;
    seed_import_table(&scratch).await;

    // Enough rows to span more than one batch before the failure, so the
    // rollback has something to undo.
    let mut rows = (1..=5000)
        .map(|i| {
            if i == 4000 {
                Err("the file ends in the middle of a record".to_string())
            } else {
                Ok(vec![Value::Int(i), Value::Str(format!("row {i}"))])
            }
        })
        .collect::<Vec<_>>()
        .into_iter();

    let err = import(&scratch, &mut rows)
        .await
        .expect_err("a reader error must fail the import");
    assert!(
        err.to_string().contains("the file ends in the middle"),
        "{}: the reader's own message should reach the user, got {:?}",
        target.name,
        err.to_string()
    );
    assert_eq!(
        count(&scratch).await,
        "0",
        "{}: the batches before the reader error were not undone",
        target.name
    );

    scratch.teardown().await;
}

/// A value the server refuses rolls the whole import back.
///
/// **It has to cross a batch to see a rollback at all.** This sent 100 rows
/// against an `INSERT_BATCH_ROWS` of 500 — one statement, one batch — so the
/// only thing it could observe was a single refused `INSERT` leaving nothing
/// behind, which is true of a server with no transaction as much as of one with
/// it. The row that collides now lands in the *second* batch, so passing means
/// the first batch's rows were really undone.
pub async fn a_refused_row_rolls_the_whole_import_back(target: &'static Target) {
    let scratch = Scratch::create(target, "import_refused").await;
    seed_import_table(&scratch).await;

    let total = schemaic_core::import::INSERT_BATCH_ROWS as i64 + 50;
    let collide_at = schemaic_core::import::INSERT_BATCH_ROWS as i64 + 10;
    let mut rows = (1..=total)
        .map(|i| {
            Ok(vec![
                // Two rows claiming the same primary key, a batch apart: the
                // server refuses the batch carrying the second, and the first
                // has already been sent.
                Value::Int(if i == collide_at { 1 } else { i }),
                Value::Str(format!("row {i}")),
            ])
        })
        .collect::<Vec<_>>()
        .into_iter();

    import(&scratch, &mut rows)
        .await
        .expect_err("a duplicate key must fail the import");
    assert_eq!(
        count(&scratch).await,
        "0",
        "{}: a refused import left rows behind",
        target.name
    );

    scratch.teardown().await;
}

/// Work in a manual transaction is invisible to everyone else until it commits.
///
/// The pinned `Session` is the other documented exception to
/// one-connection-per-operation, and this is what the exception buys: the
/// second connection here is a real one, and it must not see the uncommitted
/// row.
pub async fn a_manual_transaction_is_invisible_until_it_commits(target: &'static Target) {
    let scratch = Scratch::create(target, "manual_commit").await;
    seed_import_table(&scratch).await;

    let session = OpenSession(Some(
        Session::open(&scratch.db, Some(&scratch.database))
            .await
            .unwrap_or_else(|e| panic!("{}: could not pin a session: {e}", target.name)),
    ));
    session
        .ensure_tx()
        .await
        .unwrap_or_else(|e| panic!("{}: could not begin: {e}", target.name));

    let insert = format!(
        "INSERT INTO {} (id, name) VALUES (1, 'staged')",
        scratch.qualified("imp")
    );
    session
        .fetch_query(&insert, 10, CancellationToken::new())
        .await
        .result
        .unwrap_or_else(|e| panic!("{}: the insert failed: {e}", target.name));

    // A different connection entirely — `Scratch::exec` opens its own.
    assert_eq!(
        count(&scratch).await,
        "0",
        "{}: an uncommitted row was visible to another connection",
        target.name
    );

    session
        .commit()
        .await
        .unwrap_or_else(|e| panic!("{}: the commit failed: {e}", target.name));
    assert_eq!(
        count(&scratch).await,
        "1",
        "{}: the row did not survive the commit",
        target.name
    );
    // `OpenSession` closes it on every other path too — see its own note.
    drop(session);

    scratch.teardown().await;
}

/// A manual transaction that is rolled back leaves nothing behind.
pub async fn a_rolled_back_manual_transaction_leaves_nothing(target: &'static Target) {
    let scratch = Scratch::create(target, "manual_rollback").await;
    seed_import_table(&scratch).await;

    let session = OpenSession(Some(
        Session::open(&scratch.db, Some(&scratch.database))
            .await
            .unwrap_or_else(|e| panic!("{}: could not pin a session: {e}", target.name)),
    ));
    session
        .ensure_tx()
        .await
        .unwrap_or_else(|e| panic!("{}: could not begin: {e}", target.name));

    let insert = format!(
        "INSERT INTO {} (id, name) VALUES (1, 'discarded')",
        scratch.qualified("imp")
    );
    session
        .fetch_query(&insert, 10, CancellationToken::new())
        .await
        .result
        .unwrap_or_else(|e| panic!("{}: the insert failed: {e}", target.name));

    session
        .rollback()
        .await
        .unwrap_or_else(|e| panic!("{}: the rollback failed: {e}", target.name));
    assert_eq!(
        count(&scratch).await,
        "0",
        "{}: a rolled-back row is still there",
        target.name
    );
    // `OpenSession` closes it on every other path too — see its own note.
    drop(session);

    scratch.teardown().await;
}

/// Cancelling a running statement stops it at the server, and returns before it
/// would have finished.
///
/// **The elapsed time is the assertion.** A client that gave up locally and
/// returned `Cancelled` while the statement ran on would pass any check of the
/// error alone, and would be lying: the user is told their query stopped while
/// the server is still holding whatever it holds.
pub async fn a_cancelled_query_stops_at_the_server(target: &'static Target) {
    let scratch = Scratch::create(target, "cancel").await;

    let cancel = CancellationToken::new();
    let armed = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(250)).await;
        armed.cancel();
    });

    let started = Instant::now();
    let outcome = scratch
        .db
        .fetch_query(Some(&scratch.database), &target.sleep_sql(), 10, cancel)
        .await;
    let elapsed = started.elapsed();

    // **`Cancelled` specifically, not merely an error.** A sleep statement this
    // server does not understand would come back instantly with a syntax error,
    // and a test that only asked "did it fail, and fast?" would pass having
    // cancelled nothing at all.
    assert!(
        matches!(outcome, Err(DbError::Cancelled)),
        "{}: expected the cancellation, got {:?}",
        target.name,
        outcome.map(|rs| rs.row_count()).map_err(|e| e.to_string())
    );
    // The statement sleeps for SLEEP_SECS; anything close to that means the
    // cancel waited for it rather than stopping it.
    assert!(
        elapsed < Duration::from_secs(SLEEP_SECS) - CANCEL_MARGIN,
        "{}: the cancel took {elapsed:?}, so the statement ran to completion",
        target.name
    );

    // **And the server agrees.** Everything above is about the *client*:
    // `tokio::select!` returns `Cancelled` fast whether or not anything was sent
    // to the server, so deleting `kill_query`/`cancel_query` from both arms left
    // all six legs passing in ~250 ms while three servers slept on. This is the
    // half that asks. Polled rather than read once, because a `KILL QUERY` is
    // asynchronous — the row leaves the view shortly after the client returns —
    // and the wait is bounded well inside the sleep so a statement that really
    // ran on cannot pass by outlasting it.
    let deadline = Instant::now() + (Duration::from_secs(SLEEP_SECS) - CANCEL_MARGIN);
    let mut still = target.running_sleeps(&scratch.db).await;
    while still > 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
        still = target.running_sleeps(&scratch.db).await;
    }
    assert_eq!(
        still, 0,
        "{}: the client returned Cancelled but the server is still running the statement",
        target.name
    );

    scratch.teardown().await;
}

/// A pinned [`Session`] that is closed **on every path out**, including a panic.
///
/// The manual-transaction tests reach `session.close()` on their last line, so
/// every `unwrap_or_else(panic)` above it skipped the close and left an open
/// transaction on the server. `DROP DATABASE` then waits on it — measured on
/// MariaDB — so a failing assertion here took the scratch teardown with it, and
/// the first failure was reported as a teardown that hung rather than as itself.
struct OpenSession(Option<std::sync::Arc<Session>>);

impl std::ops::Deref for OpenSession {
    type Target = Session;
    fn deref(&self) -> &Session {
        self.0.as_ref().expect("held until drop")
    }
}

impl Drop for OpenSession {
    fn drop(&mut self) {
        let Some(session) = self.0.take() else {
            return;
        };
        // On its own thread with its own runtime, for `Scratch::drop`'s reason:
        // a drop is synchronous and blocking on the current runtime from inside
        // one deadlocks.
        let _ = std::thread::spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                rt.block_on(session.close());
            }
        })
        .join();
    }
}

/// How far inside [`SLEEP_SECS`] the two cancellation assertions have to land.
///
/// Subtracted rather than written as a second literal, and as a `Duration` so
/// the arithmetic cannot underflow: `SLEEP_SECS - 2` on a `u64` wraps for any
/// sleep under two seconds, which is exactly what someone shortening this
/// constant would try.
const CANCEL_MARGIN: Duration = Duration::from_secs(2);

/// How long the cancellation test's statement would run for if nothing stopped
/// it. Long enough that finishing it is unmistakable, short enough that a leg
/// failing this does not stall the suite.
pub const SLEEP_SECS: u64 = 5;

/// Feed `stmts` through the channel `run_script` reads, and report how it ended.
///
/// **The feeding is a task of its own**, and that is not tidiness: the channel
/// holds 16, and filling it inline meant the seventeenth `send` awaited a
/// receiver nothing was polling yet — `run_script` is called on the line below.
/// A 17-statement script test would have hung until the CI job's
/// `timeout-minutes` cut it off, reported as "still running" rather than as
/// itself. This is the shape the app itself uses (`script::feed` alongside
/// `Db::run_script`), so the test now drives the same arrangement it is testing.
async fn run_script(scratch: &Scratch, stmts: &[String]) -> (ExecEnd, usize) {
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let owned: Vec<String> = stmts.to_vec();
    let feed = tokio::spawn(async move {
        for (i, sql) in owned.into_iter().enumerate() {
            // A send that fails means the runner stopped early — a refused
            // statement, which is the case several of these tests are about — so
            // it ends the feed rather than failing the test.
            if tx
                .send(Statement {
                    sql,
                    // One statement per line, so a reported line is its index.
                    line: i as u64 + 1,
                    offset: 0,
                })
                .await
                .is_err()
            {
                return;
            }
        }
    });
    let out = scratch
        .db
        .run_script(&scratch.database, rx, CancellationToken::new())
        .await;
    feed.await.expect("the feeding task must not panic");
    out
}

async fn seed_import_table(scratch: &Scratch) {
    scratch
        .exec(&format!(
            "CREATE TABLE {} (id INTEGER NOT NULL PRIMARY KEY, name VARCHAR(32))",
            scratch.qualified("imp")
        ))
        .await;
}

async fn import(
    scratch: &Scratch,
    rows: &mut (dyn Iterator<Item = Result<Vec<Value>, String>> + Send),
) -> Result<u64, schemaic_db::DbError> {
    let columns = ["id".to_string(), "name".to_string()];
    scratch
        .db
        .import_rows(
            ImportTarget {
                database: &scratch.database,
                schema: scratch.namespace,
                table: "imp",
                columns: &columns,
            },
            rows,
            CancellationToken::new(),
        )
        .await
}

/// How many rows the import table holds, read on a **fresh** connection.
async fn count(scratch: &Scratch) -> String {
    let rs = scratch
        .exec(&format!(
            "SELECT COUNT(*) FROM {}",
            scratch.qualified("imp")
        ))
        .await;
    rs.cell(0, 0).expect("a count").display().to_string()
}

/// One column of a query, top to bottom.
async fn column(scratch: &Scratch, sql: &str) -> Vec<String> {
    let rs = scratch.exec(sql).await;
    (0..rs.row_count())
        .map(|r| {
            rs.cell(r, 0)
                .expect("a selected cell")
                .display()
                .to_string()
        })
        .collect()
}
