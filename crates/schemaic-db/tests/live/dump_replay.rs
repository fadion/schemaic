//! **A dump replayed onto the database it came from** — the primary way
//! anyone tests a dump — leaves that database as it was. Each test here is a
//! thing a replay lost or refused on a real server while the plan's pure
//! tests all passed: the statements were each right, and what went wrong was
//! what the server did with them in that order.

use schemaic_core::schema::partitions_are_tables;
use schemaic_core::script::ExecEnd;

use crate::endpoint::Target;
use crate::partitions::{count, dump_of};
use crate::runtime::run_file;
use crate::scratch::Scratch;

/// The foreign keys `table` holds, by name.
async fn foreign_keys(s: &Scratch, table: &str) -> Vec<String> {
    let outer = s.namespace_ref();
    let ns = outer.schema.as_deref().unwrap_or(&s.database);
    let rs = s
        .exec(&format!(
            "SELECT constraint_name FROM information_schema.table_constraints \
             WHERE table_schema = '{ns}' AND table_name = '{table}' \
             AND constraint_type = 'FOREIGN KEY' ORDER BY constraint_name"
        ))
        .await;
    (0..rs.row_count())
        .map(|r| rs.cell(r, 0).expect("a name").display().to_string())
        .collect()
}

/// **A key from a table outside the file onto one in it survives the
/// replay.** On PostgreSQL the file's `DROP TABLE … CASCADE` takes such a key
/// with the table, and the file put back only the keys between its own
/// tables: `orders` was left keyed onto nothing, and the replay said nothing.
pub async fn a_replay_keeps_the_key_a_table_outside_the_file_holds(target: &'static Target) {
    let s = Scratch::create(target, "replay_outside_key").await;
    let (customers, orders) = (s.qualified("customers"), s.qualified("orders"));
    s.exec(&format!(
        "CREATE TABLE {customers} (id INTEGER PRIMARY KEY)"
    ))
    .await;
    s.exec(&format!(
        "CREATE TABLE {orders} (id INTEGER PRIMARY KEY, cust_id INTEGER, \
         CONSTRAINT orders_cust_fk FOREIGN KEY (cust_id) REFERENCES {customers} (id))"
    ))
    .await;
    s.exec(&format!("INSERT INTO {customers} VALUES (1), (2)"))
        .await;
    s.exec(&format!("INSERT INTO {orders} VALUES (1, 1), (2, 2)"))
        .await;
    let file = dump_of(&s, &["customers"]).await;
    assert_eq!(
        run_file(&s, &file).await,
        ExecEnd::Done,
        "{}: the dump did not replay onto its source:\n{file}",
        target.name
    );
    assert_eq!(count(&s, "customers").await, "2", "{}", target.name);
    assert_eq!(
        foreign_keys(&s, "orders").await,
        ["orders_cust_fk"],
        "{}: the key from outside the file is gone:\n{file}",
        target.name
    );
    // Enforced, not merely listed.
    assert!(
        s.try_exec(&format!("INSERT INTO {orders} VALUES (3, 99)"))
            .await
            .is_err(),
        "{}: the restored key does not hold",
        target.name
    );
    s.teardown().await;
}

/// **A dump carrying a routine replays onto the database it came from.** The
/// routine was written as a bare `CREATE FUNCTION` and dropped first only where
/// the file drops up front, so on every other engine the replay stopped at a
/// name already taken, after the tables had been replaced.
pub async fn a_replay_replaces_the_routines_it_carries(target: &'static Target) {
    let s = Scratch::create(target, "replay_routine").await;
    let (t, f) = (s.qualified("t"), s.qualified("f_one"));
    s.exec(&format!("CREATE TABLE {t} (id INTEGER PRIMARY KEY)"))
        .await;
    s.exec(&format!("INSERT INTO {t} VALUES (1), (2)")).await;
    if schemaic_core::ddl::routine_takes_language(s.dialect()) {
        s.exec(&format!(
            "CREATE FUNCTION {f}() RETURNS integer LANGUAGE sql AS $$ SELECT 1 $$"
        ))
        .await;
    } else {
        s.exec(&format!(
            "CREATE FUNCTION {f}() RETURNS INTEGER DETERMINISTIC NO SQL RETURN 1"
        ))
        .await;
    }
    let file = dump_of(&s, &["t"]).await;
    assert!(
        file.contains("f_one"),
        "{}: the fixture no longer carries the routine:\n{file}",
        target.name
    );
    assert_eq!(
        run_file(&s, &file).await,
        ExecEnd::Done,
        "{}: the dump did not replay onto its source:\n{file}",
        target.name
    );
    assert_eq!(count(&s, "t").await, "2", "{}", target.name);
    let one = s.exec(&format!("SELECT {f}()")).await;
    assert_eq!(
        one.cell(0, 0).expect("a value").display().to_string(),
        "1",
        "{}",
        target.name
    );
    s.teardown().await;
}

/// **A dump carrying a type, a domain and a sequence replays onto the
/// database it came from, and leaves each as it found it.** They were written
/// as bare `CREATE`s, so the replay stopped at the first name already taken.
/// None can be dropped and recreated — a column outside the file may be typed
/// with one, and a sequence's position is state no script carries — so each is
/// created only where it is missing.
pub async fn a_replay_keeps_the_types_and_sequences_it_carries(target: &'static Target) {
    use schemaic_core::ddl::{ObjectKind, supports_object_creation};
    let s = Scratch::create(target, "replay_types").await;
    let d = s.dialect();
    if !supports_object_creation(d, ObjectKind::Enum)
        || !supports_object_creation(d, ObjectKind::Domain)
    {
        crate::endpoint::note_no_op(target, "no standalone type to carry here");
        s.teardown().await;
        return;
    }
    let (t, mood, pos, seq) = (
        s.qualified("t"),
        s.qualified("mood"),
        s.qualified("pos"),
        s.qualified("counter"),
    );
    s.exec(&format!("CREATE TYPE {mood} AS ENUM ('sad', 'happy')"))
        .await;
    s.exec(&format!("CREATE DOMAIN {pos} AS integer CHECK (VALUE > 0)"))
        .await;
    s.exec(&format!("CREATE SEQUENCE {seq}")).await;
    for _ in 0..3 {
        s.exec(&format!("SELECT nextval('{seq}')")).await;
    }
    s.exec(&format!(
        "CREATE TABLE {t} (id INTEGER PRIMARY KEY, m {mood}, p {pos})"
    ))
    .await;
    s.exec(&format!("INSERT INTO {t} VALUES (1, 'happy', 7)"))
        .await;
    let file = dump_of(&s, &["t"]).await;
    assert_eq!(
        run_file(&s, &file).await,
        ExecEnd::Done,
        "{}: the dump did not replay onto its source:\n{file}",
        target.name
    );
    assert_eq!(count(&s, "t").await, "1", "{}", target.name);
    let next = s.exec(&format!("SELECT nextval('{seq}')")).await;
    assert_eq!(
        next.cell(0, 0).expect("a value").display().to_string(),
        "4",
        "{}: the replay moved the sequence:\n{file}",
        target.name
    );
    s.teardown().await;
}

/// **A one-partition replay fires none of its parent's row triggers on the
/// rows it restores.** The parent is left standing, and the partition the file
/// creates `OF` it takes a clone of each of the parent's row triggers — so an
/// audit trigger wrote a second audit row for every restored row. A chosen
/// table's own triggers are created after its rows for exactly this reason.
/// Each clone is left as it is on the parent: one on, one off.
pub async fn a_partition_replay_fires_no_parent_trigger_on_its_rows(target: &'static Target) {
    let s = Scratch::create(target, "replay_part_trigger").await;
    if !partitions_are_tables(s.dialect()) {
        crate::endpoint::note_no_op(target, "a partition is not a table to dump here");
        s.teardown().await;
        return;
    }
    let (events, audit, f) = (
        s.qualified("events"),
        s.qualified("audit"),
        s.qualified("audit_fn"),
    );
    s.exec(&format!(
        "CREATE TABLE {events} (id INTEGER NOT NULL, at DATE NOT NULL, PRIMARY KEY (id, at)) \
         PARTITION BY RANGE (at)"
    ))
    .await;
    for year in [2023, 2024] {
        s.exec(&format!(
            "CREATE TABLE {} PARTITION OF {events} \
             FOR VALUES FROM ('{year}-01-01') TO ('{}-01-01')",
            s.qualified(&format!("events_{year}")),
            year + 1
        ))
        .await;
    }
    s.exec(&format!("CREATE TABLE {audit} (id INTEGER)")).await;
    s.exec(&format!(
        "CREATE FUNCTION {f}() RETURNS trigger LANGUAGE plpgsql AS $body$ BEGIN \
           INSERT INTO {audit} VALUES (NEW.id); RETURN NEW; END $body$"
    ))
    .await;
    s.exec(&format!(
        "CREATE TRIGGER events_audit AFTER INSERT ON {events} \
         FOR EACH ROW EXECUTE FUNCTION {f}()"
    ))
    .await;
    // A second, switched off on the parent, which the replay must leave off.
    s.exec(&format!(
        "CREATE TRIGGER events_audit_off AFTER INSERT ON {events} \
         FOR EACH ROW EXECUTE FUNCTION {f}()"
    ))
    .await;
    s.exec(&format!(
        "ALTER TABLE {events} DISABLE TRIGGER events_audit_off"
    ))
    .await;
    s.exec(&format!(
        "INSERT INTO {events} VALUES (1, '2023-03-01'), (2, '2024-02-01'), (3, '2024-05-01')"
    ))
    .await;
    assert_eq!(count(&s, "audit").await, "3", "{}", target.name);

    let file = dump_of(&s, &["events_2024"]).await;
    assert_eq!(
        run_file(&s, &file).await,
        ExecEnd::Done,
        "{}: the partition's dump did not replay:\n{file}",
        target.name
    );
    assert_eq!(count(&s, "events_2024").await, "2", "{}", target.name);
    assert_eq!(
        count(&s, "audit").await,
        "3",
        "{}: the parent's trigger fired on the restored rows:\n{file}",
        target.name
    );
    // Its clones are as the parent's are: one on, one off.
    let states = s
        .exec(&format!(
            "SELECT tgname, tgenabled::text FROM pg_trigger \
             WHERE tgrelid = '{}'::regclass AND NOT tgisinternal ORDER BY tgname",
            s.qualified("events_2024")
        ))
        .await;
    let states: Vec<(String, String)> = (0..states.row_count())
        .map(|r| {
            (
                states.cell(r, 0).expect("a name").display().to_string(),
                states.cell(r, 1).expect("a state").display().to_string(),
            )
        })
        .collect();
    assert_eq!(
        states,
        [
            ("events_audit".to_string(), "O".to_string()),
            ("events_audit_off".to_string(), "D".to_string())
        ],
        "{}:\n{file}",
        target.name
    );
    // And it fires on the next row, as it did before.
    s.exec(&format!("INSERT INTO {events} VALUES (4, '2024-06-01')"))
        .await;
    assert_eq!(count(&s, "audit").await, "4", "{}", target.name);
    s.teardown().await;
}
