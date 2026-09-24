//! [`Db::fetch_query_enforced`]: the session agreeing with a text gate.
//!
//! **The gate reads the statement; only the server sees what it does.** The
//! headless read path's gate (`sql::read_only_reason`) takes a statement that
//! opens with a read head and names no denied keyword — and
//! `SELECT purge_all()` is exactly that, while `purge_all()` deletes a table. On an ordinary session it
//! runs, which is how the MCP server's `run_query` and `schemaic query` were
//! shown deleting rows through a gate documented as read-only. These tests are
//! the session's half of the guard, on every server leg.
//!
//! [`Db::fetch_query_enforced`]: schemaic_db::Db::fetch_query_enforced

use schemaic_db::Enforce;
use tokio_util::sync::CancellationToken;

use crate::endpoint::Target;
use crate::scratch::Scratch;

/// A table of three rows and a function that empties it.
async fn seeded(target: &'static Target, test: &str) -> (Scratch, String) {
    let scratch = Scratch::create(target, test).await;
    let t = scratch.qualified("keep");
    scratch
        .exec(&format!(
            "CREATE TABLE {t} (id INTEGER NOT NULL PRIMARY KEY)"
        ))
        .await;
    scratch
        .exec(&format!("INSERT INTO {t} (id) VALUES (1), (2), (3)"))
        .await;
    scratch
        .exec(&target.purging_function.replace("{table}", &t))
        .await;
    (scratch, t)
}

async fn enforced(scratch: &Scratch, sql: &str, enforce: Enforce) -> Result<u64, String> {
    scratch
        .db
        .fetch_query_enforced(
            Some(&scratch.database),
            sql,
            100,
            CancellationToken::new(),
            enforce,
        )
        .await
        .map(|rs| rs.row_count() as u64)
        .map_err(|e| e.to_string())
}

async fn rows_left(scratch: &Scratch, t: &str) -> String {
    let rs = scratch.exec(&format!("SELECT COUNT(*) FROM {t}")).await;
    rs.cell(0, 0).expect("a count").display().to_string()
}

/// **A write a `SELECT` hides is refused by the session.** The gate is asked
/// first and passes it — that composition is the bug — and the rows are
/// counted afterwards, because an error alone could be a session that refuses
/// everything.
pub async fn a_read_only_session_refuses_a_write_a_select_hides(target: &'static Target) {
    let (scratch, t) = seeded(target, "enforce_ro").await;
    assert!(
        schemaic_core::sql::read_only_reason("SELECT purge_all()", scratch.dialect()).is_ok(),
        "{}: the text gate is expected to pass this — the session is what refuses it",
        target.name
    );

    let got = enforced(&scratch, "SELECT purge_all()", Enforce::ReadOnly).await;

    assert!(
        got.is_err(),
        "{}: a read-only session ran a function that deletes",
        target.name
    );
    assert_eq!(
        rows_left(&scratch, &t).await,
        "3",
        "{}: rows the refused statement deleted",
        target.name
    );
    scratch.teardown().await;
}

/// The refusal is about the write: an ordinary read runs on the same session.
pub async fn a_read_only_session_still_reads(target: &'static Target) {
    let (scratch, t) = seeded(target, "enforce_reads").await;
    let got = enforced(&scratch, &format!("SELECT id FROM {t}"), Enforce::ReadOnly).await;
    assert_eq!(got, Ok(3), "{}: a read on a read-only session", target.name);
    scratch.teardown().await;
}

/// `AsJudged` is not read-only — `exec` on a writable connection runs on it —
/// so the pin it adds must leave a write working.
pub async fn a_session_pinned_to_the_gates_lexer_still_writes(target: &'static Target) {
    let (scratch, t) = seeded(target, "enforce_judged").await;
    enforced(
        &scratch,
        &format!("DELETE FROM {t} WHERE id = 1"),
        Enforce::AsJudged,
    )
    .await
    .unwrap_or_else(|e| panic!("{}: a write on an AsJudged session: {e}", target.name));
    assert_eq!(rows_left(&scratch, &t).await, "2", "{}", target.name);
    scratch.teardown().await;
}

/// **A Manual tab's pinned session is read-only for its whole life** on a
/// read-only connection: every transaction `ensure_tx` opens on it refuses the
/// hidden write. The unenforced session beside it runs the same statement,
/// which is what makes the refusal about the session rather than a function
/// that fails anyway — and neither commits, so the table is untouched.
pub async fn a_read_only_pinned_session_refuses_a_write_a_select_hides(target: &'static Target) {
    use schemaic_db::Session;
    let (scratch, t) = seeded(target, "enforce_pinned").await;
    let run = |enforce: Option<Enforce>| {
        let scratch = &scratch;
        async move {
            let s = Session::open_enforced(&scratch.db, Some(&scratch.database), enforce)
                .await
                .unwrap_or_else(|e| panic!("{}: open: {e}", target.name));
            s.ensure_tx().await.expect("BEGIN");
            let got = s
                .fetch_query("SELECT purge_all()", 100, CancellationToken::new())
                .await
                .result
                .map(|_| ())
                .map_err(|e| e.to_string());
            let _ = s.rollback().await;
            s.close().await;
            got
        }
    };
    assert!(
        run(Some(Enforce::ReadOnly)).await.is_err(),
        "{}: a read-only pinned session ran a function that deletes",
        target.name
    );
    run(None).await.unwrap_or_else(|e| {
        panic!(
            "{}: the ordinary session could not run it: {e}",
            target.name
        )
    });
    assert_eq!(rows_left(&scratch, &t).await, "3", "{}", target.name);
    scratch.teardown().await;
}

/// EXPLAIN ANALYZE **executes** the statement, inside a transaction it rolls
/// back — which undoes neither a MyISAM write nor a PostgreSQL sequence. On a
/// read-only connection that transaction is read-only and refuses the write;
/// on any other the same measurement runs.
pub async fn a_read_only_explain_analyze_refuses_a_write_a_select_hides(target: &'static Target) {
    let (scratch, t) = seeded(target, "enforce_explain").await;
    let explain = |read_only: bool| {
        scratch.db.explain(
            Some(&scratch.database),
            "SELECT purge_all()",
            true,
            read_only,
            CancellationToken::new(),
        )
    };
    assert!(
        explain(true).await.is_err(),
        "{}: a read-only EXPLAIN ANALYZE ran a function that deletes",
        target.name
    );
    explain(false)
        .await
        .unwrap_or_else(|e| panic!("{}: the ordinary measurement failed: {e}", target.name));
    assert_eq!(rows_left(&scratch, &t).await, "3", "{}", target.name);
    scratch.teardown().await;
}

/// The *All rows* export re-runs the tab's statement, so on a read-only
/// connection its stream is refused the hidden write the run was.
pub async fn a_read_only_stream_refuses_a_write_a_select_hides(target: &'static Target) {
    let (scratch, t) = seeded(target, "enforce_stream").await;
    let stream = |enforce: Option<Enforce>| {
        let scratch = &scratch;
        async move {
            let (tx, mut rx) = tokio::sync::mpsc::channel(4);
            let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
            let got = scratch
                .db
                .stream_query_enforced(
                    Some(&scratch.database),
                    "SELECT purge_all()",
                    100,
                    CancellationToken::new(),
                    tx,
                    enforce,
                )
                .await;
            let _ = drain.await;
            got.map(|_| ()).map_err(|e| e.to_string())
        }
    };
    assert!(
        stream(Some(Enforce::ReadOnly)).await.is_err(),
        "{}: a read-only stream ran a function that deletes",
        target.name
    );
    assert_eq!(rows_left(&scratch, &t).await, "3", "{}", target.name);
    stream(None)
        .await
        .unwrap_or_else(|e| panic!("{}: the unenforced stream failed: {e}", target.name));
    assert_eq!(
        rows_left(&scratch, &t).await,
        "0",
        "{}: the unenforced stream is the one that deletes",
        target.name
    );
    scratch.teardown().await;
}

/// **Run All on a read-only connection** shares one connection across its
/// statements, and that connection refuses the hidden write; the same batch
/// unenforced runs it, so the refusal is the session's.
pub async fn a_read_only_batch_refuses_a_write_a_select_hides(target: &'static Target) {
    let (scratch, t) = seeded(target, "enforce_batch").await;
    let stmts = vec![
        format!("SELECT id FROM {t}"),
        "SELECT purge_all()".to_string(),
    ];
    let run = |enforce: Option<Enforce>| {
        let (scratch, stmts) = (&scratch, stmts.clone());
        async move {
            let mut out = Vec::new();
            scratch
                .db
                .run_batch_enforced(
                    Some(&scratch.database),
                    &stmts,
                    100,
                    CancellationToken::new(),
                    |_, r| out.push(r.is_ok()),
                    enforce,
                )
                .await;
            out
        }
    };
    assert_eq!(
        run(Some(Enforce::ReadOnly)).await,
        vec![true, false],
        "{}: the read runs and the hidden write is refused",
        target.name
    );
    assert_eq!(rows_left(&scratch, &t).await, "3", "{}", target.name);
    assert_eq!(run(None).await, vec![true, true], "{}", target.name);
    assert_eq!(
        rows_left(&scratch, &t).await,
        "0",
        "{}: the unenforced batch is the one that deletes",
        target.name
    );
    scratch.teardown().await;
}
