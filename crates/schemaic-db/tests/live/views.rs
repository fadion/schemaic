//! Views: introspected, redefined, renamed — and what a result over one is
//! allowed to claim.
//!
//! Same round-trip property the table designer has, and the same reason for
//! stating it that way: a view whose body the server reformats on the way in and
//! the introspector reads back differently is *correct on the server* and
//! permanently dirty in the editor. Views make that failure likelier than tables
//! do, because both engines rewrite a view's `SELECT` rather than storing it —
//! MySQL fully qualifies and back-quotes it, PostgreSQL re-prints it from the
//! parse tree — so what comes back is never the text that went in.
//!
//! **What a view column's provenance reports is recorded per leg, not asserted
//! in common.** It is a genuine divergence: a result over a view can be
//! attributed to the view, to the base table underneath it, or to nothing at
//! all, and each is a defensible thing for a driver to say. The claim held in
//! common is the one that actually protects data — that the editing system never
//! offers a writable key it cannot stand behind.

use schemaic_core::ddl::{self, ViewDraft};
use schemaic_core::schema::TableInfo;
use tokio_util::sync::CancellationToken;

use crate::endpoint::Target;
use crate::scratch::Scratch;

/// The table every view here is built over.
const BASE: &str = "(id INTEGER NOT NULL PRIMARY KEY, name VARCHAR(32))";

/// A view read off the server, emitted from its own draft and read back reports
/// the same definition — and the editor is then clean against the *first*
/// reading.
///
/// **The gate crosses the emitter and the server**, and it has to. Diffing a
/// view against `ViewDraft::from_table` of *itself* compares two copies of one
/// expression: `diff_view` computes `old_body` by calling `from_table(current)`
/// itself, so both sides are the same value by construction and the answer is
/// "0 changes" for any body at all. Proved against the real `schemaic-core`: a
/// `view_definition` of `">>> not a query at all <<<"`, of `""`, and of `"😀"`
/// each diffs to zero changes on all three dialects. The test would have passed
/// against a server that returned nothing.
///
/// So the draft is diffed against a *deliberately stale* reading — which is what
/// makes `diff_view` emit — the statement is run, and the two assertions below
/// compare values the server produced at two different times.
pub async fn an_introspected_view_diffs_to_nothing_against_its_own_draft(target: &'static Target) {
    let dialect = target.engine.dialect();
    let scratch = Scratch::create(target, "view_identity").await;
    seed_view(&scratch, "SELECT id, name FROM {} WHERE id > 0").await;

    let view = view_of(&scratch).await;
    let draft = ViewDraft::from_table(&view)
        .unwrap_or_else(|| panic!("{}: the view did not draft at all", target.name));

    // A body no server would ever report, purely to make the diff non-empty:
    // the draft below is the server's own, and it is that draft the emitter has
    // to be able to write back.
    let mut stale = view.clone();
    stale.view_definition = Some("SELECT id FROM nothing_at_all".to_string());
    let set = ddl::diff_view(&stale, &draft, dialect);
    assert!(
        !set.changes.is_empty(),
        "{}: the gate is vacuous — a differing body proposed no change",
        target.name
    );
    let stmts = set.emit();
    scratch
        .db
        .run_ddl(&scratch.database, &stmts, CancellationToken::new())
        .await
        .unwrap_or_else(|e| {
            panic!(
                "{}: re-applying the server's own view failed at statement {} of {stmts:?}: {}",
                target.name, e.at, e.message
            )
        });

    // The server's two readings of the same view, either side of the round trip.
    let after = view_of(&scratch).await;
    assert_eq!(
        after.view_definition, view.view_definition,
        "{}: the view's definition changed by being written back through the emitter",
        target.name
    );

    // And the editor is clean — the claim this file opens with. `current` and
    // `draft` are now two independent readings, so this comparison has content.
    let settled = ddl::diff_view(
        &view,
        &ViewDraft::from_table(&after)
            .unwrap_or_else(|| panic!("{}: the view did not draft after", target.name)),
        dialect,
    );
    assert!(
        settled.changes.is_empty(),
        "{}: the view no longer round-trips: {:?}\n      emitting {:?}",
        target.name,
        settled.changes,
        settled.emit()
    );

    scratch.teardown().await;
}

/// A redefined body lands, reads back clean, and the view returns the new rows.
///
/// Both halves matter and they fail separately: the rows prove the server took
/// the new definition, and the second diff proves the editor will not keep
/// offering the same edit forever.
pub async fn an_edited_view_body_lands_and_settles(target: &'static Target) {
    let scratch = Scratch::create(target, "view_edit").await;
    seed_view(&scratch, "SELECT id, name FROM {} WHERE id > 0").await;

    let view = view_of(&scratch).await;
    let mut draft = ViewDraft::from_table(&view).expect("a view draft");
    draft.select = format!(
        "SELECT id, name FROM {} WHERE id > 1",
        scratch.qualified("t")
    );

    apply_view(&scratch, &view, &draft, target).await;

    // **Against the pre-edit reading, not against itself.** Re-drafting `after`
    // and diffing it against `after` is one expression compared with itself and
    // answers "no change" for any body — see
    // `an_introspected_view_diffs_to_nothing_against_its_own_draft`. Diffing the
    // *new* reading against the *old* view must, by contrast, report exactly the
    // change that was applied: an edit that reported nothing would mean the
    // server had not taken it.
    let after = view_of(&scratch).await;
    let landed = ddl::diff_view(
        &view,
        &ViewDraft::from_table(&after).expect("a view draft"),
        target.engine.dialect(),
    );
    assert!(
        !landed.changes.is_empty(),
        "{}: the server reports the same view body it had before the edit",
        target.name
    );
    // …and re-applying what the server now reports settles: the second reading
    // is stable, which is the "permanently dirty in the editor" claim.
    let settled = ddl::diff_view(
        &after,
        &ViewDraft::from_table(&view_of(&scratch).await).expect("a view draft"),
        target.engine.dialect(),
    );
    assert!(
        settled.changes.is_empty(),
        "{}: after the edit the view no longer round-trips: {:?}",
        target.name,
        settled.changes
    );

    let rows = scratch
        .exec(&format!(
            "SELECT name FROM {} ORDER BY id",
            scratch.qualified("v")
        ))
        .await;
    let names: Vec<String> = (0..rows.row_count())
        .map(|r| rows.cell(r, 0).expect("a cell").display().to_string())
        .collect();
    assert_eq!(
        names,
        ["two", "three"],
        "{}: the view still returns its old rows",
        target.name
    );

    scratch.teardown().await;
}

/// **Dropping a column from a view's `SELECT`** — the redefinition PostgreSQL
/// cannot do with `CREATE OR REPLACE`, and MySQL can.
///
/// This is `diff_view`'s destructive arm: `pg_replaceable` answers `Some(false)`
/// for a narrower column list, and the change set then carries
/// `recreate: true`, which drops the view, recreates it and replays whatever
/// depended on it. No live case satisfied that predicate, so the arm — the one
/// with a `DROP` in it — had never run against a server on any leg.
///
/// Asserted per leg because the answer genuinely differs, which is what
/// `supports_or_replace_view` and `pg_replaceable` exist to say.
pub async fn a_view_that_drops_a_column_takes_the_destructive_arm_where_it_must(
    target: &'static Target,
) {
    let dialect = target.engine.dialect();
    let scratch = Scratch::create(target, "view_narrow").await;
    seed_view(&scratch, "SELECT id, name FROM {} WHERE id > 0").await;

    let view = view_of(&scratch).await;
    let mut draft = ViewDraft::from_table(&view).expect("a view draft");
    draft.select = format!("SELECT id FROM {}", scratch.qualified("t"));

    let set = ddl::diff_view(&view, &draft, dialect);
    let recreated = set
        .changes
        .iter()
        .any(|c| matches!(c, ddl::Change::ReplaceView { recreate: true, .. }));
    assert_eq!(
        recreated,
        dialect == schemaic_core::intel::SqlDialect::Postgres,
        "{}: narrowing a view's column list took the {} arm",
        target.name,
        if recreated { "recreate" } else { "replace" }
    );

    apply_view(&scratch, &view, &draft, target).await;

    // Whichever arm it took, the server has the new view.
    let after = view_of(&scratch).await;
    assert_eq!(
        after
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["id"],
        "{}: the view still reports its old column list",
        target.name
    );
    let rows = scratch
        .exec(&format!("SELECT id FROM {}", scratch.qualified("v")))
        .await;
    assert_eq!(
        rows.row_count(),
        3,
        "{}: the narrowed view returns nothing",
        target.name
    );
    // …and it round-trips, which a re-create that lost an option would not.
    let settled = ddl::diff_view(
        &after,
        &ViewDraft::from_table(&view_of(&scratch).await).expect("a view draft"),
        dialect,
    );
    assert!(
        settled.changes.is_empty(),
        "{}: the narrowed view no longer round-trips: {:?}",
        target.name,
        settled.changes
    );

    scratch.teardown().await;
}

/// A renamed view lands under the new name and nothing answers to the old one.
///
/// Gated on the capability rather than the engine: `supports_view_rename` is
/// what the app asks, and a leg that answered `false` would have nothing to
/// assert here.
pub async fn a_renamed_view_lands_under_the_new_name(target: &'static Target) {
    let dialect = target.engine.dialect();
    assert!(
        ddl::supports_view_rename(dialect),
        "{}: this leg cannot rename a view, so the test below asserts nothing — \
         it needs the capability's other arm written before that is true",
        target.name
    );

    let scratch = Scratch::create(target, "view_rename").await;
    seed_view(&scratch, "SELECT id, name FROM {} WHERE id > 0").await;

    let view = view_of(&scratch).await;
    let mut draft = ViewDraft::from_table(&view).expect("a view draft");
    draft.name = "renamed".to_string();

    apply_view(&scratch, &view, &draft, target).await;

    let names = view_names(&scratch).await;
    assert!(
        names.contains(&"renamed".to_string()),
        "{}: the view is not under its new name, views are {names:?}",
        target.name
    );
    assert!(
        !names.contains(&"v".to_string()),
        "{}: the old name still answers, views are {names:?}",
        target.name
    );

    scratch.teardown().await;
}

/// Introspection reports a view as a view, with its columns.
pub async fn a_view_is_introspected_as_a_view(target: &'static Target) {
    let scratch = Scratch::create(target, "view_shape").await;
    seed_view(&scratch, "SELECT id, name FROM {} WHERE id > 0").await;

    let view = view_of(&scratch).await;
    assert!(view.is_view, "{}: a view read as a base table", target.name);
    let cols: Vec<&str> = view.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(cols, ["id", "name"], "{}: the view's columns", target.name);
    assert!(
        view.view_definition.is_some(),
        "{}: the view came back with no definition, so nothing can draft it",
        target.name
    );

    scratch.teardown().await;
}

/// A result over a view claims no row key it cannot stand behind.
///
/// **All three servers answer "read-only" today**, and that is the branch that
/// asserts. The other branch is not dead weight: a driver may perfectly well
/// start attributing a view's columns to the table underneath — that is a
/// defensible thing for one to do — and the moment it does, the key the resolver
/// picks has to actually identify one row. So the test uses it, by counting what
/// it matches. Which of the two branches runs is itself the finding.
pub async fn a_view_is_never_writable_through_a_key_that_does_not_identify_a_row(
    target: &'static Target,
) {
    let scratch = Scratch::create(target, "view_editable").await;
    seed_view(&scratch, "SELECT id, name FROM {} WHERE id > 0").await;

    let (rs, model) = scratch
        .edit_model(&format!("SELECT * FROM {}", scratch.qualified("v")))
        .await;

    let Some(table) = model.table(0) else {
        // **Today's answer on all three**, and the safe one: neither driver
        // attributes a view's column to a base table in a way the key resolver
        // will write through. Asserted rather than returned — an early return
        // here made this whole test a no-op on every leg, which is how it passed
        // before it checked anything.
        let editable: Vec<&str> = (0..rs.col_count())
            .filter(|&ci| model.editable(ci))
            .map(|ci| rs.columns[ci].name.as_str())
            .collect();
        assert!(
            editable.is_empty(),
            "{}: no table was resolved, yet {editable:?} are offered as editable",
            target.name
        );
        scratch.teardown().await;
        return;
    };

    // It claimed a key. Then the key has to work: count the rows it matches.
    let key: Vec<String> = table
        .key_cols
        .iter()
        .map(|&ci| {
            let col = &rs.columns[ci];
            let value = rs.cell(0, ci).expect("the first row").display().to_string();
            format!(
                "{} = '{}'",
                schemaic_core::export::ident_if_needed(&col.name, target.engine.dialect()),
                value.replace('\'', "''")
            )
        })
        .collect();
    assert!(
        !key.is_empty(),
        "{}: the model offered a writable view with an empty key",
        target.name
    );

    let matched = scratch
        .exec(&format!(
            "SELECT COUNT(*) FROM {} WHERE {}",
            table_ref(&scratch, table.table.as_str(), table.schema.as_deref()),
            key.join(" AND ")
        ))
        .await;
    assert_eq!(
        matched.cell(0, 0).expect("a count").display(),
        "1",
        "{}: the key {key:?} the model chose for the view matches the wrong number of rows",
        target.name
    );

    scratch.teardown().await;
}

/// Create the base table, three rows, and a view `v` over it. `body` carries a
/// single `{}` for the qualified base table.
async fn seed_view(scratch: &Scratch, body: &str) {
    scratch
        .exec(&format!("CREATE TABLE {} {BASE}", scratch.qualified("t")))
        .await;
    scratch
        .exec(&format!(
            "INSERT INTO {} (id, name) VALUES (1, 'one'), (2, 'two'), (3, 'three')",
            scratch.qualified("t")
        ))
        .await;
    let body = body.replace("{}", &scratch.qualified("t"));
    scratch
        .exec(&format!("CREATE VIEW {} AS {body}", scratch.qualified("v")))
        .await;
}

/// Diff the view draft, emit it, run it — the view editor's own path.
async fn apply_view(scratch: &Scratch, current: &TableInfo, draft: &ViewDraft, target: &Target) {
    let set = ddl::diff_view(current, draft, target.engine.dialect());
    assert!(
        !set.changes.is_empty(),
        "{}: the view draft proposed no change — the test changed nothing",
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
                "{}: the view plan failed at statement {} of {stmts:?}: {}",
                target.name, e.at, e.message
            )
        });
}

async fn view_of(scratch: &Scratch) -> TableInfo {
    named_table(scratch, "v").await
}

async fn named_table(scratch: &Scratch, name: &str) -> TableInfo {
    let schema = scratch
        .db
        .fetch_schema(&scratch.database, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("introspecting {}: {e}", scratch.database));
    schema
        .tables
        .into_iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("no object {name:?} in {}", scratch.database))
}

async fn view_names(scratch: &Scratch) -> Vec<String> {
    let schema = scratch
        .db
        .fetch_schema(&scratch.database, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("introspecting {}: {e}", scratch.database));
    schema
        .tables
        .iter()
        .filter(|t| t.is_view)
        .map(|t| t.name.clone())
        .collect()
}

/// `db.table` or `schema.table`, quoted for this server — the same shape
/// `Scratch::qualified` builds, for a name that came from the model rather than
/// from the test.
fn table_ref(scratch: &Scratch, table: &str, schema: Option<&str>) -> String {
    let outer = schema.unwrap_or(&scratch.database);
    format!(
        "{}.{}",
        schemaic_core::export::ident_sql(outer, scratch.dialect()),
        schemaic_core::export::ident_sql(table, scratch.dialect())
    )
}
