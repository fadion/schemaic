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

    // …and a second one carrying every field this leg's grammar has, so the
    // whole-struct compare below is handed something to lose. See
    // `wide_trigger`.
    {
        let table = table_of(&scratch).await;
        let mut draft = TriggerSetDraft::from_table(&table);
        draft.triggers.push(wide_trigger(&scratch, target, "wide"));
        apply(&scratch, &table, &draft, target).await;
    }

    let table = table_of(&scratch).await;
    assert_eq!(
        table.triggers.len(),
        2,
        "{}: introspection did not find both triggers",
        target.name
    );
    // The fixture is only worth what the server read back off it: on a leg with
    // a `WHEN` guard, a `condition` that came back `None` means the assertion
    // below is comparing two defaults.
    if target.trigger_condition.is_some() {
        let wide = table
            .triggers
            .iter()
            .find(|t| t.name == "wide")
            .unwrap_or_else(|| panic!("{}: the wide trigger is not in the model", target.name));
        assert!(
            wide.condition.is_some(),
            "{}: the WHEN guard was not read back, so the round trip below \
             asserts nothing about it",
            target.name
        );
        assert_eq!(
            wide.update_columns, target.trigger_update_columns,
            "{}: the UPDATE OF columns were not read back",
            target.name
        );
    }

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
    // The drafted event actually landed. Nothing else here looks at it, so an
    // emitter that wrote the second trigger out as `BEFORE INSERT` like the
    // first would leave a set of two the rest of this test cannot tell apart.
    let up2 = both
        .triggers
        .iter()
        .find(|t| t.name == "up2")
        .unwrap_or_else(|| panic!("{}: the second trigger is not in the model", target.name));
    assert_eq!(
        up2.events,
        [TriggerEvent::Update],
        "{}: the second trigger did not land on the event it was drafted for",
        target.name
    );
    // The settle gate, over a set of two — the case where "the set changed" and
    // "this trigger changed" stop being the same statement.
    assert_writes_back_unchanged(&scratch, target, "adding a second trigger").await;

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

    // And the renamed trigger survives a trip back through the emitter — the
    // case where the draft's `original` and its `name` differ, which the
    // identity test's fresh fixture never has.
    assert_writes_back_unchanged(&scratch, target, "the rename").await;

    scratch.teardown().await;
}

/// The base table, and — on a server whose triggers call one — the function they
/// call.
/// **Two triggers in one `(table, timing, event)` group, which no *live* leg
/// had** — every fixture here varies the event, so `order` came back `None` on
/// all three servers and the fault below was invisible to a green suite.
///
/// Narrower than it first reads, and the broad version is false: `core::dump`'s
/// `a_dumped_trigger_group_names_nothing_the_file_has_not_created_yet` builds
/// exactly this group in the unit tier. That is the shape of the gap rather
/// than an exception to it — the resolution step was written for the dump and
/// tested by the dump, and the apply path had neither.
///
/// The catalogue gives a group's **leader** `PRECEDES <successor>` and every
/// other member `FOLLOWS <predecessor>`, and `ChangeSet::trigger_statements`
/// emitted every create's clause unconditionally. So recreating the group —
/// which is what a dump, a Copy DDL and any edit inside the group all do — put
/// `PRECEDES b` on the *first* statement, naming a trigger the plan had not
/// created yet. Both servers refuse that, and because MySQL-family DDL has no
/// transaction the drops above it have already committed: **both triggers gone**,
/// and unrepairable in-app since the modal has no control for `order`.
///
/// Measured before the fix, through this very test: MariaDB 10.11.14
/// `ERROR 4031`, MySQL 8.4.11 `ERROR 3011`, on the first `CREATE TRIGGER` of the
/// write-back.
///
/// **Both directions, and the second is why the fix is not just "strip it".**
/// Editing one trigger inside a group that already exists must keep its clause,
/// or the recreated trigger is appended last instead of landing back where it
/// was — so the test edits `b` and asserts the group's order afterwards, by
/// firing it.
///
/// MySQL-family only, and gated on `Target::trigger_body` rather than on an
/// engine: PostgreSQL has no ordering clause in its `CREATE TRIGGER` grammar at
/// all (it orders by name), and `pg::run_ddl` is transactional besides.
pub async fn an_ordered_pair_of_triggers_writes_back_without_naming_a_dropped_one(
    target: &'static Target,
) {
    let Some(_) = target.trigger_body else {
        // PostgreSQL: no ordering clause to resolve.
        return;
    };
    let scratch = Scratch::create(target, "trg_order").await;
    seed(&scratch, target).await;

    // Two BEFORE INSERT triggers on one table — the group the fixtures never
    // built. Each appends a letter, so the *order* they ran in is readable off
    // the row afterwards.
    for (name, letter) in [("oa", "A"), ("ob", "B")] {
        let table = table_of(&scratch).await;
        let mut draft = TriggerSetDraft::from_table(&table);
        let mut t = new_trigger(&scratch, target, name);
        t.info.action = TriggerAction::Body(format!("SET NEW.name = CONCAT(NEW.name, '{letter}')"));
        draft.triggers.push(t);
        apply(&scratch, &table, &draft, target).await;
    }
    let table = table_of(&scratch).await;
    assert_eq!(
        table.triggers.len(),
        2,
        "{}: the group did not land",
        target.name
    );
    // The premise: the server really did report an ordering clause, or the rest
    // of this test is about nothing.
    assert!(
        table.triggers.iter().any(|t| t.order.is_some()),
        "{}: no trigger in the group carries an ordering clause, so this test \
         asserts nothing about resolving one: {:?}",
        target.name,
        table.triggers
    );
    assert_eq!(insert_and_read(&scratch, 1, "x").await, "xAB");

    // The write-back of the server's own reading — the step that destroyed both.
    assert_writes_back_unchanged(&scratch, target, "a group of two ordered triggers").await;
    assert_eq!(
        insert_and_read(&scratch, 2, "y").await,
        "yAB",
        "{}: the group came back in the wrong order",
        target.name
    );

    // And an edit *inside* the group keeps the edited trigger's position, which
    // is the half a conservative "always strip it" fix would lose.
    let table = table_of(&scratch).await;
    let mut draft = TriggerSetDraft::from_table(&table);
    let second = draft
        .triggers
        .iter_mut()
        .find(|t| t.info.name == "ob")
        .expect("the second trigger is in the draft");
    second.info.action = TriggerAction::Body("SET NEW.name = CONCAT(NEW.name, 'C')".to_string());
    apply(&scratch, &table, &draft, target).await;
    assert_eq!(
        insert_and_read(&scratch, 3, "z").await,
        "zAC",
        "{}: editing the second trigger moved it out of position",
        target.name
    );

    scratch.teardown().await;
}

/// **A body whose first token merely *starts* with an ordering keyword survives
/// the round trip**, and the trigger it belongs to survives an unrelated edit.
///
/// `mysql::trigger_body_of` matched `FOLLOWS`/`PRECEDES` as a bare byte prefix
/// where the `FOR EACH ROW` anchor beside it goes through `sql::skip_noncode`
/// for exactly that reason. A labelled compound statement is legal SQL and a
/// label is any identifier, so `followsx: BEGIN … END followsx` matched, one
/// identifier's worth was eaten as an ordering clause, and the body came back
/// without its first token — into **both** the editable draft and the diff
/// baseline. `TriggerDraft::validate` parses no body, so nothing objected;
/// applying any unrelated edit then emitted `DROP TRIGGER` and an invalid
/// `CREATE`, run in sequence with no transaction, and the measured outcome on
/// MariaDB 10.11.14 was `ERROR 1064` with the trigger gone and unrepairable
/// in-app.
///
/// **Two assertions, and the second is the one that matters.** That the body
/// reads back whole is the unit property, and the unit test beside
/// `trigger_body_of` already holds it. What only a server can say is that the
/// *write-back* of that reading is a statement it accepts — which is the step
/// that destroyed the trigger.
///
/// Gated on `Target::trigger_body`, not on an engine: a leg that carries its
/// action on the trigger is a leg with a body to label, and PostgreSQL's action
/// is a function call with no body and no ordering clause in its grammar at all.
pub async fn a_body_that_opens_like_an_ordering_clause_survives_the_round_trip(
    target: &'static Target,
) {
    let Some(_) = target.trigger_body else {
        // PostgreSQL: no body on the trigger, so nothing here to label.
        return;
    };
    let scratch = Scratch::create(target, "trg_label").await;
    seed(&scratch, target).await;

    // Both keywords, and a label that is the keyword plus one letter — the
    // narrowest version of the bug.
    //
    // **Different events, deliberately.** Two triggers in one
    // `(table, timing, event)` group is a *different* fault's fixture — the
    // emitter writes an ordering clause naming a trigger the same plan has not
    // created yet — and putting both here made this test fail on that instead
    // (ERROR 4031 on MariaDB, 3011 on MySQL). One test, one property:
    // `an_ordered_pair_of_triggers_writes_back_without_naming_a_dropped_one`
    // owns the ordering.
    for (name, label, event) in [
        ("lf", "followsx", TriggerEvent::Insert),
        ("lp", "precedes_it", TriggerEvent::Update),
    ] {
        let table = table_of(&scratch).await;
        let mut draft = TriggerSetDraft::from_table(&table);
        let mut t = new_trigger(&scratch, target, name);
        t.info.events = vec![event];
        t.info.action = TriggerAction::Body(format!(
            "{label}: BEGIN SET NEW.name = UPPER(NEW.name); END {label}"
        ));
        draft.triggers.push(t);
        apply(&scratch, &table, &draft, target).await;

        // **Read through `Db::trigger_source`, which is the path that parses
        // `SHOW CREATE TRIGGER`.** Introspection reads
        // `information_schema.ACTION_STATEMENT` and so never reaches
        // `trigger_body_of` at all — asserting over `table_of` here passed
        // against the unfixed reader, which is how nearly it became a
        // decoration. `trigger_source` is also what the trigger editor calls,
        // so this is the reading the user actually gets.
        let source = scratch
            .db
            .trigger_source(Some(&scratch.database), name)
            .await
            .unwrap_or_else(|e| panic!("{}: trigger_source({name}): {e}", target.name))
            .unwrap_or_else(|| {
                panic!(
                    "{}: trigger_source returned nothing for {name}, so this \
                     test is asserting over a path it does not reach",
                    target.name
                )
            });
        assert!(
            source.body.trim_start().starts_with(label),
            "{}: {name}'s body lost its opening token — the server printed a \
             label the reader ate as an ordering clause: {:?}",
            target.name,
            source.body
        );
    }

    // And the whole set written back through the emitter is a plan the server
    // accepts and that changes nothing — the step where a damaged body becomes
    // a dropped trigger.
    assert_writes_back_unchanged(
        &scratch,
        target,
        "a label that opens like an ordering clause",
    )
    .await;

    // The triggers still fire, which is the property a catalogue row cannot
    // report.
    assert_eq!(insert_and_read(&scratch, 1, "abc").await, "ABC");

    scratch.teardown().await;
}

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

/// The same trigger with **everything this leg's grammar has on it**: an
/// `UPDATE OF <columns>` event, a `WHEN` guard, and — where the action allows
/// it — `AFTER` rather than `BEFORE`.
///
/// `TriggerInfo::condition` and `TriggerInfo::update_columns` are two of the
/// fields the model widened itself for, and every fixture here left them at
/// their defaults, so an emitter that dropped either on the drop-and-create
/// every trigger edit performs would pass every test in this file — leaving a
/// trigger that fires on every row and every column instead of the ones it was
/// written for. The whole-struct `assert_eq!` in the identity test is strong
/// enough to see it; it was being handed a fixture at the narrow end of the
/// model's width.
///
/// `AFTER` rides on the same `(body, function)` distinction `new_trigger`
/// already switches on rather than a new field: MySQL's body assigns to `NEW`,
/// which an `AFTER` trigger may not do, while PostgreSQL's function returns
/// `NEW` and an `AFTER ROW` trigger simply ignores it.
///
/// Still uncovered, and said here rather than left to be re-derived:
/// `TriggerLevel::Statement`, which needs a per-leg action that references no
/// row, and `TriggerTiming::InsteadOf`, which needs a view to be on.
fn wide_trigger(scratch: &Scratch, target: &Target, name: &str) -> TriggerDraft {
    let mut draft = new_trigger(scratch, target, name);
    draft.info.events = vec![TriggerEvent::Update];
    draft.info.update_columns = target
        .trigger_update_columns
        .iter()
        .map(|c| c.to_string())
        .collect();
    draft.info.condition = target.trigger_condition.map(str::to_string);
    if target.trigger_body.is_none() {
        draft.info.timing = TriggerTiming::After;
    }
    draft
}

/// Diff the trigger set, emit it, run it — the trigger modal's own path.
async fn apply(scratch: &Scratch, current: &TableInfo, draft: &TriggerSetDraft, target: &Target) {
    let set = ddl::diff_triggers(&current.triggers, draft, target.engine.dialect());
    scratch.apply_plan(&set, "trigger").await;
}

/// The trigger set the server now reports, written back through the emitter,
/// comes back **unchanged** — asked after a specific edit rather than on a fresh
/// fixture.
///
/// **This is the shape a settle assertion has to have here, and two tests had
/// the other one.** `TriggerSetDraft::from_table(&after)` builds each draft's
/// `info` by cloning the very `TriggerInfo` the diff then compares it against,
/// so `diff_triggers(&after.triggers, &from_table(&after))` compares one
/// expression with itself: it answers "0 changes" for anything the server could
/// return, a silently rewritten trigger included, and the message those sites
/// carried — "the editor is not left proposing the rename again" — was not a
/// claim their code made. `ddl::an_untouched_trigger_set_is_not_a_change` pins
/// the vacuity without a server.
///
/// **And the obvious repair does not work**, which is worth stating so it is not
/// tried again: `diff_triggers(&after.triggers, the_draft_that_was_applied)` —
/// the shape that *is* right for a table — proposes a drop and a create on all
/// three engines, because [`TriggerDraft::original`] is pre-apply bookkeeping.
/// A draft that renamed `up` to `up_renamed` still carries `original: Some("up")`
/// after the rename has landed, and a newly pushed trigger still carries
/// `original: None`; against the server's new reading both read as further work.
/// The editor re-derives its draft from a fresh reading after an apply, so that
/// staleness is the test's, not the app's.
///
/// So the same escape [`views`](crate::views) uses, in the shape triggers allow:
/// an **empty** current forces the emitter to write every trigger out, the
/// statements run, and the two assertions compare values the server produced at
/// two different times.
async fn assert_writes_back_unchanged(scratch: &Scratch, target: &Target, what: &str) {
    let dialect = target.engine.dialect();
    let before = table_of(scratch).await;
    let draft = TriggerSetDraft::from_table(&before);

    // Drop first: `CREATE TRIGGER` of a name that already exists is an error on
    // both engines, and the editor's own path for a changed trigger is a drop
    // and a create. `TriggerSetDraft::default()` would lose the table it is
    // *on*, and PostgreSQL's `DROP TRIGGER … ON ""` names nothing.
    let mut empty = draft.clone();
    empty.triggers.clear();
    let drop_set = ddl::diff_triggers(&before.triggers, &empty, dialect);
    assert!(
        !drop_set.changes.is_empty(),
        "{}: the settle gate is vacuous after {what} — dropping every trigger \
         proposed no change",
        target.name
    );
    run_ddl(scratch, &drop_set.emit(), target).await;

    let create_set = ddl::diff_triggers(&[], &draft, dialect);
    assert!(
        !create_set.changes.is_empty(),
        "{}: the settle gate is vacuous after {what} — the server's own triggers \
         against an empty set proposed no change",
        target.name
    );
    run_ddl(scratch, &create_set.emit(), target).await;

    let after = table_of(scratch).await;
    assert_eq!(
        after.triggers, before.triggers,
        "{}: after {what} the trigger set changed by being written back through \
         the emitter",
        target.name
    );
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
