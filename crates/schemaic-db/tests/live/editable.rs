//! Per-column provenance, and the row key the editing system builds from it.
//!
//! **This is the half of the editing system nothing else can reach.** Every
//! `edit::analyze_edit` unit test hands the ladder a `ColumnOrigin` written out
//! by hand, so what they prove is that it works on metadata a test *imagined*.
//! Whether a real driver reports `org_table` for an aliased column, a
//! `table_oid` for each side of a join, a primary-key flag at all, or anything
//! whatsoever for an expression is decided on the wire — and until this file,
//! the two halves met only inside the running app.
//!
//! The ladder itself (`edit.rs`) goes: primary key, else a unique non-foreign
//! index whose columns are all present and all `NOT NULL`, else the backend's
//! implicit key, else the table is **read-only**. The refusals matter more than
//! the rungs, because a wrong key does not fail loudly: it writes to a row
//! nobody asked for, and only the 1-row safety net stands behind it — so each
//! refusal has a test here, and so do the first two rungs.
//!
//! **The third rung is not tested here and cannot be.** The implicit key is
//! SQLite's `rowid`, and SQLite is not a leg of this tier — it is the one
//! backend the pure suite reaches directly, and `core::edit`'s own tests are
//! where that rung is pinned. Saying "each rung has a test here" read as a
//! coverage claim this file does not make.

use schemaic_core::edit::{EditModel, EditTable};
use schemaic_core::model::{ColumnOrigin, ResultSet};

use crate::endpoint::Target;
use crate::scratch::Scratch;

/// A table with a primary key and a plain column, the shape most of these start
/// from.
const KEYED: &str = "(id INTEGER NOT NULL PRIMARY KEY, name VARCHAR(32))";

/// `SELECT *` carries each column's real database, namespace, table and column
/// name, and flags the primary key.
///
/// The first rung of everything else: a key cannot be resolved for a table the
/// result cannot name.
pub async fn a_select_star_carries_each_columns_provenance(target: &'static Target) {
    let scratch = Scratch::create(target, "provenance").await;
    seed(&scratch, "p", KEYED).await;
    let rs = scratch
        .exec(&format!("SELECT * FROM {}", scratch.qualified("p")))
        .await;

    let id = origin_of(&rs, "id", target);
    assert_eq!(
        id.database, scratch.database,
        "{}: id's database",
        target.name
    );
    assert_eq!(
        id.schema.as_deref(),
        scratch.namespace,
        "{}: id's namespace",
        target.name
    );
    assert_eq!(id.table, "p", "{}: id's table", target.name);
    assert_eq!(id.column, "id", "{}: id's column", target.name);
    assert!(
        id.flags.primary_key,
        "{}: the primary key was not flagged on the wire",
        target.name
    );

    let name = origin_of(&rs, "name", target);
    assert_eq!(name.table, "p", "{}: name's table", target.name);
    assert_eq!(name.column, "name", "{}: name's column", target.name);
    assert!(
        !name.flags.primary_key,
        "{}: an ordinary column was flagged as the primary key",
        target.name
    );

    scratch.teardown().await;
}

/// An alias renames the column in the result and **not** in the provenance.
///
/// The write is built from the real name, so a driver that reported the alias
/// would produce `UPDATE p SET ident = …` against a table that has no such
/// column — or worse, against one that does.
pub async fn an_alias_does_not_hide_the_real_column(target: &'static Target) {
    let scratch = Scratch::create(target, "alias").await;
    seed(&scratch, "p", KEYED).await;
    let rs = scratch
        .exec(&format!(
            "SELECT id AS ident, name AS label FROM {}",
            scratch.qualified("p")
        ))
        .await;

    let names: Vec<&str> = rs.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        ["ident", "label"],
        "{}: the result shows the aliases",
        target.name
    );
    let ident = origin_of(&rs, "ident", target);
    assert_eq!(
        ident.column, "id",
        "{}: the alias reached the provenance",
        target.name
    );
    let label = origin_of(&rs, "label", target);
    assert_eq!(
        label.column, "name",
        "{}: the alias reached the provenance",
        target.name
    );

    // And the key still resolves, because it is looked up by the real name.
    let (rs, model) = scratch
        .edit_model(&format!(
            "SELECT id AS ident, name AS label FROM {}",
            scratch.qualified("p")
        ))
        .await;
    let table = sole_table(&model, target);
    assert_eq!(
        key_names(&rs, table),
        ["ident"],
        "{}: an aliased primary key is still the key",
        target.name
    );

    scratch.teardown().await;
}

/// An expression has no provenance, and is therefore not editable.
///
/// There is no row behind `id + 1` to write to, and a column the grid let
/// someone type into would have nowhere to send it.
pub async fn an_expression_column_has_no_provenance(target: &'static Target) {
    let scratch = Scratch::create(target, "expression").await;
    seed(&scratch, "p", KEYED).await;
    let (rs, model) = scratch
        .edit_model(&format!(
            "SELECT id, id + 1 AS bumped FROM {}",
            scratch.qualified("p")
        ))
        .await;

    let bumped = index_of(&rs, "bumped", target);
    assert!(
        rs.columns[bumped].origin.is_none(),
        "{}: an expression reported provenance {:?}",
        target.name,
        rs.columns[bumped].origin
    );
    assert!(
        !model.editable(bumped),
        "{}: an expression column was offered as editable",
        target.name
    );
    // The real column beside it is unaffected — a read-only column must not make
    // its whole row read-only.
    let id = index_of(&rs, "id", target);
    assert!(
        model.editable(id),
        "{}: an expression made the key column read-only too",
        target.name
    );

    scratch.teardown().await;
}

/// In a join, each column is attributed to the table it actually came from —
/// including two columns that share a name.
///
/// `parent.id` and `child.id` are one word apart in the result and different
/// rows of different tables underneath; an edit attributed to the wrong one
/// writes to a table the user was not looking at.
pub async fn a_join_attributes_each_column_to_its_own_table(target: &'static Target) {
    let scratch = Scratch::create(target, "join").await;
    seed(&scratch, "parent", KEYED).await;
    seed(
        &scratch,
        "child",
        "(id INTEGER NOT NULL PRIMARY KEY, parent_id INTEGER)",
    )
    .await;
    let (rs, model) = scratch
        .edit_model(&format!(
            "SELECT p.id, c.id, c.parent_id FROM {} p JOIN {} c ON c.parent_id = p.id",
            scratch.qualified("parent"),
            scratch.qualified("child")
        ))
        .await;

    let tables: Vec<&str> = (0..rs.col_count())
        .map(|ci| {
            rs.columns[ci]
                .origin
                .as_ref()
                .map(|o| o.table.as_str())
                .unwrap_or("<none>")
        })
        .collect();
    assert_eq!(
        tables,
        ["parent", "child", "child"],
        "{}: each column's table",
        target.name
    );

    // Two writable tables in one result, so there is no single destination a new
    // row could go to.
    assert!(
        model.insert_target().is_none(),
        "{}: a two-table join offered an insert target",
        target.name
    );

    scratch.teardown().await;
}

/// A primary key present in the result becomes the write key.
pub async fn a_primary_key_becomes_the_write_key(target: &'static Target) {
    let scratch = Scratch::create(target, "pk").await;
    seed(&scratch, "p", KEYED).await;
    let (rs, model) = scratch
        .edit_model(&format!("SELECT * FROM {}", scratch.qualified("p")))
        .await;

    let table = sole_table(&model, target);
    assert_eq!(key_names(&rs, table), ["id"], "{}: the key", target.name);
    assert_eq!(table.table, "p", "{}: the table written to", target.name);
    assert!(
        table.confirm_cols.is_empty(),
        "{}: a real key needs no confirming columns",
        target.name
    );

    scratch.teardown().await;
}

/// **The key the model resolved, used to write** — through `edit::build_edits`
/// and `commit_writes`, over every type this leg calls writable.
///
/// `build_edits` and `row_key` were called by **nothing** in this tier: every
/// live write built its `RowEdit` by hand, so every key value one was given was
/// a value the test had written down. What that leaves untested is precisely the
/// composition the tier exists for — whether the text the grid shows for a key
/// column, handed back as a literal, re-selects the row it came from. A type
/// whose rendering does not round-trip through its own key is a write that lands
/// on the wrong row or on none, and the 1-row net is all that stands behind it.
pub async fn a_write_built_from_the_resolved_key_lands_on_that_row(target: &'static Target) {
    use schemaic_core::edit;
    use schemaic_core::model::GridWrite;
    use tokio_util::sync::CancellationToken;

    let scratch = Scratch::create(target, "keyed_write").await;
    let mut failures = Vec::new();
    let mut keyed = 0usize;
    let mut plain = 0usize;

    for case in target.type_cases().filter(|c| c.writable) {
        let table = format!("kw_{}", case.name);
        let qualified = scratch.qualified(&table);
        // **Whether this type can *be* a key is the engine's answer, not a
        // list here.** PostgreSQL has no btree operator class for `json`, and
        // MySQL refuses a `TEXT` key without a length — so the shape is tried
        // and the fallback used when it is refused, rather than a hand-kept
        // exception list that would rot as the case set grows.
        let as_key = scratch
            .db
            .run_ddl(
                &scratch.database,
                &[format!(
                    "CREATE TABLE {qualified} (v {} NOT NULL PRIMARY KEY, payload VARCHAR(32))",
                    case.sql_type
                )],
                CancellationToken::new(),
            )
            .await
            .is_ok();
        if as_key {
            keyed += 1;
            scratch
                .exec(&format!(
                    "INSERT INTO {qualified} (v, payload) VALUES ({}, 'before')",
                    case.literal
                ))
                .await;
        } else {
            plain += 1;
            scratch
                .exec(&format!(
                    "CREATE TABLE {qualified} (id INTEGER NOT NULL PRIMARY KEY, v {},                      payload VARCHAR(32))",
                    case.sql_type
                ))
                .await;
            scratch
                .exec(&format!(
                    "INSERT INTO {qualified} (id, v, payload) VALUES (1, {}, 'before')",
                    case.literal
                ))
                .await;
        }

        let (mut rs, mut model) = scratch
            .edit_model(&format!("SELECT * FROM {qualified}"))
            .await;
        // **A type that cannot be a *reliable* key is refused by the model, and
        // that is the right answer** — `edit.rs`'s C2/C4 makes a floating-point
        // or binary key read-only, because equality on one does not identify a
        // row. So the table is rebuilt on an integer key rather than counted as
        // a failure: the write still has to go through `build_edits`, which is
        // what this test is about.
        let mut as_key = as_key;
        if as_key && model.table(0).is_none() {
            as_key = false;
            keyed -= 1;
            plain += 1;
            scratch.exec(&format!("DROP TABLE {qualified}")).await;
            scratch
                .exec(&format!(
                    "CREATE TABLE {qualified} (id INTEGER NOT NULL PRIMARY KEY, v {},                      payload VARCHAR(32))",
                    case.sql_type
                ))
                .await;
            scratch
                .exec(&format!(
                    "INSERT INTO {qualified} (id, v, payload) VALUES (1, {}, 'before')",
                    case.literal
                ))
                .await;
            let re = scratch
                .edit_model(&format!("SELECT * FROM {qualified}"))
                .await;
            rs = re.0;
            model = re.1;
        }
        let Some(t) = model.table(0) else {
            failures.push(format!(
                "{}: no writable table even on an integer key",
                case.name
            ));
            continue;
        };
        if t.key_cols.len() != 1 {
            failures.push(format!("{}: {} key columns", case.name, t.key_cols.len()));
            continue;
        }

        // Stage a dirty cell on `payload`, exactly as the grid does, and let
        // `build_edits` derive the key from the row it is on — which is the
        // composition nothing in this tier reached.
        let payload = rs
            .columns
            .iter()
            .position(|c| c.name == "payload")
            .expect("the payload column");
        let dirty: edit::DirtyCells = [(
            (0usize, payload),
            schemaic_core::model::CellEdit::Text("after".to_string()),
        )]
        .into_iter()
        .collect();
        let edits = edit::build_edits(&model, &rs, &dirty);
        if edits.len() != 1 {
            failures.push(format!("{}: built {} edits", case.name, edits.len()));
            continue;
        }
        // The key the model built is the cell the grid shows — the half
        // `row_key` decides, and the one a rendering that does not round-trip
        // gets wrong.
        let shown = rs
            .cell(0, t.key_cols[0])
            .expect("the key cell")
            .display()
            .to_string();
        let carried = edits[0].key.first().map(|(_, v)| v.display().to_string());
        if carried.as_deref() != Some(shown.as_str()) {
            failures.push(format!(
                "{}: the grid shows {shown:?} and the key carries {carried:?}",
                case.name
            ));
        }

        match scratch
            .db
            .commit_writes(
                &GridWrite {
                    updates: edits,
                    ..Default::default()
                },
                CancellationToken::new(),
            )
            .await
        {
            Ok(1) => {}
            Ok(n) => {
                failures.push(format!("{}: the write reported {n} rows", case.name));
                continue;
            }
            Err(e) => {
                failures.push(format!("{}: the write failed: {e}", case.name));
                continue;
            }
        }
        // …and it is *that* row that changed. Read back by the key the table
        // actually has: `v = <literal>` where the type is comparable, and by
        // `id` where it is not (PostgreSQL has no `json` equality either).
        let where_clause = if as_key {
            format!("v = {}", case.literal)
        } else {
            "id = 1".to_string()
        };
        let back = scratch
            .exec(&format!(
                "SELECT payload FROM {qualified} WHERE {where_clause}"
            ))
            .await;
        let got = back.cell(0, 0).map(|c| c.display().to_string());
        if got.as_deref() != Some("after") {
            failures.push(format!("{}: the row reads back {got:?}", case.name));
        }
    }

    scratch.teardown().await;
    assert_eq!(
        keyed + plain,
        target.type_cases().filter(|c| c.writable).count(),
        "{}: only {} writable types were written through a resolved key",
        target.name,
        keyed + plain
    );
    assert!(
        keyed > 0,
        "{}: no type could be a key at all, so nothing exercised `row_key` on a          value the server rendered",
        target.name
    );
    assert!(
        failures.is_empty(),
        "{} — {} of {}'s writable types could not be written through the key the          model resolved:
  {}",
        target.endpoint(),
        failures.len(),
        target.name,
        failures.join("
  ")
    );
}

/// With no primary key, a unique index over `NOT NULL` columns is the key.
pub async fn a_not_null_unique_index_is_the_fallback_key(target: &'static Target) {
    let scratch = Scratch::create(target, "unique").await;
    seed(
        &scratch,
        "u",
        "(code VARCHAR(16) NOT NULL UNIQUE, name VARCHAR(32))",
    )
    .await;
    let (rs, model) = scratch
        .edit_model(&format!("SELECT * FROM {}", scratch.qualified("u")))
        .await;

    let table = sole_table(&model, target);
    assert_eq!(key_names(&rs, table), ["code"], "{}: the key", target.name);

    scratch.teardown().await;
}

/// A unique index over a **nullable** column is not a key, and the table is
/// read-only.
///
/// SQL's uniqueness does not extend to NULL: two rows may both hold one, so the
/// index does not identify a row and a `WHERE code IS NULL` would match both. A
/// ladder that stopped at "unique" would write to whichever came first.
pub async fn a_nullable_unique_index_is_no_key_at_all(target: &'static Target) {
    let scratch = Scratch::create(target, "nullable_unique").await;
    seed(&scratch, "n", "(code VARCHAR(16) UNIQUE, name VARCHAR(32))").await;
    let (rs, model) = scratch
        .edit_model(&format!("SELECT * FROM {}", scratch.qualified("n")))
        .await;

    assert_read_only(&rs, &model, target, "a nullable unique index");

    scratch.teardown().await;
}

/// A table with no key of any kind is read-only on these two engines — every way
/// of naming a row is a column, and this one has none that identify it.
pub async fn a_table_with_no_key_is_read_only(target: &'static Target) {
    let scratch = Scratch::create(target, "keyless").await;
    seed(&scratch, "k", "(a INTEGER, b INTEGER)").await;
    let (rs, model) = scratch
        .edit_model(&format!("SELECT * FROM {}", scratch.qualified("k")))
        .await;

    assert_read_only(&rs, &model, target, "a table with no key");

    scratch.teardown().await;
}

/// A key the result does not expose cannot be used, so the result is read-only —
/// the write has nothing to put in its `WHERE`.
pub async fn a_key_left_out_of_the_select_makes_the_result_read_only(target: &'static Target) {
    let scratch = Scratch::create(target, "keyless_select").await;
    seed(&scratch, "p", KEYED).await;
    let (rs, model) = scratch
        .edit_model(&format!("SELECT name FROM {}", scratch.qualified("p")))
        .await;

    assert_read_only(&rs, &model, target, "a result without its table's key");

    scratch.teardown().await;
}

/// The same base column exposed twice refuses the whole table.
///
/// `id, id AS id2` gives two cells over one column: an edit to either is an edit
/// to the same row and the same field, and there is no answer to which value
/// wins. Refusing is the only safe reading, and it is the wire's `org_name` that
/// makes the duplicate visible at all — the result's own column names differ.
pub async fn the_same_column_twice_refuses_the_whole_table(target: &'static Target) {
    let scratch = Scratch::create(target, "dup_column").await;
    seed(&scratch, "p", KEYED).await;
    let (rs, model) = scratch
        .edit_model(&format!(
            "SELECT id, id AS id2, name FROM {}",
            scratch.qualified("p")
        ))
        .await;

    assert_read_only(&rs, &model, target, "a column exposed twice");

    scratch.teardown().await;
}

/// A binary column is read-only, and the rest of its row is not.
///
/// Its cell is the `<n bytes>` placeholder rather than the value, so writing it
/// back would store the placeholder; the row around it is still perfectly
/// editable, and a guard that refused the table would take a whole table's
/// editing away over one column.
pub async fn a_binary_column_is_read_only_inside_an_editable_row(target: &'static Target) {
    let scratch = Scratch::create(target, "binary_column").await;
    seed(
        &scratch,
        "b",
        &format!(
            "(id INTEGER NOT NULL PRIMARY KEY, name VARCHAR(32), payload {})",
            target.binary_type
        ),
    )
    .await;
    let (rs, model) = scratch
        .edit_model(&format!("SELECT * FROM {}", scratch.qualified("b")))
        .await;

    let payload = index_of(&rs, "payload", target);
    assert!(
        !model.text_editable(payload),
        "{}: a binary column was offered a text edit",
        target.name
    );
    assert!(
        model.editable(payload) && model.binary(payload),
        "{}: a binary column takes a bytes write",
        target.name
    );
    let name = index_of(&rs, "name", target);
    assert!(
        model.editable(name),
        "{}: a binary column made its row read-only",
        target.name
    );
    let table = sole_table(&model, target);
    assert_eq!(
        key_names(&rs, table),
        ["id"],
        "{}: the key beside a binary column",
        target.name
    );

    scratch.teardown().await;
}

/// A single writable table is the destination a new row would go to.
pub async fn one_table_offers_itself_as_the_insert_target(target: &'static Target) {
    let scratch = Scratch::create(target, "insert_target").await;
    seed(&scratch, "p", KEYED).await;
    let (_, model) = scratch
        .edit_model(&format!("SELECT * FROM {}", scratch.qualified("p")))
        .await;

    let target_table = model.insert_target().unwrap_or_else(|| {
        panic!(
            "{}: a keyed single table offered no insert target",
            target.name
        )
    });
    assert_eq!(target_table.table, "p", "{}: insert target", target.name);
    assert_eq!(
        target_table.schema.as_deref(),
        scratch.namespace,
        "{}: the insert target's namespace",
        target.name
    );

    scratch.teardown().await;
}

/// Create `name` with `ddl` and put one row in it, so a result over it has a row
/// to carry.
async fn seed(scratch: &Scratch, name: &str, ddl: &str) {
    scratch
        .exec(&format!("CREATE TABLE {} {ddl}", scratch.qualified(name)))
        .await;
}

/// The provenance of the result column called `name`, or a failure naming what
/// the result actually held.
fn origin_of<'a>(rs: &'a ResultSet, name: &str, target: &Target) -> &'a ColumnOrigin {
    let ci = index_of(rs, name, target);
    rs.columns[ci].origin.as_ref().unwrap_or_else(|| {
        panic!(
            "{}: column {name:?} came back with no provenance at all",
            target.name
        )
    })
}

fn index_of(rs: &ResultSet, name: &str, target: &Target) -> usize {
    rs.columns
        .iter()
        .position(|c| c.name == name)
        .unwrap_or_else(|| {
            panic!(
                "{}: no column {name:?} in {:?}",
                target.name,
                rs.columns.iter().map(|c| &c.name).collect::<Vec<_>>()
            )
        })
}

/// The one table the model writes to, or a failure saying how many it found.
fn sole_table<'a>(model: &'a EditModel, target: &Target) -> &'a EditTable {
    model.table(0).unwrap_or_else(|| {
        panic!(
            "{}: the result mapped to no writable table at all",
            target.name
        )
    })
}

/// The key columns, by the names the result shows them under.
fn key_names(rs: &ResultSet, table: &EditTable) -> Vec<String> {
    table
        .key_cols
        .iter()
        .map(|&ci| rs.columns[ci].name.clone())
        .collect()
}

/// No column of the result may be edited, and no table may be written to.
///
/// Both halves, because they are separately wrong: a model with no tables still
/// answers `editable` per column, and a column left pointing at a table that was
/// refused is exactly the state that writes somewhere unintended.
fn assert_read_only(rs: &ResultSet, model: &EditModel, target: &Target, why: &str) {
    let editable: Vec<&str> = (0..rs.col_count())
        .filter(|&ci| model.editable(ci))
        .map(|ci| rs.columns[ci].name.as_str())
        .collect();
    assert!(
        editable.is_empty(),
        "{}: {why} left {editable:?} editable",
        target.name
    );
    assert!(
        model.insert_target().is_none(),
        "{}: {why} still offered an insert target",
        target.name
    );
}
