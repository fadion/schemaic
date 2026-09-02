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

/// A trigger read off the server, emitted from its own draft and read back is
/// the same trigger — and the editor is then clean against the *first* reading.
///
/// **The gate crosses the emitter and the server**, and it has to. Diffing a
/// trigger set against `TriggerSetDraft::from_table` of the same table compares
/// `d.info == *cur` where `TriggerDraft::from_info` set `info: t.clone()` — two
/// copies of one value, so the answer is "0 changes" for any trigger at all, and
/// the test would have passed against a server that returned nothing.
///
/// So the draft is diffed against an **empty** current, which makes the emitter
/// write the trigger out, and what the server reports afterwards is compared
/// with what it reported before.
pub async fn an_introspected_trigger_diffs_to_nothing_against_its_own_draft(
    target: &'static Target,
) {
    let dialect = target.engine.dialect();
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

    // Drop it, then put the server's *own* reading of it back through the
    // emitter. Dropping first because a `CREATE TRIGGER` of a name that already
    // exists is an error on both engines — the editor's own path is a drop and
    // a create, which is what `diff_triggers` emits for a changed one.
    let draft = TriggerSetDraft::from_table(&table);
    // The same set with no triggers in it — `TriggerSetDraft::default()` would
    // also lose the table it is *on*, and PostgreSQL's `DROP TRIGGER … ON ""`
    // names nothing.
    let mut empty = draft.clone();
    empty.triggers.clear();
    let set = ddl::diff_triggers(&table.triggers, &empty, dialect);
    assert!(!set.changes.is_empty(), "{}: nothing to drop", target.name);
    run_ddl(&scratch, &set.emit(), target).await;
    let set = ddl::diff_triggers(&[], &draft, dialect);
    assert!(
        !set.changes.is_empty(),
        "{}: the gate is vacuous — a trigger against an empty set proposed no change",
        target.name
    );
    run_ddl(&scratch, &set.emit(), target).await;

    // The server's two readings of the same trigger, either side of the trip.
    let after = table_of(&scratch).await;
    assert_eq!(
        after.triggers, table.triggers,
        "{}: the trigger changed by being written back through the emitter",
        target.name
    );

    // And the editor is clean: `current` and `draft` are now independent
    // readings, so this comparison has content.
    let settled = ddl::diff_triggers(
        &table.triggers,
        &TriggerSetDraft::from_table(&after),
        dialect,
    );
    assert!(
        settled.changes.is_empty(),
        "{}: the trigger no longer round-trips: {:?}\n      emitting {:?}",
        target.name,
        settled.changes,
        settled.emit()
    );

    // It still fires, which is what a trigger in the catalogue doing nothing
    // would not.
    assert_eq!(
        insert_and_read(&scratch, 1, "ada").await,
        "ADA",
        "{}: the re-created trigger is in the catalogue and does nothing",
        target.name
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

    // **PostgreSQL's trigger function outlives the trigger**, which is the
    // divergence this file leads with and the one nothing asserted: dropping the
    // trigger must not take the function with it, because the function is a
    // separate object the user may still be calling, and dropping it out from
    // under another trigger breaks every write to that table.
    if let Some(function) = target.trigger_function_name {
        let still = scratch
            .exec(&format!(
                "SELECT count(*) FROM pg_proc WHERE proname = '{function}'"
            ))
            .await;
        assert_eq!(
            still.cell(0, 0).expect("a count").text(),
            "1",
            "{}: dropping the trigger took its function {function} with it",
            target.name
        );
    }

    scratch.teardown().await;
}

/// **Two triggers on one table**, so `diff_triggers`' set semantics are asked
/// something at last: every other test here has zero or one, where "the set
/// changed" and "this trigger changed" are the same statement.
///
/// Dropping one leaves the other firing, which is the failure a set diff that
/// re-emitted the whole set would produce and that a one-trigger fixture cannot
/// see.
pub async fn one_of_two_triggers_can_be_dropped_without_the_other(target: &'static Target) {
    let scratch = Scratch::create(target, "trg_pair").await;
    seed(&scratch, target).await;
    add_trigger(&scratch, target, "up").await;

    // The second fires on **UPDATE**, so the two cannot be confused for one and
    // the pair is legal on every engine here — MySQL refuses `SET NEW.x` in an
    // AFTER trigger (ERROR 1362), which is what makes the timing the wrong axis
    // to vary and the event the right one.
    let table = table_of(&scratch).await;
    let mut draft = TriggerSetDraft::from_table(&table);
    let mut second = new_trigger(&scratch, target, "up2");
    second.info.events = vec![TriggerEvent::Update];
    draft.triggers.push(second);
    apply(&scratch, &table, &draft, target).await;

    let both = table_of(&scratch).await;
    assert_eq!(
        both.triggers.len(),
        2,
        "{}: the second trigger did not land",
        target.name
    );
    // The identity gate, over a set of two: still no change proposed.
    let settled = ddl::diff_triggers(
        &both.triggers,
        &TriggerSetDraft::from_table(&both),
        target.engine.dialect(),
    );
    assert!(
        settled.changes.is_empty(),
        "{}: a two-trigger set proposed {:?}",
        target.name,
        settled.changes
    );

    // Drop only the AFTER one.
    let mut draft = TriggerSetDraft::from_table(&both);
    draft.triggers.retain(|t| t.info.name != "up2");
    apply(&scratch, &both, &draft, target).await;

    let left = table_of(&scratch).await;
    let names: Vec<&str> = left.triggers.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        ["up"],
        "{}: dropping one trigger of two left {names:?}",
        target.name
    );
    // And the survivor still fires — a set diff that re-emitted everything
    // would have dropped and recreated it, and a set diff that dropped the
    // wrong one would leave it gone.
    assert_eq!(
        insert_and_read(&scratch, 1, "ada").await,
        "ADA",
        "{}: the trigger that was kept has stopped firing",
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

/// Run an already-emitted plan, failing loudly with the statement that refused.
async fn run_ddl(scratch: &Scratch, stmts: &[String], target: &Target) {
    assert!(!stmts.is_empty(), "{}: nothing to run", target.name);
    scratch
        .db
        .run_ddl(&scratch.database, stmts, CancellationToken::new())
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
