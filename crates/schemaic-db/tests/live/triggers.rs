//! Triggers: introspected, added, edited, dropped — and made to fire.
//!
//! **A trigger is the object the two engines disagree about most.** MySQL carries
//! the body on the trigger itself; PostgreSQL has none at all and calls a
//! function with its own separate lifetime — dropping the trigger leaves the
//! function behind, and dropping the function out from under the trigger breaks
//! every write to the table. `TriggerInfo` holds both shapes so introspection
//! never has to lie about what a server reported, which means the model is
//! *wider* than either engine and only a real one can say whether what comes back
//! is restatable.
//!
//! Every test here ends by making the trigger fire, or by proving it no longer
//! does. A trigger that exists in the catalogue and does nothing is the failure
//! that reads as success everywhere else: `SHOW CREATE TRIGGER` is happy, the
//! diff is empty, and the table quietly stops being maintained.

use schemaic_core::ddl::{self, TriggerDraft, TriggerSetDraft};
use schemaic_core::schema::{TableInfo, TriggerAction, TriggerEvent, TriggerLevel, TriggerTiming};
use tokio_util::sync::CancellationToken;

use crate::endpoint::Target;
use crate::scratch::Scratch;

/// A trigger read off the server and drafted straight back proposes no change.
pub async fn an_introspected_trigger_diffs_to_nothing_against_its_own_draft(
    target: &'static Target,
) {
    let scratch = Scratch::create(target, "trg_identity").await;
    seed(&scratch, target).await;
    add_trigger(&scratch, target, "up").await;

    let table = table_of(&scratch).await;
    assert_eq!(
        table.triggers.len(),
        1,
        "{}: introspection did not find the trigger",
        target.name
    );
    let set = ddl::diff_triggers(
        &table.triggers,
        &TriggerSetDraft::from_table(&table),
        target.engine.dialect(),
    );

    assert!(
        set.changes.is_empty(),
        "{}: a trigger drafted from itself proposed {:?}\n      emitting {:?}",
        target.name,
        set.changes,
        set.emit()
    );

    scratch.teardown().await;
}

/// A trigger added through the editor lands, and fires.
pub async fn an_added_trigger_lands_and_fires(target: &'static Target) {
    let scratch = Scratch::create(target, "trg_add").await;
    seed(&scratch, target).await;
    add_trigger(&scratch, target, "up").await;

    assert_eq!(
        insert_and_read(&scratch, 1, "quiet").await,
        "QUIET",
        "{}: the trigger is in the catalogue and did not fire",
        target.name
    );

    scratch.teardown().await;
}

/// A trigger dropped through the editor stops firing.
pub async fn a_dropped_trigger_stops_firing(target: &'static Target) {
    let scratch = Scratch::create(target, "trg_drop").await;
    seed(&scratch, target).await;
    add_trigger(&scratch, target, "up").await;
    assert_eq!(
        insert_and_read(&scratch, 1, "before").await,
        "BEFORE",
        "{}: the trigger never fired to begin with",
        target.name
    );

    // A trigger missing from the set draft is a drop.
    let table = table_of(&scratch).await;
    let mut draft = TriggerSetDraft::from_table(&table);
    draft.triggers.clear();
    apply(&scratch, &table, &draft, target).await;

    assert!(
        table_of(&scratch).await.triggers.is_empty(),
        "{}: the trigger is still on the table",
        target.name
    );
    assert_eq!(
        insert_and_read(&scratch, 2, "after").await,
        "after",
        "{}: the dropped trigger is still firing",
        target.name
    );

    scratch.teardown().await;
}

/// A renamed trigger keeps firing under its new name.
///
/// A rename is a drop and a create on both engines — there is no verb for it —
/// so what this really asserts is that the pair is emitted as one plan and the
/// table is never left without the trigger it is supposed to have.
pub async fn a_renamed_trigger_still_fires(target: &'static Target) {
    let scratch = Scratch::create(target, "trg_rename").await;
    seed(&scratch, target).await;
    add_trigger(&scratch, target, "up").await;

    let table = table_of(&scratch).await;
    let mut draft = TriggerSetDraft::from_table(&table);
    draft.triggers[0].info.name = "up_renamed".to_string();
    apply(&scratch, &table, &draft, target).await;

    let after = table_of(&scratch).await;
    let names: Vec<&str> = after.triggers.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        ["up_renamed"],
        "{}: the triggers on the table after a rename",
        target.name
    );
    assert_eq!(
        insert_and_read(&scratch, 1, "renamed").await,
        "RENAMED",
        "{}: the renamed trigger does not fire",
        target.name
    );

    // And it still round-trips, so the editor is not left proposing the rename
    // again every time it opens.
    let settled = ddl::diff_triggers(
        &after.triggers,
        &TriggerSetDraft::from_table(&after),
        target.engine.dialect(),
    );
    assert!(
        settled.changes.is_empty(),
        "{}: after the rename the trigger no longer round-trips: {:?}",
        target.name,
        settled.changes
    );

    scratch.teardown().await;
}

/// The base table, and — on a server whose triggers call one — the function they
/// call.
async fn seed(scratch: &Scratch, target: &Target) {
    scratch
        .exec(&format!(
            "CREATE TABLE {} (id INTEGER NOT NULL PRIMARY KEY, name VARCHAR(32))",
            scratch.qualified("t")
        ))
        .await;
    if let Some(ddl) = target.trigger_function_ddl {
        scratch.exec(ddl).await;
    }
}

/// Add a trigger named `name` through the editor's own path, and check it landed.
async fn add_trigger(scratch: &Scratch, target: &Target, name: &str) {
    let table = table_of(scratch).await;
    let mut draft = TriggerSetDraft::from_table(&table);
    draft.triggers.push(new_trigger(scratch, target, name));
    apply(scratch, &table, &draft, target).await;
    assert_eq!(
        table_of(scratch).await.triggers.len(),
        1,
        "{}: the trigger did not land",
        target.name
    );
}

/// A `BEFORE INSERT … FOR EACH ROW` trigger that uppercases `name`, in whichever
/// of the two shapes this server has.
fn new_trigger(scratch: &Scratch, target: &Target, name: &str) -> TriggerDraft {
    let mut draft = TriggerDraft::blank(name, "t", scratch.namespace.map(str::to_string));
    draft.info.timing = TriggerTiming::Before;
    draft.info.events = vec![TriggerEvent::Insert];
    draft.info.level = TriggerLevel::Row;
    draft.info.action = match (target.trigger_body, target.trigger_function_name) {
        (Some(body), _) => TriggerAction::Body(body.to_string()),
        (None, Some(function)) => TriggerAction::Function {
            name: function.to_string(),
            args: Vec::new(),
        },
        (None, None) => panic!(
            "{}: this leg describes neither a trigger body nor a trigger function",
            target.name
        ),
    };
    draft
}

/// Diff the trigger set, emit it, run it — the trigger modal's own path.
async fn apply(scratch: &Scratch, current: &TableInfo, draft: &TriggerSetDraft, target: &Target) {
    let set = ddl::diff_triggers(&current.triggers, draft, target.engine.dialect());
    assert!(
        !set.changes.is_empty(),
        "{}: the trigger draft proposed no change — the test changed nothing",
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
                "{}: the trigger plan failed at statement {} of {stmts:?}: {}",
                target.name, e.at, e.message
            )
        });
}

/// Insert a row and read back what the table actually stored — the only way to
/// ask whether a trigger ran.
async fn insert_and_read(scratch: &Scratch, id: i64, name: &str) -> String {
    scratch
        .exec(&format!(
            "INSERT INTO {} (id, name) VALUES ({id}, '{name}')",
            scratch.qualified("t")
        ))
        .await;
    let rs = scratch
        .exec(&format!(
            "SELECT name FROM {} WHERE id = {id}",
            scratch.qualified("t")
        ))
        .await;
    rs.cell(0, 0)
        .expect("the row just inserted")
        .display()
        .to_string()
}

async fn table_of(scratch: &Scratch) -> TableInfo {
    let schema = scratch
        .db
        .fetch_schema(&scratch.database, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("introspecting {}: {e}", scratch.database));
    schema
        .tables
        .into_iter()
        .find(|t| t.name == "t")
        .unwrap_or_else(|| panic!("no table t in {}", scratch.database))
}
