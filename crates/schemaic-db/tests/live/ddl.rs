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
//!
//! **That list used to be the whole of it, and it read as a promise it did not
//! keep.** Every test here mutated `draft.columns` and nothing else, so
//! `AddIndex`, `DropForeignKey`, `AddCheck`, `RenameTable` and `TableOptions`
//! — every change the designer raises that is not about one column's own
//! definition — reached no server on any leg, while the paragraph above named
//! only views and triggers as absent. They are covered now. What is still
//! uncovered, stated so the next reader does not have to re-derive it:
//! **dropping** an index or a check (the add is asserted, the drop is not),
//! `Change::PrimaryKey`, and MySQL's `engine`/`collation` table options — the
//! comment is the one all three legs carry.

use schemaic_core::ddl::{self, CheckDraft, ColumnDraft, IndexDraft, TableDraft};
use schemaic_core::schema::{CheckInfo, ColumnInfo, IndexColumn, IndexInfo, TableInfo};
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
    // **A CHECK and a foreign key**, which no shape here had — so `CheckDraft`
    // and `ForeignKeyDraft` went through the identity gate on nothing at all,
    // and `alter_column_disturbs_checks` (the capability that decides whether a
    // column change has to route around a constraint) had zero live coverage.
    // A CHECK clause is also where MySQL 8 and MariaDB disagree about escaping,
    // which is the divergence this file's own module doc names.
    (
        "checks_and_fks",
        "(id INTEGER NOT NULL PRIMARY KEY, n INTEGER NOT NULL, \
         parent INTEGER, \
         CONSTRAINT n_positive CHECK (n > 0), \
         CONSTRAINT parent_fk FOREIGN KEY (parent) REFERENCES ddl_parent(id))",
    ),
];

/// **An index whose *method* is not the default**, which is MySQL's alone and is
/// the shape nothing here covered.
///
/// `FULLTEXT` is a different index kind, not a flag on an ordinary one: an
/// emitter that restates the key without it leaves a plain `KEY` behind, and
/// every `MATCH … AGAINST` against the table then fails outright. It is the
/// attribute a rename has to carry across, and the round-trip assertion cannot
/// see the loss — a table whose FULLTEXT became a KEY round-trips through its
/// own draft perfectly well.
///
/// Kept out of `SHAPES` because it is one engine's: `supports_index_methods` is
/// not a capability the model has, so the test that uses it asks the dialect
/// directly and returns elsewhere.
const FULLTEXT_SHAPE: &str =
    "(id INTEGER NOT NULL PRIMARY KEY, body TEXT NOT NULL, FULLTEXT KEY ft_body (body))";

/// The table `checks_and_fks` points at. Created before every shape, because a
/// `REFERENCES` to a table that is not there is refused outright.
const PARENT: &str = "(id INTEGER NOT NULL PRIMARY KEY)";

/// A table read off the server and drafted straight back proposes no change.
///
/// The first thing that must hold, and the one most likely not to: any asymmetry
/// between what the introspector reads and what the emitter would write shows up
/// here as a change nobody asked for.
pub async fn an_introspected_table_diffs_to_nothing_against_its_own_draft(target: &'static Target) {
    let scratch = Scratch::create(target, "ddl_identity").await;
    let mut failures = Vec::new();

    // The foreign key's target, first — a `REFERENCES` to a table that is not
    // there is refused outright.
    scratch
        .exec(&format!(
            "CREATE TABLE {} {PARENT}",
            scratch.qualified("ddl_parent")
        ))
        .await;

    for (name, ddl_sql) in SHAPES {
        // The shape's `REFERENCES ddl_parent(id)` is unqualified, which resolves
        // in the scratch database on MySQL and in the scratch schema on
        // PostgreSQL — the same place `qualified` puts the table.
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
    // **Every attribute non-default**, which is what makes this a test of "as
    // drafted". It asked for `INTEGER`, nullable, no default — the engine's own
    // answer to every question — so `AddColumn` could have emitted the name
    // alone and the assertion below, which only looked for the name, would not
    // have noticed.
    draft.columns.push(ColumnDraft::new(ColumnInfo {
        name: "added".to_string(),
        type_name: "INTEGER".to_string(),
        nullable: false,
        default: Some("7".to_string()),
        ..Default::default()
    }));

    apply(&scratch, &current, &draft, target).await;
    assert_round_trips(&scratch, "t", target, "adding a column").await;
    // …and it is the table that was *drafted*, which the round trip
    // cannot say: both its sides come from the applied table.
    assert_matches_draft(&scratch, "t", &draft, target, "adding a column").await;
    let after = table_of(&scratch, "t").await;
    let col = after
        .columns
        .iter()
        .find(|c| c.name == "added")
        .unwrap_or_else(|| panic!("{}: the column is not on the server", target.name));
    assert!(
        !col.nullable,
        "{}: the column came back nullable, which the draft did not ask for",
        target.name
    );
    assert!(
        ddl::defaults_equal(col.default.as_deref(), Some("7")),
        "{}: the default came back as {:?}, not 7",
        target.name,
        col.default
    );
    assert!(
        ddl::types_equal(&col.type_name, "INTEGER", target.engine.dialect()),
        "{}: the type came back as {:?}, not INTEGER",
        target.name,
        col.type_name
    );

    scratch.teardown().await;
}

/// A column moved through the designer lands in its new position, **with its
/// data**.
///
/// Gated on the capability rather than the engine, the way `views.rs` gates the
/// rename: `supports_column_reorder` is what the app asks, and it had no live
/// coverage at all — on MySQL a reorder is an `ALTER … MODIFY … AFTER`, which
/// restates the column's whole definition, so getting it wrong silently drops a
/// default or a NOT NULL along with moving it. The data assertion is the
/// expensive half: a reorder implemented as a drop-and-add would pass a test
/// that checked only the order.
pub async fn a_reordered_column_lands_where_it_was_put(target: &'static Target) {
    let dialect = target.engine.dialect();
    if !ddl::supports_column_reorder(dialect) {
        // PostgreSQL has no column order to change, which is the capability's
        // whole point — there is nothing here to assert, and saying so is better
        // than a test that quietly does nothing.
        return;
    }
    let scratch = Scratch::create(target, "ddl_reorder").await;
    scratch
        .exec(&format!(
            "CREATE TABLE {} (id INTEGER NOT NULL PRIMARY KEY, a VARCHAR(8) NOT NULL DEFAULT 'x', \
             b VARCHAR(8))",
            scratch.qualified("t")
        ))
        .await;
    scratch
        .exec(&format!(
            "INSERT INTO {} (id, a, b) VALUES (1, 'kept', 'also')",
            scratch.qualified("t")
        ))
        .await;

    let current = table_of(&scratch, "t").await;
    // What `a` was before the move, in the server's own spelling — comparing
    // against that rather than against `"x"` is what keeps this a test of the
    // reorder instead of a test of how each engine quotes a default.
    let before = current
        .columns
        .iter()
        .find(|c| c.name == "a")
        .expect("a")
        .clone();
    let mut draft = TableDraft::from_table(&current);
    // Move `b` ahead of `a`.
    let b = draft
        .columns
        .iter()
        .position(|c| c.info.name == "b")
        .expect("b is there");
    let moved = draft.columns.remove(b);
    draft.columns.insert(1, moved);

    apply(&scratch, &current, &draft, target).await;
    assert_round_trips(&scratch, "t", target, "reordering a column").await;
    // …and it is the table that was *drafted*, which the round trip
    // cannot say: both its sides come from the applied table.
    assert_matches_draft(&scratch, "t", &draft, target, "reordering a column").await;

    let after = table_of(&scratch, "t").await;
    assert_eq!(
        column_names(&after),
        ["id", "b", "a"],
        "{}: the column did not move",
        target.name
    );
    // The column that moved past kept everything it had — a reorder that
    // restates a definition is where that gets lost.
    let a = after.columns.iter().find(|c| c.name == "a").expect("a");
    assert!(!a.nullable, "{}: `a` came back nullable", target.name);
    assert!(
        ddl::defaults_equal(a.default.as_deref(), before.default.as_deref()),
        "{}: `a`'s default was {:?} and came back as {:?}",
        target.name,
        before.default,
        a.default
    );
    assert!(
        ddl::types_equal(&a.type_name, &before.type_name, dialect),
        "{}: `a`'s type was {:?} and came back as {:?}",
        target.name,
        before.type_name,
        a.type_name
    );
    // And the row is still there, with its values.
    let rows = scratch
        .exec(&format!(
            "SELECT a, b FROM {} WHERE id = 1",
            scratch.qualified("t")
        ))
        .await;
    assert_eq!(
        (
            rows.cell(0, 0).expect("a").display().to_string(),
            rows.cell(0, 1).expect("b").display().to_string()
        ),
        ("kept".to_string(), "also".to_string()),
        "{}: the reorder lost the row's data",
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
    assert_round_trips(&scratch, "t", target, "dropping a column").await;
    // …and it is the table that was *drafted*, which the round trip
    // cannot say: both its sides come from the applied table.
    assert_matches_draft(&scratch, "t", &draft, target, "dropping a column").await;
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
    assert_round_trips(&scratch, "t", target, "renaming a column").await;
    // …and it is the table that was *drafted*, which the round trip
    // cannot say: both its sides come from the applied table.
    assert_matches_draft(&scratch, "t", &draft, target, "renaming a column").await;

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
    assert_round_trips(&scratch, "t", target, "retyping a column").await;
    // …and it is the table that was *drafted*, which the round trip
    // cannot say: both its sides come from the applied table.
    assert_matches_draft(&scratch, "t", &draft, target, "retyping a column").await;

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
/// **An edit to an index must carry its kind across with it.**
///
/// `FULLTEXT` is a different index *kind*, not a flag on an ordinary one: an
/// edit that restates the key without it leaves a plain `KEY` behind, and every
/// `MATCH … AGAINST` against the table then answers `ERROR 1191`. On MySQL any
/// edit to the index at all is a `DROP INDEX` plus an `ADD INDEX`, so a rename
/// of the index is the whole gesture.
///
/// This is the shape the `assert_settled` finding names, and the coverage the
/// index-type fix said in its own commit message it could not have — *"the live
/// tier is unreachable from Windows in this environment"*. It is reachable now.
/// The round-trip assertion passes over the loss, because a table whose FULLTEXT
/// became a KEY round-trips through its own draft perfectly well;
/// `assert_matches_draft` is what sees it, and the `MATCH` at the end is what
/// sees it on the server rather than in the model.
pub async fn a_renamed_column_keeps_its_indexs_kind(target: &'static Target) {
    let dialect = target.engine.dialect();
    if dialect != schemaic_core::intel::SqlDialect::MySql {
        // `FULLTEXT` is MySQL's spelling; PostgreSQL's full-text index is a
        // GIN over an expression, which is a different shape and a different
        // test.
        return;
    }
    let scratch = Scratch::create(target, "ddl_ft").await;
    scratch
        .exec(&format!(
            "CREATE TABLE {} {FULLTEXT_SHAPE}",
            scratch.qualified("t")
        ))
        .await;

    let current = table_of(&scratch, "t").await;
    // The premise: the model read the method at all. Without it the assertion
    // below would be comparing `None` with `None`.
    assert!(
        current
            .indexes
            .iter()
            .any(|ix| ix.name == "ft_body" && ix.method.is_some()),
        "{}: the FULLTEXT index came back with no method: {:?}",
        target.name,
        current.indexes
    );

    let mut draft = TableDraft::from_table(&current);
    // Rename the **index**, which is what makes the emitter restate it. A column
    // rename alone does not: MySQL carries the index across `CHANGE COLUMN`
    // itself, so nothing is re-emitted and nothing can be lost.
    let ix = draft
        .indexes
        .iter_mut()
        .find(|ix| ix.info.name == "ft_body")
        .expect("the drafted index");
    ix.info.name = "ft_content".to_string();
    apply(&scratch, &current, &draft, target).await;

    assert_round_trips(&scratch, "t", target, "renaming a FULLTEXT index").await;
    assert_matches_draft(&scratch, "t", &draft, target, "renaming a FULLTEXT index").await;

    // And it really does still work as one, which no model comparison can say:
    // a FULLTEXT restated as a plain KEY answers ERROR 1191 here.
    scratch
        .exec(&format!(
            "SELECT id FROM {} WHERE MATCH(body) AGAINST('anything')",
            scratch.qualified("t")
        ))
        .await;

    scratch.teardown().await;
}

async fn apply(scratch: &Scratch, current: &TableInfo, draft: &TableDraft, target: &Target) {
    let set = ddl::diff(current, draft, target.engine.dialect());
    scratch.apply_plan(&set, "table").await;
}

/// After applying, the table read back must round-trip through **its own**
/// draft.
///
/// **Against a draft re-anchored to the applied table, not the one that was
/// edited.** A `TableDraft` is anchored to the `TableInfo` it was made from —
/// `ColumnDraft::original` is the *identity* the diff matches on — so re-diffing
/// the pre-apply draft against the post-apply table asks a question the designer
/// never asks, and gets the right answer to it: a column added with
/// `original: None` reads as "add this", and the applied one as "drop that". The
/// app re-anchors after applying, and so does this.
///
/// **This is introspector/emitter symmetry and nothing else** — which is what it
/// was renamed to say. It was called `assert_settled` and was the *primary*
/// assertion in five of the seven tests here, which made it look like a check
/// that the applied table matches the draft. It is not: both sides come from
/// `after`, so the draft that was edited is not in the comparison at all. Run
/// against an emitter that dropped `keep`'s `DEFAULT` alongside the column it
/// was asked to drop, this passes — the resulting table round-trips through its
/// own draft perfectly well. Two of B2.2's three measured defects went through
/// it for that reason. [`assert_matches_draft`] is the other half.
async fn assert_round_trips(scratch: &Scratch, table: &str, target: &Target, what: &str) {
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

/// After applying, the table read back must be **the table that was drafted** —
/// every column, every attribute, and the index/key/check sets.
///
/// The assertion `assert_round_trips` cannot make, and the one the seven tests
/// here were missing: they checked the column *names* and, in three cases, three
/// or four attributes of the single column they had touched. Nothing checked the
/// columns the test did not touch, and nothing checked the indexes, foreign
/// keys, checks or table options on any test — so an emitter that lost a
/// `DEFAULT`, a `NOT NULL`, a collation, a comment, a generated expression or an
/// index method while doing what it was asked went green.
///
/// **Compared field by field, and per field**, so a failure names what moved
/// rather than printing two structs. The type name goes through
/// `ddl::types_equal` because a server rewrites a declaration
/// (`INT` → `int(11)`, `TEXT` → `text`) and that is not a loss; everything else
/// is compared as the model holds it.
///
/// A column the draft *added* has no `original`, and the server may fill in
/// things nobody asked for — `auto_increment` is the engine's, and an implicit
/// key's type is its own — so the comparison is over what the draft **stated**:
/// a field the draft left at its default is not asserted. That is the honest
/// line, and it is still far more than the names.
async fn assert_matches_draft(
    scratch: &Scratch,
    table: &str,
    draft: &TableDraft,
    target: &Target,
    what: &str,
) {
    let after = table_of(scratch, table).await;
    let dialect = target.engine.dialect();
    let name = |t: &Target| t.name;
    let mut lost: Vec<String> = Vec::new();

    assert_eq!(
        after
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        draft
            .columns
            .iter()
            .map(|c| c.info.name.as_str())
            .collect::<Vec<_>>(),
        "{}: after {what} the columns are not the ones drafted",
        name(target)
    );

    for want in &draft.columns {
        let w = &want.info;
        let Some(got) = after.columns.iter().find(|c| c.name == w.name) else {
            continue; // the name check above already failed
        };
        let mut note = |field: &str, a: String, b: String| {
            if a != b {
                lost.push(format!(
                    "  {}.{field}: drafted {b:?}, server has {a:?}",
                    w.name
                ));
            }
        };
        if !ddl::types_equal(&got.type_name, &w.type_name, dialect) {
            note("type", got.type_name.clone(), w.type_name.clone());
        }
        note("nullable", got.nullable.to_string(), w.nullable.to_string());
        note(
            "default",
            format!("{:?}", got.default),
            format!("{:?}", w.default),
        );
        note(
            "generated",
            format!("{:?}", got.generated),
            format!("{:?}", w.generated),
        );
        // Only where the draft asked for one: a server fills in a collation and a
        // comment is not read back on every engine.
        if w.collation.is_some() {
            note(
                "collation",
                format!("{:?}", got.collation),
                format!("{:?}", w.collation),
            );
        }
        if w.comment.is_some() {
            note(
                "comment",
                format!("{:?}", got.comment),
                format!("{:?}", w.comment),
            );
        }
        if w.on_update.is_some() {
            note(
                "on_update",
                format!("{:?}", got.on_update),
                format!("{:?}", w.on_update),
            );
        }
    }

    // The sets the tests never looked at. Names and the attributes the model
    // carries — an index that came back as a plain `KEY` where a `FULLTEXT` was
    // drafted is the shape B2.2 measured, and no assertion here could see it.
    let want_ix: Vec<(String, bool, Option<String>)> = draft
        .indexes
        .iter()
        .map(|ix| (ix.info.name.clone(), ix.info.unique, ix.info.method.clone()))
        .collect();
    let got_ix: Vec<(String, bool, Option<String>)> = after
        .indexes
        .iter()
        .filter(|ix| !ix.is_primary())
        .map(|ix| (ix.name.clone(), ix.unique, ix.method.clone()))
        .collect();
    for w in &want_ix {
        if !got_ix
            .iter()
            .any(|g| g.0 == w.0 && g.1 == w.1 && g.2 == w.2)
        {
            lost.push(format!(
                "  index {:?}: drafted {w:?}, server has {got_ix:?}",
                w.0
            ));
        }
    }
    let want_fk: Vec<String> = draft
        .foreign_keys
        .iter()
        .map(|f| f.info.name.clone())
        .collect();
    for w in &want_fk {
        if !after.foreign_keys.iter().any(|f| f.name == *w) {
            lost.push(format!("  foreign key {w:?} is gone"));
        }
    }
    let want_ck: Vec<String> = draft
        .check_constraints
        .iter()
        .map(|c| c.info.name.clone())
        .collect();
    for w in &want_ck {
        if !w.is_empty() && !after.check_constraints.iter().any(|c| c.name == *w) {
            lost.push(format!("  check {w:?} is gone"));
        }
    }

    assert!(
        lost.is_empty(),
        "{}: after {what} the table is not the one that was drafted:\n{}",
        name(target),
        lost.join("\n")
    );
}

/// **An index the model cannot hold whole must say so, and must not be restated
/// from the parts that were read.**
///
/// `IndexInfo::lossy` is the whole guard: `ddl::diff` turns an edit to a lossy
/// index into `KeepLossyIndex` rather than a `DROP` plus a `CREATE`, and
/// `TableInfo::create_ddl` emits the server's own statement for one. Read as
/// `false`, both do the destructive thing quietly — an edit anywhere on the
/// table recreates the index narrower, and a structure dump rewrites it with **no
/// edit at all** and reports success.
///
/// Three shapes, each of which `pg_index` reports in a place the per-key-column
/// query cannot see: `INCLUDE` columns live past `indnkeyatts` and are dropped
/// by the ordinality join before `pg_attribute` is consulted;
/// `NULLS NOT DISTINCT` is a column of `pg_index` that PostgreSQL 15 added, so
/// it cannot even be named in a query that must also parse on 13 and 14; and a
/// storage parameter is `pg_class.reloptions`, which nothing asked for.
///
/// Gated on the capability rather than the engine, the way the reorder above is
/// — but the shapes are **PostgreSQL's**, it being the only leg in this tier
/// that answers yes, and a fourth engine answering yes would need its own.
pub async fn a_partly_read_index_says_so_and_is_emitted_whole(target: &'static Target) {
    let dialect = target.engine.dialect();
    if !ddl::publishes_index_ddl(dialect) {
        // MySQL has no per-index `CREATE` to fall back on, so a partly-read
        // index there can only ever be refused — a different claim, asserted by
        // `a_refused_plan_says_where_it_stopped`.
        return;
    }
    let scratch = Scratch::create(target, "ddl_lossy_index").await;
    let t = scratch.qualified("t");
    scratch
        .exec(&format!(
            "CREATE TABLE {t} (a INTEGER, b INTEGER, c INTEGER)"
        ))
        .await;
    for sql in [
        format!("CREATE INDEX ix_inc ON {t} (a, b) INCLUDE (c)"),
        format!("CREATE UNIQUE INDEX ix_nd ON {t} (b) NULLS NOT DISTINCT"),
        format!("CREATE INDEX ix_ff ON {t} (c) WITH (fillfactor=70)"),
    ] {
        scratch.exec(&sql).await;
    }

    let current = table_of(&scratch, "t").await;
    for name in ["ix_inc", "ix_nd", "ix_ff"] {
        let ix = current
            .indexes
            .iter()
            .find(|i| i.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "{}: no index {name}; have {:?}",
                    target.name,
                    current.indexes.iter().map(|i| &i.name).collect::<Vec<_>>()
                )
            });
        assert!(
            ix.lossy,
            "{}: {name} was read as complete, so an edit would recreate it \
             narrower and a dump would rewrite it",
            target.name
        );
        assert!(
            ix.create_sql.is_some(),
            "{}: {name} is lossy with no text to replay",
            target.name
        );
    }

    // And the emitted DDL keeps what the model has no field for — the dump path,
    // which reaches `create_ddl` with no edit anywhere.
    let ddl_text = current.create_ddl(dialect);
    for clause in ["INCLUDE (c)", "NULLS NOT DISTINCT", "fillfactor"] {
        assert!(
            ddl_text.contains(clause),
            "{}: emitted DDL dropped {clause}:\n{ddl_text}",
            target.name
        );
    }

    // **The dump's own round trip**: drop the table and replay the emitted text,
    // which is exactly what a structure dump and its restore do. Before the fix
    // the file came back with an `ix_inc` that no longer covers, an `ix_nd` that
    // accepts two NULLs and an `ix_ff` at the default fillfactor — with no edit
    // anywhere and the dump reporting success.
    let before: Vec<(String, Option<String>)> = current
        .indexes
        .iter()
        .map(|i| (i.name.clone(), i.create_sql.clone()))
        .collect();
    scratch.exec(&format!("DROP TABLE {t}")).await;
    for stmt in ddl_text.split(
        ";
",
    ) {
        let stmt = stmt.trim().trim_end_matches(';');
        if !stmt.is_empty() {
            scratch.exec(stmt).await;
        }
    }
    let back = table_of(&scratch, "t").await;
    for (name, sql) in before {
        let got = back
            .indexes
            .iter()
            .find(|i| i.name == name)
            .unwrap_or_else(|| panic!("{}: {name} did not come back", target.name))
            .create_sql
            .clone();
        assert_eq!(
            got, sql,
            "{}: {name} came back as a different index",
            target.name
        );
    }
    scratch.teardown().await;
}

/// An index the DBA has **switched off** is read as switched off, so an edit
/// to it is withheld rather than bringing it silently back to life.
///
/// A DBA hides an index to test a plan change — MySQL 8's
/// `ALTER TABLE … ALTER INDEX … INVISIBLE`, MariaDB 10.6's `… IGNORED`. Neither
/// flag was read: the index came back as an ordinary `KEY` with `lossy = false`,
/// so renaming it (or touching it in any way) took `ddl::diff`'s
/// `DROP INDEX` + `ADD INDEX` arm — and the recreate carries no visibility
/// clause, so the index came back **live**, the optimizer started using it
/// again, and the preview said nothing at all.
///
/// Both halves are asserted because either alone passes for the wrong reason:
/// the read (`lossy`), and the composition through `diff` that the read exists
/// to govern. PostgreSQL has no such flag and returns early.
pub async fn a_switched_off_index_is_not_silently_brought_back(target: &'static Target) {
    let Some(disable_sql) = target.disable_index_sql else {
        return;
    };
    let scratch = Scratch::create(target, "ddl_off_index").await;
    let t = scratch.qualified("t");
    scratch
        .exec(&format!("CREATE TABLE {t} (a INTEGER, b INTEGER)"))
        .await;
    scratch
        .exec(&format!("CREATE INDEX ix_off ON {t} (a)"))
        .await;
    scratch
        .exec(
            &disable_sql
                .replace("{table}", &t)
                .replace("{index}", "ix_off"),
        )
        .await;

    let current = table_of(&scratch, "t").await;
    let ix = current
        .indexes
        .iter()
        .find(|i| i.name == "ix_off")
        .unwrap_or_else(|| panic!("{}: no index ix_off", target.name));
    assert!(
        ix.lossy,
        "{}: a switched-off index was read as one the model holds whole",
        target.name
    );

    // The composition the read is for: rename it in the designer's own way and
    // the plan must withhold the edit, not drop and recreate it.
    let mut draft = TableDraft::from_table(&current);
    let slot = draft
        .indexes
        .iter_mut()
        .find(|i| i.info.name == "ix_off")
        .unwrap_or_else(|| panic!("{}: the draft lost ix_off", target.name));
    slot.info.name = "ix_renamed".to_string();
    let set = ddl::diff(&current, &draft, target.engine.dialect());
    assert!(
        set.changes
            .iter()
            .any(|c| matches!(c, ddl::Change::KeepLossyIndex { name } if name == "ix_off")),
        "{}: the plan does not withhold the edit — {:?}",
        target.name,
        set.changes
    );
    let emitted = set.emit().join("\n");
    assert!(
        !emitted.to_ascii_uppercase().contains("IX_OFF"),
        "{}: the plan touches the switched-off index:\n{emitted}",
        target.name
    );

    scratch.teardown().await;
}

/// A **functional** index does not stop the database being read at all.
///
/// The assertion that matters is the weakest-looking one: `fetch_schema`
/// returns. MySQL 8 gives a functional key part a NULL `COLUMN_NAME` in
/// `information_schema.STATISTICS`, that cell was bound as a non-`Option`
/// `String`, and `mysql_async`'s `from_row` **panics** rather than erroring —
/// inside the spawned fetch task, so the call site never got a `Result` to map
/// to `SchemaState::Failed`. One `CREATE INDEX ix ON t ((a + b))` anywhere in a
/// database and the tree spun on "loading" for ever, with no error row, no
/// toast and nothing to retry against; Export, the dump, schema compare and
/// every MCP `list_schema` took the same shape.
///
/// So it is written against `fetch_schema`, not against the fold: the fold is
/// pure and was never what panicked, and a test on it alone is green on the
/// unfixed tree — the thirteen-green-tests failure CLAUDE.md records.
///
/// The two reads behind it are asserted as well, because a bind widened to
/// `Option` and then silently dropped would satisfy the no-panic half: the key
/// part is marked `expression`, and the index is restatable — which the two
/// engines answer differently and both legitimately. PostgreSQL keeps
/// `pg_get_indexdef`'s whole text in `create_sql`, so it can recreate the index
/// verbatim and is not lossy; MySQL publishes no per-index `CREATE` and
/// reconstructs one from the key parts, which it cannot do for an expression,
/// so it marks the index `lossy` and the edit is withheld instead. What must
/// never hold is neither: an index modelled as an ordinary column key with no
/// `CREATE` behind it is one a table edit drop-and-recreates as `KEY (<the
/// expression as a column name>)`.
///
/// MariaDB 10.11 rejects the syntax outright, so its leg has no clause and
/// returns early — which is exactly how two of three servers hid this.
pub async fn a_functional_index_does_not_stop_the_schema_being_read(target: &'static Target) {
    let Some(expr_sql) = target.expression_index_sql else {
        return;
    };
    let scratch = Scratch::create(target, "ddl_expr_index").await;
    let t = scratch.qualified("t");
    scratch
        .exec(&format!("CREATE TABLE {t} (a INTEGER, b INTEGER)"))
        .await;
    scratch
        .exec(
            &expr_sql
                .replace("{table}", &t)
                .replace("{index}", "ix_expr"),
        )
        .await;

    // Not `table_of`: that unwraps, and the point here is that the whole
    // schema read comes back at all rather than dying in its task.
    let schema = scratch
        .db
        .fetch_schema(&scratch.database, CancellationToken::new())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "{}: a functional index made the database unreadable — {e}",
                target.name
            )
        });
    let table = schema
        .tables
        .iter()
        .find(|t| t.name == "t")
        .unwrap_or_else(|| panic!("{}: no table t in {}", target.name, scratch.database));
    let ix = table
        .indexes
        .iter()
        .find(|i| i.name == "ix_expr")
        .unwrap_or_else(|| {
            panic!(
                "{}: the functional index is not in the model at all — indexes are {:?}",
                target.name,
                table.indexes.iter().map(|i| &i.name).collect::<Vec<_>>()
            )
        });
    assert!(
        ix.columns.iter().any(|c| c.expression),
        "{}: the functional key part is modelled as an ordinary column — {:?}",
        target.name,
        ix.columns
    );
    let restatable = ix
        .create_sql
        .as_deref()
        .is_some_and(|s| s.contains('(') && s.contains('+'));
    assert!(
        ix.lossy || restatable,
        "{}: a functional index is neither withheld nor restatable — a table \
         edit would recreate it as a plain column key. lossy={}, create_sql={:?}",
        target.name,
        ix.lossy,
        ix.create_sql
    );

    scratch.teardown().await;
}

/// A column added **in the middle** lands where it was put, with the rest of
/// the table intact.
///
/// The gap this fills is narrow and was expensive: every other test here
/// *appends* (`draft.columns.push`), which is the one insertion position that
/// raises no reposition at all, and the reorder test moves an existing column,
/// which raises a reposition with no `AddColumn` beside it. One plan carrying
/// both is the composition neither reaches — and it is where MySQL's
/// `MODIFY … AFTER <the new column>` was emitted **before** the `ADD COLUMN`
/// that creates it (`ERROR 1054: Unknown column`, measured on MariaDB 10.11.14
/// and MySQL 8.4.11).
///
/// Gated on `supports_column_reorder` for the reason the reorder test is:
/// PostgreSQL has no column order, so "the middle" is not a place there.
pub async fn a_column_inserted_in_the_middle_lands_there(target: &'static Target) {
    let dialect = target.engine.dialect();
    if !ddl::supports_column_reorder(dialect) {
        return;
    }
    let scratch = Scratch::create(target, "ddl_mid_insert").await;
    let t = scratch.qualified("t");
    scratch
        .exec(&format!(
            "CREATE TABLE {t} (id INTEGER NOT NULL PRIMARY KEY, name VARCHAR(32) NOT NULL)"
        ))
        .await;
    scratch
        .exec(&format!("INSERT INTO {t} (id, name) VALUES (1, 'one')"))
        .await;

    let current = table_of(&scratch, "t").await;
    let mut draft = TableDraft::from_table(&current);
    draft.columns.insert(
        1,
        ColumnDraft::new(ColumnInfo {
            name: "added".to_string(),
            type_name: "INTEGER".to_string(),
            nullable: true,
            ..Default::default()
        }),
    );

    apply(&scratch, &current, &draft, target).await;
    assert_round_trips(&scratch, "t", target, "inserting a column mid-table").await;
    assert_matches_draft(
        &scratch,
        "t",
        &draft,
        target,
        "inserting a column mid-table",
    )
    .await;

    let after = table_of(&scratch, "t").await;
    assert_eq!(
        column_names(&after),
        ["id", "added", "name"],
        "{}: the new column is not where it was put",
        target.name
    );
    let rows = scratch
        .exec(&format!("SELECT name FROM {t} WHERE id = 1"))
        .await;
    assert_eq!(
        rows.cell(0, 0).expect("name").display().to_string(),
        "one",
        "{}: the insert lost the row's data",
        target.name
    );

    scratch.teardown().await;
}

/// An index **added** through the designer lands, as the index that was
/// drafted.
///
/// Nothing in this tier ever wrote `draft.indexes` before, so `Change::AddIndex`
/// — one of the designer's commonest edits — reached no server on any leg. The
/// two attributes asserted beyond the name are the ones an emitter loses
/// silently: `unique`, which turns a constraint into a hint, and the key's
/// **order**, which decides whether the index answers the query it was added
/// for at all.
pub async fn an_added_index_lands_as_the_index_drafted(target: &'static Target) {
    let scratch = Scratch::create(target, "ddl_add_index").await;
    let t = scratch.qualified("t");
    scratch
        .exec(&format!(
            "CREATE TABLE {t} (id INTEGER NOT NULL PRIMARY KEY, a INTEGER NOT NULL, \
             b INTEGER NOT NULL)"
        ))
        .await;

    let current = table_of(&scratch, "t").await;
    let mut draft = TableDraft::from_table(&current);
    draft.indexes.push(IndexDraft::new(IndexInfo {
        name: "ix_ba".to_string(),
        columns: vec![IndexColumn::plain("b"), IndexColumn::plain("a")],
        unique: true,
        ..Default::default()
    }));

    apply(&scratch, &current, &draft, target).await;
    assert_round_trips(&scratch, "t", target, "adding an index").await;
    assert_matches_draft(&scratch, "t", &draft, target, "adding an index").await;

    let after = table_of(&scratch, "t").await;
    let ix = after
        .indexes
        .iter()
        .find(|i| i.name == "ix_ba")
        .unwrap_or_else(|| {
            panic!(
                "{}: the index was not added — indexes are {:?}",
                target.name,
                after.indexes.iter().map(|i| &i.name).collect::<Vec<_>>()
            )
        });
    assert!(ix.unique, "{}: the index came back non-unique", target.name);
    assert_eq!(
        ix.columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["b", "a"],
        "{}: the index's key order is not the one drafted",
        target.name
    );
    // Unique means the server refuses the duplicate — the half a catalogue read
    // cannot tell from a plain `KEY` that happens to be flagged.
    scratch
        .exec(&format!("INSERT INTO {t} (id, a, b) VALUES (1, 1, 2)"))
        .await;
    assert!(
        scratch
            .try_exec(&format!("INSERT INTO {t} (id, a, b) VALUES (2, 1, 2)"))
            .await
            .is_err(),
        "{}: the server accepted a duplicate through a UNIQUE index",
        target.name
    );

    scratch.teardown().await;
}

/// A foreign key **dropped** through the designer goes, and its column stays.
///
/// `Change::DropForeignKey` had no live coverage either, and the failure it
/// guards is asymmetric in the expensive direction: a drop implemented as a
/// column drop takes the data with it, and a drop that silently does nothing
/// leaves the table refusing the very writes the user removed the key to allow.
/// So both halves are asserted — the key is gone from the catalogue, **and** a
/// row the key would have refused is now accepted.
pub async fn a_dropped_foreign_key_goes_and_the_column_stays(target: &'static Target) {
    let scratch = Scratch::create(target, "ddl_drop_fk").await;
    let parent = scratch.qualified("par");
    let t = scratch.qualified("t");
    scratch
        .exec(&format!(
            "CREATE TABLE {parent} (id INTEGER NOT NULL PRIMARY KEY)"
        ))
        .await;
    scratch
        .exec(&format!(
            "CREATE TABLE {t} (id INTEGER NOT NULL PRIMARY KEY, pid INTEGER, \
             CONSTRAINT t_par_fk FOREIGN KEY (pid) REFERENCES {parent} (id))"
        ))
        .await;
    scratch
        .exec(&format!("INSERT INTO {parent} (id) VALUES (1)"))
        .await;
    scratch
        .exec(&format!("INSERT INTO {t} (id, pid) VALUES (1, 1)"))
        .await;

    let current = table_of(&scratch, "t").await;
    assert!(
        current.foreign_keys.iter().any(|f| f.name == "t_par_fk"),
        "{}: the fixture's foreign key was not read back, so the drop below \
         would assert nothing",
        target.name
    );
    let mut draft = TableDraft::from_table(&current);
    draft.foreign_keys.retain(|f| f.info.name != "t_par_fk");

    apply(&scratch, &current, &draft, target).await;
    assert_round_trips(&scratch, "t", target, "dropping a foreign key").await;

    let after = table_of(&scratch, "t").await;
    assert!(
        !after.foreign_keys.iter().any(|f| f.name == "t_par_fk"),
        "{}: the foreign key is still there",
        target.name
    );
    assert!(
        column_names(&after).contains(&"pid".to_string()),
        "{}: dropping the key took its column with it",
        target.name
    );
    // The key really is not enforced any more — a value with no parent row.
    scratch
        .exec(&format!("INSERT INTO {t} (id, pid) VALUES (2, 99)"))
        .await;

    scratch.teardown().await;
}

/// A `CHECK` **added** through the designer is enforced by the server.
///
/// The catalogue half alone would be worth little here: MySQL 8 and MariaDB
/// hand `CHECK_CLAUSE` back with different escaping — the divergence this
/// file's own module doc names — so a constraint can read back under its name
/// and still have been created from mangled text. The assertion is therefore
/// behavioural: the row the check forbids is refused, and the row it allows is
/// not.
pub async fn an_added_check_is_enforced_by_the_server(target: &'static Target) {
    let scratch = Scratch::create(target, "ddl_add_check").await;
    let t = scratch.qualified("t");
    scratch
        .exec(&format!(
            "CREATE TABLE {t} (id INTEGER NOT NULL PRIMARY KEY, n INTEGER NOT NULL, \
             s VARCHAR(8) NOT NULL)"
        ))
        .await;

    let current = table_of(&scratch, "t").await;
    let mut draft = TableDraft::from_table(&current);
    // A quote inside the predicate on purpose: the escaping is what diverges
    // between these servers, and a check on a number alone would never meet it.
    draft.check_constraints.push(CheckDraft::new(CheckInfo {
        name: "n_positive".to_string(),
        expression: "n > 0 AND s <> 'no'".to_string(),
        enforced: true,
        validated: true,
        ..Default::default()
    }));

    apply(&scratch, &current, &draft, target).await;
    assert_round_trips(&scratch, "t", target, "adding a check").await;

    let after = table_of(&scratch, "t").await;
    assert!(
        after
            .check_constraints
            .iter()
            .any(|c| c.name == "n_positive"),
        "{}: the check is not in the catalogue — checks are {:?}",
        target.name,
        after
            .check_constraints
            .iter()
            .map(|c| &c.name)
            .collect::<Vec<_>>()
    );
    scratch
        .exec(&format!("INSERT INTO {t} (id, n, s) VALUES (1, 1, 'ok')"))
        .await;
    assert!(
        scratch
            .try_exec(&format!("INSERT INTO {t} (id, n, s) VALUES (2, 0, 'ok')"))
            .await
            .is_err(),
        "{}: the server accepted a row the check forbids (the number half)",
        target.name
    );
    assert!(
        scratch
            .try_exec(&format!("INSERT INTO {t} (id, n, s) VALUES (3, 1, 'no')"))
            .await
            .is_err(),
        "{}: the server accepted a row the check forbids (the quoted half)",
        target.name
    );

    scratch.teardown().await;
}

/// A **renamed table** lands under its new name, with its rows and its keys.
///
/// `Change::RenameTable` reached no server here either, and it is the change
/// whose failure is least recoverable: the rename is emitted first in the plan
/// on MySQL, so everything after it names a table that no longer exists under
/// the name the plan used.
pub async fn a_renamed_table_keeps_its_rows_and_its_keys(target: &'static Target) {
    let scratch = Scratch::create(target, "ddl_rename_table").await;
    let t = scratch.qualified("t");
    scratch
        .exec(&format!(
            "CREATE TABLE {t} (id INTEGER NOT NULL PRIMARY KEY, code VARCHAR(8) NOT NULL)"
        ))
        .await;
    scratch
        .exec(&format!("CREATE INDEX ix_code ON {t} (code)"))
        .await;
    scratch
        .exec(&format!("INSERT INTO {t} (id, code) VALUES (1, 'keep')"))
        .await;

    let current = table_of(&scratch, "t").await;
    let mut draft = TableDraft::from_table(&current);
    draft.name = "t_renamed".to_string();

    apply(&scratch, &current, &draft, target).await;
    assert_round_trips(&scratch, "t_renamed", target, "renaming a table").await;

    let after = table_of(&scratch, "t_renamed").await;
    assert!(
        after.indexes.iter().any(|i| i.name == "ix_code"),
        "{}: the rename lost the table's index — indexes are {:?}",
        target.name,
        after.indexes.iter().map(|i| &i.name).collect::<Vec<_>>()
    );
    let rows = scratch
        .exec(&format!(
            "SELECT code FROM {} WHERE id = 1",
            scratch.qualified("t_renamed")
        ))
        .await;
    assert_eq!(
        rows.cell(0, 0).expect("code").display().to_string(),
        "keep",
        "{}: the rename lost the table's rows",
        target.name
    );

    scratch.teardown().await;
}

/// A **table comment** written through the designer lands.
///
/// Gated on the table having read one back at all rather than on the engine:
/// `TableDraft::comment` is the one table option all three legs in this tier
/// carry, and a leg whose introspection does not return it has nothing here to
/// assert. `Change::TableOptions` had no live coverage on any leg.
pub async fn a_table_comment_lands_and_reads_back(target: &'static Target) {
    let scratch = Scratch::create(target, "ddl_table_opts").await;
    let t = scratch.qualified("t");
    scratch
        .exec(&format!(
            "CREATE TABLE {t} (id INTEGER NOT NULL PRIMARY KEY)"
        ))
        .await;

    let current = table_of(&scratch, "t").await;
    let mut draft = TableDraft::from_table(&current);
    // An apostrophe on purpose: a table comment goes through the same literal
    // quoting a column comment does, and it is the one table option that is
    // free text.
    draft.comment = Some("what it's for".to_string());

    apply(&scratch, &current, &draft, target).await;
    assert_round_trips(&scratch, "t", target, "setting a table comment").await;

    let after = table_of(&scratch, "t").await;
    assert_eq!(
        after.comment.as_deref(),
        Some("what it's for"),
        "{}: the table comment did not land",
        target.name
    );

    scratch.teardown().await;
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
