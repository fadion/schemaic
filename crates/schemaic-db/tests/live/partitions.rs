//! **Partitioned tables, read back and restored.** The catalogue reads that
//! feed `TableInfo::partitioning`, `partition_of` and the `inherited` marks
//! are SQL no pure test reaches — the fold tests in `schemaic-db` cover the
//! folding of rows they are handed, not the queries that produce them — and a
//! broken one reads every partition as a plain table again: the dump then
//! writes the parent's rows twice and restates cloned keys, and nothing fails.
//! So the read is asserted here against a real partitioned table on each
//! engine, and the dump of one is restored.
//!
//! SQL Server's leg is in [`crate::mssql`], beside its own scratch.

use schemaic_core::dump::{DumpOptions, DumpStep, plan, render_rows};
use schemaic_core::schema::{DbSchema, TableInfo, display_name, partitions_are_tables};
use schemaic_core::script::ExecEnd;
use tokio_util::sync::CancellationToken;

use crate::endpoint::Target;
use crate::runtime::run_file;
use crate::scratch::Scratch;

/// `events`, range-partitioned on `at` into 2023 and 2024 with rows in each,
/// keyed onto `customers`, indexed on `at`; and `orders`, keyed onto
/// `events`. Where a partition is storage rather than a table, the same
/// partitioning without the keys, which those engines refuse on a
/// partitioned table.
async fn seed(s: &Scratch) {
    let (events, customers, orders) = (
        s.qualified("events"),
        s.qualified("customers"),
        s.qualified("orders"),
    );
    if partitions_are_tables(s.dialect()) {
        s.exec(&format!(
            "CREATE TABLE {customers} (id INTEGER PRIMARY KEY)"
        ))
        .await;
        s.exec(&format!(
            "CREATE TABLE {events} (id INTEGER NOT NULL, at DATE NOT NULL, \
             cust_id INTEGER REFERENCES {customers} (id), PRIMARY KEY (id, at)) \
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
        s.exec(&format!("CREATE INDEX events_at_idx ON {events} (at)"))
            .await;
        // A trigger on the parent, which a dump of one partition must not
        // restate over the one standing there. On a built-in function, so
        // the file carries no routine of its own.
        s.exec(&format!(
            "CREATE TRIGGER events_same BEFORE UPDATE ON {events} \
             FOR EACH ROW EXECUTE FUNCTION suppress_redundant_updates_trigger()"
        ))
        .await;
        s.exec(&format!(
            "CREATE TABLE {orders} (id INTEGER PRIMARY KEY, event_id INTEGER, event_at DATE, \
             FOREIGN KEY (event_id, event_at) REFERENCES {events} (id, at))"
        ))
        .await;
        s.exec(&format!("INSERT INTO {customers} VALUES (1), (2)"))
            .await;
        s.exec(&format!(
            "INSERT INTO {events} VALUES (1, '2023-03-01', 1), (2, '2023-07-01', 2), \
             (3, '2024-02-01', 1), (4, '2024-05-01', 2), (5, '2024-09-01', 1)"
        ))
        .await;
        s.exec(&format!(
            "INSERT INTO {orders} VALUES (1, 1, '2023-03-01'), (2, 4, '2024-05-01')"
        ))
        .await;
    } else {
        s.exec(&format!(
            "CREATE TABLE {events} (id INTEGER NOT NULL, at DATE NOT NULL, \
             PRIMARY KEY (id, at)) PARTITION BY RANGE (YEAR(at)) \
             (PARTITION p2023 VALUES LESS THAN (2024), PARTITION p2024 VALUES LESS THAN (2025))"
        ))
        .await;
        s.exec(&format!(
            "INSERT INTO {events} VALUES (1, '2023-03-01'), (2, '2023-07-01'), \
             (3, '2024-02-01'), (4, '2024-05-01'), (5, '2024-09-01')"
        ))
        .await;
    }
}

async fn schema_of(s: &Scratch) -> DbSchema {
    Box::pin(s.db.fetch_schema(&s.database, CancellationToken::new()))
        .await
        .expect("the schema")
}

fn table<'a>(schema: &'a DbSchema, name: &str) -> &'a TableInfo {
    schema
        .tables
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("{name} was not read: {:?}", schema.tables))
}

/// A default dump of the tables `chosen`, as the app's writer writes it.
/// Shared with [`crate::dump_replay`].
pub(crate) async fn dump_of(src: &Scratch, chosen: &[&str]) -> String {
    let dialect = src.dialect();
    let schema = schema_of(src).await;
    let chosen: Vec<String> = schema
        .tables
        .iter()
        .map(|t| display_name(t.schema.as_deref(), &t.name))
        .filter(|n| {
            chosen
                .iter()
                .any(|c| n == c || n.ends_with(&format!(".{c}")))
        })
        .collect();
    let dump = plan(
        &schema,
        &src.database,
        &chosen,
        DumpOptions::default(),
        dialect,
    );
    let mut file = String::new();
    for step in dump.steps {
        match step {
            DumpStep::Text(sql) => {
                file.push_str(&sql);
                file.push_str("\n\n");
            }
            DumpStep::Rows {
                database,
                insert_database,
                schema,
                table,
                select,
                server,
            } => {
                // On the session the app's dump reads on (`cli::dump::run`).
                let rs = Box::pin(src.db.for_rows().fetch_query(
                    Some(&database),
                    &select,
                    10_000,
                    CancellationToken::new(),
                ))
                .await
                .expect("the rows");
                let order: Vec<usize> = (0..rs.row_count()).collect();
                let mut out = Vec::new();
                render_rows(
                    &mut out,
                    &mut schemaic_core::export::OneChunk::new(&rs, &order),
                    (&insert_database, schema.as_deref(), &table),
                    &server,
                    dialect,
                )
                .expect("the rows render");
                file.push_str(&String::from_utf8(out).expect("UTF-8 rows"));
                file.push('\n');
            }
        }
    }
    file
}

pub(crate) async fn count(s: &Scratch, table: &str) -> String {
    let rs = s
        .exec(&format!("SELECT COUNT(*) FROM {}", s.qualified(table)))
        .await;
    rs.cell(0, 0).expect("a count").display().to_string()
}

/// **A partitioned table reads back partitioned**: its key, each partition's
/// bound, and — where a partition is a table — the partition under its
/// parent, with the index and keys it took from the parent marked
/// `inherited`, as are the clones of `orders`' key onto each partition.
pub async fn a_partitioned_table_reads_back_partitioned(target: &'static Target) {
    let s = Scratch::create(target, "part_read").await;
    seed(&s).await;
    let schema = schema_of(&s).await;
    let events = table(&schema, "events");
    let p = events
        .partitioning
        .as_ref()
        .unwrap_or_else(|| panic!("{}: events read as unpartitioned", target.name));
    if partitions_are_tables(s.dialect()) {
        assert_eq!(p.key, "RANGE (at)", "{}", target.name);
        let p2024 = table(&schema, "events_2024");
        let of = p2024
            .partition_of
            .as_ref()
            .unwrap_or_else(|| panic!("{}: events_2024 read as a plain table", target.name));
        assert_eq!(of.parent, "events", "{}", target.name);
        assert_eq!(
            of.bound, "FOR VALUES FROM ('2024-01-01') TO ('2025-01-01')",
            "{}",
            target.name
        );
        assert!(
            p2024.foreign_keys.iter().all(|fk| fk.inherited) && !p2024.foreign_keys.is_empty(),
            "{}: the partition's key onto customers is its parent's: {:?}",
            target.name,
            p2024.foreign_keys
        );
        assert!(
            p2024
                .indexes
                .iter()
                .any(|ix| ix.inherited && !ix.is_primary()),
            "{}: the partition's index on `at` is its parent's: {:?}",
            target.name,
            p2024.indexes
        );
        assert!(
            events.foreign_keys.iter().all(|fk| !fk.inherited),
            "{}: the parent's own key: {:?}",
            target.name,
            events.foreign_keys
        );
        let orders = table(&schema, "orders");
        let (own, clones): (Vec<_>, Vec<_>) =
            orders.foreign_keys.iter().partition(|fk| !fk.inherited);
        assert_eq!(
            own.iter()
                .map(|fk| fk.ref_table.as_str())
                .collect::<Vec<_>>(),
            ["events"],
            "{}: {:?}",
            target.name,
            orders.foreign_keys
        );
        let mut targets: Vec<&str> = clones.iter().map(|fk| fk.ref_table.as_str()).collect();
        targets.sort();
        assert_eq!(
            targets,
            ["events_2023", "events_2024"],
            "{}: the clones of orders' key onto each partition",
            target.name
        );
    } else {
        assert!(
            p.key.to_ascii_uppercase().starts_with("RANGE"),
            "{}: {}",
            target.name,
            p.key
        );
        let parts: Vec<(&str, &str)> = p
            .partitions
            .iter()
            .map(|x| (x.name.as_str(), x.bound.as_str()))
            .collect();
        assert_eq!(
            parts,
            [
                ("p2023", "VALUES LESS THAN (2024)"),
                ("p2024", "VALUES LESS THAN (2025)")
            ],
            "{}",
            target.name
        );
    }
    s.teardown().await;
}

/// **A dump of a partitioned database restores partitioned, each partition
/// with its own rows**, into an empty database and again onto itself.
pub async fn a_partitioned_dump_restores_with_every_partitions_rows(target: &'static Target) {
    let src = Scratch::create(target, "part_dump_src").await;
    seed(&src).await;
    let file = dump_of(&src, &["events", "customers", "orders"]).await;
    let dst = Scratch::create(target, "part_dump_dst").await;
    let file = file.replace(&src.database, &dst.database);
    // Twice: the second run is the file replayed onto what it restored.
    for run in ["into an empty database", "onto itself"] {
        assert_eq!(
            run_file(&dst, &file).await,
            ExecEnd::Done,
            "{}: the dump did not restore {run}:\n{file}",
            target.name
        );
        assert_eq!(count(&dst, "events").await, "5", "{} {run}", target.name);
        if partitions_are_tables(dst.dialect()) {
            assert_eq!(
                count(&dst, "events_2023").await,
                "2",
                "{} {run}",
                target.name
            );
            assert_eq!(
                count(&dst, "events_2024").await,
                "3",
                "{} {run}",
                target.name
            );
            assert_eq!(count(&dst, "orders").await, "2", "{} {run}", target.name);
        }
        let schema = schema_of(&dst).await;
        assert!(
            table(&schema, "events").partitioning.is_some(),
            "{} {run}: restored unpartitioned",
            target.name
        );
    }
    dst.teardown().await;
    src.teardown().await;
}

/// **A dump of one partition, replayed onto the database it came from,
/// leaves the other partitions and their rows alone.** It brings its parent
/// only to be created `OF`, and once dropped that parent with `CASCADE`:
/// every sibling partition went with it, and the file put back only its own.
pub async fn a_dumped_partition_replays_beside_its_siblings(target: &'static Target) {
    let s = Scratch::create(target, "part_one").await;
    if !partitions_are_tables(s.dialect()) {
        crate::endpoint::note_no_op(target, "a partition is not a table to dump here");
        s.teardown().await;
        return;
    }
    seed(&s).await;
    let file = dump_of(&s, &["events_2024"]).await;
    assert_eq!(
        run_file(&s, &file).await,
        ExecEnd::Done,
        "{}: the partition's dump did not replay:\n{file}",
        target.name
    );
    assert_eq!(
        count(&s, "events_2023").await,
        "2",
        "{}:\n{file}",
        target.name
    );
    assert_eq!(
        count(&s, "events_2024").await,
        "3",
        "{}:\n{file}",
        target.name
    );
    assert_eq!(count(&s, "orders").await, "2", "{}:\n{file}", target.name);
    // The partition's `DROP … CASCADE` did not take `orders`' key onto the
    // parent with it: the replayed partition is under it again.
    let keys = s
        .exec(&format!(
            "SELECT conname, conparentid <> 0 FROM pg_constraint \
             WHERE conrelid = '{}'::regclass AND contype = 'f' ORDER BY 2, 1",
            s.qualified("orders")
        ))
        .await;
    assert_eq!(
        keys.row_count(),
        3,
        "{}: orders' key and its clone onto each partition:\n{file}",
        target.name
    );
    // And into an empty database it brings the parent it is created in.
    let dst = Scratch::create(target, "part_one_dst").await;
    assert_eq!(
        run_file(&dst, &file.replace(&s.database, &dst.database)).await,
        ExecEnd::Done,
        "{}: the partition's dump did not restore into an empty database:\n{file}",
        target.name
    );
    assert_eq!(count(&dst, "events").await, "3", "{}", target.name);
    // With the parent's own trigger, created where it was missing.
    let trig = dst
        .exec(&format!(
            "SELECT COUNT(*) FROM pg_trigger WHERE tgrelid = '{}'::regclass \
             AND tgname = 'events_same'",
            dst.qualified("events")
        ))
        .await;
    assert_eq!(
        trig.cell(0, 0).expect("a count").display().to_string(),
        "1",
        "{}:\n{file}",
        target.name
    );
    dst.teardown().await;
    s.teardown().await;
}

/// **A comparison of a partitioned table plans what the server takes**: a
/// new partition, a column and an index added to the parent. The plan
/// restated each on every partition, and added the new partition's key
/// clone beside the one the server makes, and the server refused it whole.
pub async fn a_compared_partitioned_table_plans_what_the_server_takes(target: &'static Target) {
    let dst = Scratch::create(target, "part_cmp_dst").await;
    if !partitions_are_tables(dst.dialect()) {
        crate::endpoint::note_no_op(target, "a partition is not a table to compare here");
        dst.teardown().await;
        return;
    }
    seed(&dst).await;
    let src = Scratch::create(target, "part_cmp_src").await;
    seed(&src).await;
    let events = src.qualified("events");
    src.exec(&format!(
        "CREATE TABLE {} PARTITION OF {events} \
         FOR VALUES FROM ('2025-01-01') TO ('2026-01-01')",
        src.qualified("events_2025")
    ))
    .await;
    src.exec(&format!("ALTER TABLE {events} ADD COLUMN note TEXT"))
        .await;
    src.exec(&format!(
        "CREATE INDEX events_cust_idx ON {events} (cust_id)"
    ))
    .await;
    let plan = schemaic_core::compare::SchemaComparison::of(
        &schema_of(&dst).await,
        &schema_of(&src).await,
        dst.dialect(),
    )
    .plan(|_| true);
    let sql = plan
        .emit()
        .iter()
        .map(|s| format!("{s}\n"))
        .collect::<String>();
    assert_eq!(
        run_file(&dst, &sql).await,
        ExecEnd::Done,
        "{}: the plan was refused:\n{sql}",
        target.name
    );
    // And the target now compares the same.
    let again = schemaic_core::compare::SchemaComparison::of(
        &schema_of(&dst).await,
        &schema_of(&src).await,
        dst.dialect(),
    )
    .plan(|_| true)
    .emit();
    assert!(again.is_empty(), "{}: still to do: {again:?}", target.name);
    src.teardown().await;
    dst.teardown().await;
}
