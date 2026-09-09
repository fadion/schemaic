//! Stored routines: what a redefinition has to carry across, proved against a
//! server rather than against the emitter's own idea of itself.
//!
//! **PostgreSQL's `CREATE OR REPLACE FUNCTION` replaces the whole routine.**
//! Every attribute the statement does not restate reverts to the server's
//! default, and the defaults are all the *quiet* ones: `PARALLEL UNSAFE`,
//! `COST 100`, `ROWS 1000`, not leakproof. So a redefinition that drops one
//! breaks nothing a test can see from the SQL — the function still exists, still
//! returns the same answers, and merely stops being usable in a parallel plan or
//! starts lying to the planner by a factor of twenty. That is exactly why this
//! has to be asserted **off the catalogue**, and why the pure emitter tests in
//! `core::schema` cannot stand in for it: they can only agree with whatever the
//! emitter already writes.
//!
//! MySQL has none of these attributes and no `CREATE OR REPLACE` for a routine
//! at all, so this module's one test returns early on those legs. It is the only
//! thing in the tier that does — see [`crate`]'s note on why a leg that quietly
//! passes is worth less than no test at all — and it says so on stderr.

use schemaic_core::intel::SqlDialect;
use schemaic_core::schema::{Parallel, RoutineInfo};
use tokio_util::sync::CancellationToken;

use crate::endpoint::Target;
use crate::scratch::Scratch;

/// A function declared `PARALLEL SAFE COST 5 ROWS 3` is read back with all
/// three, redefined through Schemaic's own emitter, and still has all three.
///
/// The redefinition is the point. Reading is half the fix and would pass with
/// an emitter that writes nothing; emitting is the other half and would pass
/// against a reader that invented the values. Only the round trip through the
/// server catches either.
///
/// `LEAKPROOF` is exercised **only if the connection may set it** — PostgreSQL
/// requires superuser for that one attribute, and the live user is not one on a
/// stock setup. It is attempted, and the assertion is made conditional on the
/// attempt having worked, so the test neither skips it silently on a superuser
/// connection nor fails on an ordinary one.
pub async fn a_pg_redefinition_keeps_a_functions_planner_attributes(target: &'static Target) {
    if target.engine.dialect() != SqlDialect::Postgres {
        eprintln!(
            "live: {} has no PARALLEL/COST/ROWS to lose — this test asserted nothing",
            target.name
        );
        return;
    }
    let scratch = Scratch::create(target, "routine_attrs").await;
    let ns = target.namespace.expect("PostgreSQL has a schema");

    // A set-returning function, so `ROWS` is applicable — the server refuses it
    // outright on anything else, which is why the model only carries it for one.
    scratch
        .exec(
            "CREATE FUNCTION f(x integer) RETURNS SETOF integer \
             LANGUAGE sql IMMUTABLE PARALLEL SAFE COST 5 ROWS 3 \
             AS 'SELECT x'",
        )
        .await;
    // Superuser-only, so it is a separate statement whose failure is tolerated.
    let leakproof = scratch
        .db
        .run_ddl(
            &scratch.database,
            &[format!("ALTER FUNCTION {ns}.f(integer) LEAKPROOF")],
            CancellationToken::new(),
        )
        .await
        .is_ok();

    let before = routine_of(&scratch).await;
    assert_eq!(
        before.parallel,
        Parallel::Safe,
        "{}: PARALLEL SAFE was not read off the catalogue",
        target.name
    );
    assert_eq!(before.cost.as_deref(), Some("5"), "{}: COST", target.name);
    assert_eq!(before.rows.as_deref(), Some("3"), "{}: ROWS", target.name);
    assert_eq!(
        before.leakproof,
        leakproof,
        "{}: LEAKPROOF read back as {} after an ALTER that {}",
        target.name,
        before.leakproof,
        if leakproof {
            "succeeded"
        } else {
            "was refused"
        }
    );

    // The edit a user would make: one line of the body, nothing else. The
    // statement is Schemaic's own — this is the emitter under test.
    let mut edited = before.clone();
    edited.body = "SELECT x + 0".to_string();
    let sql = edited.create_sql(SqlDialect::Postgres, true);
    scratch
        .db
        .run_ddl(
            &scratch.database,
            std::slice::from_ref(&sql),
            CancellationToken::new(),
        )
        .await
        .unwrap_or_else(|e| panic!("{}: the redefinition was refused: {e}\n{sql}", target.name));

    let after = routine_of(&scratch).await;
    assert_eq!(
        after.body.trim(),
        "SELECT x + 0",
        "{}: the edit did not land, so nothing below is about a redefinition",
        target.name
    );
    assert_eq!(
        after.parallel,
        Parallel::Safe,
        "{}: the redefinition reset the function to PARALLEL UNSAFE\n{sql}",
        target.name
    );
    assert_eq!(
        after.cost.as_deref(),
        Some("5"),
        "{}: the redefinition reset COST\n{sql}",
        target.name
    );
    assert_eq!(
        after.rows.as_deref(),
        Some("3"),
        "{}: the redefinition reset ROWS\n{sql}",
        target.name
    );
    assert_eq!(
        after.leakproof, leakproof,
        "{}: the redefinition changed LEAKPROOF\n{sql}",
        target.name
    );

    scratch.teardown().await;
}

async fn routine_of(scratch: &Scratch) -> RoutineInfo {
    let schema = scratch
        .db
        .fetch_schema(&scratch.database, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("introspecting {}: {e}", scratch.database));
    schema
        .routines
        .iter()
        .find(|r| r.name == "f")
        .map(|r| (**r).clone())
        .unwrap_or_else(|| panic!("no function f in {}", scratch.database))
}
