//! The suite, written once and run against every configured server.
//!
//! Each function here takes the [`Target`] it is running against and asserts a
//! claim that is true of **all** of them. What genuinely differs between servers
//! is read off the target ([`Target::namespace`]) rather than branched on the
//! engine, for the reason the production code asks a capability instead of
//! comparing a dialect: an `if engine == Postgres` compiles cleanly while
//! silently sorting a third server onto whichever side it happens to fall.
//!
//! Two shapes live here. The four **path** tests are one claim each — connect,
//! execute and decode, introspect, and the fixture's own create-and-drop — and
//! fail at the first thing wrong, because there is one thing to be wrong. The
//! two **matrix** tests walk every type in [`crate::cases`], collect every
//! failure and fail once with the list: a decoding change moves a whole group of
//! types at once, and twenty runs to see the group is twenty runs wasted.
//!
//! Still to come: per-column provenance and the editability it drives,
//! write-back through the edit model, the DDL round trip, scripts and manual
//! transactions.

use std::time::Duration;

use schemaic_core::export::sql_literal;
use schemaic_core::model::ResultSet;
use tokio_util::sync::CancellationToken;

use crate::cases::TypeCase;
use crate::endpoint::Target;
use crate::scratch::Scratch;

/// The seed table, in the one spelling all three servers accept — no
/// `AUTO_INCREMENT`, no `SERIAL`, nothing a dialect has an opinion about. Slice 0
/// is about the wire, not the type system.
const SEED_DDL: &str = "(id INTEGER NOT NULL PRIMARY KEY, name VARCHAR(32))";

/// The endpoint answers at all.
///
/// First because every other failure in the tier reads as this one until it is
/// ruled out: a suite whose server is not running fails in the fixture, several
/// frames from the sentence that says so.
pub async fn a_ping_reaches_the_server(target: &'static Target) {
    target
        .base_db()
        .ping(Duration::from_secs(10))
        .await
        .unwrap_or_else(|e| {
            panic!(
                "live tier could not reach {}: {e}\n\
                 Start the server, or name the legs you do have in SCHEMAIC_IT_ENGINES.",
                target.endpoint()
            )
        });
}

/// Rows written through one connection come back through another as the text the
/// grid renders — including the distinction between NULL and the empty string,
/// which every engine has its own way of losing.
pub async fn a_seeded_table_round_trips_through_a_query(target: &'static Target) {
    let scratch = Scratch::create(target, "roundtrip").await;
    let t = scratch.qualified("t");

    scratch.exec(&format!("CREATE TABLE {t} {SEED_DDL}")).await;
    let inserted = scratch
        .exec(&format!(
            "INSERT INTO {t} (id, name) VALUES (1, 'alpha'), (2, NULL)"
        ))
        .await;
    assert_eq!(
        inserted.affected,
        Some(2),
        "{}: an INSERT of two rows reported {:?} affected",
        target.name,
        inserted.affected
    );

    let rs = scratch
        .exec(&format!("SELECT id, name FROM {t} ORDER BY id"))
        .await;
    assert_eq!(rs.row_count(), 2, "{}: row count", target.name);
    assert_eq!(rs.col_count(), 2, "{}: column count", target.name);
    let names: Vec<&str> = rs.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "name"], "{}: column names", target.name);
    // A row-returning result reports no affected count, so the grid can tell the
    // two kinds of statement apart.
    assert_eq!(
        rs.affected, None,
        "{}: SELECT reported a row count as affected",
        target.name
    );

    let cell = |r: usize, c: usize| rs.cell(r, c).expect("a cell that was just selected");
    assert_eq!(cell(0, 0).display(), "1", "{}: integer text", target.name);
    assert_eq!(
        cell(0, 1).display(),
        "alpha",
        "{}: string text",
        target.name
    );
    assert!(cell(1, 1).is_null(), "{}: NULL was not tagged", target.name);
    assert_eq!(
        cell(1, 1).display(),
        "NULL",
        "{}: NULL renders",
        target.name
    );
    assert_eq!(
        cell(1, 1).text(),
        "",
        "{}: a NULL's stored text is empty",
        target.name
    );

    scratch.teardown().await;
}

/// Introspection sees the table that was just created, in the namespace this
/// server puts it in.
pub async fn introspection_finds_the_seeded_table(target: &'static Target) {
    let scratch = Scratch::create(target, "introspect").await;
    scratch
        .exec(&format!(
            "CREATE TABLE {} {SEED_DDL}",
            scratch.qualified("t")
        ))
        .await;

    let schema = scratch
        .db
        .fetch_schema(&scratch.database, CancellationToken::new())
        .await
        .unwrap_or_else(|e| panic!("{}: fetch_schema failed: {e}", target.name));

    let table = schema
        .tables
        .iter()
        .find(|t| t.name == "t")
        .unwrap_or_else(|| {
            panic!(
                "{}: introspection of {} listed {:?}, not the table just created",
                target.name,
                scratch.database,
                schema.tables.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        });

    assert!(
        !table.is_view,
        "{}: a base table read as a view",
        target.name
    );
    assert_eq!(
        table.schema.as_deref(),
        scratch.namespace,
        "{}: namespace",
        target.name
    );
    let cols: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        cols,
        ["id", "name"],
        "{}: introspected columns",
        target.name
    );

    scratch.teardown().await;
}

/// Every type this server can hand back renders as the text the grid shows —
/// and a NULL in that column is a NULL, not an empty string.
///
/// **The matrix reports every case, then fails once.** A loop that panicked at
/// the first mismatch would turn a decoding change into twenty runs, and the
/// interesting shape of a failure here is usually which *group* of types moved
/// together.
pub async fn every_type_renders_as_the_grid_shows_it(target: &'static Target) {
    let scratch = Scratch::create(target, "render").await;
    let mut failures = Vec::new();
    let mut ran = 0;

    for case in target.type_cases() {
        ran += 1;
        let rows = match seed_case(&scratch, case).await {
            Ok(rows) => rows,
            Err(e) => {
                failures.push(format!("{} ({}): {e}", case.name, case.sql_type));
                continue;
            }
        };
        if rows.row_count() != 2 {
            failures.push(format!(
                "{} ({}): selected {} rows, not the 2 inserted",
                case.name,
                case.sql_type,
                rows.row_count()
            ));
            continue;
        }
        let value = rows.cell(0, 0).expect("the seeded row");
        if let Some(want) = case.display
            && value.display() != want
        {
            failures.push(format!(
                "{} ({}): rendered {:?}, expected {:?}",
                case.name,
                case.sql_type,
                value.display(),
                want
            ));
        }
        // The NULL row, for every type: the two are one byte apart on the wire
        // and a column that loses the difference makes an empty cell
        // indistinguishable from a missing one.
        let null = rows.cell(1, 0).expect("the NULL row");
        if !null.is_null() {
            failures.push(format!(
                "{} ({}): a NULL came back as {:?}",
                case.name,
                case.sql_type,
                null.display()
            ));
        }
    }

    scratch.teardown().await;
    report(target, "rendered wrongly", failures, ran);
}

/// The text the grid shows is the value: written back as a literal, it produces
/// the same cell again.
///
/// This is where a rendering that *looks* right and is lossy is caught — a
/// `DECIMAL` that dropped its scale, a timestamp that lost its fraction. Such a
/// value passes the rendering matrix above (nothing says it must not) and cannot
/// pass this.
///
/// **It goes back through `export::sql_literal`**, the app's own quoter, rather
/// than a `format!` written here. The composition is the thing under test: the
/// pure function has its own unit tests and they cannot see what a real cell
/// from a real driver hands it.
pub async fn the_text_the_grid_shows_writes_back_unchanged(target: &'static Target) {
    let scratch = Scratch::create(target, "writeback").await;
    let mut failures = Vec::new();
    let mut ran = 0;

    for case in target.type_cases().filter(|c| c.writable) {
        ran += 1;
        let rows = match seed_case(&scratch, case).await {
            Ok(rows) => rows,
            Err(e) => {
                failures.push(format!("{} ({}): {e}", case.name, case.sql_type));
                continue;
            }
        };
        let Some(shown) = rows.cell(0, 0) else {
            failures.push(format!("{} ({}): no seeded row", case.name, case.sql_type));
            continue;
        };
        let shown_text = shown.display().to_string();
        let literal = sql_literal(&shown.to_value(), scratch.dialect());

        let table = scratch.qualified(&format!("tc_{}", case.name));
        if let Err(e) = scratch
            .try_exec(&format!(
                "INSERT INTO {table} (id, v) VALUES (3, {literal})"
            ))
            .await
        {
            failures.push(format!(
                "{} ({}): writing {literal} back failed: {e}",
                case.name, case.sql_type
            ));
            continue;
        }
        let back = match scratch
            .try_exec(&format!("SELECT v FROM {table} WHERE id = 3"))
            .await
        {
            Ok(rs) => rs,
            Err(e) => {
                failures.push(format!(
                    "{} ({}): re-reading failed: {e}",
                    case.name, case.sql_type
                ));
                continue;
            }
        };
        let again = back.cell(0, 0).map(|c| c.display().to_string());
        if again.as_deref() != Some(shown_text.as_str()) {
            failures.push(format!(
                "{} ({}): showed {shown_text:?}, wrote {literal}, read back {:?}",
                case.name, case.sql_type, again
            ));
        }
    }

    scratch.teardown().await;
    report(target, "did not survive a write back", failures, ran);
}

/// Give `case` a table of its own holding the value and a NULL, and return both
/// rows in insertion order.
///
/// A table per case rather than one wide table: a type the server rejects
/// outright then costs its own case rather than every case after it.
async fn seed_case(scratch: &Scratch, case: &TypeCase) -> Result<ResultSet, String> {
    let table = scratch.qualified(&format!("tc_{}", case.name));
    scratch
        .try_exec(&format!(
            "CREATE TABLE {table} (id INTEGER NOT NULL PRIMARY KEY, v {})",
            case.sql_type
        ))
        .await
        .map_err(|e| format!("the server refused the column type: {e}"))?;
    scratch
        .try_exec(&format!(
            "INSERT INTO {table} (id, v) VALUES (1, {}), (2, NULL)",
            case.literal
        ))
        .await
        .map_err(|e| format!("the server refused the literal {}: {e}", case.literal))?;
    scratch
        .try_exec(&format!("SELECT v FROM {table} ORDER BY id"))
        .await
        .map_err(|e| format!("selecting it back failed: {e}"))
}

/// The fewest cases a leg may run before the matrix is assumed to have lost
/// them rather than passed them.
///
/// A floor, not a count, so adding a type does not edit this — and it is here
/// because both matrix tests are green when they run **nothing at all**: an
/// empty `type_cases()`, a filter that matched none, and the suite reports two
/// passes having asserted less than any single test would. The smallest run is
/// MySQL's write back, at twenty-one of its twenty-two cases.
const TYPE_CASE_FLOOR: usize = 20;

/// Fail once, listing every case that did not hold.
fn report(target: &Target, what: &str, failures: Vec<String>, ran: usize) {
    assert!(
        ran >= TYPE_CASE_FLOOR,
        "{}: the type matrix ran {ran} cases, fewer than the {TYPE_CASE_FLOOR} \
         every leg carries — it lost them rather than passing them",
        target.name
    );
    assert!(
        failures.is_empty(),
        "{} — {} of {ran} of {}'s types {what}:\n  {}",
        target.endpoint(),
        failures.len(),
        target.name,
        failures.join("\n  ")
    );
}

/// The fixture's own claim: the scratch database exists while the test runs and
/// is gone afterwards.
///
/// It guards the tier against the failure that would make every other test here
/// untrustworthy — a namespace that outlives its test, so the next run inherits
/// state it did not create.
pub async fn a_scratch_database_is_gone_once_torn_down(target: &'static Target) {
    let scratch = Scratch::create(target, "teardown").await;
    let name = scratch.database.clone();
    let base = target.base_db();

    let listed = |dbs: &[String]| dbs.contains(&name);
    let before = base
        .fetch_databases()
        .await
        .unwrap_or_else(|e| panic!("{}: fetch_databases failed: {e}", target.name));
    assert!(
        listed(&before),
        "{}: the scratch database {name} was created but not listed",
        target.name
    );

    scratch.teardown().await;

    let after = base
        .fetch_databases()
        .await
        .unwrap_or_else(|e| panic!("{}: fetch_databases failed: {e}", target.name));
    assert!(!listed(&after), "{}: {name} survived teardown", target.name);
}
