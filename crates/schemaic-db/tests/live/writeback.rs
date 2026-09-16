//! Committing the grid's staged edits, and the 1-row safety net behind them.
//!
//! **The net is the only thing between an over-optimistic key and a corrupted
//! table.** `edit::analyze_edit` decides what identifies a row; if it is ever
//! wrong, the statement it produced still runs, and what stops it is
//! `one_row_verdict` seeing a count that is not 1 and rolling the batch back.
//! That count comes from the server, through a driver, under a connection flag —
//! none of which a pure test can supply. `model.rs`'s own tests assert the
//! verdict given a number; these assert the number.
//!
//! Two claims here exist only at this seam and would be invisible anywhere else:
//!
//! - **`CLIENT_FOUND_ROWS`.** MySQL reports *changed* rows by default, so an
//!   edit that sets a cell to the value it already holds affects 0 — and the net
//!   would fail a perfectly good write, roll the batch back and tell the user
//!   their row had vanished. `Db::opts` sets the flag to count *matched* rows
//!   instead. Nothing but a live server can tell the two apart.
//! - **`Rollback::note`.** MySQL's `MyISAM` accepts `BEGIN` and `ROLLBACK` and
//!   ignores both, so a failed batch leaves its earlier statements in the table
//!   while `ROLLBACK` reports success. The error has to say so; claiming
//!   "rolled back all changes" over rows that are still there is how a user
//!   re-runs an import and gets duplicates.

use schemaic_core::model::{CellEdit, GridWrite, RowDelete, RowEdit, RowInsert, Value};
use tokio_util::sync::CancellationToken;

use crate::endpoint::Target;
use crate::scratch::Scratch;

/// The table these write to: a key, a column to change, and a column to leave
/// alone so a write that touches too much is visible.
const WRITABLE: &str = "(id INTEGER NOT NULL PRIMARY KEY, name VARCHAR(32), note VARCHAR(32))";

/// A staged edit lands on exactly the row its key names, and on no other.
pub async fn a_staged_update_writes_exactly_the_row_it_names(target: &'static Target) {
    let scratch = Scratch::create(target, "update").await;
    seed_rows(&scratch, "w", WRITABLE).await;

    let written = commit(
        &scratch,
        GridWrite {
            updates: vec![edit(
                &scratch,
                "w",
                &[("name", Some("changed"))],
                &[("id", Value::Int(2))],
            )],
            ..Default::default()
        },
    )
    .await
    .unwrap_or_else(|e| panic!("{}: the commit failed: {e}", target.name));

    assert_eq!(written, 1, "{}: rows written", target.name);
    // **Both columns, not just the staged one.** `note` was never staged, so it
    // has to be exactly what it was — an update whose `SET` list carried more
    // than the dirty cells would show up here and nowhere else.
    assert_eq!(
        rows(&scratch, "w").await,
        [
            ("one".to_string(), "n1".to_string()),
            ("changed".to_string(), "n2".to_string()),
            ("three".to_string(), "n3".to_string()),
        ],
        "{}: only row 2's name should have moved",
        target.name
    );

    scratch.teardown().await;
}

/// An edit that sets a cell to the value it already holds is still one row.
///
/// **The `CLIENT_FOUND_ROWS` test.** Without the flag MySQL answers 0 here —
/// nothing *changed* — the net reads that as "the row is gone", and a user who
/// retyped the same value is told their edit matched nothing and had the rest of
/// the batch rolled back with it.
pub async fn an_update_to_an_unchanged_value_still_counts_as_one_row(target: &'static Target) {
    let scratch = Scratch::create(target, "unchanged").await;
    seed_rows(&scratch, "w", WRITABLE).await;

    let written = commit(
        &scratch,
        GridWrite {
            updates: vec![edit(
                &scratch,
                "w",
                // The value row 2 already holds.
                &[("name", Some("two"))],
                &[("id", Value::Int(2))],
            )],
            ..Default::default()
        },
    )
    .await
    .unwrap_or_else(|e| {
        panic!(
            "{}: setting a cell to its own value was refused: {e}\n\
             This is what CLIENT_FOUND_ROWS exists for — the server counted \
             changed rows, not matched ones.",
            target.name
        )
    });

    assert_eq!(written, 1, "{}: rows written", target.name);

    scratch.teardown().await;
}

/// A staged insert lands, and the columns it leaves out take their default.
pub async fn a_staged_insert_lands_with_defaults_for_what_it_omits(target: &'static Target) {
    let scratch = Scratch::create(target, "insert").await;
    seed_rows(&scratch, "w", WRITABLE).await;

    commit(
        &scratch,
        GridWrite {
            inserts: vec![RowInsert {
                database: scratch.database.clone(),
                schema: scratch.namespace.map(str::to_string),
                table: "w".to_string(),
                cols: vec![
                    ("id".to_string(), CellEdit::Text("4".to_string())),
                    ("name".to_string(), CellEdit::Text("four".to_string())),
                ],
            }],
            ..Default::default()
        },
    )
    .await
    .unwrap_or_else(|e| panic!("{}: the insert failed: {e}", target.name));

    assert_eq!(
        names(&scratch, "w").await,
        ["one", "two", "three", "four"],
        "{}: the new row",
        target.name
    );
    let note = one_cell(&scratch, "w", "SELECT note FROM {} WHERE id = 4").await;
    assert_eq!(
        note, "NULL",
        "{}: an omitted column should take its default, not an empty string",
        target.name
    );

    scratch.teardown().await;
}

/// A staged delete removes its row and leaves the rest.
pub async fn a_staged_delete_removes_exactly_its_row(target: &'static Target) {
    let scratch = Scratch::create(target, "delete").await;
    seed_rows(&scratch, "w", WRITABLE).await;

    commit(
        &scratch,
        GridWrite {
            deletes: vec![RowDelete {
                database: scratch.database.clone(),
                schema: scratch.namespace.map(str::to_string),
                table: "w".to_string(),
                key: vec![("id".to_string(), Value::Int(1))],
            }],
            ..Default::default()
        },
    )
    .await
    .unwrap_or_else(|e| panic!("{}: the delete failed: {e}", target.name));

    assert_eq!(
        names(&scratch, "w").await,
        ["two", "three"],
        "{}: what survived",
        target.name
    );

    scratch.teardown().await;
}

/// Setting a cell to `None` writes SQL `NULL`, not the four letters.
///
/// The grid renders a NULL as the word `NULL`, so a path that carried the
/// rendering instead of the value would store a string that reads correctly in
/// the grid it came from and is wrong everywhere else — including in the very
/// `IS NULL` filter the user would use to find it.
pub async fn a_staged_null_is_written_as_a_null(target: &'static Target) {
    let scratch = Scratch::create(target, "null").await;
    seed_rows(&scratch, "w", WRITABLE).await;

    commit(
        &scratch,
        GridWrite {
            updates: vec![edit(
                &scratch,
                "w",
                &[("name", None)],
                &[("id", Value::Int(2))],
            )],
            ..Default::default()
        },
    )
    .await
    .unwrap_or_else(|e| panic!("{}: the commit failed: {e}", target.name));

    let nulls = one_cell(
        &scratch,
        "w",
        "SELECT COUNT(*) FROM {} WHERE id = 2 AND name IS NULL",
    )
    .await;
    assert_eq!(
        nulls, "1",
        "{}: the cell did not become a real NULL",
        target.name
    );

    scratch.teardown().await;
}

/// Deletes run before inserts, so a row can be replaced by one carrying the same
/// unique key in a single batch.
///
/// `GridWrite::plan`'s ordering is assertable without a server and its
/// *consequence* is not: the unique index is the thing that would reject the
/// insert, and it only exists on a real one.
pub async fn deletes_run_before_inserts_so_a_unique_key_can_be_reused(target: &'static Target) {
    let scratch = Scratch::create(target, "reuse_key").await;
    scratch
        .exec(&format!(
            "CREATE TABLE {} (id INTEGER NOT NULL PRIMARY KEY, code VARCHAR(16) NOT NULL UNIQUE)",
            scratch.qualified("u")
        ))
        .await;
    scratch
        .exec(&format!(
            "INSERT INTO {} (id, code) VALUES (1, 'taken')",
            scratch.qualified("u")
        ))
        .await;

    commit(
        &scratch,
        GridWrite {
            deletes: vec![RowDelete {
                database: scratch.database.clone(),
                schema: scratch.namespace.map(str::to_string),
                table: "u".to_string(),
                key: vec![("id".to_string(), Value::Int(1))],
            }],
            inserts: vec![RowInsert {
                database: scratch.database.clone(),
                schema: scratch.namespace.map(str::to_string),
                table: "u".to_string(),
                cols: vec![
                    ("id".to_string(), CellEdit::Text("2".to_string())),
                    ("code".to_string(), CellEdit::Text("taken".to_string())),
                ],
            }],
            ..Default::default()
        },
    )
    .await
    .unwrap_or_else(|e| {
        panic!(
            "{}: reusing a unique key in one batch failed: {e}\n\
             The insert ran before the delete — GridWrite::plan's order did not hold.",
            target.name
        )
    });

    let ids = one_cell(&scratch, "u", "SELECT id FROM {} WHERE code = 'taken'").await;
    assert_eq!(ids, "2", "{}: the surviving row", target.name);

    scratch.teardown().await;
}

/// A key that matches no row fails the batch, and undoes what ran before it.
pub async fn a_key_that_matches_no_row_fails_the_batch_and_undoes_the_rest(
    target: &'static Target,
) {
    let scratch = Scratch::create(target, "no_match").await;
    seed_rows(&scratch, "w", WRITABLE).await;

    let err = commit(
        &scratch,
        GridWrite {
            updates: vec![
                // Runs first and succeeds.
                edit(
                    &scratch,
                    "w",
                    &[("name", Some("changed"))],
                    &[("id", Value::Int(1))],
                ),
                // Then this one, whose row does not exist.
                edit(
                    &scratch,
                    "w",
                    &[("name", Some("ghost"))],
                    &[("id", Value::Int(99))],
                ),
            ],
            ..Default::default()
        },
    )
    .await
    .expect_err("a key matching no row must fail the batch");

    let text = err.to_string();
    assert!(
        text.contains("affected 0 rows"),
        "{}: the error should say what the guard saw, got {text:?}",
        target.name
    );
    assert_eq!(
        names(&scratch, "w").await,
        ["one", "two", "three"],
        "{}: the successful statement before it was not undone",
        target.name
    );

    scratch.teardown().await;
}

/// A key that matches two rows fails the batch rather than rewriting both.
///
/// This is the failure the net exists for. `analyze_edit` would not choose a
/// non-unique column as a key — but "would not" is a property of code that can
/// change, and the whole point of a safety net is that it holds when the thing
/// above it is wrong.
pub async fn a_key_that_matches_two_rows_fails_the_batch_and_undoes_the_rest(
    target: &'static Target,
) {
    let scratch = Scratch::create(target, "two_matches").await;
    seed_rows(&scratch, "w", WRITABLE).await;
    scratch
        .exec(&format!(
            "UPDATE {} SET name = 'dup' WHERE id IN (1, 3)",
            scratch.qualified("w")
        ))
        .await;

    let err = commit(
        &scratch,
        GridWrite {
            updates: vec![
                // **Runs first and succeeds**, so there is a "rest" for the
                // refusal to undo. The batch used to hold the doomed statement
                // alone, which made the name and the doc a promise the body did
                // not keep: what was asserted was that the *offending*
                // statement left nothing behind, and its 0-row sibling above
                // does stage a preceding write and check it was reverted. The
                // two reach the rollback from different places — this one from
                // `one_row_verdict` after a **successful** `exec_drop` — so a
                // rollback dropped from the verdict arm while the batch still
                // held earlier work left this test green.
                edit(
                    &scratch,
                    "w",
                    &[("note", Some("first"))],
                    &[("id", Value::Int(2))],
                ),
                edit(
                    &scratch,
                    "w",
                    &[("note", Some("touched"))],
                    &[("name", Value::Str("dup".to_string()))],
                ),
            ],
            ..Default::default()
        },
    )
    .await
    .expect_err("a key matching two rows must fail the batch");

    let text = err.to_string();
    assert!(
        text.contains("affected 2 rows"),
        "{}: the error should say what the guard saw, got {text:?}",
        target.name
    );
    let touched = one_cell(
        &scratch,
        "w",
        "SELECT COUNT(*) FROM {} WHERE note = 'touched'",
    )
    .await;
    assert_eq!(
        touched, "0",
        "{}: rows were rewritten despite the refusal",
        target.name
    );
    // …and the statement that ran *before* the refusal is gone too — the half
    // the name promises and nothing checked.
    assert_eq!(
        rows(&scratch, "w").await,
        [
            ("dup".to_string(), "n1".to_string()),
            ("two".to_string(), "n2".to_string()),
            ("dup".to_string(), "n3".to_string()),
        ],
        "{}: the successful statement before the refusal was not undone",
        target.name
    );

    scratch.teardown().await;
}

/// A failed batch says what the rollback actually achieved — and on a table
/// whose storage engine ignores `ROLLBACK`, it admits the rows are still there.
///
/// Both halves where the server has both. On PostgreSQL there is no
/// non-transactional table to write, so only the first half runs, and it is a
/// real assertion rather than a skip: the promise `Rollback::Complete` makes is
/// exactly the one that engine always keeps.
pub async fn a_failed_batch_says_what_the_rollback_actually_undid(target: &'static Target) {
    let scratch = Scratch::create(target, "rollback").await;
    seed_rows(&scratch, "w", WRITABLE).await;

    let err = commit(&scratch, doomed_batch(&scratch, "w"))
        .await
        .expect_err("the batch must fail");
    assert!(
        err.to_string().contains("rolled back all changes"),
        "{}: a transactional table should promise a complete rollback, got {:?}",
        target.name,
        err.to_string()
    );
    assert_eq!(
        names(&scratch, "w").await,
        ["one", "two", "three"],
        "{}: the batch was not undone",
        target.name
    );

    let Some(clause) = target.non_transactional else {
        scratch.teardown().await;
        return;
    };

    // The same batch against a table that accepts BEGIN and ignores it.
    scratch
        .exec(&format!(
            "CREATE TABLE {} {WRITABLE} {clause}",
            scratch.qualified("m")
        ))
        .await;
    scratch
        .exec(&format!(
            "INSERT INTO {} (id, name) VALUES (1, 'one'), (2, 'two'), (3, 'three')",
            scratch.qualified("m")
        ))
        .await;

    let err = commit(&scratch, doomed_batch(&scratch, "m"))
        .await
        .expect_err("the batch must fail");
    let text = err.to_string();
    assert!(
        text.contains("did NOT undo them"),
        "{}: a {clause} table cannot roll back, and the error claimed otherwise: {text:?}",
        target.name
    );
    assert_eq!(
        names(&scratch, "m").await,
        ["changed", "two", "three"],
        "{}: the surviving write is what the error has to admit to",
        target.name
    );

    scratch.teardown().await;
}

/// **A cancelled commit reports "nothing was written" only when nothing was.**
///
/// The twin of `a_cancelled_import_on_a_non_transactional_table_says_the_rows_remain`,
/// and it exists for the same reason one layer down: `DbError::Cancelled` is the
/// variant the modal renders as *nothing was written*, and on a `MyISAM` table
/// every statement already executed is durable.
///
/// What this can only be asked of a server is whether the `ROLLBACK` that
/// licenses that claim is heard at all. It was not: the cancel was a
/// `tokio::select!` around the whole write, whose arm runs after the branch
/// future is **dropped** — mid-statement — which desynchronises `mysql_async`'s
/// result stream, so the `ROLLBACK` and its `SHOW WARNINGS` read replies that
/// were not their own and `Rollback::Complete` was classified off them. The same
/// construct was removed from `import_rows` for the same measured reason.
///
/// The batch is long rather than slow: each staged edit is its own round trip,
/// so a few thousand of them run for well over the cancel's delay and the token
/// is guaranteed to fire with statements both behind and ahead of it. There is
/// no client-side pacing lever here the way `slow_rows` gives the import — a
/// `GridWrite` arrives fully built.
pub async fn a_cancelled_commit_on_a_non_transactional_table_says_the_rows_remain(
    target: &'static Target,
) {
    let Some(clause) = target.non_transactional else {
        return;
    };
    const ROWS: i64 = 4000;
    let scratch = Scratch::create(target, "commitcancel").await;
    scratch
        .exec(&format!(
            "CREATE TABLE {} {WRITABLE} {clause}",
            scratch.qualified("m")
        ))
        .await;
    let seed: Vec<String> = (1..=ROWS).map(|i| format!("({i}, 'before')")).collect();
    scratch
        .exec(&format!(
            "INSERT INTO {} (id, name) VALUES {}",
            scratch.qualified("m"),
            seed.join(", ")
        ))
        .await;

    let cancel = CancellationToken::new();
    let armed = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        armed.cancel();
    });

    let updates = (1..=ROWS)
        .map(|i| {
            edit(
                &scratch,
                "m",
                &[("name", Some("after"))],
                &[("id", Value::Int(i))],
            )
        })
        .collect();
    let outcome = scratch
        .db
        .commit_writes(
            &GridWrite {
                updates,
                ..Default::default()
            },
            cancel,
        )
        .await;

    let changed = one_cell(
        &scratch,
        "m",
        "SELECT COUNT(*) FROM {} WHERE name = 'after'",
    )
    .await;
    match outcome {
        // The whole batch landed before the token fired — the timing missed, and
        // there is nothing to assert about a cancel that did not happen.
        Ok(_) => {}
        Err(schemaic_db::DbError::Cancelled) => assert_eq!(
            changed, "0",
            "{}: the commit claimed nothing was written, and {changed} rows of a \
             {clause} table were changed",
            target.name
        ),
        Err(e) => assert!(
            e.to_string().contains("did NOT undo") || changed == "0",
            "{}: {changed} rows changed and the cancel neither undid them nor \
             admitted it: {e}",
            target.name
        ),
    }

    scratch.teardown().await;
}

/// An empty batch writes nothing and says so, without opening a transaction.
pub async fn an_empty_batch_writes_nothing(target: &'static Target) {
    let scratch = Scratch::create(target, "empty").await;
    seed_rows(&scratch, "w", WRITABLE).await;

    let written = commit(&scratch, GridWrite::default())
        .await
        .unwrap_or_else(|e| panic!("{}: an empty batch failed: {e}", target.name));
    assert_eq!(written, 0, "{}: rows written", target.name);
    assert_eq!(
        names(&scratch, "w").await,
        ["one", "two", "three"],
        "{}: an empty batch touched the table",
        target.name
    );

    scratch.teardown().await;
}

/// One statement that lands followed by one that cannot — the shape every
/// rollback claim is made about.
fn doomed_batch(scratch: &Scratch, table: &str) -> GridWrite {
    GridWrite {
        updates: vec![
            edit(
                scratch,
                table,
                &[("name", Some("changed"))],
                &[("id", Value::Int(1))],
            ),
            edit(
                scratch,
                table,
                &[("name", Some("ghost"))],
                &[("id", Value::Int(99))],
            ),
        ],
        ..Default::default()
    }
}

/// Create `table` with `ddl` and put three named rows in it.
async fn seed_rows(scratch: &Scratch, table: &str, ddl: &str) {
    scratch
        .exec(&format!("CREATE TABLE {} {ddl}", scratch.qualified(table)))
        .await;
    // `note` is seeded, not left NULL, so it is a value a write could *change*.
    // `WRITABLE`'s comment calls it "a column to leave alone" and nothing ever
    // read it back, so an update that put every staged column in its `SET` list
    // — or one that reset the unstaged ones — would have passed.
    scratch
        .exec(&format!(
            "INSERT INTO {} (id, name, note) VALUES \
             (1, 'one', 'n1'), (2, 'two', 'n2'), (3, 'three', 'n3')",
            scratch.qualified(table)
        ))
        .await;
}

/// **The statement that runs immediately after a commit, and writes straight
/// into the grid the user is looking at.**
///
/// Ten live tests cover `commit_writes`; nothing covered `refetch_rows`, which
/// runs next and whose rows are spliced over the ones on screen. `grep -rn
/// refetch tests/live` found two prose comments and no code — so the whole leg
/// between the write and the visible cell was untested against a server.
///
/// The property is the one the splice exists to keep: **a re-fetched row is the
/// row a fresh `SELECT` would show.** Anything else and the grid disagrees with
/// itself about a row nobody edited, and a CSV or clipboard export taken between
/// the two writes out the wrong text.
///
/// Composed the way the app composes it — `edit::refetch_template` +
/// `edit::refetch_key`, the same two functions `GridState::build_refetch` calls
/// — because the defect this catches lives in the protocol the *re-fetch* uses,
/// not in either function alone.
pub async fn a_spliced_row_is_the_row_a_fresh_select_would_show(target: &'static Target) {
    let scratch = Scratch::create(target, "refetch").await;
    let t = scratch.qualified("t");
    // **`ZEROFILL` is the MySQL family's own instance of this defect**, and the
    // sharpest one: the text protocol sends `0007` and the binary protocol
    // sends the number 7, so the column is in the table only where the engine
    // has the attribute. PostgreSQL has nothing to add here.
    let padded = !matches!(
        target.engine.dialect(),
        schemaic_core::intel::SqlDialect::Postgres
    );
    let pad_col = if padded { ", pad" } else { "" };
    // Shapes whose *text* form the binary protocol does not reproduce on its
    // own: a datetime with a zero time, a time-of-day, and a 32-bit float.
    // Deliberately awkward, for the reason the DDL shapes are.
    let ddl = match target.engine.dialect() {
        schemaic_core::intel::SqlDialect::Postgres => format!(
            "CREATE TABLE {t} (id INTEGER PRIMARY KEY, note VARCHAR(20), \
             due TIMESTAMP, micros TIMESTAMP(3), plain DATE, dur TIME, \
             ratio REAL, exact DOUBLE PRECISION)"
        ),
        _ => format!(
            "CREATE TABLE {t} (id INT PRIMARY KEY, note VARCHAR(20), \
             due DATETIME, micros DATETIME(3), plain DATE, dur TIME, \
             ratio FLOAT, exact DOUBLE, pad INT(4) UNSIGNED ZEROFILL)"
        ),
    };
    scratch.exec(&ddl).await;
    // A zero time on a `DATETIME` (the one `as_sql` drops), a declared
    // fractional precision (which must be printed, and not to six places), a
    // bare `DATE` (which must *not* grow a time), a duration past midnight, and
    // a float whose `f64` widening is visible.
    scratch
        .exec(&format!(
            "INSERT INTO {t} (id, note, due, micros, plain, dur, ratio, exact{pad_col}) \
             VALUES (1, 'a', '2024-01-15 00:00:00', '2024-01-15 08:09:10.120', \
             '2024-01-15', '10:30:00', 3.14, 3.14{})",
            if padded { ", 7" } else { "" }
        ))
        .await;

    let select = format!(
        "SELECT id, note, due, micros, plain, dur, ratio, exact{pad_col} FROM {t} ORDER BY id"
    );
    let (rs, model) = scratch.edit_model(&select).await;
    let template = schemaic_core::edit::refetch_template(&rs, &model).expect("a single base table");

    // Stage a change on the one column the assertion is *not* about, so every
    // other cell is untouched by the write and can only differ because the
    // re-fetch read it differently from the load.
    let note_ci = rs
        .columns
        .iter()
        .position(|c| c.name == "note")
        .expect("the note column");
    let mut edited = std::collections::HashMap::new();
    edited.insert(note_ci, CellEdit::Text("b".to_string()));
    let key = schemaic_core::edit::refetch_key(&template, &rs, 0, &edited);

    let write = GridWrite {
        updates: vec![edit(
            &scratch,
            "t",
            &[("note", Some("b"))],
            &[("id", Value::Int(1))],
        )],
        ..Default::default()
    };
    commit(&scratch, write).await.expect("the update commits");

    let spliced = scratch
        .db
        .refetch_rows(
            &template,
            &[schemaic_core::model::RefetchRow { data_row: 0, key }],
            CancellationToken::new(),
        )
        .await
        .expect("the re-fetch runs");
    let (_, cells) = spliced.first().expect("one row came back");

    // What a fresh load shows for the same row — the same path the grid used to
    // draw it in the first place.
    let fresh = scratch.exec(&select).await;
    for (ci, col) in rs.columns.iter().enumerate() {
        let want = fresh.cell(0, ci).expect("a cell").display().to_string();
        let got = cells[ci].display();
        assert_eq!(
            got, want,
            "{}: column {} came back from the re-fetch as {got:?} and from a \
             fresh SELECT as {want:?} — the grid would disagree with itself \
             about a cell nobody edited",
            target.name, col.name
        );
    }

    scratch.teardown().await;
}

fn edit(
    scratch: &Scratch,
    table: &str,
    set: &[(&str, Option<&str>)],
    key: &[(&str, Value)],
) -> RowEdit {
    RowEdit {
        database: scratch.database.clone(),
        schema: scratch.namespace.map(str::to_string),
        table: table.to_string(),
        set: set
            .iter()
            .map(|(c, v)| (c.to_string(), CellEdit::from_opt(v.map(str::to_string))))
            .collect(),
        key: key
            .iter()
            .map(|(c, v)| (c.to_string(), v.clone()))
            .collect(),
    }
}

async fn commit(scratch: &Scratch, write: GridWrite) -> Result<u64, schemaic_db::DbError> {
    scratch
        .db
        .commit_writes(&write, CancellationToken::new())
        .await
}

/// The `name` column of every row, in key order — the cheapest way to say what a
/// write did and, more to the point, what it did not do.
async fn names(scratch: &Scratch, table: &str) -> Vec<String> {
    let rs = scratch
        .exec(&format!(
            "SELECT name FROM {} ORDER BY id",
            scratch.qualified(table)
        ))
        .await;
    (0..rs.row_count())
        .map(|r| {
            rs.cell(r, 0)
                .expect("a selected cell")
                .display()
                .to_string()
        })
        .collect()
}

/// The `(name, note)` pair of every row, in key order — [`names`] plus the
/// column a write must leave alone.
async fn rows(scratch: &Scratch, table: &str) -> Vec<(String, String)> {
    let rs = scratch
        .exec(&format!(
            "SELECT name, note FROM {} ORDER BY id",
            scratch.qualified(table)
        ))
        .await;
    (0..rs.row_count())
        .map(|r| {
            let cell = |c| {
                rs.cell(r, c)
                    .expect("a selected cell")
                    .display()
                    .to_string()
            };
            (cell(0), cell(1))
        })
        .collect()
}

/// One cell from a one-row query. `sql` carries a single `{}` where the
/// qualified `table` goes.
/// A refused batch inside a **manual transaction** undoes only itself: the
/// transaction survives, and the statements the user already ran in it stay.
///
/// **The Manual-mode write path had no live test at all.** Every other test
/// here reaches `write_on` with `TxScope::Transaction`, through
/// `Db::commit_writes` on a fresh connection. A Manual tab's Commit goes
/// through `Session::commit_writes`, which runs the same `write_on` with
/// `TxScope::Savepoint` — a different `begin_sql`/`rollback_sql`/`commit_sql`
/// triple, where the undo is `ROLLBACK TO SAVEPOINT` and MySQL's warning-1196
/// reading is being applied to a savepoint rather than to a transaction. So the
/// two claims this file's header says "exist only at this seam" were asserted
/// for the outer transaction and not for the nested one.
///
/// The property is `StmtOutcome::FailedIsolated`'s, and it is the one the
/// savepoint exists to make true: on PostgreSQL a bare failure aborts the whole
/// transaction, and reporting the isolated case as the bare one tells a user
/// their uncommitted work is lost and offers only the action that loses it. So
/// all three halves are asserted — the batch is undone, the earlier statement
/// is not, and the transaction is still committable — and the commit at the end
/// is what proves the third from outside.
pub async fn a_refused_write_in_a_transaction_undoes_only_itself(target: &'static Target) {
    use crate::runtime::OpenSession;
    use schemaic_core::tx::StmtOutcome;
    use schemaic_db::Session;

    let scratch = Scratch::create(target, "savepoint_write").await;
    seed_rows(&scratch, "w", WRITABLE).await;

    let session = OpenSession(Some(
        Session::open(&scratch.db, Some(&scratch.database))
            .await
            .unwrap_or_else(|e| panic!("{}: could not pin a session: {e}", target.name)),
    ));
    session
        .ensure_tx()
        .await
        .unwrap_or_else(|e| panic!("{}: could not begin: {e}", target.name));

    // What the user has already done in this transaction, and what the refused
    // batch below must not take with it.
    session
        .fetch_query(
            &format!(
                "UPDATE {} SET note = 'in tx' WHERE id = 1",
                scratch.qualified("w")
            ),
            10,
            CancellationToken::new(),
        )
        .await
        .result
        .unwrap_or_else(|e| panic!("{}: the in-transaction update failed: {e}", target.name));

    let out = session
        .commit_writes(
            &GridWrite {
                updates: vec![
                    // Succeeds, and is part of what the savepoint must undo.
                    edit(
                        &scratch,
                        "w",
                        &[("name", Some("changed"))],
                        &[("id", Value::Int(2))],
                    ),
                    // Then this, whose row does not exist.
                    edit(
                        &scratch,
                        "w",
                        &[("name", Some("ghost"))],
                        &[("id", Value::Int(99))],
                    ),
                ],
                ..Default::default()
            },
            CancellationToken::new(),
        )
        .await;

    let err = out
        .result
        .as_ref()
        .err()
        .unwrap_or_else(|| panic!("{}: a key matching no row must fail the batch", target.name))
        .to_string();
    assert!(
        err.contains("affected 0 rows"),
        "{}: the error should say what the guard saw, got {err:?}",
        target.name
    );
    assert_eq!(
        out.stmt,
        StmtOutcome::FailedIsolated,
        "{}: a savepoint-isolated failure was reported as {:?}, which tells the \
         user the whole transaction is lost",
        target.name,
        out.stmt
    );

    // Through the session's own connection, which is the only place the
    // uncommitted statement is visible at all.
    let seen = session
        .fetch_query(
            &format!(
                "SELECT name, note FROM {} ORDER BY id",
                scratch.qualified("w")
            ),
            10,
            CancellationToken::new(),
        )
        .await
        .result
        .unwrap_or_else(|e| panic!("{}: reading back inside the tx failed: {e}", target.name));
    let inside: Vec<(String, String)> = (0..seen.row_count())
        .map(|r| {
            let cell = |c| {
                seen.cell(r, c)
                    .expect("a selected cell")
                    .display()
                    .to_string()
            };
            (cell(0), cell(1))
        })
        .collect();
    assert_eq!(
        inside,
        [
            ("one".to_string(), "in tx".to_string()),
            ("two".to_string(), "n2".to_string()),
            ("three".to_string(), "n3".to_string()),
        ],
        "{}: the savepoint rollback did not leave the transaction where it was",
        target.name
    );

    // And the transaction is still committable, which is the claim
    // `FailedIsolated` makes and the one a caller acts on.
    session
        .commit()
        .await
        .unwrap_or_else(|e| panic!("{}: the transaction was not committable: {e}", target.name));
    assert_eq!(
        rows(&scratch, "w").await,
        [
            ("one".to_string(), "in tx".to_string()),
            ("two".to_string(), "n2".to_string()),
            ("three".to_string(), "n3".to_string()),
        ],
        "{}: what a fresh connection sees after the commit",
        target.name
    );
    drop(session);

    scratch.teardown().await;
}

async fn one_cell(scratch: &Scratch, table: &str, sql: &str) -> String {
    let sql = sql.replace("{}", &scratch.qualified(table));
    let rs = scratch.exec(&sql).await;
    rs.cell(0, 0)
        .expect("a one-row, one-column result")
        .display()
        .to_string()
}
