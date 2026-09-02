//! Two namespaces holding the same table name, and the identity that keeps them
//! apart.
//!
//! **This is a data-safety rule, not a browsing convenience.** `analyze_edit`
//! groups a result's columns by `(database, schema, table)` and its own comment
//! says why: without the namespace, same-named tables in two of them collapse
//! into one, and an `UPDATE` built for one addresses the other's rows. Nothing
//! about that failure is loud — the statement succeeds, one row is affected, the
//! net is satisfied, and the wrong table changed.
//!
//! It needs two namespaces to test in, and what a namespace *is* differs:
//! PostgreSQL has a level between database and table, MySQL does not and a
//! database is that level. `Scratch::alt_namespace` makes whichever this server
//! has, so the tests below are written once and mean the same thing on both.

use schemaic_core::model::{GridWrite, RowEdit, Value};
use tokio_util::sync::CancellationToken;

use crate::endpoint::Target;
use crate::scratch::{Namespace, Scratch};

/// Two namespaces, each with an `orders` table of a different shape.
///
/// Deliberately different column sets: two tables that merely share a name would
/// hide a collapse behind identical shapes, and this is the fixture the
/// `warehouse` sample database exists for at a larger scale. They also share one
/// column, which is what the *write* test needs — see [`seed_both`].
pub async fn same_named_tables_in_two_namespaces_stay_distinct(target: &'static Target) {
    let mut scratch = Scratch::create(target, "ns_distinct").await;
    let alt = seed_both(&mut scratch).await;

    let here = table_in(&scratch, &scratch.namespace_ref(), "orders").await;
    let there = table_in(&scratch, &alt, "orders").await;

    assert_eq!(
        here.iter().map(String::as_str).collect::<Vec<_>>(),
        ["id", "amount", "label"],
        "{}: the primary namespace's orders",
        target.name
    );
    assert_eq!(
        there.iter().map(String::as_str).collect::<Vec<_>>(),
        ["id", "customer", "label"],
        "{}: the second namespace's orders",
        target.name
    );

    scratch.teardown().await;
}

/// A result says which namespace it read from, not merely which table.
pub async fn a_result_names_the_namespace_it_read_from(target: &'static Target) {
    let mut scratch = Scratch::create(target, "ns_provenance").await;
    let alt = seed_both(&mut scratch).await;

    let rs = scratch
        .exec_in(
            &alt,
            &format!("SELECT * FROM {}", scratch.qualified_in(&alt, "orders")),
        )
        .await;

    let origin = rs.columns[0]
        .origin
        .as_ref()
        .unwrap_or_else(|| panic!("{}: no provenance on the second namespace", target.name));
    assert_eq!(
        origin.database, alt.database,
        "{}: the database the column came from",
        target.name
    );
    assert_eq!(
        origin.schema, alt.schema,
        "{}: the namespace the column came from",
        target.name
    );
    // And it is not the other one, which is the whole point.
    assert!(
        (origin.database.as_str(), origin.schema.as_deref())
            != (scratch.database.as_str(), scratch.namespace),
        "{}: the second namespace's column was attributed to the first",
        target.name
    );

    scratch.teardown().await;
}

/// An edit read from one namespace lands in that one, and leaves the other alone.
///
/// The failure this guards is silent: the statement succeeds, exactly one row is
/// affected, the 1-row net is satisfied — and the wrong table changed. So the
/// assertion is on *both* tables, and the one that matters is the untouched one.
pub async fn an_edit_lands_in_the_namespace_it_was_read_from(target: &'static Target) {
    let mut scratch = Scratch::create(target, "ns_write").await;
    let alt = seed_both(&mut scratch).await;

    // Read from the second namespace, and let the model decide where a write
    // would go — the identity under test is the one it resolves, not one the
    // test asserts by hand.
    let (_, model) = scratch
        .edit_model(&format!(
            "SELECT * FROM {}",
            scratch.qualified_in(&alt, "orders")
        ))
        .await;
    let table = model.table(0).unwrap_or_else(|| {
        panic!(
            "{}: the second namespace's table is not writable",
            target.name
        )
    });
    assert_eq!(
        (table.database.as_str(), table.schema.as_deref()),
        (alt.database.as_str(), alt.schema.as_deref()),
        "{}: the model resolved the wrong namespace to write to",
        target.name
    );

    scratch
        .db
        .commit_writes(
            &GridWrite {
                updates: vec![RowEdit {
                    database: table.database.clone(),
                    schema: table.schema.clone(),
                    table: table.table.clone(),
                    // **A column both namespaces have.** Writing `customer`
                    // would be refused outright by the wrong table, so the
                    // assertion below could not have failed; `label` is in both,
                    // so a write aimed at the wrong one lands silently.
                    set: vec![("label".to_string(), Some("moved".to_string()))],
                    key: vec![("id".to_string(), Value::Int(1))],
                }],
                ..Default::default()
            },
            CancellationToken::new(),
        )
        .await
        .unwrap_or_else(|e| panic!("{}: the write failed: {e}", target.name));

    assert_eq!(
        cell(
            &scratch,
            &alt,
            &format!(
                "SELECT label FROM {} WHERE id = 1",
                scratch.qualified_in(&alt, "orders")
            )
        )
        .await,
        "moved",
        "{}: the row in the namespace that was read did not change",
        target.name
    );
    assert_eq!(
        cell(
            &scratch,
            &scratch.namespace_ref(),
            &format!(
                "SELECT label FROM {} WHERE id = 1",
                scratch.qualified("orders")
            )
        )
        .await,
        "here",
        "{}: the other namespace's table was written to",
        target.name
    );

    scratch.teardown().await;
}

/// `orders` in both namespaces, with one row each: one column each namespace has
/// **and one they share**. Returns the second namespace.
///
/// The differing columns are what let the read tests prove the model resolved
/// the right table — a `SELECT customer` against the wrong `orders` is an error,
/// not a wrong answer. But that also made the *write* test's second assertion
/// impossible to fail: a `SET customer = …` aimed at the wrong namespace would
/// be refused by the server long before it could silently change the row this
/// test then checks. `label` exists in both, so a write to the wrong namespace
/// **succeeds** — which is the silent failure the module doc names, and the only
/// state in which the untouched-table assertion can catch it.
async fn seed_both(scratch: &mut Scratch) -> Namespace {
    scratch
        .exec(&format!(
            "CREATE TABLE {} (id INTEGER NOT NULL PRIMARY KEY, amount INTEGER, \
             label VARCHAR(16))",
            scratch.qualified("orders")
        ))
        .await;
    scratch
        .exec(&format!(
            "INSERT INTO {} (id, amount, label) VALUES (1, 10, 'here')",
            scratch.qualified("orders")
        ))
        .await;

    let alt = scratch.alt_namespace().await;
    let there = scratch.qualified_in(&alt, "orders");
    scratch
        .exec_in(
            &alt,
            &format!(
                "CREATE TABLE {there} (id INTEGER NOT NULL PRIMARY KEY, customer VARCHAR(16), \
                 label VARCHAR(16))"
            ),
        )
        .await;
    scratch
        .exec_in(
            &alt,
            &format!("INSERT INTO {there} (id, customer, label) VALUES (1, 'original', 'there')"),
        )
        .await;
    alt
}

/// The column names of `table` as `ns` reports them.
async fn table_in(scratch: &Scratch, ns: &Namespace, table: &str) -> Vec<String> {
    let schema = scratch
        .db
        .clone()
        .with_database(Some(&ns.database))
        .fetch_schema(&ns.database, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("introspecting {}: {e}", ns.database));
    schema
        .tables
        .iter()
        .find(|t| t.name == table && t.schema.as_deref() == ns.schema.as_deref())
        .unwrap_or_else(|| panic!("no {table:?} in {}", ns.database))
        .columns
        .iter()
        .map(|c| c.name.clone())
        .collect()
}

async fn cell(scratch: &Scratch, ns: &Namespace, sql: &str) -> String {
    let rs = scratch.exec_in(ns, sql).await;
    rs.cell(0, 0)
        .expect("a one-row result")
        .display()
        .to_string()
}
