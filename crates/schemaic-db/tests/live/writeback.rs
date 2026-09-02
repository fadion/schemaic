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

use schemaic_core::model::{GridWrite, RowDelete, RowEdit, RowInsert, Value};
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
                    ("id".to_string(), Some("4".to_string())),
                    ("name".to_string(), Some("four".to_string())),
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
                    ("id".to_string(), Some("2".to_string())),
                    ("code".to_string(), Some("taken".to_string())),
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
            updates: vec![edit(
                &scratch,
                "w",
                &[("note", Some("touched"))],
                &[("name", Value::Str("dup".to_string()))],
            )],
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
            .map(|(c, v)| (c.to_string(), v.map(str::to_string)))
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
async fn one_cell(scratch: &Scratch, table: &str, sql: &str) -> String {
    let sql = sql.replace("{}", &scratch.qualified(table));
    let rs = scratch.exec(&sql).await;
    rs.cell(0, 0)
        .expect("a one-row, one-column result")
        .display()
        .to_string()
}
