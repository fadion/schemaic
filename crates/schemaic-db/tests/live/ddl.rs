//! The table designer's round trip: introspect → draft → diff → emit → run →
//! introspect again.
//!
//! **The property every test here is a case of:** a table read off the server,
//! turned into a draft, changed, applied, and read back must diff to *nothing*
//! against the draft that was asked for. It is worth stating that way rather
//! than as "the ALTER worked", because the failure it catches is the asymmetric
//! one — an emitter that writes something the introspector reads back
//! differently. Such a table is *correct on the server* and permanently dirty in
//! the designer: it offers to apply the same change again, every time it is
//! opened, and applying it changes nothing. Neither half is wrong alone, which
//! is why neither half's own tests find it.
//!
//! The SQLite suite already asserts this shape
//! (`an_introspected_table_diffs_to_nothing_against_its_own_draft`); these are
//! the two engines where it could not be asserted at all.
//!
//! What is **not** here: views, triggers and the object drafts around them. A
//! view column's provenance and a trigger's restatement diverge by engine in
//! ways that need their own cases, and folding them in here would mean pinning
//! whatever these three servers happen to answer today.

use schemaic_core::ddl::{self, ColumnDraft, TableDraft};
use schemaic_core::schema::{ColumnInfo, TableInfo};
use tokio_util::sync::CancellationToken;

use crate::endpoint::Target;
use crate::scratch::Scratch;

/// Table shapes the designer must be able to read and restate unchanged.
///
/// Deliberately awkward rather than representative: a plain two-column table
/// round-trips through almost any implementation, and proves correspondingly
/// little.
const SHAPES: &[(&str, &str)] = &[
    (
        "plain",
        "(id INTEGER NOT NULL PRIMARY KEY, name VARCHAR(32))",
    ),
    (
        "defaults",
        "(id INTEGER NOT NULL PRIMARY KEY, n INTEGER DEFAULT 7, s VARCHAR(8) DEFAULT 'x')",
    ),
    (
        "nullability",
        "(id INTEGER NOT NULL PRIMARY KEY, required VARCHAR(8) NOT NULL, optional VARCHAR(8))",
    ),
    (
        "unique_index",
        "(id INTEGER NOT NULL PRIMARY KEY, code VARCHAR(16) NOT NULL UNIQUE, name VARCHAR(32))",
    ),
    (
        "composite_key",
        "(a INTEGER NOT NULL, b INTEGER NOT NULL, v VARCHAR(8), PRIMARY KEY (a, b))",
    ),
];

/// A table read off the server and drafted straight back proposes no change.
///
/// The first thing that must hold, and the one most likely not to: any asymmetry
/// between what the introspector reads and what the emitter would write shows up
/// here as a change nobody asked for.
pub async fn an_introspected_table_diffs_to_nothing_against_its_own_draft(target: &'static Target) {
    let scratch = Scratch::create(target, "ddl_identity").await;
    let mut failures = Vec::new();

    for (name, ddl_sql) in SHAPES {
        scratch
            .exec(&format!(
                "CREATE TABLE {} {ddl_sql}",
                scratch.qualified(name)
            ))
            .await;
        let current = table_of(&scratch, name).await;
        let draft = TableDraft::from_table(&current);
        let set = ddl::diff(&current, &draft, target.engine.dialect());
        if !set.changes.is_empty() {
            failures.push(format!(
                "{name}: proposed {:?}\n      emitting {:?}",
                set.changes,
                set.emit()
            ));
        }
    }

    scratch.teardown().await;
    assert!(
        failures.is_empty(),
        "{}: {} of {} table shapes did not survive introspect → draft → diff:\n  {}",
        target.name,
        failures.len(),
        SHAPES.len(),
        failures.join("\n  ")
    );
}

/// A column added through the designer lands, and the table reads back as the
/// draft that asked for it.
pub async fn an_added_column_lands_and_reads_back_as_drafted(target: &'static Target) {
    let scratch = Scratch::create(target, "ddl_add").await;
    scratch
        .exec(&format!(
            "CREATE TABLE {} (id INTEGER NOT NULL PRIMARY KEY, name VARCHAR(32))",
            scratch.qualified("t")
        ))
        .await;

    let current = table_of(&scratch, "t").await;
    let mut draft = TableDraft::from_table(&current);
    draft.columns.push(ColumnDraft::new(ColumnInfo {
        name: "added".to_string(),
        type_name: "INTEGER".to_string(),
        nullable: true,
        ..Default::default()
    }));

    apply(&scratch, &current, &draft, target).await;
    assert_settled(&scratch, "t", target, "adding a column").await;
    assert!(
        column_names(&table_of(&scratch, "t").await).contains(&"added".to_string()),
        "{}: the column is not on the server",
        target.name
    );

    scratch.teardown().await;
}

/// A column dropped through the designer goes, and nothing else moves.
pub async fn a_dropped_column_goes_and_the_rest_stays(target: &'static Target) {
    let scratch = Scratch::create(target, "ddl_drop").await;
    scratch
        .exec(&format!(
            "CREATE TABLE {} (id INTEGER NOT NULL PRIMARY KEY, keep VARCHAR(8), go VARCHAR(8))",
            scratch.qualified("t")
        ))
        .await;

    let current = table_of(&scratch, "t").await;
    let mut draft = TableDraft::from_table(&current);
    draft.columns.retain(|c| c.info.name != "go");

    apply(&scratch, &current, &draft, target).await;
    assert_settled(&scratch, "t", target, "dropping a column").await;
    assert_eq!(
        column_names(&table_of(&scratch, "t").await),
        ["id", "keep"],
        "{}: what is left",
        target.name
    );

    scratch.teardown().await;
}

/// A renamed column keeps its data and reads back under the new name.
///
/// The rename is the change that has to carry `ColumnDraft::original`: the diff
/// reads the old name from it, and a draft that lost it would emit a *drop and
/// add* instead — same shape on the server, and the column's data gone.
pub async fn a_renamed_column_keeps_its_data(target: &'static Target) {
    let scratch = Scratch::create(target, "ddl_rename").await;
    scratch
        .exec(&format!(
            "CREATE TABLE {} (id INTEGER NOT NULL PRIMARY KEY, before_name VARCHAR(8))",
            scratch.qualified("t")
        ))
        .await;
    scratch
        .exec(&format!(
            "INSERT INTO {} (id, before_name) VALUES (1, 'kept')",
            scratch.qualified("t")
        ))
        .await;

    let current = table_of(&scratch, "t").await;
    let mut draft = TableDraft::from_table(&current);
    // Through `rename_column`, not by assigning the name: it is what keeps
    // `original` (the identity a rename is read from) and the key bookkeeping in
    // step, and assigning around it is how a rename becomes a drop-plus-add.
    let idx = draft
        .columns
        .iter()
        .position(|c| c.info.name == "before_name")
        .unwrap_or_else(|| panic!("{}: the draft lost the column", target.name));
    draft.rename_column(idx, "after_name");

    apply(&scratch, &current, &draft, target).await;
    assert_settled(&scratch, "t", target, "renaming a column").await;

    let rs = scratch
        .exec(&format!(
            "SELECT after_name FROM {} WHERE id = 1",
            scratch.qualified("t")
        ))
        .await;
    assert_eq!(
        rs.cell(0, 0).map(|c| c.display().to_string()),
        Some("kept".to_string()),
        "{}: a rename that lost the data emitted a drop and an add",
        target.name
    );

    scratch.teardown().await;
}

/// A retyped column lands and reads back as the type that was drafted.
pub async fn a_retyped_column_reads_back_as_the_new_type(target: &'static Target) {
    let scratch = Scratch::create(target, "ddl_retype").await;
    scratch
        .exec(&format!(
            "CREATE TABLE {} (id INTEGER NOT NULL PRIMARY KEY, n VARCHAR(8))",
            scratch.qualified("t")
        ))
        .await;

    let current = table_of(&scratch, "t").await;
    let mut draft = TableDraft::from_table(&current);
    let col = draft
        .columns
        .iter_mut()
        .find(|c| c.info.name == "n")
        .unwrap_or_else(|| panic!("{}: the draft lost the column", target.name));
    col.info.type_name = "VARCHAR(64)".to_string();

    apply(&scratch, &current, &draft, target).await;
    assert_settled(&scratch, "t", target, "retyping a column").await;

    // Through `ddl::types_equal` rather than a string compare: `VARCHAR(64)`
    // comes back as `varchar(64)` on one server and `character varying(64)` on
    // another, and the comparator that already knows this is the one the diff
    // itself uses.
    let after = table_of(&scratch, "t").await;
    let col = after
        .columns
        .iter()
        .find(|c| c.name == "n")
        .unwrap_or_else(|| panic!("{}: the column is gone", target.name));
    assert!(
        ddl::types_equal(&col.type_name, "VARCHAR(64)", target.engine.dialect()),
        "{}: the column reads back as {:?}, not the drafted VARCHAR(64)",
        target.name,
        col.type_name
    );

    scratch.teardown().await;
}

/// A plan the server refuses reports where it stopped, and how much of it
/// survived.
///
/// **The two engines answer differently and both answers are right.**
/// PostgreSQL's DDL is transactional and `pg::run_ddl` wraps the plan, so a
/// refused plan leaves *nothing* behind and `applied` is 0. MySQL and MariaDB
/// commit each `ALTER` as it runs, so the statements before the failure are on
/// the table for good — which is the whole reason `DdlError::applied` exists and
/// the preview reports it. A test that asserted one number would have been
/// wrong on two servers out of three.
pub async fn a_refused_plan_says_where_it_stopped(target: &'static Target) {
    let scratch = Scratch::create(target, "ddl_refused").await;
    scratch
        .exec(&format!(
            "CREATE TABLE {} (id INTEGER NOT NULL PRIMARY KEY)",
            scratch.qualified("t")
        ))
        .await;

    let table = scratch.qualified("t");
    let err = scratch
        .db
        .run_ddl(
            &scratch.database,
            &[
                format!("ALTER TABLE {table} ADD COLUMN good INTEGER"),
                // No such type, on any of them.
                format!("ALTER TABLE {table} ADD COLUMN bad NOSUCHTYPE"),
                format!("ALTER TABLE {table} ADD COLUMN never INTEGER"),
            ],
            CancellationToken::new(),
        )
        .await
        .expect_err("the server must refuse the second statement");

    // Which statement failed is the same everywhere; what survived is not.
    assert_eq!(err.at, 1, "{}: the statement that failed", target.name);

    let names = column_names(&table_of(&scratch, "t").await);
    let survived = names.contains(&"good".to_string());
    if target.transactional_ddl {
        assert_eq!(
            err.applied, 0,
            "{}: a transactional plan that failed reported statements applied",
            target.name
        );
        assert!(
            !survived,
            "{}: the plan rolled back, yet the column is there: {names:?}",
            target.name
        );
    } else {
        assert_eq!(
            err.applied, 1,
            "{}: the statement before the failure is on the table and must be counted",
            target.name
        );
        assert!(
            survived,
            "{}: the first statement should have applied, columns are {names:?}",
            target.name
        );
    }
    // Neither engine goes past its failure.
    assert!(
        !names.contains(&"never".to_string()),
        "{}: the plan continued past its failure, columns are {names:?}",
        target.name
    );

    scratch.teardown().await;
}

/// Diff the draft against the server, emit it, run it — the designer's own path,
/// including its refusal to run an empty plan.
async fn apply(scratch: &Scratch, current: &TableInfo, draft: &TableDraft, target: &Target) {
    let set = ddl::diff(current, draft, target.engine.dialect());
    assert!(
        !set.changes.is_empty(),
        "{}: the draft proposed no change at all — the test changed nothing",
        target.name
    );
    let stmts = set.emit();
    assert!(
        !stmts.is_empty(),
        "{}: {:?} emitted no statements",
        target.name,
        set.changes
    );
    scratch
        .db
        .run_ddl(&scratch.database, &stmts, CancellationToken::new())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "{}: the plan failed at statement {} of {stmts:?}: {}",
                target.name, e.at, e.message
            )
        });
}

/// After applying, the table read back must round-trip through its own draft.
///
/// **Against a draft re-anchored to the applied table, not the one that was
/// edited.** A `TableDraft` is anchored to the `TableInfo` it was made from —
/// `ColumnDraft::original` is the *identity* the diff matches on — so re-diffing
/// the pre-apply draft against the post-apply table asks a question the designer
/// never asks, and gets the right answer to it: a column added with
/// `original: None` reads as "add this", and the applied one as "drop that". The
/// app re-anchors after applying, and so does this.
async fn assert_settled(scratch: &Scratch, table: &str, target: &Target, what: &str) {
    let after = table_of(scratch, table).await;
    let settled = ddl::diff(
        &after,
        &TableDraft::from_table(&after),
        target.engine.dialect(),
    );
    assert!(
        settled.changes.is_empty(),
        "{}: after {what} the table no longer round-trips: {:?}
      emitting {:?}",
        target.name,
        settled.changes,
        settled.emit()
    );
}

async fn table_of(scratch: &Scratch, name: &str) -> TableInfo {
    let schema = scratch
        .db
        .fetch_schema(&scratch.database, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("introspecting {}: {e}", scratch.database));
    schema
        .tables
        .into_iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("no table {name:?} in {}", scratch.database))
}

fn column_names(t: &TableInfo) -> Vec<String> {
    t.columns.iter().map(|c| c.name.clone()).collect()
}
