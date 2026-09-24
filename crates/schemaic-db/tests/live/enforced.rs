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
