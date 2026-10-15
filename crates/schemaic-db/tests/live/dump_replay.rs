//! **A dump replayed onto the database it came from** — the primary way
//! anyone tests a dump — leaves that database as it was. Each test here is a
//! thing a replay lost or refused on a real server while the plan's pure
//! tests all passed: the statements were each right, and what went wrong was
//! what the server did with them in that order.

use schemaic_core::script::ExecEnd;

use crate::endpoint::Target;
use crate::partitions::{count, dump_of};
use crate::runtime::run_file;
use crate::scratch::Scratch;

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
