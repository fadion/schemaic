//! Whole-table export: rows off the wire and into a channel, uncapped.
//!
//! **The row cap is the thing this path exists to escape**, and escaping it is
//! the part no pure test reaches: a capped fetch answers "what is in this table"
//! and an export answers "give me the table", where a cap is not a kindness but a
//! silently short file. So the assertions here are about *completeness* and about
//! how a failure reaches the writer.
//!
//! That second half is the subtle one. The writer sits on the far end of a
//! channel, and a channel that simply closes reads as "the table ended" — so a
//! failure that only came back from `stream_query`'s return value would be a
//! half-written file reported as finished. The error therefore rides the channel
//! too, and both tests below check the channel rather than the return value alone.

use schemaic_core::model::Value;
use tokio_util::sync::CancellationToken;

use crate::endpoint::Target;
use crate::scratch::Scratch;

/// How many rows the export tests load. Comfortably more than one chunk, and
/// small enough that seeding it is not what the suite spends its time on.
const ROWS: i64 = 5_000;

/// Rows per chunk — several chunks over `ROWS`, so the chunking is exercised
/// rather than merely tolerated.
const CHUNK: usize = 900;

/// Every row reaches the writer, in chunks, with the columns on each.
pub async fn a_streamed_export_delivers_every_row(target: &'static Target) {
    let scratch = Scratch::create(target, "export").await;
    seed(&scratch).await;

    let (chunks, sent) = stream(
        &scratch,
        &format!(
            "SELECT id, name FROM {} ORDER BY id",
            scratch.qualified("big")
        ),
    )
    .await;
    let sent = sent.unwrap_or_else(|e| panic!("{}: the export failed: {e}", target.name));

    let rows: usize = chunks
        .iter()
        .map(|c| c.as_ref().map_or(0, |rs| rs.row_count()))
        .sum();
    assert_eq!(
        sent, ROWS as u64,
        "{}: the export reported {sent} rows sent",
        target.name
    );
    assert_eq!(
        rows, ROWS as usize,
        "{}: {rows} rows actually reached the channel",
        target.name
    );
    assert!(
        chunks.len() > 1,
        "{}: {ROWS} rows arrived as {} chunk(s) — nothing was chunked",
        target.name,
        chunks.len()
    );
    // Every chunk carries the columns, because the writer needs a header from
    // whichever chunk it happens to see first.
    for (i, chunk) in chunks.iter().enumerate() {
        let rs = chunk
            .as_ref()
            .unwrap_or_else(|e| panic!("{}: chunk {i} carried an error: {e}", target.name));
        let names: Vec<&str> = rs.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["id", "name"],
            "{}: chunk {i}'s columns",
            target.name
        );
        assert!(
            !rs.truncated,
            "{}: chunk {i} claims truncation, but an export has no cap to hit",
            target.name
        );
    }

    // **The rows themselves, in the order asked for.** Counting chunks and rows
    // says the right *number* arrived; it says nothing about whether they are
    // the table's rows, in the `ORDER BY`'s order, or the same row 5,000 times.
    // Concatenated across the chunk seams, which is where a re-fetched or
    // dropped page would show.
    let ids: Vec<String> = chunks
        .iter()
        .filter_map(|c| c.as_ref().ok())
        .flat_map(|rs| {
            (0..rs.row_count())
                .map(move |r| rs.cell(r, 0).expect("an id cell").display().to_string())
        })
        .collect();
    let want: Vec<String> = (1..=ROWS).map(|i| i.to_string()).collect();
    assert_eq!(
        ids, want,
        "{}: the ids that arrived are not 1..={ROWS} in order",
        target.name
    );

    scratch.teardown().await;
}

/// A **cancelled** export tells the writer it stopped, rather than closing the
/// channel as though the table had ended.
///
/// This is the file's own opening claim — "a channel that simply closes reads as
/// 'the table ended', so a failure that only came back from `stream_query`'s
/// return value would be a half-written file reported as finished" — and it was
/// the one failure mode with no case: every test here fails the *statement*, and
/// none of them cancels.
pub async fn a_cancelled_export_tells_the_writer_it_stopped(target: &'static Target) {
    let scratch = Scratch::create(target, "export_cancel").await;
    seed(&scratch).await;

    let cancel = CancellationToken::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let db = scratch.db.clone();
    let database = scratch.database.clone();
    let sql = format!(
        "SELECT id, name FROM {} ORDER BY id",
        scratch.qualified("big")
    );
    let token = cancel.clone();
    let export = tokio::spawn(async move {
        db.stream_query(Some(&database), &sql, CHUNK, token, tx)
            .await
    });

    // Take one chunk, then stop: the export is mid-table, which is the state a
    // Cancel is actually pressed in.
    let mut chunks = Vec::new();
    if let Some(first) = rx.recv().await {
        chunks.push(first);
    }
    cancel.cancel();
    // **Then wait before draining**, and the reason is the whole shape of a
    // cancel: it is a `select!` arm, so it can only win at a point where the
    // read *yields*. Draining immediately keeps the channel free, and 5,000 tiny
    // rows off a local MySQL then run to completion without the read ever
    // pending — the export reported `Ok(5000)` with the token already cancelled.
    // That is not a defect (a cancel cannot interrupt work that never yields,
    // and a real export is far larger), but it makes the timing the test's
    // problem: the pause lets the sender fill the channel and block, which is
    // where the cancel is seen.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    while let Some(c) = rx.recv().await {
        chunks.push(c);
    }
    let sent = export.await.expect("the export task");

    assert!(
        matches!(sent, Err(schemaic_db::DbError::Cancelled)),
        "{}: a cancelled export reported {:?}",
        target.name,
        sent.map(|n| n.to_string()).map_err(|e| e.to_string())
    );
    let rows: usize = chunks
        .iter()
        .map(|c| c.as_ref().map_or(0, |rs| rs.row_count()))
        .sum();
    assert!(
        rows < ROWS as usize,
        "{}: the cancel arrived after the whole table had been sent, so it \
         cancelled nothing",
        target.name
    );
    assert!(
        chunks.iter().any(|c| c.is_err()),
        "{}: the writer saw a closed channel and no error, so it would rename a \
         half-written file over the user's",
        target.name
    );

    scratch.teardown().await;
}

/// A statement that returns no result set is refused as an export, down the
/// channel as well as to the caller.
///
/// The export path never offers such a statement, but this is public API and the
/// next caller may not be as careful — and a writer that saw a closed channel
/// with nothing in it would produce an empty file and call it done.
pub async fn a_statement_with_no_rows_to_export_is_refused(target: &'static Target) {
    let scratch = Scratch::create(target, "export_norows").await;
    seed(&scratch).await;

    let (chunks, sent) = stream(
        &scratch,
        &format!(
            "UPDATE {} SET name = name WHERE id = 1",
            scratch.qualified("big")
        ),
    )
    .await;

    let err = sent.expect_err("a statement returning no rows must be refused");
    assert!(
        err.to_string().contains("no rows to export"),
        "{}: the refusal should say what is wrong, got {:?}",
        target.name,
        err.to_string()
    );
    assert!(
        chunks.iter().any(|c| c.is_err()),
        "{}: the writer saw a closed channel and no error, so it would call an \
         empty file finished",
        target.name
    );

    scratch.teardown().await;
}

/// A statement the server refuses reports the failure down the channel.
pub async fn a_failed_export_reports_the_failure_to_the_writer(target: &'static Target) {
    let scratch = Scratch::create(target, "export_failed").await;
    seed(&scratch).await;

    let (chunks, sent) = stream(
        &scratch,
        &format!("SELECT no_such_column FROM {}", scratch.qualified("big")),
    )
    .await;

    assert!(
        sent.is_err(),
        "{}: a refused statement reported success",
        target.name
    );
    assert!(
        chunks.iter().any(|c| c.is_err()),
        "{}: the failure never reached the writer",
        target.name
    );

    scratch.teardown().await;
}

/// Run an export and collect everything the writer would have seen.
///
/// The receiver is drained on this task while the export runs on another,
/// because the channel is bounded — that back-pressure is the mechanism keeping
/// a whole table out of memory, and a test that collected first would deadlock
/// on it.
async fn stream(
    scratch: &Scratch,
    sql: &str,
) -> (
    Vec<Result<schemaic_core::model::ResultSet, String>>,
    Result<u64, schemaic_db::DbError>,
) {
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let db = scratch.db.clone();
    let database = scratch.database.clone();
    let sql = sql.to_string();
    let handle = tokio::spawn(async move {
        db.stream_query(Some(&database), &sql, CHUNK, CancellationToken::new(), tx)
            .await
    });

    let mut chunks = Vec::new();
    while let Some(chunk) = rx.recv().await {
        chunks.push(chunk);
    }
    let sent = handle.await.expect("the export task");
    (chunks, sent)
}

/// A table with `ROWS` rows, loaded through the import path because it is the
/// fastest way to put that many there and is itself already covered.
async fn seed(scratch: &Scratch) {
    scratch
        .exec(&format!(
            "CREATE TABLE {} (id INTEGER NOT NULL PRIMARY KEY, name VARCHAR(32))",
            scratch.qualified("big")
        ))
        .await;
    let mut rows = (1..=ROWS)
        .map(|i| Ok(vec![Value::Int(i), Value::Str(format!("row {i}"))]))
        .collect::<Vec<_>>()
        .into_iter();
    let columns = ["id".to_string(), "name".to_string()];
    scratch
        .db
        .import_rows(
            schemaic_db::ImportTarget {
                database: &scratch.database,
                schema: scratch.namespace,
                table: "big",
                columns: &columns,
            },
            &mut rows,
            CancellationToken::new(),
        )
        .await
        .expect("seeding the export table");
}
