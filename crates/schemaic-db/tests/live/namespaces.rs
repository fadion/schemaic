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

use schemaic_core::model::{CellEdit, GridWrite, RowEdit, Value};
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

    // **The two assertions above are nearly free on a leg whose namespaces are
    // databases.** `table_in` issues a separate `fetch_schema` per namespace,
    // and `collect_schema`'s table query is bound to that same argument
    // (`TABLE_SCHEMA = ?`), so on MySQL and MariaDB the two column lists differ
    // by construction: the only regression they can catch there is one that
    // ignores the `database` parameter outright, not the identity collapse this
    // test is named for. Only the PostgreSQL leg, where both tables come back
    // from **one** call and the discriminator is `t.schema`, tests the claim.
    //
    // So this asks a question about *one* call: within a single introspection,
    // the namespace has to be what picks between two same-named tables.
    //
    // **On PostgreSQL that is the real thing** — both `orders` tables come back
    // from this one `fetch_schema`, and `t.schema` is the only thing telling
    // them apart, so a collapse shows up here. On a leg where a namespace *is*
    // a database there is nothing stronger available, and saying so is better
    // than implying otherwise: one call covers one database by definition, so
    // the alt table is not in this answer at all and the assertion is close to
    // free there. The cross-namespace join
    // (`a_join_across_namespaces_stays_two_tables`) is where those legs are
    // actually held to the claim, because one result set carries both.
    let primary = scratch
        .db
        .clone()
        .with_database(Some(&scratch.database))
        .fetch_schema(&scratch.database, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("introspecting {}: {e}", scratch.database));
    let ns = scratch.namespace_ref();
    let mine: Vec<&schemaic_core::schema::TableInfo> = primary
        .tables
        .iter()
        .filter(|t| t.name == "orders" && t.schema.as_deref() == ns.schema.as_deref())
        .collect();
    assert_eq!(
        mine.len(),
        1,
        "{}: one introspection returned {} tables called orders in this \
         namespace",
        target.name,
        mine.len()
    );
    assert!(
        mine[0].columns.iter().all(|c| c.name != "customer"),
        "{}: the primary namespace's orders came back with the *alt* table's \
         shape — the two collapsed inside one introspection",
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
                    set: vec![("label".to_string(), CellEdit::Text("moved".to_string()))],
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

/// **A sequence cannot be owned by a table in another namespace**, and this is
/// the test that says so — because the emitter depends on it.
///
/// `SequenceInfo::create_sql` qualifies the `OWNED BY` table with the
/// *sequence's* schema, which is the one place in `schema.rs` where a namespace
/// is borrowed from a different object. That reads as a bug — `CREATE SEQUENCE
/// sales.s; ALTER SEQUENCE sales.s OWNED BY public.orders.id;` would then copy
/// out as `OWNED BY "sales"."orders"."id"`, either failing or binding the
/// sequence to the wrong table — and it was filed as one (B10.1-L1-05). It is
/// not: PostgreSQL 16 refuses the `ALTER` outright with *"sequence must be in
/// same schema as table it is linked to"*, so the state the emitter would get
/// wrong is one the server will not create.
///
/// Pinned here rather than argued in a comment, because the argument is a fact
/// about a server and nothing in this repository could otherwise check it. If a
/// future PostgreSQL relaxes the rule, this test goes red and the emitter needs
/// the namespace the model does not carry.
///
/// PostgreSQL only: MySQL has no sequences owned by a column, and MariaDB's
/// carry no owner.
pub async fn a_sequence_cannot_be_owned_across_namespaces(target: &'static Target) {
    if target.namespace.is_none() {
        return;
    }
    let mut scratch = Scratch::create(target, "ns_seqowner").await;
    let alt = scratch.alt_namespace().await;

    scratch
        .exec_in(
            &alt,
            &format!(
                "CREATE TABLE {} (id INTEGER)",
                scratch.qualified_in(&alt, "owner_t")
            ),
        )
        .await;
    scratch
        .exec(&format!(
            "CREATE TABLE {} (id INTEGER)",
            scratch.qualified("here_t")
        ))
        .await;
    scratch
        .exec(&format!("CREATE SEQUENCE {}", scratch.qualified("s")))
        .await;

    let refused = scratch
        .try_exec(&format!(
            "ALTER SEQUENCE {} OWNED BY {}.\"id\"",
            scratch.qualified("s"),
            scratch.qualified_in(&alt, "owner_t")
        ))
        .await;
    let err = match refused {
        Err(e) => e.to_string(),
        Ok(_) => panic!(
            "{}: the server accepted a cross-namespace OWNED BY — `SequenceInfo::create_sql` \
             qualifies the owner with the sequence's own schema and would now name the wrong \
             table",
            target.name
        ),
    };
    assert!(
        err.contains("same schema"),
        "{}: refused for some other reason: {err}",
        target.name
    );

    // And the ownership that *is* constructible — same namespace, which is the
    // only one there is — reads back and emits with that namespace on both
    // halves, so this test is not only about the refusal.
    scratch
        .exec(&format!(
            "ALTER SEQUENCE {} OWNED BY {}.\"id\"",
            scratch.qualified("s"),
            scratch.qualified("here_t")
        ))
        .await;
    let schema = scratch
        .db
        .fetch_schema(&scratch.database, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("introspecting {}: {e}", scratch.database));
    let seq = schema
        .sequences
        .iter()
        .find(|s| s.name == "s")
        .unwrap_or_else(|| {
            panic!(
                "{}: no sequence s; have {:?}",
                target.name,
                schema.sequences.iter().map(|s| &s.name).collect::<Vec<_>>()
            )
        });
    assert_eq!(
        seq.owned_by.as_ref().map(|o| o.table.as_str()),
        Some("here_t"),
        "{}: the owning table",
        target.name
    );
    let sql = seq.create_sql(target.engine.dialect());
    let ns = scratch.namespace.expect("a PostgreSQL namespace");
    assert!(
        sql.contains(&format!("OWNED BY \"{ns}\".\"here_t\".\"id\""))
            || (ns == "public" && sql.contains("OWNED BY \"here_t\".\"id\"")),
        "{}: {sql}",
        target.name
    );

    scratch.teardown().await;
}

/// **Two namespaces in one result — the collapse this module's doc opens with,
/// which nothing in the tier could reproduce.**
///
/// Every other test here reads from one namespace at a time, so
/// `analyze_edit` builds exactly one group and the namespace terms in the key
/// are load-bearing for nothing. On both MySQL legs it is worse than that:
/// `Scratch::exec_in` scopes the connection to the alt *database*, so stamping
/// each column's origin with the connection's current database instead of the
/// wire packet's per-column value produces the identical answer and every
/// assertion still holds. One line of `db/lib.rs` could be regressed and only
/// the PostgreSQL leg would notice.
///
/// `core::edit`'s `same_table_name_in_two_schemas_stays_two_edit_tables` does
/// pin the grouping — over `ColumnOrigin`s written out by hand, which is
/// exactly the composition `Scratch::edit_model`'s own doc says a live test
/// exists to go beyond. This is where the two halves meet.
///
/// The untouched row is the assertion that matters. `label` is in both tables
/// (see [`seed_both`]) precisely so a write aimed at the wrong namespace
/// **succeeds**: the statement runs, one row is affected, the 1-row net is
/// satisfied, and the wrong table changed.
pub async fn a_join_across_namespaces_stays_two_tables(target: &'static Target) {
    let mut scratch = Scratch::create(target, "ns_join").await;
    let alt = seed_both(&mut scratch).await;

    let here = scratch.qualified("orders");
    let there = scratch.qualified_in(&alt, "orders");
    let (_, model) = scratch
        .edit_model(&format!(
            "SELECT a.id AS a_id, a.label AS a_label, b.id AS b_id, b.label AS b_label \
             FROM {here} a JOIN {there} b ON a.id = b.id"
        ))
        .await;

    let mut seen: Vec<(String, Option<String>)> = (0..2)
        .map(|i| {
            let t = model.table(i).unwrap_or_else(|| {
                panic!(
                    "{}: the join resolved to fewer than two writable tables — \
                     the two namespaces collapsed into one",
                    target.name
                )
            });
            assert_eq!(t.table, "orders", "{}: unexpected table", target.name);
            (t.database.clone(), t.schema.clone())
        })
        .collect();
    seen.sort();
    let mut want = vec![
        (
            scratch.namespace_ref().database.clone(),
            scratch.namespace_ref().schema.clone(),
        ),
        (alt.database.clone(), alt.schema.clone()),
    ];
    want.sort();
    assert_eq!(
        seen, want,
        "{}: the two sides of the join do not carry their own namespaces",
        target.name
    );
    assert!(
        model.table(2).is_none(),
        "{}: a two-table join produced a third group",
        target.name
    );

    // Write to the alt side through the identity the model resolved, and check
    // the *other* namespace's row is still what it was.
    let alt_table = (0..2)
        .filter_map(|i| model.table(i))
        .find(|t| t.database == alt.database && t.schema == alt.schema)
        .unwrap_or_else(|| panic!("{}: the alt side is not writable", target.name));
    scratch
        .db
        .commit_writes(
            &GridWrite {
                updates: vec![RowEdit {
                    database: alt_table.database.clone(),
                    schema: alt_table.schema.clone(),
                    table: alt_table.table.clone(),
                    set: vec![("label".to_string(), CellEdit::Text("moved".to_string()))],
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
            &format!("SELECT label FROM {there} WHERE id = 1")
        )
        .await,
        "moved",
        "{}: the row the write named did not change",
        target.name
    );
    assert_eq!(
        cell(
            &scratch,
            &scratch.namespace_ref(),
            &format!("SELECT label FROM {here} WHERE id = 1")
        )
        .await,
        "here",
        "{}: the other namespace's table was written to",
        target.name
    );

    scratch.teardown().await;
}

/// **The branch that qualifies generated DDL has never reached a server.**
///
/// `schema::sql_qualifier` returns `None` for `public`, so the qualifying arm
/// is taken only for a non-default schema — and every table in the DDL, view
/// and trigger tiers is addressed through `Scratch::qualified`, which resolves
/// to `public` on the PostgreSQL leg and to the connection's own database on
/// both MySQL legs. `alt_namespace` had three callers, all in this file, and
/// all three were reads or a `commit_writes` UPDATE: no `run_ddl` had ever run
/// against a table outside the default namespace on any engine.
///
/// What that hides is not a loud failure. Drop the qualifier from the emitted
/// `ALTER TABLE` and PostgreSQL resolves the bare name through `search_path`
/// (`"$user", public`), so a designer edit to `alt.orders` alters
/// `public.orders` instead — same table name, a plausible shape, and the
/// statement succeeds.
///
/// **The untouched table is the assertion that matters**, which is why both
/// namespaces get a table of the same shape: an `ALTER` aimed at the wrong one
/// has to *succeed* for the failure to be silent.
pub async fn generated_ddl_lands_in_the_namespace_it_was_drafted_from(target: &'static Target) {
    use schemaic_core::ddl::{self, ColumnDraft, TableDraft};
    use schemaic_core::schema::ColumnInfo;

    let mut scratch = Scratch::create(target, "ns_ddl").await;
    let alt = scratch.alt_namespace().await;
    let shape = "(id INTEGER NOT NULL PRIMARY KEY, label VARCHAR(16))";
    scratch
        .exec(&format!(
            "CREATE TABLE {} {shape}",
            scratch.qualified("orders")
        ))
        .await;
    scratch
        .exec_in(
            &alt,
            &format!(
                "CREATE TABLE {} {shape}",
                scratch.qualified_in(&alt, "orders")
            ),
        )
        .await;

    let current = table_info_in(&scratch, &alt, "orders").await;
    assert_eq!(
        current.schema.as_deref(),
        alt.schema.as_deref(),
        "{}: the introspected table does not carry the alt namespace, so this \
         test could not tell the two apart",
        target.name
    );
    let mut draft = TableDraft::from_table(&current);
    draft.columns.push(ColumnDraft::new(ColumnInfo {
        name: "added".to_string(),
        type_name: "INTEGER".to_string(),
        nullable: true,
        ..Default::default()
    }));
    let set = ddl::diff(&current, &draft, target.engine.dialect());
    scratch
        .apply_plan_in(&alt, &set, "alt-namespace table")
        .await;

    let after = table_info_in(&scratch, &alt, "orders").await;
    assert!(
        after.columns.iter().any(|c| c.name == "added"),
        "{}: the column did not land in the namespace it was drafted from",
        target.name
    );
    let other = table_info_in(&scratch, &scratch.namespace_ref(), "orders").await;
    assert!(
        other.columns.iter().all(|c| c.name != "added"),
        "{}: the ALTER landed on the *other* namespace's same-named table",
        target.name
    );

    scratch.teardown().await;
}

/// The `TableInfo` for `table` as `ns` reports it — the whole row the designer
/// would draft from, not just its column names (cf. [`table_in`]).
async fn table_info_in(
    scratch: &Scratch,
    ns: &Namespace,
    table: &str,
) -> schemaic_core::schema::TableInfo {
    let schema = scratch
        .db
        .clone()
        .with_database(Some(&ns.database))
        .fetch_schema(&ns.database, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("introspecting {}: {e}", ns.database));
    schema
        .tables
        .into_iter()
        .find(|t| t.name == table && t.schema.as_deref() == ns.schema.as_deref())
        .unwrap_or_else(|| panic!("no {table:?} in {}", ns.database))
}
