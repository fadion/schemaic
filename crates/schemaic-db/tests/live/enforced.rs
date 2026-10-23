//! [`Db::fetch_query_enforced`]: the session agreeing with a text gate.
//!
//! **The gate reads the statement; only the server sees what it does.** The
//! headless read path's gate (`sql::read_only_reason`) takes a statement that
//! opens with a read head and names no denied keyword — and
//! `SELECT purge_all()` is exactly that, while `purge_all()` deletes a table. On an ordinary session it
//! runs, which is how the MCP server's `run_query` and `schemaic query` were
//! shown deleting rows through a gate documented as read-only. PostgreSQL's
//! gate now refuses the call itself, since `purge_all` is on no list of reads,
//! but a view's body or a trigger is out of any text gate's sight on every
//! engine. These tests are the session's half of the guard, on every server leg.
//!
//! [`Db::fetch_query_enforced`]: schemaic_db::Db::fetch_query_enforced

use schemaic_db::{DbError, DbRefusal, Enforce};
use tokio_util::sync::CancellationToken;

use crate::endpoint::Target;
use crate::scratch::Scratch;

/// A table of three rows and a function that empties it.
async fn seeded(target: &'static Target, test: &str) -> (Scratch, String) {
    let scratch = Scratch::create(target, test).await;
    let t = scratch.qualified("keep");
    scratch
        .exec(&format!(
            "CREATE TABLE {t} (id INTEGER NOT NULL PRIMARY KEY)"
        ))
        .await;
    scratch
        .exec(&format!("INSERT INTO {t} (id) VALUES (1), (2), (3)"))
        .await;
    scratch
        .exec(&target.purging_function.replace("{table}", &t))
        .await;
    (scratch, t)
}

async fn enforced(scratch: &Scratch, sql: &str, enforce: Enforce) -> Result<u64, String> {
    scratch
        .db
        .fetch_query_enforced(
            Some(&scratch.database),
            sql,
            100,
            CancellationToken::new(),
            enforce,
        )
        .await
        .map(|rs| rs.row_count() as u64)
        .map_err(|e| e.to_string())
}

async fn rows_left(scratch: &Scratch, t: &str) -> String {
    let rs = scratch.exec(&format!("SELECT COUNT(*) FROM {t}")).await;
    rs.cell(0, 0).expect("a count").display().to_string()
}

/// **A write a `SELECT` hides is refused by the session.** The gate is asked
/// first and, where calls are answered by a deny list, passes it — that
/// composition is the bug — and the rows are counted afterwards, because an
/// error alone could be a session that refuses everything. PostgreSQL's gate
/// refuses the call by its function allowlist; the session is asked there all
/// the same, as it is what stands behind a view.
pub async fn a_read_only_session_refuses_a_write_a_select_hides(target: &'static Target) {
    use schemaic_core::intel::SqlDialect;
    let (scratch, t) = seeded(target, "enforce_ro").await;
    let gate = schemaic_core::sql::read_only_reason("SELECT purge_all()", scratch.dialect());
    // SQL Server answers calls by an allowlist too; its leg is outside this
    // suite (`crate::mssql`), and this is its answer should it join.
    let gate_refuses_the_call = match scratch.dialect() {
        SqlDialect::Postgres | SqlDialect::MsSql => true,
        SqlDialect::MySql | SqlDialect::Sqlite => false,
    };
    assert_eq!(
        gate.is_err(),
        gate_refuses_the_call,
        "{}: the text gate answered {gate:?}",
        target.name
    );

    let got = enforced(&scratch, "SELECT purge_all()", Enforce::ReadOnly).await;

    assert!(
        got.is_err(),
        "{}: a read-only session ran a function that deletes",
        target.name
    );
    assert_eq!(
        rows_left(&scratch, &t).await,
        "3",
        "{}: rows the refused statement deleted",
        target.name
    );
    scratch.teardown().await;
}

/// The refusal is about the write: an ordinary read runs on the same session.
pub async fn a_read_only_session_still_reads(target: &'static Target) {
    let (scratch, t) = seeded(target, "enforce_reads").await;
    let got = enforced(&scratch, &format!("SELECT id FROM {t}"), Enforce::ReadOnly).await;
    assert_eq!(got, Ok(3), "{}: a read on a read-only session", target.name);
    scratch.teardown().await;
}

/// `AsJudged` is not read-only — `exec` on a writable connection runs on it —
/// so the pin it adds must leave a write working.
pub async fn a_session_pinned_to_the_gates_lexer_still_writes(target: &'static Target) {
    let (scratch, t) = seeded(target, "enforce_judged").await;
    enforced(
        &scratch,
        &format!("DELETE FROM {t} WHERE id = 1"),
        Enforce::AsJudged,
    )
    .await
    .unwrap_or_else(|e| panic!("{}: a write on an AsJudged session: {e}", target.name));
    assert_eq!(rows_left(&scratch, &t).await, "2", "{}", target.name);
    scratch.teardown().await;
}

/// **A Manual tab's pinned session is read-only for its whole life** on a
/// read-only connection: every transaction `ensure_tx` opens on it refuses the
/// hidden write. The unenforced session beside it runs the same statement,
/// which is what makes the refusal about the session rather than a function
/// that fails anyway — and neither commits, so the table is untouched.
pub async fn a_read_only_pinned_session_refuses_a_write_a_select_hides(target: &'static Target) {
    use schemaic_db::Session;
    let (scratch, t) = seeded(target, "enforce_pinned").await;
    let run = |enforce: Option<Enforce>| {
        let scratch = &scratch;
        async move {
            let s = Session::open_enforced(&scratch.db, Some(&scratch.database), enforce)
                .await
                .unwrap_or_else(|e| panic!("{}: open: {e}", target.name));
            s.ensure_tx().await.expect("BEGIN");
            let got = s
                .fetch_query("SELECT purge_all()", 100, CancellationToken::new())
                .await
                .result
                .map(|_| ())
                .map_err(|e| e.to_string());
            let _ = s.rollback().await;
            s.close().await;
            got
        }
    };
    assert!(
        run(Some(Enforce::ReadOnly)).await.is_err(),
        "{}: a read-only pinned session ran a function that deletes",
        target.name
    );
    run(None).await.unwrap_or_else(|e| {
        panic!(
            "{}: the ordinary session could not run it: {e}",
            target.name
        )
    });
    assert_eq!(rows_left(&scratch, &t).await, "3", "{}", target.name);
    scratch.teardown().await;
}

/// EXPLAIN ANALYZE **executes** the statement, inside a transaction it rolls
/// back — which undoes neither a MyISAM write nor a PostgreSQL sequence. On a
/// read-only connection that transaction is read-only and refuses the write;
/// on any other the same measurement runs.
pub async fn a_read_only_explain_analyze_refuses_a_write_a_select_hides(target: &'static Target) {
    let (scratch, t) = seeded(target, "enforce_explain").await;
    let explain = |read_only: bool| {
        scratch.db.explain(
            Some(&scratch.database),
            "SELECT purge_all()",
            true,
            read_only,
            CancellationToken::new(),
        )
    };
    assert!(
        explain(true).await.is_err(),
        "{}: a read-only EXPLAIN ANALYZE ran a function that deletes",
        target.name
    );
    explain(false)
        .await
        .unwrap_or_else(|e| panic!("{}: the ordinary measurement failed: {e}", target.name));
    assert_eq!(rows_left(&scratch, &t).await, "3", "{}", target.name);
    scratch.teardown().await;
}

/// **A plan is not inert, so a read-only one runs on a read-only session.**
/// The plain `EXPLAIN` executes nothing *of the statement* — but MySQL and
/// MariaDB run a stored function to materialise a derived table or fold a
/// constant while planning, and PostgreSQL evaluates an `IMMUTABLE` function at
/// plan time, which may call a volatile one. Each emptied the table through a
/// plain `EXPLAIN` on an ordinary session, read-only connection or not, and the
/// MCP server's `explain` tool asks exactly this with nobody at the keyboard.
pub async fn a_read_only_plan_refuses_a_write_its_planner_makes(target: &'static Target) {
    let (scratch, t) = seeded(target, "enforce_plan").await;
    let sql = match scratch.db.engine() {
        schemaic_db::Engine::Postgres => {
            scratch
                .exec(
                    "CREATE FUNCTION fold_purge() RETURNS int IMMUTABLE LANGUAGE plpgsql AS \
                     $$ BEGIN PERFORM purge_all(); RETURN 1; END $$",
                )
                .await;
            "SELECT fold_purge()"
        }
        _ => "SELECT * FROM (SELECT purge_all() AS x) d",
    };
    let got = scratch
        .db
        .explain(
            Some(&scratch.database),
            sql,
            false,
            true,
            CancellationToken::new(),
        )
        .await;
    assert_eq!(
        rows_left(&scratch, &t).await,
        "3",
        "{}: a read-only plan let its planner delete ({got:?})",
        target.name
    );
    assert!(got.is_err(), "{}: the write was not refused", target.name);
    // A plan of a write: PostgreSQL plans it in a read-only transaction
    // (`EXPLAIN` alone never reaches the executor's read-only check); MySQL and
    // MariaDB refuse to, and the refusal says so in Schemaic's words rather
    // than as "Cannot execute statement".
    let write = scratch
        .db
        .explain(
            Some(&scratch.database),
            &format!("DELETE FROM {t}"),
            false,
            true,
            CancellationToken::new(),
        )
        .await;
    match scratch.db.engine() {
        schemaic_db::Engine::Postgres => {
            write.unwrap_or_else(|e| panic!("{}: a read-only plan of a DELETE: {e}", target.name));
        }
        _ => {
            assert!(
                matches!(&write, Err(schemaic_db::DbError::Refused(m))
                    if m.to_string().contains("read-only")
                        && m.to_string().contains("SELECT form")),
                "{}: {write:?}",
                target.name
            );
            // The SELECT was refused for its function, and is told so rather
            // than to plan the SELECT form it already is.
            assert!(
                matches!(&got, Err(schemaic_db::DbError::Refused(m))
                    if !m.to_string().contains("SELECT form")
                        && m.to_string().contains("function")),
                "{}: {got:?}",
                target.name
            );
        }
    }
    assert_eq!(rows_left(&scratch, &t).await, "3", "{}", target.name);
    scratch.teardown().await;
}

/// The *All rows* export re-runs the tab's statement, so on a read-only
/// connection its stream is refused the hidden write the run was.
pub async fn a_read_only_stream_refuses_a_write_a_select_hides(target: &'static Target) {
    let (scratch, t) = seeded(target, "enforce_stream").await;
    let stream = |enforce: Option<Enforce>| {
        let scratch = &scratch;
        async move {
            let (tx, mut rx) = tokio::sync::mpsc::channel(4);
            let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
            let got = scratch
                .db
                .stream_query_enforced(
                    Some(&scratch.database),
                    "SELECT purge_all()",
                    100,
                    CancellationToken::new(),
                    tx,
                    enforce,
                )
                .await;
            let _ = drain.await;
            got.map(|_| ()).map_err(|e| e.to_string())
        }
    };
    assert!(
        stream(Some(Enforce::ReadOnly)).await.is_err(),
        "{}: a read-only stream ran a function that deletes",
        target.name
    );
    // A dump's, a copy's: the app's own `SELECT`, still on a session that
    // refuses what the object it reads does (release review S1-L1-02).
    assert!(
        stream(Some(Enforce::Generated)).await.is_err(),
        "{}: a generated read's stream ran a function that deletes",
        target.name
    );
    assert_eq!(rows_left(&scratch, &t).await, "3", "{}", target.name);
    stream(None)
        .await
        .unwrap_or_else(|e| panic!("{}: the unenforced stream failed: {e}", target.name));
    assert_eq!(
        rows_left(&scratch, &t).await,
        "0",
        "{}: the unenforced stream is the one that deletes",
        target.name
    );
    scratch.teardown().await;
}

/// **Run All on a read-only connection** shares one connection across its
/// statements, and that connection refuses the hidden write; the same batch
/// unenforced runs it, so the refusal is the session's.
pub async fn a_read_only_batch_refuses_a_write_a_select_hides(target: &'static Target) {
    let (scratch, t) = seeded(target, "enforce_batch").await;
    let stmts = vec![
        format!("SELECT id FROM {t}"),
        "SELECT purge_all()".to_string(),
    ];
    let run = |enforce: Option<Enforce>| {
        let (scratch, stmts) = (&scratch, stmts.clone());
        async move {
            let mut out = Vec::new();
            scratch
                .db
                .run_batch_enforced(
                    Some(&scratch.database),
                    &stmts,
                    100,
                    CancellationToken::new(),
                    |_, r| out.push(r.is_ok()),
                    enforce,
                )
                .await;
            out
        }
    };
    assert_eq!(
        run(Some(Enforce::ReadOnly)).await,
        vec![true, false],
        "{}: the read runs and the hidden write is refused",
        target.name
    );
    assert_eq!(rows_left(&scratch, &t).await, "3", "{}", target.name);
    assert_eq!(run(None).await, vec![true, true], "{}", target.name);
    assert_eq!(
        rows_left(&scratch, &t).await,
        "0",
        "{}: the unenforced batch is the one that deletes",
        target.name
    );
    scratch.teardown().await;
}

/// [`seeded`], with the server's [`Target::lifting_function`] created too.
async fn seeded_with_a_lift(target: &'static Target, test: &str) -> (Scratch, String) {
    let (scratch, t) = seeded(target, test).await;
    if let Some(ddl) = target.lifting_function {
        scratch.exec(ddl).await;
    }
    (scratch, t)
}

/// **A read-only batch stays read-only after a statement that lifts it.** Run
/// All shares one connection, and on PostgreSQL the session's read-only state
/// is a setting — `default_transaction_read_only` — which a `SELECT` reaching
/// `set_config` turns off for every transaction after its own. The write that
/// follows is a plain `DELETE`, refused by its head anywhere a text gate is
/// asked; this is the session's half, which has to hold without one.
pub async fn a_read_only_batch_cannot_be_lifted_for_a_later_statement(target: &'static Target) {
    let (scratch, t) = seeded_with_a_lift(target, "enforce_lift_batch").await;
    let stmts = vec![target.lifting_call.to_string(), format!("DELETE FROM {t}")];
    let mut out = Vec::new();
    scratch
        .db
        .run_batch_enforced(
            Some(&scratch.database),
            &stmts,
            100,
            CancellationToken::new(),
            |_, r| out.push(r.map(|_| ()).map_err(|e| e.to_string())),
            Some(Enforce::ReadOnly),
        )
        .await;
    assert!(
        out.get(1).is_some_and(Result::is_err),
        "{}: the write after the lift ran: {out:?}",
        target.name
    );
    assert_eq!(rows_left(&scratch, &t).await, "3", "{}", target.name);
    scratch.teardown().await;
}

/// **One text holding the lift and a hidden write is refused too.** The
/// editor sends a selection of several statements as one text, which MySQL's
/// driver runs as a multi-statement query, and the text gate passes both when
/// neither names a write.
pub async fn a_read_only_text_cannot_lift_itself_for_its_next_statement(target: &'static Target) {
    let (scratch, t) = seeded_with_a_lift(target, "enforce_lift_text").await;
    let sql = format!("{}; SELECT purge_all()", target.lifting_call);
    let got = enforced(&scratch, &sql, Enforce::ReadOnly).await;
    assert_eq!(
        rows_left(&scratch, &t).await,
        "3",
        "{}: the hidden write after the lift ran ({got:?})",
        target.name
    );
    scratch.teardown().await;
}

/// **A read-only Manual tab stays read-only across a commit.** Every
/// transaction the pinned session opens is read-only in its own right, so a
/// lift inside one cannot reach the next one the tab begins.
pub async fn a_read_only_pinned_session_cannot_be_lifted_across_a_commit(target: &'static Target) {
    use schemaic_db::Session;
    let (scratch, t) = seeded_with_a_lift(target, "enforce_lift_pinned").await;
    let s = Session::open_enforced(
        &scratch.db,
        Some(&scratch.database),
        Some(Enforce::ReadOnly),
    )
    .await
    .unwrap_or_else(|e| panic!("{}: open: {e}", target.name));
    s.ensure_tx().await.expect("BEGIN");
    // Whether the lift itself is refused is the server's business; what it
    // must not do is reach the transaction after the commit.
    let _ = s
        .fetch_query(target.lifting_call, 100, CancellationToken::new())
        .await;
    let _ = s.commit().await;
    s.ensure_tx().await.expect("the second BEGIN");
    let got = s
        .fetch_query(&format!("DELETE FROM {t}"), 100, CancellationToken::new())
        .await
        .result
        .map(|_| ())
        .map_err(|e| e.to_string());
    let _ = s.commit().await;
    s.close().await;
    assert!(
        got.is_err(),
        "{}: the write after the lift ran",
        target.name
    );
    assert_eq!(rows_left(&scratch, &t).await, "3", "{}", target.name);
    scratch.teardown().await;
}

/// **A read-only PostgreSQL session reads `pg_catalog` first** (security
/// review S24). The read-only gates allowlist builtin functions by their
/// bare names, which is only an answer about the builtin while `pg_catalog`
/// is searched first — PostgreSQL's default, unless a path names it after a
/// schema of the user's. Init SQL may set `search_path`, and so may a role's
/// server-side default, so `SET search_path TO public, pg_catalog` let a
/// `public.version()` stand in for the builtin. The unenforced read shows the
/// shadow is real; the read-only one never reaches it — refused now before the
/// statement runs, since `version` is a name the read-only gate allows
/// ([`a_read_only_session_refuses_a_path_that_shadows_a_builtin`]).
/// PostgreSQL only.
pub async fn a_read_only_session_reads_the_catalog_before_a_users_functions(
    target: &'static Target,
) {
    if target.engine != schemaic_db::Engine::Postgres {
        return;
    }
    let scratch = Scratch::create(target, "enforce_catalog_first").await;
    scratch
        .exec(
            "CREATE FUNCTION public.version() RETURNS text LANGUAGE sql \
             AS $$ SELECT 'shadowed'::text $$",
        )
        .await;
    let db = scratch
        .db
        .clone()
        .with_init_sql("SET search_path TO public, pg_catalog");
    let shadowed = db
        .fetch_query(
            Some(&scratch.database),
            "SELECT version()",
            1,
            CancellationToken::new(),
        )
        .await
        .unwrap_or_else(|e| panic!("SELECT version(): {e}"));
    assert_eq!(
        shadowed.cell(0, 0).expect("a value").display().to_string(),
        "shadowed",
        "the fixture does not shadow"
    );
    let enforced = db
        .fetch_query_enforced(
            Some(&scratch.database),
            "SELECT version()",
            1,
            CancellationToken::new(),
            Enforce::ReadOnly,
        )
        .await;
    match enforced {
        Err(DbError::Refused(DbRefusal::ShadowedBuiltin { schema, name, .. })) => {
            assert_eq!((schema.as_str(), name.as_str()), ("public", "version"));
        }
        Ok(rs) => panic!(
            "a read-only session ran on a shadowing path: {}",
            rs.cell(0, 0).expect("a value").display()
        ),
        Err(e) => panic!("refused, but not for the shadow: {e}"),
    }
    scratch.teardown().await;
}

/// **Setting a read-only session up resolves no name through the user's
/// path.** The statement that puts `pg_catalog` first runs while the path it
/// is correcting is still in force, so any function or operator it calls by a
/// bare name is the user's to supply. A `public.set_config` there ran in place
/// of the builtin, kept `public` first, and turned the read-only default off,
/// so the next statement — the user's — wrote. PostgreSQL only.
pub async fn a_read_only_session_calls_no_users_function_while_setting_itself_up(
    target: &'static Target,
) {
    if target.engine != schemaic_db::Engine::Postgres {
        return;
    }
    let (scratch, t) = seeded(target, "enforce_setup_unshadowed").await;
    scratch
        .exec(
            "CREATE FUNCTION public.set_config(text, text, boolean) RETURNS text \
             LANGUAGE plpgsql AS $$ BEGIN \
             PERFORM pg_catalog.set_config('default_transaction_read_only', 'off', false); \
             RETURN pg_catalog.current_setting('search_path'); END $$",
        )
        .await;
    let db = scratch
        .db
        .clone()
        .with_init_sql("SET search_path TO public, pg_catalog");
    let shadowed = db
        .fetch_query(
            Some(&scratch.database),
            "SELECT set_config('application_name', 'x', false)",
            1,
            CancellationToken::new(),
        )
        .await
        .unwrap_or_else(|e| panic!("the unenforced call: {e}"));
    assert_ne!(
        shadowed.cell(0, 0).expect("a value").display().to_string(),
        "x",
        "the fixture does not shadow"
    );
    let got = db
        .fetch_query_enforced(
            Some(&scratch.database),
            &format!("DELETE FROM {t}"),
            100,
            CancellationToken::new(),
            Enforce::ReadOnly,
        )
        .await
        .map(|_| ())
        .map_err(|e| e.to_string());
    assert!(got.is_err(), "a read-only session wrote");
    assert_eq!(rows_left(&scratch, &t).await, "3");
    scratch.teardown().await;
}

/// What each way into a read-only PostgreSQL session answers for one
/// database: a plain read, a batch, a plan, an Analyze and a pinned session, each opened
/// enforced. `Ok(())` where it ran, and the error where it did not.
async fn read_only_entries(scratch: &Scratch) -> Vec<(&'static str, Result<(), DbError>)> {
    use schemaic_db::Session;
    let database = Some(scratch.database.as_str());
    let mut out = Vec::new();
    let read = scratch
        .db
        .fetch_query_enforced(
            database,
            "SELECT 1",
            1,
            CancellationToken::new(),
            Enforce::ReadOnly,
        )
        .await
        .map(|_| ());
    out.push(("read", read));
    let mut batch = Vec::new();
    scratch
        .db
        .run_batch_enforced(
            database,
            &["SELECT 1".to_string()],
            1,
            CancellationToken::new(),
            |_, r| batch.push(r.map(|_| ())),
            Some(Enforce::ReadOnly),
        )
        .await;
    out.push(("batch", batch.remove(0)));
    let plan = scratch
        .db
        .explain(database, "SELECT 1", false, true, CancellationToken::new())
        .await
        .map(|_| ());
    out.push(("plan", plan));
    // The read-only Analyze sets its session up apart from `fetch_query`, which
    // the plain plan above returns through (release review S1-L6-05).
    let analyze = scratch
        .db
        .explain(database, "SELECT 1", true, true, CancellationToken::new())
        .await
        .map(|_| ());
    out.push(("analyze", analyze));
    let pinned = match Session::open_enforced(&scratch.db, database, Some(Enforce::ReadOnly)).await
    {
        Ok(s) => {
            s.close().await;
            Ok(())
        }
        Err(e) => Err(e),
    };
    out.push(("pinned", pinned));
    out
}

/// **A read-only PostgreSQL session refuses to run on a path where a
/// builtin's name is shadowed** (pentest round 3, V3-1). The read-only gate
/// allowlists functions by name, and PostgreSQL picks the overload that
/// matches the call's argument types best across the whole path — so a
/// `public.lower(varchar)` answers `lower(a_varchar)` though `pg_catalog`
/// comes first, and the gate's yes was about a function the database's owner
/// wrote. Every entry refuses, before the statement is sent, naming it.
/// PostgreSQL only.
pub async fn a_read_only_session_refuses_a_path_that_shadows_a_builtin(target: &'static Target) {
    if target.engine != schemaic_db::Engine::Postgres {
        return;
    }
    let scratch = Scratch::create(target, "enforce_shadowed_function").await;
    scratch
        .exec(
            "CREATE FUNCTION public.lower(varchar) RETURNS text LANGUAGE sql \
             AS $$ SELECT 'shadowed'::text $$",
        )
        .await;
    // The premise: the plant wins even with the catalogue first on the path.
    let premise = scratch
        .db
        .clone()
        .with_init_sql("SET search_path TO pg_catalog, public")
        .fetch_query(
            Some(&scratch.database),
            "SELECT lower('A'::varchar)",
            1,
            CancellationToken::new(),
        )
        .await
        .unwrap_or_else(|e| panic!("the premise: {e}"));
    assert_eq!(
        premise.cell(0, 0).expect("a value").display().to_string(),
        "shadowed",
        "the fixture does not shadow"
    );
    for (entry, got) in read_only_entries(&scratch).await {
        match got {
            Err(DbError::Refused(DbRefusal::ShadowedBuiltin {
                schema,
                name,
                operator,
            })) => assert_eq!(
                (schema.as_str(), name.as_str(), operator),
                ("public", "lower", false),
                "{entry}"
            ),
            Ok(()) => panic!("{entry}: a read-only session ran on a shadowing path"),
            Err(e) => panic!("{entry}: refused, but not for the shadow: {e}"),
        }
    }
    scratch.teardown().await;
}

/// **A read the app wrote runs beside a shadowed builtin** (release review
/// S1-L1-02). The shadow refusal exists so that the text gate's allowlist
/// means the builtin; a dump's, the Live Monitor's or a sample's `SELECT *`
/// has no gate and calls no function by name, and refusing it took `schemaic
/// dump` and the monitor away from every PostgreSQL database with the common
/// `public.round(double precision, integer)` add-on. Its session still
/// refuses a write, on every engine. PostgreSQL's half only.
pub async fn a_generated_read_runs_beside_a_shadowed_builtin(target: &'static Target) {
    if target.engine != schemaic_db::Engine::Postgres {
        return;
    }
    let (scratch, _t) = seeded(target, "enforce_generated_shadow").await;
    scratch
        .exec(
            "CREATE FUNCTION public.round(double precision, integer) RETURNS numeric \
             LANGUAGE sql AS $$ SELECT pg_catalog.round($1::numeric, $2) $$",
        )
        .await;
    let ns = scratch.namespace_ref();
    let monitored = scratch
        .db
        .fetch_table(
            &scratch.database,
            ns.schema.as_deref(),
            "keep",
            None,
            10,
            CancellationToken::new(),
        )
        .await
        .unwrap_or_else(|e| panic!("the monitor's read was refused: {e}"));
    assert_eq!(monitored.row_count(), 3);
    // The dump's and the copy's read.
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let streamed = scratch
        .db
        .stream_query_enforced(
            Some(&scratch.database),
            &format!("SELECT * FROM {}", scratch.qualified("keep")),
            100,
            CancellationToken::new(),
            tx,
            Some(Enforce::Generated),
        )
        .await;
    let _ = drain.await;
    assert_eq!(
        streamed.unwrap_or_else(|e| panic!("the dump's read was refused: {e}")),
        3
    );
    for (entry, got) in read_only_entries(&scratch).await {
        assert!(
            matches!(
                got,
                Err(DbError::Refused(DbRefusal::ShadowedBuiltin { .. }))
            ),
            "{entry}: the gated paths keep the refusal: {got:?}"
        );
    }
    scratch.teardown().await;
}

/// [`a_read_only_session_refuses_a_path_that_shadows_a_builtin`] for an
/// operator: **any** operator outside `pg_catalog` that no extension owns,
/// whatever its name, since the text gate does not read operators at all and
/// an exact-signature one on a path that puts its schema first replaces the
/// builtin outright. PostgreSQL only.
pub async fn a_read_only_session_refuses_a_path_with_a_users_operator(target: &'static Target) {
    if target.engine != schemaic_db::Engine::Postgres {
        return;
    }
    let scratch = Scratch::create(target, "enforce_shadowed_operator").await;
    scratch
        .exec(
            "CREATE FUNCTION public.always(integer, integer) RETURNS boolean \
             LANGUAGE sql AS $$ SELECT true $$",
        )
        .await;
    scratch
        .exec(
            "CREATE OPERATOR public.=== \
             (LEFTARG = integer, RIGHTARG = integer, FUNCTION = public.always)",
        )
        .await;
    for (entry, got) in read_only_entries(&scratch).await {
        match got {
            Err(DbError::Refused(DbRefusal::ShadowedBuiltin {
                schema,
                name,
                operator,
            })) => assert_eq!(
                (schema.as_str(), name.as_str(), operator),
                ("public", "===", true),
                "{entry}"
            ),
            Ok(()) => panic!("{entry}: a read-only session ran beside a user's operator"),
            Err(e) => panic!("{entry}: refused, but not for the operator: {e}"),
        }
    }
    scratch.teardown().await;
}

/// **An extension's functions and operators are no shadow.** `citext`
/// installs `lower(citext)` — an allowlisted name — and a dozen operators into
/// `public`, which is where most databases have it; refusing them would refuse
/// every read-only session on those databases for something the database's
/// owner did not write. Skipped, saying so, where the server has no `citext`.
/// PostgreSQL only.
pub async fn a_read_only_session_runs_beside_an_extensions_functions(target: &'static Target) {
    if target.engine != schemaic_db::Engine::Postgres {
        return;
    }
    let scratch = Scratch::create(target, "enforce_extension_member").await;
    let available = scratch
        .exec("SELECT count(*) FROM pg_available_extensions WHERE name = 'citext'")
        .await;
    if available.cell(0, 0).expect("a count").display() == "0" {
        eprintln!(
            "{}: no citext on this server; the control is skipped",
            target.name
        );
        scratch.teardown().await;
        return;
    }
    scratch.exec("CREATE EXTENSION citext SCHEMA public").await;
    for (entry, got) in read_only_entries(&scratch).await {
        got.unwrap_or_else(|e| panic!("{entry}: refused beside citext: {e}"));
    }
    scratch.teardown().await;
}

/// **What a database's owner added to an extension is no extension's.** A
/// non-superuser who owns a database can install a trusted extension such as
/// `citext` and then `ALTER EXTENSION citext ADD FUNCTION` a function of their
/// own — measured on PostgreSQL 16 — which gives it the `pg_depend` row that
/// marks a member. The members the extension's script installs are owned by a
/// superuser whoever installed it, and only those are spared. The fixture
/// builds that end state as this suite's superuser: the plant is handed to a
/// role that is not one, then added. Skipped, saying so, where the server has
/// no `citext` or the suite's role cannot create a role. PostgreSQL only.
pub async fn a_read_only_session_refuses_what_an_owner_added_to_an_extension(
    target: &'static Target,
) {
    if target.engine != schemaic_db::Engine::Postgres {
        return;
    }
    const OWNER: &str = "schemaic_it_extension_owner";
    let scratch = Scratch::create(target, "enforce_extension_addition").await;
    let able = scratch
        .exec(
            "SELECT (SELECT count(*) FROM pg_available_extensions WHERE name = 'citext')::text \
             || (SELECT (rolsuper OR rolcreaterole)::text FROM pg_roles \
                 WHERE rolname = current_user)",
        )
        .await;
    if able.cell(0, 0).expect("a value").display() != "1true" {
        eprintln!(
            "{}: no citext, or no right to create a role; the case is skipped",
            target.name
        );
        scratch.teardown().await;
        return;
    }
    scratch
        .exec(&format!(
            "DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{OWNER}') \
             THEN CREATE ROLE {OWNER} NOLOGIN; END IF; END $$"
        ))
        .await;
    scratch.exec("CREATE EXTENSION citext SCHEMA public").await;
    scratch
        .exec(
            "CREATE FUNCTION public.lower(varchar) RETURNS text LANGUAGE sql \
             AS $$ SELECT 'planted'::text $$",
        )
        .await;
    scratch
        .exec(&format!(
            "ALTER FUNCTION public.lower(varchar) OWNER TO {OWNER}"
        ))
        .await;
    scratch
        .exec("ALTER EXTENSION citext ADD FUNCTION public.lower(varchar)")
        .await;
    for (entry, got) in read_only_entries(&scratch).await {
        match got {
            Err(DbError::Refused(DbRefusal::ShadowedBuiltin {
                schema,
                name,
                operator,
            })) => assert_eq!(
                (schema.as_str(), name.as_str(), operator),
                ("public", "lower", false),
                "{entry}"
            ),
            Ok(()) => panic!("{entry}: a read-only session trusted the owner's addition"),
            Err(e) => panic!("{entry}: refused, but not for the addition: {e}"),
        }
    }
    let base = scratch.db.clone();
    scratch.teardown().await;
    base.fetch_query(
        None,
        &format!("DROP ROLE IF EXISTS {OWNER}"),
        1,
        CancellationToken::new(),
    )
    .await
    .unwrap_or_else(|e| panic!("dropping {OWNER}: {e}"));
}
