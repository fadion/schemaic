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

/// The single value `sql` selects, as text.
async fn scalar(s: &Scratch, sql: &str) -> String {
    let rs = s.exec(sql).await;
    rs.cell(0, 0).expect("a value").display().to_string()
}

/// **A replay never drops a view outside the file.** On PostgreSQL the
/// file's `DROP TABLE … CASCADE` took every view and materialized view over a
/// dumped table — the materialized view's rows with it — and the replay
/// reported success with them gone. Where the drop would take one, the replay
/// now stops before dropping anything and names it; where the views are in
/// the file, it replays. On the engines whose drop takes no view, it replays
/// and they stand.
pub async fn a_replay_leaves_the_views_outside_the_file_standing(target: &'static Target) {
    let s = Scratch::create(target, "replay_outside_view").await;
    let (customers, v_out, v_chain, mv) = (
        s.qualified("customers"),
        s.qualified("v_out"),
        s.qualified("v_chain"),
        s.qualified("mv_out"),
    );
    s.exec(&format!(
        "CREATE TABLE {customers} (id INTEGER PRIMARY KEY)"
    ))
    .await;
    s.exec(&format!("INSERT INTO {customers} VALUES (1), (2)"))
        .await;
    s.exec(&format!(
        "CREATE VIEW {v_out} AS SELECT id FROM {customers}"
    ))
    .await;
    s.exec(&format!("CREATE VIEW {v_chain} AS SELECT id FROM {v_out}"))
        .await;
    // Where the engine has them: its rows are a snapshot, so a third
    // customer added after it tells a survivor from a recomputed copy.
    let matview = s
        .try_exec(&format!(
            "CREATE MATERIALIZED VIEW {mv} AS SELECT COUNT(*) AS n FROM {customers}"
        ))
        .await
        .is_ok();
    s.exec(&format!("INSERT INTO {customers} VALUES (3)")).await;

    let file = dump_of(&s, &["customers"]).await;
    let end = run_file(&s, &file).await;
    if let ExecEnd::Failed { message, .. } = &end {
        assert!(
            message.contains("v_out"),
            "{}: the refusal does not name the view:\n{message}",
            target.name
        );
    } else {
        assert_eq!(end, ExecEnd::Done, "{}:\n{file}", target.name);
    }
    assert_eq!(count(&s, "customers").await, "3", "{}", target.name);
    assert_eq!(
        scalar(&s, &format!("SELECT COUNT(*) FROM {v_chain}")).await,
        "3",
        "{}: a view outside the file is gone:\n{file}",
        target.name
    );
    if matview {
        assert_eq!(
            scalar(&s, &format!("SELECT n FROM {mv}")).await,
            "2",
            "{}: the materialized view outside the file was dropped:\n{file}",
            target.name
        );
        s.exec(&format!("DROP MATERIALIZED VIEW {mv}")).await;
    }

    // With the views in the file, nothing outside it depends on the drops,
    // and the replay goes through.
    let file = dump_of(&s, &["customers", "v_out", "v_chain"]).await;
    assert_eq!(
        run_file(&s, &file).await,
        ExecEnd::Done,
        "{}: the dump did not replay onto its source:\n{file}",
        target.name
    );
    assert_eq!(
        scalar(&s, &format!("SELECT COUNT(*) FROM {v_chain}")).await,
        "3",
        "{}",
        target.name
    );
    s.teardown().await;
}

/// **A dump carrying a materialized view replays onto the database it came
/// from.** Its drop asked only whether it was a view, so the file wrote
/// `DROP VIEW IF EXISTS` for it, which PostgreSQL refuses (*"mv_in" is not a
/// view*), and every such replay stopped there — unless an earlier table's
/// `DROP … CASCADE` had already taken the view, which is why this one reads a
/// table the file does not carry. Where the engine has no materialized views
/// there is nothing to check.
pub async fn a_replay_replaces_the_materialized_view_it_carries(target: &'static Target) {
    let s = Scratch::create(target, "replay_matview").await;
    let (customers, mv) = (s.qualified("customers"), s.qualified("mv_in"));
    s.exec(&format!(
        "CREATE TABLE {customers} (id INTEGER PRIMARY KEY)"
    ))
    .await;
    s.exec(&format!("INSERT INTO {customers} VALUES (1), (2)"))
        .await;
    if s.try_exec(&format!(
        "CREATE MATERIALIZED VIEW {mv} AS SELECT COUNT(*) AS n FROM {customers}"
    ))
    .await
    .is_err()
    {
        s.teardown().await;
        return;
    }
    let file = dump_of(&s, &["mv_in"]).await;
    assert_eq!(
        run_file(&s, &file).await,
        ExecEnd::Done,
        "{}: the dump did not replay onto its source:\n{file}",
        target.name
    );
    assert_eq!(
        scalar(&s, &format!("SELECT n FROM {mv}")).await,
        "2",
        "{}: the materialized view did not come back with its rows:\n{file}",
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

/// **A MySQL-family `TIMESTAMP` restores as the instant it was, whatever
/// zone either session is in.** MySQL hands one over in the session's zone
/// and converts a literal back from it, and the file named none: dumped from
/// a `+05:00` session and replayed on a `-03:00` one, every `TIMESTAMP` moved
/// eight hours. The rows are read at `+00:00` (`Db::for_rows`) and the file
/// replays at `+00:00` (`dump::literal_mode_guard_sql`).
pub async fn a_timestamp_restores_as_the_instant_it_was(target: &'static Target) {
    let mut s = Scratch::create(target, "replay_zone").await;
    if !s.dialect().eq(&schemaic_core::intel::SqlDialect::MySql) {
        crate::endpoint::note_no_op(target, "spells its timestamps' zone");
        s.teardown().await;
        return;
    }
    let t = s.qualified("t");
    s.exec(&format!(
        "CREATE TABLE {t} (id INTEGER PRIMARY KEY, at TIMESTAMP NULL)"
    ))
    .await;
    // `FROM_UNIXTIME` stores the instant whatever the session's zone.
    s.exec(&format!(
        "INSERT INTO {t} VALUES (1, FROM_UNIXTIME(1760000000))"
    ))
    .await;
    let base = s.db.clone();
    s.db = base.clone().with_init_sql("SET time_zone = '+05:00'");
    let file = dump_of(&s, &["t"]).await;
    s.db = base.clone().with_init_sql("SET time_zone = '-03:00'");
    let ran = run_file(&s, &file).await;
    let at = scalar(&s, &format!("SELECT UNIX_TIMESTAMP(at) FROM {t}")).await;
    s.db = base;
    assert_eq!(ran, ExecEnd::Done, "the dump did not replay:\n{file}");
    assert_eq!(at, "1760000000", "the replay moved the timestamp:\n{file}");
    s.teardown().await;
}

/// **A MariaDB replay keeps every grant on the routines it replaces.** The
/// file dropped each routine before its `CREATE`, and with
/// `automatic_sp_privileges` (the default) the `DROP` revoked every account's
/// `EXECUTE` on it: the application's next call was *execute command denied*.
/// MariaDB's `CREATE OR REPLACE` keeps them. MySQL 8 has no such form, so the
/// file still drops there and says so in its header.
pub async fn a_mariadb_replay_keeps_the_grants_on_the_routines_it_replaces(
    target: &'static Target,
) {
    if target.name != "mariadb" {
        crate::endpoint::note_no_op(target, "replaces a routine by dropping it");
        return;
    }
    let s = Scratch::create(target, "replay_grant").await;
    let (t, f) = (s.qualified("t"), s.qualified("f_one"));
    // A role rather than a user: no password, and the name carries the scratch
    // database's, so a stranded one is swept with it.
    let role = format!("{}_r", s.database);
    s.exec(&format!("CREATE TABLE {t} (id INTEGER PRIMARY KEY)"))
        .await;
    s.exec(&format!(
        "CREATE FUNCTION {f}() RETURNS INTEGER DETERMINISTIC NO SQL RETURN 1"
    ))
    .await;
    s.exec(&format!("CREATE ROLE `{role}`")).await;
    s.exec(&format!("GRANT EXECUTE ON FUNCTION {f} TO `{role}`"))
        .await;
    let grants = format!(
        "SELECT COUNT(*) FROM mysql.procs_priv WHERE Db = '{}' AND Routine_name = 'f_one'",
        s.database
    );
    assert_eq!(
        scalar(&s, &grants).await,
        "1",
        "the fixture's grant is missing"
    );
    let file = dump_of(&s, &["t"]).await;
    let ran = run_file(&s, &file).await;
    let kept = scalar(&s, &grants).await;
    s.exec(&format!("DROP ROLE `{role}`")).await;
    assert_eq!(
        ran,
        ExecEnd::Done,
        "the dump did not replay onto its source:\n{file}"
    );
    assert_eq!(kept, "1", "the replay revoked the routine's grant:\n{file}");
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

/// **A MariaDB sequence is dumped, and a replay puts it back where it was.**
/// The file carried only a comment pointing at `SHOW CREATE SEQUENCE`, so a
/// restore lost the sequence and every table whose default calls `nextval`
/// on it. Now the file drops and recreates it before the tables, with its
/// options, and restarts it at the position the source had reached — so the
/// next value is the one the source would have handed out after a restart,
/// never one already used.
pub async fn a_replay_restores_a_mariadb_sequence_and_its_position(target: &'static Target) {
    if target.name != "mariadb" {
        crate::endpoint::note_no_op(target, "has no table-shaped sequence");
        return;
    }
    let s = Scratch::create(target, "replay_sequence").await;
    let (t, seq) = (s.qualified("t"), s.qualified("counter"));
    s.exec(&format!(
        "CREATE SEQUENCE {seq} START WITH 10 INCREMENT BY 5 NOCACHE"
    ))
    .await;
    s.exec(&format!(
        "CREATE TABLE {t} (id INTEGER PRIMARY KEY DEFAULT (NEXTVAL({seq})), v INTEGER)"
    ))
    .await;
    s.exec(&format!("INSERT INTO {t} (v) VALUES (1), (2)"))
        .await;
    let file = dump_of(&s, &["counter", "t"]).await;
    // Onto its source first, the table standing while the file drops and
    // recreates the sequence its default calls: MariaDB tracks no such
    // dependency, so the drop is not refused.
    assert_eq!(
        run_file(&s, &file).await,
        ExecEnd::Done,
        "{}: the dump did not replay onto its source:\n{file}",
        target.name
    );
    // Then a restore, not a replay over what is still there: a file that left
    // the sequence alone would pass the first, whatever it said about it.
    s.exec(&format!("DROP TABLE {t}")).await;
    s.exec(&format!("DROP SEQUENCE {seq}")).await;
    assert_eq!(
        run_file(&s, &file).await,
        ExecEnd::Done,
        "{}: the dump did not restore:\n{file}",
        target.name
    );
    assert_eq!(count(&s, "t").await, "2", "{}", target.name);
    // 10 and 15 were used; with no cache the next not handed out is 20.
    s.exec(&format!("INSERT INTO {t} (v) VALUES (3)")).await;
    assert_eq!(
        scalar(&s, &format!("SELECT id FROM {t} WHERE v = 3")).await,
        "20",
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
