//! MySQL/MariaDB backend, built on [`mysql_async`].
//!
//! Dispatched to from [`crate::Db`]'s public methods when the connection's
//! engine is [`crate::Engine::MySql`], the same way [`crate::pg`] and
//! [`crate::sqlite`] are — free `pub(crate)` functions named for the method that
//! calls them, not a trait, because the signatures genuinely differ per engine.
//!
//! **This module exists to end an asymmetry, not to shorten a file.** For most
//! of the crate's life PostgreSQL and SQLite had modules and MySQL's bodies sat
//! inline in `lib.rs`, so `pg.rs` and `sqlite.rs` were peers of each other and
//! of nothing else, and every reader had to be told so before reading the
//! dispatcher. The two tests that hold the convention —
//! `every_engine_module_answers_the_whole_interface` and
//! `the_dispatcher_calls_every_engine_module_for_every_entry_point` — could only
//! ever check two engines out of three for the same reason, and the engine that
//! ships most was the one they could not check. Both check this module now, and
//! the first thing they did was find three entry points it was missing.
//!
//! **Organised by family, not by layer**: each group of functions is preceded by
//! the entry points that reach it, so the users queries sit under
//! `fetch_principals`, the wire decoders under `fetch_query`, and the statement
//! builders under `commit_writes`. The alternative — every `pub(crate)` door at
//! the top and every helper below — reads well in a table of contents and badly
//! at the point where somebody is trying to find out what `collect_schema` is
//! for.
//!
//! **What stays in `lib.rs` is what more than one engine reads**, which is the
//! rule for what may move here at all: `assemble_schema`, `ColRow`, `IdxRow`,
//! `FkColRow`, `TxScope`, `DdlError`, `lock_wait_sql` and the `NumKind` /
//! `num_kind` / `parse_as` / `parse_typed` family are all called from `pg.rs`
//! despite their MySQL-flavoured vocabulary, and moving any of them here would
//! make PostgreSQL depend on a module named for another engine. `ident_sqlite`
//! is the mirror-image trap: it sits next to MySQL's own `ident` in `lib.rs` and
//! belongs to neither this module nor that neighbour.

use std::collections::HashMap;

use futures_util::StreamExt;
use mysql_async::consts::{ColumnFlags, ColumnType};
use mysql_async::prelude::Queryable;
use mysql_async::{Column as MyColumn, Conn, Row, Value as MyValue};
use schemaic_core::activity::{self, KillKind, SessionInfo};
use schemaic_core::blob::{BlobRef, BlobValue, FETCH_CAP};
use schemaic_core::intel::SqlDialect;
use schemaic_core::model::{
    CellEdit, Column, ColumnFlags as CoreColFlags, ColumnOrigin, GridWrite, RefetchRow,
    RefetchTemplate, ResultBuilder, ResultSet, Rollback, RowDelete, RowEdit, RowInsert, Value,
    WriteStep, binary_display, one_row_verdict,
};
use schemaic_core::schema::{
    CheckInfo, ColumnInfo, DbSchema, EventInfo, EventSchedule, EventSource, EventStatus,
    RoutineInfo, TableInfo, TriggerAction, TriggerEvent, TriggerInfo, TriggerOrder, TriggerSource,
    TriggerTiming, ViewOptions, event_interval_expr, event_time_expr,
};
use schemaic_core::stats::{Freshness, IndexStats, SchemaStats, TableStats};
use schemaic_core::users::{self, Grants, MyUserRow, Principal};
use schemaic_core::{export, sql};
use tokio_util::sync::CancellationToken;

use mysql_async::Params;

use crate::{
    ColRow, Db, DbError, DdlError, EXPLAIN_ROW_CAP, FkColRow, IdxRow, ImportTarget, NumKind,
    RowDest, RowSource, TxScope, assemble_schema, lock_wait_sql, next_batch_off_executor, num_kind,
    order_by_clause, parse_as, parse_typed, pg,
};

/// The binary collation id (`binary`) — a column with this charset holds raw
/// bytes (BLOB/BINARY/VARBINARY) rather than text.
const BINARY_CHARSET: u16 = 63;

/// The accounts this server knows.
///
/// The dispatcher has already refused a dialect without accounts, so the
/// capability question is not asked twice here — see [`crate::Db::fetch_principals`].
pub(crate) async fn fetch_principals(db: &Db) -> Result<users::Principals, DbError> {
    let mut conn = db.open(None, false).await?;
    let out = collect_my_users(&mut conn).await;
    let _ = conn.disconnect().await;
    out
}

/// What one account is allowed to do, as `GRANT` statements.
///
/// **No `database` parameter, unlike [`crate::pg::fetch_grants`]**, and that is
/// the asymmetry the engine modules exist to carry rather than hide: MySQL's
/// grant tables are server-wide and answer for every database at once, so a
/// parameter here would be one this function is documented to ignore.
pub(crate) async fn fetch_grants(db: &Db, principal: &Principal) -> Result<Grants, DbError> {
    // `SHOW GRANTS FOR` takes an account, not a placeholder, so the pair goes
    // in as SQL — through `users::account_sql`, which is the one literal
    // quoting and the reason a host of `it's` cannot end the statement early.
    let sql = format!(
        "SHOW GRANTS FOR {}",
        users::account_sql(principal, SqlDialect::MySql)
    );
    let mut conn = db.open(None, false).await?;
    let out = conn
        .query_map(sql, |g: String| {
            // **Every statement out of here is redacted**, at the boundary
            // rather than in the view: MariaDB answers with the account's
            // password hash inline, and a second reader of this method — the
            // grant/revoke step, a copy-all button — would otherwise have to
            // remember to do it too.
            users::redact_secrets(&g, SqlDialect::MySql)
        })
        .await
        .map_err(|e| DbError::Query(e.to_string()))
        // **A note, not `None`.** `SHOW GRANTS` is direct-only on both
        // servers, so everything the account holds through a granted role is
        // absent from this list — and on a role-provisioned server that is
        // most of it. See `users::my_scope_note`.
        .map(|statements| Grants {
            statements,
            note: Some(users::my_scope_note()),
        });
    let _ = conn.disconnect().await;
    out
}

/// `mysql.user`, MariaDB's spelling. `is_role` is the flag that makes a role a
/// role, and only MariaDB has it.
const MY_USERS_MARIADB_SQL: &str = "SELECT CAST(User AS CHAR), CAST(Host AS CHAR), \
            CAST(plugin AS CHAR), CAST(password_expired AS CHAR), CAST(is_role AS CHAR) \
     FROM mysql.user ORDER BY User, Host";

/// The same, MySQL 8's spelling — `account_locked` in place of `is_role`, which
/// does not exist there, plus the **presence** of a stored credential.
///
/// **The sixth column is a `> 0`, never the hash.** `authentication_string`
/// holds a password hash, which for the older plugins is credential-equivalent
/// and has no business crossing into this process — so the comparison is done
/// on the server and what comes back is a `'Y'`/`'N'`. It is here because it is
/// the third of the three flags MySQL's own `CREATE ROLE` sets, and the one that
/// separates a role from a locked, password-expired *user*; see
/// `users::MyUserRow::has_credential` and the offer it withholds.
///
/// MariaDB's query does not need it — `is_role` answers there outright — so the
/// column is asked for only on the rung that has no better answer.
const MY_USERS_MYSQL_SQL: &str = "SELECT CAST(User AS CHAR), CAST(Host AS CHAR), \
            CAST(plugin AS CHAR), CAST(password_expired AS CHAR), CAST(account_locked AS CHAR), \
            IF(LENGTH(authentication_string) > 0, 'Y', 'N') \
     FROM mysql.user ORDER BY User, Host";

/// **`is_role` on its own**, for a MariaDB that has roles but not password
/// expiry — every 10.1, 10.2 and 10.3, which is a window still in service.
///
/// Without this rung such a server fell through to the pair below, where
/// `is_role` is `None` on every row, so `from_mysql_rows` folded each role into
/// a `User` and kept the host MariaDB stores as `''`. A role `readers` then
/// listed as `readers@` and three of the four actions built from it are errors
/// the model already documents: `SHOW GRANTS FOR 'readers'@''` is 1141,
/// `GRANT … TO 'readers'@''` is 1133, and `DROP ROLE` has no `@host` grammar at
/// all. Role-ness is the one column whose absence changes what a statement
/// *is*, so it gets a rung of its own rather than sharing the pair's.
const MY_USERS_ROLE_SQL: &str = "SELECT CAST(User AS CHAR), CAST(Host AS CHAR), CAST(is_role AS CHAR) \
     FROM mysql.user ORDER BY User, Host";

/// The pair alone, for a server that has none of the extra columns — and for the
/// version of `mysql.user` a future release trims again.
const MY_USERS_PLAIN_SQL: &str =
    "SELECT CAST(User AS CHAR), CAST(Host AS CHAR) FROM mysql.user ORDER BY User, Host";

/// The last resort, and the one an *application* account can actually read.
///
/// `mysql.user` needs `SELECT` on the `mysql` database. A connection that hasn't
/// got it — which is every properly-provisioned application account — can still
/// read `information_schema.USER_PRIVILEGES`, where it sees its own row. One
/// account is a poor list, but it is a true one, and it is the account whose
/// grants the person opening this is most likely asking about.
const MY_USERS_GRANTEE_SQL: &str =
    "SELECT DISTINCT GRANTEE FROM information_schema.USER_PRIVILEGES ORDER BY GRANTEE";

/// One `mysql.user` row as [`MY_USERS_MARIADB_SQL`] projects it.
type MyUserTuple = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// One `mysql.user` row as [`MY_USERS_MYSQL_SQL`] projects it — the same five
/// columns plus the credential-presence flag that rung alone asks for.
///
/// **A type of its own rather than a sixth `Option` on [`MyUserTuple`].** The
/// two rungs no longer project the same shape, and a shared tuple with a
/// trailing `None` would compile for either query while meaning something
/// different in each — which is the mistake
/// `the_fifth_column_lands_in_the_field_this_servers_spelling_meant` exists
/// because of.
type MyUserTupleMysql = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// The MySQL/MariaDB half of [`Db::fetch_principals`]: five queries, of which
/// exactly one runs to completion.
///
/// **The fallbacks fire on an *error*, not on an empty result** — the same rule
/// the lock-wait pair in [`collect_sessions`] had to learn. `mysql.user` is never
/// legitimately empty (the server cannot run without accounts), so an empty
/// answer would mean the query was denied, and an error is what actually says
/// so. Trying MariaDB's column first and MySQL's second costs one failed
/// round-trip on MySQL and none on MariaDB, and there is no version probe that
/// would be cheaper: `SELECT @@version` is itself a round-trip, and it answers
/// the wrong question — what matters is which columns this build of `mysql.user`
/// has, not what it calls itself.
async fn collect_my_users(conn: &mut Conn) -> Result<users::Principals, DbError> {
    if let Ok(rows) = conn
        .query_map(MY_USERS_MARIADB_SQL, |r: MyUserTuple| r)
        .await
    {
        return Ok(users::Principals::complete(users::from_mysql_rows(
            &my_mariadb_rows(rows),
        )));
    }
    if let Ok(rows) = conn
        .query_map(MY_USERS_MYSQL_SQL, |r: MyUserTupleMysql| r)
        .await
    {
        return Ok(users::Principals::complete(users::from_mysql_rows(
            &my_mysql_rows(rows),
        )));
    }
    if let Ok(rows) = conn
        .query_map(MY_USERS_ROLE_SQL, |r: (String, String, Option<String>)| r)
        .await
    {
        return Ok(users::Principals::complete(users::from_mysql_rows(
            &my_role_rows(rows),
        )));
    }
    if let Ok(rows) = conn
        .query_map(MY_USERS_PLAIN_SQL, |(u, h): (String, String)| MyUserRow {
            user: u,
            host: h,
            ..Default::default()
        })
        .await
    {
        return Ok(users::Principals::complete(users::from_mysql_rows(&rows)));
    }
    // The one whose failure the caller sees, because by here every wider read
    // has already been refused and this error is the reason why.
    let grantees: Vec<String> = conn
        .query_map(MY_USERS_GRANTEE_SQL, |g: String| g)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;
    let rows: Vec<MyUserRow> = grantees
        .iter()
        // A cell that isn't an account pair is dropped rather than guessed at —
        // see `users::parse_grantee`.
        .filter_map(|g| users::parse_grantee(g))
        .map(|(user, host)| MyUserRow {
            user,
            host,
            ..Default::default()
        })
        .collect();
    // **Reaching here is news, and the list has to say so.** Every wider read
    // was refused, so what follows is this connection's own row and nothing
    // else — which renders identically to a server that genuinely has one
    // account. The three `if let Ok` above discard their errors deliberately
    // (a denied read is the expected case, not a failure), and that is exactly
    // why the *absence* has to be carried out rather than inferred from a count.
    Ok(users::Principals {
        list: users::from_mysql_rows(&rows),
        note: Some(users::my_own_account_only_note()),
    })
}

/// [`MY_USERS_MARIADB_SQL`]'s five columns, as rows — the fifth being `is_role`.
///
/// **One function per rung rather than one with a `mariadb: bool`.** The fifth
/// column means a different thing in each query, and a pair of boolean literals
/// twelve lines apart deciding which was how transposing them could have made
/// every locked MySQL account a `Role`, dropped its host, and left
/// `DROP USER "app"` resolving to a different account. Now the two projections
/// have different arities and the compiler holds them apart; the fold that reads
/// them — `users::from_mysql_rows` — is still the one place that decides what a
/// [`Principal`] *says*.
fn my_mariadb_rows(rows: Vec<MyUserTuple>) -> Vec<MyUserRow> {
    rows.into_iter()
        .map(|(user, host, plugin, expired, is_role)| MyUserRow {
            user,
            host,
            plugin,
            password_expired: expired,
            is_role,
            account_locked: None,
            // MariaDB's rung does not ask for it, and `None` says so rather
            // than claiming the row has no credential — see `MyUserRow`.
            has_credential: None,
        })
        .collect()
}

/// [`MY_USERS_MYSQL_SQL`]'s six columns, as rows — no `is_role` to be had, and
/// the credential flag that stands in for the part of it MySQL withholds.
fn my_mysql_rows(rows: Vec<MyUserTupleMysql>) -> Vec<MyUserRow> {
    rows.into_iter()
        .map(
            |(user, host, plugin, expired, locked, credential)| MyUserRow {
                user,
                host,
                plugin,
                password_expired: expired,
                is_role: None,
                account_locked: locked,
                has_credential: credential,
            },
        )
        .collect()
}

/// [`MY_USERS_ROLE_SQL`]'s three columns, as rows.
///
/// A named function rather than a closure inside the ladder, for the reason
/// [`my_mariadb_rows`] and [`my_mysql_rows`] are: the mapping is the whole
/// content of a rung, and a rung whose mapping is inline is a rung no test can
/// reach.
fn my_role_rows(rows: Vec<(String, String, Option<String>)>) -> Vec<MyUserRow> {
    rows.into_iter()
        .map(|(user, host, is_role)| MyUserRow {
            user,
            host,
            is_role,
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod my_user_tests {
    use super::{MY_USERS_MYSQL_SQL, MyUserRow, my_mariadb_rows, my_mysql_rows, my_role_rows};

    /// **Which column the fifth one is.** It used to be two bare boolean
    /// literals twelve lines apart, with nothing in any tier asserting the
    /// result: transposing them makes every locked MySQL account a `Role`, drops
    /// its host, and `DROP USER "app"` then resolves to a *different* account.
    /// The live role test finds its role by name and never asks what kind it is.
    #[test]
    fn the_fifth_column_lands_in_the_field_this_servers_spelling_meant() {
        // MariaDB's fifth column is `is_role`…
        let maria = my_mariadb_rows(vec![(
            "app".to_string(),
            "%".to_string(),
            Some("plugin".to_string()),
            Some("N".to_string()),
            Some("Y".to_string()),
        )]);
        assert_eq!(maria[0].is_role.as_deref(), Some("Y"));
        assert_eq!(maria[0].account_locked, None);
        // …and MySQL 8's is `account_locked`, which does not make a role.
        let mysql = my_mysql_rows(vec![(
            "app".to_string(),
            "%".to_string(),
            Some("plugin".to_string()),
            Some("N".to_string()),
            Some("Y".to_string()),
            Some("Y".to_string()),
        )]);
        assert_eq!(mysql[0].is_role, None);
        assert_eq!(mysql[0].account_locked.as_deref(), Some("Y"));
        // The other four columns are the same either way.
        assert_eq!(maria[0].user, mysql[0].user);
        assert_eq!(maria[0].host, mysql[0].host);
        assert_eq!(maria[0].plugin, mysql[0].plugin);
        assert_eq!(maria[0].password_expired, mysql[0].password_expired);
    }

    /// **The sixth column reaches `has_credential`, and only that rung has it.**
    /// The flag is what withholds **Reset password** from a MySQL 8 row that may
    /// be a role, so a mapping that dropped it would put the offer back with
    /// `core`'s own tests still green — the fold cannot tell "not published"
    /// from "no credential" if the projection never fills it in.
    #[test]
    fn the_credential_flag_reaches_the_fold_only_on_the_rung_that_asks_for_it() {
        let row = |credential: &str| {
            my_mysql_rows(vec![(
                "zz".to_string(),
                "%".to_string(),
                None,
                Some("Y".to_string()),
                Some("Y".to_string()),
                Some(credential.to_string()),
            )])
        };
        assert_eq!(row("N")[0].has_credential.as_deref(), Some("N"));
        assert_eq!(row("Y")[0].has_credential.as_deref(), Some("Y"));
        // …and that is the difference between an offer and a refusal.
        let role = schemaic_core::users::from_mysql_rows(&row("N"));
        assert!(role[0].role_ambiguous);
        let user = schemaic_core::users::from_mysql_rows(&row("Y"));
        assert!(!user[0].role_ambiguous);

        // The rungs that do not ask for it leave `None`, which the fold reads as
        // "this server does not publish it" rather than as "no credential".
        let maria = my_mariadb_rows(vec![(
            "zz".to_string(),
            "%".to_string(),
            None,
            Some("Y".to_string()),
            None,
        )]);
        assert_eq!(maria[0].has_credential, None);
    }

    /// The hash itself never crosses the wire into this process: the sixth
    /// column is a server-side `> 0`, and `authentication_string` appears in the
    /// query only inside it. Pinned because "just select the column and compare
    /// here" is the natural next edit and it would put a credential-equivalent
    /// into a `MyUserRow`.
    #[test]
    fn the_credential_column_is_asked_as_a_presence_not_a_value() {
        assert!(MY_USERS_MYSQL_SQL.contains("IF(LENGTH(authentication_string) > 0, 'Y', 'N')"));
        assert_eq!(
            MY_USERS_MYSQL_SQL.matches("authentication_string").count(),
            1,
            "authentication_string is projected somewhere other than the presence test"
        );
    }

    /// The rung that exists so a MariaDB with roles but no password expiry still
    /// knows a role when it sees one. Fed through `from_mysql_rows`, because
    /// what the missing column costs is a *principal*, not a field: a role kept
    /// as a `User` carries MariaDB's empty host into `'readers'@''`, which three
    /// of the four statements reject.
    #[test]
    fn the_role_rung_still_tells_a_role_from_a_user() {
        let rows = my_role_rows(vec![
            ("app".into(), "%".into(), Some("N".into())),
            ("readers".into(), String::new(), Some("Y".into())),
        ]);
        let out = schemaic_core::users::from_mysql_rows(&rows);
        let role = out
            .iter()
            .find(|p| p.name == "readers")
            .expect("the role is listed");
        assert_eq!(role.kind, schemaic_core::users::PrincipalKind::Role);
        // A role has no host, so nothing writes `'readers'@''`.
        assert_eq!(role.host, None);
        assert_eq!(role.display(), "readers");

        let user = out.iter().find(|p| p.name == "app").expect("the user");
        assert_eq!(user.kind, schemaic_core::users::PrincipalKind::User);
        assert_eq!(user.host.as_deref(), Some("%"));

        // And the rung below it — the bare pair — is what this one exists to
        // stop being reached on such a server: with no `is_role` the same role
        // folds into a user with an empty host.
        let bare = schemaic_core::users::from_mysql_rows(&[MyUserRow {
            user: "readers".into(),
            host: String::new(),
            ..Default::default()
        }]);
        assert_eq!(bare[0].kind, schemaic_core::users::PrincipalKind::User);
    }
}

// ── Server Activity and table statistics ─────────────────────────────────────

/// The sessions this server is running.
///
/// **Unbounded here on purpose.** [`crate::Db::fetch_sessions`] wraps the whole
/// dispatch — this arm and PostgreSQL's alike — in `tokio::time::timeout`,
/// because the reason to reach Server Activity at all is usually that the server
/// is not behaving, and a bound written per engine is a bound one engine can be
/// missing. `every_reachability_path_is_bounded_by_a_timeout` is what holds it
/// there rather than here.
pub(crate) async fn fetch_sessions(db: &Db) -> Result<Vec<SessionInfo>, DbError> {
    let mut conn = db.open(None, false).await?;
    let out = collect_sessions(&mut conn).await;
    let _ = conn.disconnect().await;
    out
}

/// Cancel a statement, or terminate a session outright, by server id.
///
/// **A fresh connection, always** — the session being killed may be the one
/// holding up everything else, and on MySQL a `KILL` issued from a connection
/// itself waiting on that lock never gets sent. `db.open` gives one per call,
/// which is the one-connection-per-operation invariant doing the work rather
/// than a comment asking for it.
pub(crate) async fn kill_session(db: &Db, id: i64, kind: KillKind) -> Result<(), DbError> {
    // `id` is an `i64` the server itself reported and is formatted back
    // as a decimal, so there is nothing here a quoter would have to
    // escape.
    let sql = match kind {
        KillKind::Query => format!("KILL QUERY {id}"),
        KillKind::Session => format!("KILL CONNECTION {id}"),
    };
    let mut conn = db.open(None, false).await?;
    let out = conn
        .query_drop(sql)
        .await
        .map_err(|e| DbError::Query(e.to_string()));
    let _ = conn.disconnect().await;
    out
}

/// Best-effort server-side cancel: connect afresh and `KILL QUERY <id>`.
///
/// **The same statement [`kill_session`] writes for [`KillKind::Query`], and
/// deliberately not the same function.** That one answers the Server Activity
/// panel: it is gated on a capability, it takes the `i64` the server reported,
/// and it hands back an error the panel shows. This one is the *cancel* path —
/// it is reached because a statement this app is itself waiting on has to stop,
/// it takes the `u32` the driver reported for its own connection, and a failure
/// is nothing to report, because the caller's `select!` has already given up on
/// the statement either way. They sat in two files until the extraction
/// finished; sitting in one is what lets the next reader see they are the same
/// sentence to the server.
///
/// **A different door, too**: [`Db::open_serverless`] rather than `db.open`,
/// because a `KILL` names no object and a connection that first has to open the
/// user's database is one more thing that can hang on a server already
/// misbehaving.
///
/// **Bounded by [`crate::CANCEL_TIMEOUT`].** The whole reason this is reached is
/// that something is not responding, and it answers that by opening a *fresh*
/// connection — full TCP, a TLS handshake, and on `prefer` possibly a second
/// connect — to a host that may be gone. Unbounded, a Stop against a dead server
/// hangs inside a modal whose every exit maps to that same Stop, and the only
/// way out is killing the process.
pub(crate) async fn kill_query(db: &Db, conn_id: u32) {
    let kill = async {
        if let Ok(mut killer) = db.open_serverless(false).await {
            let _ = killer.query_drop(format!("KILL QUERY {conn_id}")).await;
            let _ = killer.disconnect().await;
        }
    };
    if tokio::time::timeout(crate::CANCEL_TIMEOUT, kill)
        .await
        .is_err()
    {
        tracing::debug!("kill query timed out after {:?}", crate::CANCEL_TIMEOUT);
    }
}

/// Race `fut` against `cancel`, and on a cancel **kill the statement at the
/// server and then await the future** rather than dropping it.
///
/// **This is the one shape a cancel may take on a connection that outlives the
/// statement**, and the rule is stated at length on [`write_on`] and
/// [`crate::Db::import_rows`]: a `tokio::select!` that simply drops a
/// `mysql_async` future leaves the connection's result stream desynchronised,
/// and every reply after that belongs to the statement before it. Every other
/// MySQL cancel arm in this crate is safe only because it *disconnects*
/// immediately afterwards — one connection per operation, so a poisoned
/// connection is thrown away before anyone reads from it again.
///
/// [`crate::session::Session`] is the documented exception to that invariant: it
/// pins one connection for a whole Manual-transaction tab. So it is the one
/// place a desynchronised connection survives, and it did. Measured on both
/// servers, twice each — a tab that ran an `INSERT`, had a long `SELECT`
/// cancelled, then pressed **Commit**:
///
/// - **MySQL 8.4.11**: the `COMMIT` returned `Ok(())`, the app reported success,
///   and a fresh connection saw **0 rows**. Silent data loss under a success
///   report.
/// - **MariaDB 10.11.14**: the `COMMIT` returned error 1317, the app reported
///   failure, and the row **was** committed — leaving Rollback offered over
///   durable data.
///
/// The two reports are the same desynchronisation read through two servers'
/// different replies, which is why neither looked like a protocol fault.
///
/// `let _ = fut.await` is not a wait for the statement to finish its work: the
/// `KILL QUERY` above has already stopped it, so what is awaited is the error
/// reply, and awaiting it is precisely what leaves the stream aligned. A body
/// that runs several statements propagates that error with `?` and stops, which
/// is why [`refetch_on`]'s loop needs nothing of its own.
pub(crate) async fn cancel_awaited<T>(
    fut: impl Future<Output = Result<T, DbError>>,
    db: &Db,
    conn_id: u32,
    cancel: &CancellationToken,
) -> Result<T, DbError> {
    let mut fut = std::pin::pin!(fut);
    let raced = tokio::select! {
        r = fut.as_mut() => Some(r),
        _ = cancel.cancelled() => None,
    };
    match raced {
        Some(r) => r,
        None => {
            kill_query(db, conn_id).await;
            let _ = fut.await;
            Err(DbError::Cancelled)
        }
    }
}

/// Row and size estimates, and index statistics, for one database.
pub(crate) async fn fetch_table_stats(db: &Db, database: &str) -> Result<SchemaStats, DbError> {
    let mut conn = db.open(None, false).await?;
    let out = collect_table_stats(&mut conn, database).await;
    let _ = conn.disconnect().await;
    out
}

/// `information_schema.PROCESSLIST`, minus this connection and minus the
/// server's own internal threads.
///
/// `COMMAND <> 'Daemon'` drops the event scheduler and friends: they are threads,
/// not sessions — nobody connected them, nothing can kill them, and they would
/// sit at the top of the list forever with an uptime-length duration. A
/// replication `Binlog Dump` *is* a real client and stays.
///
/// **Working threads first, then longest-standing** — and the first half of that
/// is load-bearing. Ordering by `TIME` alone reads as "keep the interesting end
/// of the list", but the panel's own attention order
/// ([`schemaic_core::activity::rank`]) puts *blocked* sessions at
/// the top, and a session that started waiting four seconds ago has the smallest
/// `TIME` on the server. On a box holding three thousand pool connections idle
/// for hours, `ORDER BY TIME DESC LIMIT 501` returned five hundred sleepers and
/// cut every row of the lock pile-up the panel was opened for — a quiet-looking
/// list during an incident.
///
/// `COMMAND <> 'Sleep'` is the proxy for "doing something", and it is a proxy on
/// purpose: a blocked thread on MySQL sits in `Query` while it waits, but
/// `PROCESSLIST` itself carries no lock information, and the view that does
/// (`INNODB_TRX`) needs `PROCESS` privileges this statement deliberately does not
/// require — see [`collect_sessions`]. Sorting by what every account can see
/// keeps the required query required.
///
/// **`USER <> 'system user'` drops the server's own threads.** A replica's
/// applier and receiver are threads, not sessions — nobody connected them, they
/// have no host, their `TIME` is the replica's uptime, and terminating one stops
/// replication — but they are not `COMMAND = 'Daemon'` (MariaDB reports
/// `Slave_IO`/`Slave_SQL`, MySQL 8 `Connect`/`Query`) and not `Sleep` either, so
/// they sat at the very *top* of the list forever with a live "Kill session"
/// under them. The account is what both engines have in common for them, and it
/// is what `SHOW PROCESSLIST` readers filter on. `Binlog Dump` still stays: that
/// is the primary side, and it really is a client.
///
/// **`LEFT(INFO, …)`, because `INFO` is the *untruncated* statement** — this
/// module says so twice, contrasting it with `SHOW PROCESSLIST`'s 100
/// characters. `activity::MAX_SESSIONS` bounds the number of rows at 500 and
/// nothing bounded the bytes: 40 sessions each running a generated 2 MB
/// multi-row `INSERT` — the ordinary shape of a bulk loader, and exactly the
/// load someone opens this panel to watch — put ~80 MB of statement text in the
/// panel, allocated fresh on every poll and scanned end to end by
/// `activity::matches_query` on every keystroke in the search box, on the UI
/// thread.
///
/// The row never draws more than `history::PREVIEW_MAX` (2,000 bytes), and
/// PostgreSQL's `pg_stat_activity.query` is truncated by the server at
/// `track_activity_query_size` — 1 KB by default — so this was a MySQL-only cost
/// inside a type whose doc calls itself engine-neutral by construction.
/// [`MY_INFO_MAX`] is the cap and says why that size.
///
/// `ccfdea2` set out to remove this and removed the *allocation* only:
/// `contains_collapsed_ignore_ascii_case` justifies its `O(n·m)` scan as "short
/// needles over a **bounded list**" — the list was bounded, the haystack was
/// not.
/// **The cap is in *bytes*, so it is taken over the binary form.** `INFO` is a
/// character column — `utf8mb3` on both engines (measured: MariaDB 10.11.14
/// reports `longtext`/`utf8mb3`, MySQL 8.4.11 `varchar(65535)`/`utf8mb3`) — and
/// `LEFT(str, n)` returns `n` **characters**. Measured on both:
/// `LENGTH(LEFT(<three CJK characters>, 3))` is `9`, not `3`. So the plain
/// `LEFT(INFO, 65536)` admitted up to 3× [`MY_INFO_MAX`], and the `const`
/// assert that pins the panel's ceiling is written in bytes — 500 rows of
/// non-Latin text against an asserted 64 MiB is ~96 MB, on exactly the
/// bulk-loader load this cap is for.
///
/// `CONVERT(… USING binary)` makes `LEFT` count bytes (measured: the same
/// expression gives `3`); converting back to `utf8mb4` restores a string the
/// driver decodes, and a cut that lands mid-character comes back as the
/// replacement character rather than as invalid UTF-8.
const MY_PROCESSLIST_SQL: &str = "SELECT ID, USER, HOST, DB, COMMAND, TIME, \
     CONVERT(LEFT(CONVERT(INFO USING binary), 65536) USING utf8mb4) \
     FROM information_schema.PROCESSLIST \
     WHERE ID <> CONNECTION_ID() AND COMMAND <> 'Daemon' AND USER <> 'system user' \
     ORDER BY (COMMAND <> 'Sleep') DESC, TIME DESC LIMIT ";

/// How many bytes of a MySQL session's statement the panel reads.
///
/// **32× what the row draws**, which is the balance: `history::preview` shows
/// 2,000 bytes, and *Copy statement* wants more than that for anything a person
/// would actually read back. 64 KiB × `activity::MAX_SESSIONS` caps the panel at
/// 32 MB in the worst case it can now reach, against unbounded before.
///
/// Spelled into [`MY_PROCESSLIST_SQL`] rather than interpolated, because a
/// `const` cannot be `format!`ed into another `const`; a test asserts the two
/// agree.
pub const MY_INFO_MAX: usize = 64 * 1024;

/// Open InnoDB transactions, keyed by the thread holding them. This is what
/// separates an idle pool connection from a client that went away mid-transaction
/// — see [`schemaic_core::activity::mysql_state`].
const MY_INNODB_TRX_SQL: &str =
    "SELECT trx_mysql_thread_id, trx_state FROM information_schema.INNODB_TRX";

/// Who is waiting on whom, MySQL 8 spelling. `performance_schema.data_lock_waits`
/// names transactions, so `INNODB_TRX` maps them back to the thread ids the rest
/// of the panel is keyed by.
const MY_LOCK_WAITS_PS_SQL: &str = "SELECT rt.trx_mysql_thread_id, bt.trx_mysql_thread_id \
     FROM performance_schema.data_lock_waits w \
     JOIN information_schema.INNODB_TRX rt ON rt.trx_id = w.REQUESTING_ENGINE_TRANSACTION_ID \
     JOIN information_schema.INNODB_TRX bt ON bt.trx_id = w.BLOCKING_ENGINE_TRANSACTION_ID";

/// The same graph, MariaDB spelling. MariaDB has no `data_lock_waits` and MySQL 8
/// removed `INNODB_LOCK_WAITS`, so neither statement works on both servers and
/// the pair is tried in turn.
///
/// If *both* fail — an account without `PROCESS`, or a build with InnoDB's lock
/// views compiled out — the panel still knows **who** is blocked (that comes from
/// `trx_state`, above) and simply cannot say by whom. That is the honest
/// degradation: a `Blocked` row with no "waiting on…" note, rather than a list
/// that quietly claims nothing is wrong.
const MY_LOCK_WAITS_IS_SQL: &str = "SELECT rt.trx_mysql_thread_id, bt.trx_mysql_thread_id \
     FROM information_schema.INNODB_LOCK_WAITS w \
     JOIN information_schema.INNODB_TRX rt ON rt.trx_id = w.requesting_trx_id \
     JOIN information_schema.INNODB_TRX bt ON bt.trx_id = w.blocking_trx_id";

/// One `PROCESSLIST` row as `mysql_async` hands it back:
/// `(id, user, host, db, command, time, info)`. Reshaped into
/// [`activity::MyProcessRow`] for the fold.
type MyProcessRow = (
    i64,
    String,
    String,
    Option<String>,
    String,
    i64,
    Option<String>,
);

/// Run the three activity queries on one connection and fold them into
/// [`SessionInfo`]s.
///
/// The process list is required — without it there is no panel — while the
/// transaction and lock-wait views are best effort, because they are the two that
/// need `PROCESS` privileges and differ by server. A user who can see their own
/// sessions and nothing else still gets a working panel.
async fn collect_sessions(conn: &mut Conn) -> Result<Vec<SessionInfo>, DbError> {
    let list_sql = format!("{MY_PROCESSLIST_SQL}{}", activity::MAX_SESSIONS + 1);
    let rows: Vec<MyProcessRow> = conn
        .query_map(list_sql, |r: MyProcessRow| r)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

    let trx: HashMap<i64, String> = conn
        .query_map(MY_INNODB_TRX_SQL, |(id, state): (i64, String)| (id, state))
        .await
        .map(|v| v.into_iter().collect())
        .unwrap_or_default();

    // MySQL 8 first, MariaDB second — only one of them exists on any given
    // server.
    //
    // **The fallback fires on an *error*, not on an empty result**, which is the
    // condition it actually means. `waits.is_empty()` could not tell "the
    // performance_schema view found no waits" — the ordinary case, since most
    // polls find none — from "the view does not exist", so on MySQL 8 every
    // quiet poll went on to run `information_schema.INNODB_LOCK_WAITS`, which
    // 8.0 removed, and paid a guaranteed round-trip failure forever.
    //
    // **And it is not reached at all unless a transaction is actually waiting.**
    // That fix removed the wasted round-trip for MySQL 8 by putting its spelling
    // first; it could not remove it for MariaDB, where the first statement can
    // never succeed — `MY_LOCK_WAITS_IS_SQL`'s doc says outright that neither
    // statement works on both servers, so with a fixed order exactly one engine
    // always pays. Measured on 10.11: ERROR 1146, every poll, 1,800 an hour at
    // the two-second interval. `trx` is `INNODB_TRX`, which is where a waiter
    // announces itself, so `wait_graph_is_worth_fetching` answers from a table
    // already in hand and the quiet poll — nearly every poll, on either engine —
    // now asks nothing.
    let waits: Vec<(i64, i64)> =
        if activity::wait_graph_is_worth_fetching(trx.values().map(String::as_str)) {
            match conn
                .query_map(MY_LOCK_WAITS_PS_SQL, |r: (i64, i64)| r)
                .await
            {
                Ok(v) => v,
                Err(_) => conn
                    .query_map(MY_LOCK_WAITS_IS_SQL, |r: (i64, i64)| r)
                    .await
                    .unwrap_or_default(),
            }
        } else {
            Vec::new()
        };
    // The fold is `activity::from_mysql_rows` — it is where every decision about
    // what a `SessionInfo` *says* lives, and it needs to be reachable from a
    // test with a literal row vector.
    let rows: Vec<activity::MyProcessRow> = rows
        .into_iter()
        .map(
            |(id, user, host, database, command, seconds, info)| activity::MyProcessRow {
                id,
                user,
                host,
                database,
                command,
                seconds,
                info,
            },
        )
        .collect();
    Ok(activity::from_mysql_rows(&rows, &trx, &waits))
}

/// One `information_schema.TABLES` statistics row. Every figure is nullable —
/// a view has none of them, and `AUTO_INCREMENT` is null on a table without one.
type MyStatRow = (
    String,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Sizes and estimates. `CAST(… AS CHAR)` on the timestamps because these are
/// shown, not computed with, and the server's own rendering is the one the user
/// would see in a client.
const MY_TABLE_STATS_SQL: &str = "SELECT CAST(TABLE_NAME AS CHAR), TABLE_ROWS, DATA_LENGTH, \
            INDEX_LENGTH, DATA_FREE, AUTO_INCREMENT, CAST(ROW_FORMAT AS CHAR), \
            CAST(ENGINE AS CHAR), CAST(CREATE_TIME AS CHAR), CAST(UPDATE_TIME AS CHAR) \
     FROM information_schema.TABLES \
     WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME";

/// Cardinality per index. `information_schema.STATISTICS` has one row per key
/// *position*, each carrying the cardinality of the prefix ending there, so the
/// index's own figure is the last one — `MAX` over the group. Reading a row
/// instead would report the **first** column's distinct count as the whole
/// index's, which on a `(status, created_at)` index is a handful against
/// millions. `NON_UNIQUE` is constant within a group; `MIN` just picks it out.
const MY_INDEX_CARDINALITY_SQL: &str = "SELECT CAST(TABLE_NAME AS CHAR), CAST(INDEX_NAME AS CHAR), \
            MAX(CARDINALITY), MIN(NON_UNIQUE) \
     FROM information_schema.STATISTICS \
     WHERE TABLE_SCHEMA = ? \
     GROUP BY TABLE_NAME, INDEX_NAME \
     ORDER BY TABLE_NAME, INDEX_NAME";

/// How often each index has actually been used. Performance Schema is the only
/// place MySQL keeps this, and it is routinely off, not instrumented, or not
/// granted — all of which must leave the scan count **absent** rather than zero,
/// because zero is what marks an index unused. So a failure here drops the whole
/// map and every index reports "we don't know".
///
/// `INDEX_NAME IS NOT NULL` because the same view carries a row for the table
/// itself, which is not an index and would otherwise be counted as one.
const MY_INDEX_USAGE_SQL: &str = "SELECT CAST(OBJECT_NAME AS CHAR), CAST(INDEX_NAME AS CHAR), COUNT_STAR \
     FROM performance_schema.table_io_waits_summary_by_index_usage \
     WHERE OBJECT_SCHEMA = ? AND INDEX_NAME IS NOT NULL";

/// The MySQL/MariaDB half of [`Db::fetch_table_stats`]: three queries, only the
/// first of which is required. The rows become a [`SchemaStats`] in
/// [`map_mysql_stats`], which is where the decisions are and is therefore where
/// the tests are.
async fn collect_table_stats(conn: &mut Conn, database: &str) -> Result<SchemaStats, DbError> {
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());

    let rows: Vec<MyStatRow> = conn
        .exec_map(MY_TABLE_STATS_SQL, (database,), |r: MyStatRow| r)
        .await
        .map_err(qerr)?;

    let idx_rows: Vec<(String, String, Option<u64>, Option<i64>)> = conn
        .exec_map(
            MY_INDEX_CARDINALITY_SQL,
            (database,),
            |r: (String, String, Option<u64>, Option<i64>)| r,
        )
        .await
        .map_err(qerr)?;

    let usage: HashMap<(String, String), u64> = conn
        .exec_map(
            MY_INDEX_USAGE_SQL,
            (database,),
            |(t, i, n): (String, String, u64)| ((t, i), n),
        )
        .await
        .map(|v| v.into_iter().collect())
        .unwrap_or_default();

    // How stale the figures above may be. MySQL 8 serves them from a cache whose
    // maximum age is this variable — 86400 (a day) out of the box, which is long
    // enough that a size can be badly wrong and the user has to be told. MariaDB
    // has no such variable and the statement errors there, which is the honest
    // `Unknown`: its statistics are refreshed on a different rule entirely.
    let freshness = match conn
        .query_first::<u64, _>("SELECT @@information_schema_stats_expiry")
        .await
    {
        Ok(Some(secs)) => Freshness::CachedUpTo(secs),
        _ => Freshness::Unknown,
    };

    Ok(map_mysql_stats(rows, &idx_rows, &usage, freshness))
}

/// The three MySQL statistics queries' rows, as the model the panel reads.
///
/// Pure, and separate from [`collect_table_stats`] because every decision in this
/// feature's MySQL half is here rather than in the round trip: which indexes
/// belong to which table, what makes one unique, and — the one that decides
/// whether an index gets flagged for deletion — that a missing `usage` entry
/// leaves `scans` **absent** rather than zero.
fn map_mysql_stats(
    rows: Vec<MyStatRow>,
    idx_rows: &[(String, String, Option<u64>, Option<i64>)],
    usage: &HashMap<(String, String), u64>,
    freshness: Freshness,
) -> SchemaStats {
    let mut by_table: HashMap<&str, Vec<IndexStats>> = HashMap::new();
    for (table, index, cardinality, non_unique) in idx_rows {
        let is_primary = index == "PRIMARY";
        by_table.entry(table).or_default().push(IndexStats {
            name: index.clone(),
            // MySQL reports one `INDEX_LENGTH` for the whole table and never
            // breaks it down, so no index here has a size of its own.
            bytes: None,
            cardinality: *cardinality,
            // Absent, not zero: Performance Schema is routinely off or ungranted,
            // and zero is what `IndexStats::is_unused` reads as "drop me".
            scans: usage.get(&(table.clone(), index.clone())).copied(),
            is_primary,
            // The primary key is unique without saying so — `NON_UNIQUE` is 0 for
            // it too, but the flag is what the panel labels the row with, and a
            // key that failed this test would be labelled an ordinary index.
            is_unique: is_primary || *non_unique == Some(0),
        });
    }

    let tables = rows
        .into_iter()
        .map(
            |(name, rows, data, index, free, auto, row_format, engine, created, updated)| {
                let indexes = by_table.remove(name.as_str()).unwrap_or_default();
                TableStats {
                    indexes,
                    table: name,
                    schema: None,
                    rows,
                    exact_rows: None,
                    data_bytes: data,
                    index_bytes: index,
                    free_bytes: free,
                    dead_rows: None,
                    auto_increment: auto,
                    row_format,
                    engine,
                    created,
                    updated,
                    freshness: freshness.clone(),
                }
            },
        )
        .collect();
    SchemaStats::new(tables)
}

/// The statement caps and the statistics mapping, next to what they guard.
///
/// **They came out of `lib.rs`'s `mod tests` with the code**, which is the half
/// of a move that is easy to leave behind: a test that stays put still compiles
/// as long as the item it names is `pub`, and then guards a thing its own file
/// no longer contains.
#[cfg(test)]
mod tests {
    use super::*;

    /// **The session list reads a bounded prefix of the statement.**
    ///
    /// `information_schema.PROCESSLIST.INFO` is the *untruncated* statement, and
    /// `MAX_SESSIONS` bounds only the number of rows — so the panel used to hold
    /// however many bytes the server's busiest sessions happened to be running,
    /// re-allocated every poll and re-scanned by `matches_query` on every
    /// keystroke. The truncation is in the SQL, so the source is the subject:
    /// the cap in the statement has to be the constant that documents it.
    #[test]
    fn the_mysql_session_list_caps_the_statement_text() {
        // **The number *and* the unit.** The first spelling checked only that
        // the SQL repeated the same digits, so `LEFT(INFO, 65536)` — which
        // counts **characters** over a `utf8mb3` column — satisfied a constant
        // documented in bytes and a `const` assert written in bytes. Measured on
        // MariaDB 10.11.14 and MySQL 8.4.11: `LENGTH(LEFT(<three CJK
        // characters>, 3))` is `9` under the plain form and `3` under the binary
        // one, so the ceiling was up to 3x off in the direction that matters.
        assert!(
            MY_PROCESSLIST_SQL.contains(&format!("{MY_INFO_MAX})")),
            "the statement text is unbounded, or the cap has drifted from \
             MY_INFO_MAX:\n{MY_PROCESSLIST_SQL}"
        );
        assert!(
            MY_PROCESSLIST_SQL.contains("LEFT(CONVERT(INFO USING binary)"),
            "the cap counts characters over a utf8mb3 column while MY_INFO_MAX \
             and the const assert below are in bytes:\n{MY_PROCESSLIST_SQL}"
        );
        // Generous against what the row draws, and bounded against what the
        // panel can hold: both halves of the balance the constant argues.
        // `const` blocks, so a cap edited past either bound is a compile error
        // rather than a test run — and so clippy does not read them as
        // assertions about nothing.
        const { assert!(MY_INFO_MAX > schemaic_core::history::PREVIEW_MAX * 8) };
        const { assert!(MY_INFO_MAX * schemaic_core::activity::MAX_SESSIONS <= 64 * 1024 * 1024) };
    }

    // ── The MySQL statistics half ─────────────────────────────────────────
    //
    // Three decisions with wrong answers that produce a plausible-looking panel
    // rather than an error: reading a cardinality per key *position* instead of
    // per index, calling the primary key an ordinary index, and turning "nobody
    // counted the scans" into "zero scans" — which is what marks an index for
    // deletion.

    /// `information_schema.STATISTICS` has one row per key position, each with the
    /// cardinality of the prefix ending there. The index's own figure is the last
    /// one, so the query has to group and take `MAX`: reading a row instead would
    /// report `(status, created_at)`'s handful of statuses as the whole index's
    /// distinct count.
    #[test]
    fn the_cardinality_query_takes_the_index_and_not_one_key_position() {
        assert!(MY_INDEX_CARDINALITY_SQL.contains("MAX(CARDINALITY)"));
        assert!(MY_INDEX_CARDINALITY_SQL.contains("GROUP BY TABLE_NAME, INDEX_NAME"));
        // `NON_UNIQUE` is constant within the group; `MIN` is how it survives the
        // grouping rather than an aggregate that means anything.
        assert!(MY_INDEX_CARDINALITY_SQL.contains("MIN(NON_UNIQUE)"));
    }

    /// The usage view carries a row for the **table** as well as its indexes, with
    /// a NULL index name. Counted, it would appear as an index nobody can find.
    #[test]
    fn the_usage_query_skips_the_tables_own_row() {
        assert!(MY_INDEX_USAGE_SQL.contains("INDEX_NAME IS NOT NULL"));
        assert!(MY_INDEX_USAGE_SQL.contains("OBJECT_SCHEMA = ?"));
    }

    fn stat_row(name: &str) -> MyStatRow {
        (
            name.to_string(),
            Some(4_213_551),
            Some(1024),
            Some(512),
            None,
            None,
            None,
            Some("InnoDB".to_string()),
            None,
            None,
        )
    }

    /// The mapping's three rules at once: indexes land on their own table, a key
    /// is unique because it is the key, and an index Performance Schema said
    /// nothing about reports **no** scan count rather than zero.
    #[test]
    fn the_mapping_keeps_a_missing_scan_count_absent() {
        let idx = vec![
            ("orders".into(), "PRIMARY".into(), Some(4_000_000), Some(0)),
            (
                "orders".into(),
                "idx_email".into(),
                Some(3_996_120),
                Some(0),
            ),
            ("orders".into(), "idx_status".into(), Some(7), Some(1)),
            ("other".into(), "PRIMARY".into(), Some(1), Some(0)),
        ];
        let usage: HashMap<(String, String), u64> =
            [(("orders".to_string(), "idx_email".to_string()), 12)].into();
        let stats = map_mysql_stats(
            vec![stat_row("orders"), stat_row("other")],
            &idx,
            &usage,
            Freshness::Unknown,
        );

        let orders = stats.find(None, "orders").expect("orders");
        assert_eq!(orders.indexes.len(), 3, "the other table's key is not here");
        let by = |n: &str| {
            orders
                .indexes
                .iter()
                .find(|i| i.name == n)
                .unwrap_or_else(|| panic!("{n}"))
        };
        assert_eq!(by("idx_email").scans, Some(12));
        // The two nobody reported: absent, so `is_unused` cannot flag them.
        assert_eq!(by("idx_status").scans, None);
        assert!(!by("idx_status").is_unused(), "not counted is not unused");
        assert!(by("PRIMARY").is_primary && by("PRIMARY").is_unique);
        assert!(by("idx_email").is_unique, "NON_UNIQUE = 0");
        assert!(!by("idx_status").is_unique, "NON_UNIQUE = 1");
        // MySQL reports one `INDEX_LENGTH` for the whole table, so no index here
        // may claim a size of its own.
        assert!(orders.indexes.iter().all(|i| i.bytes.is_none()));
        // And the cardinality is carried, marked as the estimate it is.
        assert_eq!(
            by("idx_email").cardinality_label().as_deref(),
            Some("~4m"),
            "printed as an estimate, not as 3,996,120"
        );
    }

    /// A table with no rows in `STATISTICS` — a view, or a table whose grants hide
    /// it — still gets its entry, with no indexes rather than none of it.
    #[test]
    fn the_mapping_keeps_a_table_with_no_indexes() {
        let stats = map_mysql_stats(
            vec![stat_row("v")],
            &[],
            &HashMap::new(),
            Freshness::Unknown,
        );
        let v = stats.find(None, "v").expect("v");
        assert!(v.indexes.is_empty());
        assert_eq!(v.rows, Some(4_213_551));
    }
}

// ── Schema introspection ─────────────────────────────────────────────────────

/// Everything the tree and the editors need about one database.
///
/// **The cancel arm kills the read on the server**, which is the only thing that
/// stops work already in flight: `conn.id()` is captured before the `select!` so
/// a second connection can `KILL QUERY` it. The token is checked at the
/// dispatcher's door too, before any engine opens anything — see
/// [`crate::Db::fetch_schema`] — so a token already cancelled never pays for a
/// handshake here.
pub(crate) async fn fetch_schema(
    db: &Db,
    database: &str,
    cancel: CancellationToken,
) -> Result<DbSchema, DbError> {
    let mut conn = db.open(None, false).await?;
    // The connection id, so a second connection can KILL the read that is
    // already running on the server — the same shape `count_rows` uses, and
    // the only thing that actually stops work in flight.
    let conn_id = conn.id();
    let out = tokio::select! {
        r = collect_schema(&mut conn, database) => r,
        _ = cancel.cancelled() => {
            kill_query(db, conn_id).await;
            Err(DbError::Cancelled)
        }
    };
    let _ = conn.disconnect().await;
    out
}

/// `database`'s table **list** — name, namespace and view flag, and nothing
/// else.
///
/// One query, and deliberately not five: see [`crate::Db::fetch_table_list`] for
/// why a name-listing caller must not reach [`fetch_schema`].
pub(crate) async fn fetch_table_list(db: &Db, database: &str) -> Result<DbSchema, DbError> {
    let mut conn = db.open(None, false).await?;
    let out = conn
        .exec_map(
            "SELECT CAST(TABLE_NAME AS CHAR) AS t, CAST(TABLE_TYPE AS CHAR) AS ty \
             FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
            (database,),
            |(name, ty): (String, String)| TableInfo {
                name,
                is_view: ty.eq_ignore_ascii_case("VIEW"),
                // MariaDB lists a sequence here as `SEQUENCE`; see
                // `TableInfo::is_sequence`.
                is_sequence: ty.eq_ignore_ascii_case("SEQUENCE"),
                ..Default::default()
            },
        )
        .await
        .map(|tables| DbSchema {
            tables,
            ..Default::default()
        })
        .map_err(|e| DbError::Query(e.to_string()));
    let _ = conn.disconnect().await;
    out
}

async fn collect_schema(conn: &mut Conn, database: &str) -> Result<DbSchema, DbError> {
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());

    // Tables, ordered. `TABLE_TYPE` separates base tables from views ('VIEW')
    // and, on MariaDB, from sequences ('SEQUENCE'), so the tree can render them
    // distinctly; the engine/collation/comment behind it are
    // the table-level options the schema designer edits (and `ALTER TABLE`
    // replaces wholesale, so they have to be readable before they can be shown).
    let table_opt_rows: Vec<MyTableRow> = conn
        .exec_map(
            "SELECT CAST(TABLE_NAME AS CHAR) AS t, CAST(TABLE_TYPE AS CHAR) AS ty, \
                    CAST(ENGINE AS CHAR) AS eng, CAST(TABLE_COLLATION AS CHAR) AS coll, \
                    CAST(TABLE_COMMENT AS CHAR) AS cmt \
             FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
            (database,),
            |r: MyTableRow| r,
        )
        .await
        .map_err(qerr)?;
    let table_rows: Vec<(String, String)> = table_opt_rows
        .iter()
        .map(|(t, ty, ..)| (t.clone(), ty.clone()))
        .collect();

    // Which server this is, for `mysql_column`'s default normalization — MariaDB
    // hands back SQL text where MySQL hands back a raw value, and nothing in the
    // catalogue itself says which. One extra row per schema fetch.
    //
    // The whole string is kept, not just the family: the index read below needs
    // the *number* too, since the column saying an index is switched off arrived
    // in MariaDB 10.6 and MySQL 8.0 and naming it on an older server fails the
    // query outright.
    let version: String = conn
        .query_first::<String, _>("SELECT VERSION()")
        .await
        .map_err(qerr)?
        .unwrap_or_default();
    // **The model's decision, not a second spelling of it.** This was
    // `version.to_ascii_lowercase().contains("mariadb")` written out here,
    // byte-identical to `ServerFlavour::parse_version` and driving five
    // branches — while the flavour the schema carries was then re-derived from
    // the `bool` at the bottom of this function. Two spellings of one question
    // on the path where the two servers' divergence is a data-loss class, and
    // unlike the enum a local `bool` has no `Unknown` arm, so "the server did
    // not say" folded to MySQL rather than to the documented safe default.
    let flavour = schemaic_core::schema::ServerFlavour::parse_version(&version);
    let mariadb = flavour.is_mariadb();

    // Columns for the whole schema in one pass, grouped back onto their tables.
    let col_rows: Vec<ColRow> = conn
        .exec_map(
            "SELECT CAST(TABLE_NAME AS CHAR) AS t, \
                    CAST(COLUMN_NAME AS CHAR) AS c, \
                    CAST(COLUMN_TYPE AS CHAR) AS ty, \
                    CAST(IS_NULLABLE AS CHAR) AS nullable, \
                    CAST(COLUMN_KEY AS CHAR) AS ck, \
                    CAST(COLUMN_DEFAULT AS CHAR) AS def, \
                    CAST(EXTRA AS CHAR) AS extra, \
                    CAST(COLLATION_NAME AS CHAR) AS coll, \
                    CAST(COLUMN_COMMENT AS CHAR) AS cmt, \
                    CAST(GENERATION_EXPRESSION AS CHAR) AS genexpr \
             FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = ? \
             ORDER BY TABLE_NAME, ORDINAL_POSITION",
            (database,),
            |r: MyColRow| r,
        )
        .await
        .map_err(qerr)?
        .into_iter()
        .map(|r| mysql_column(r, mariadb))
        .collect();

    // Foreign keys, one row per referencing key-column with its referenced
    // target, ordered so a composite key's columns fold in order. Drives both the
    // FOREIGN index tag (below) and the grid's "Follow FK" navigation. The
    // `REFERENCED_TABLE_NAME IS NOT NULL` filter keeps only FK usages (the same
    // view lists plain PK/unique key usages with NULL references).
    let fk_col_rows: Vec<FkColRow> = conn
        .exec_map(
            "SELECT CAST(TABLE_NAME AS CHAR) AS t, \
                    CAST(CONSTRAINT_NAME AS CHAR) AS cn, \
                    CAST(COLUMN_NAME AS CHAR) AS col, \
                    CAST(REFERENCED_TABLE_SCHEMA AS CHAR) AS rs, \
                    CAST(REFERENCED_TABLE_NAME AS CHAR) AS rt, \
                    CAST(REFERENCED_COLUMN_NAME AS CHAR) AS rc \
             FROM information_schema.KEY_COLUMN_USAGE \
             WHERE TABLE_SCHEMA = ? AND REFERENCED_TABLE_NAME IS NOT NULL \
             ORDER BY TABLE_NAME, CONSTRAINT_NAME, ORDINAL_POSITION",
            (database,),
            |(t, cn, col, rs, rt, rc): FkColRow| (t, cn, col, rs, rt, rc),
        )
        .await
        .map_err(qerr)?;

    // Each FK's referential actions, keyed by constraint. Separate from the
    // key-column rows above because they're per *constraint*, not per column —
    // and they can't be skipped: a schema editor that drops and recreates a
    // `ON DELETE CASCADE` key without restating the action silently turns it into
    // `NO ACTION`.
    let fk_rule_rows: Vec<(String, String, String, String)> = conn
        .exec_map(
            "SELECT CAST(TABLE_NAME AS CHAR) AS t, CAST(CONSTRAINT_NAME AS CHAR) AS cn, \
                    CAST(DELETE_RULE AS CHAR) AS dr, CAST(UPDATE_RULE AS CHAR) AS ur \
             FROM information_schema.REFERENTIAL_CONSTRAINTS \
             WHERE CONSTRAINT_SCHEMA = ?",
            (database,),
            |r: (String, String, String, String)| r,
        )
        .await
        .map_err(qerr)?;

    // Indexes: one row per (index, key-column); fold consecutive columns into
    // the same index, preserving `SEQ_IN_INDEX` order.
    // `EXPRESSION` is MySQL 8's column and MariaDB has none, so the row *shape*
    // is held steady with a NULL rather than the parsing branching — the same
    // trick, for the same reason, as the view query's `ALGORITHM` below: naming
    // a column that does not exist fails the whole query.
    let idx_sql = format!(
        "SELECT CAST(TABLE_NAME AS CHAR) AS t, \
                CAST(INDEX_NAME AS CHAR) AS i, \
                CAST(NON_UNIQUE AS SIGNED) AS nu, \
                CAST(COLUMN_NAME AS CHAR) AS c, \
                CAST(SUB_PART AS SIGNED) AS sub, \
                CAST(COLLATION AS CHAR) AS coll, \
                CAST(INDEX_TYPE AS CHAR) AS ty, \
                {} AS expr, \
                {} AS off \
         FROM information_schema.STATISTICS \
         WHERE TABLE_SCHEMA = ? \
         ORDER BY TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX",
        if mariadb {
            "CAST(NULL AS CHAR)"
        } else {
            "CAST(EXPRESSION AS CHAR)"
        },
        // Same trick, one column over: `IGNORED` (MariaDB 10.6+) and
        // `IS_VISIBLE` (MySQL 8.0+) are named differently, answer with opposite
        // polarity, and do not exist at all on an older server — so the
        // normalising is `core`'s and the row shape stays one shape.
        schemaic_core::schema::index_disabled_sql(&version)
    );
    type MyIdxRow = (
        String,
        String,
        i64,
        // **`COLUMN_NAME` is nullable.** MySQL 8 gives a functional key part a
        // NULL name and puts the expression in `EXPRESSION`; bound as `String`,
        // `from_row` panicked inside the fetch task, so **one** functional index
        // anywhere in a database made the whole of it unbrowsable — the tree
        // spun for ever with no error at all. MariaDB cannot reproduce it: it
        // rejects the syntax.
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        // 1 when the server says this index is switched off — see
        // `schema::index_disabled_sql`. Never NULL: the expression is a `CASE`
        // or the constant `0`.
        i64,
    );
    let idx_rows: Vec<IdxRow> = conn
        .exec_map(idx_sql.as_str(), (database,), |r: MyIdxRow| r)
        .await
        .map_err(qerr)?
        .into_iter()
        .map(|(t, i, nu, c, sub, coll, ty, expr, off)| {
            let expression = c.is_none();
            let disabled = off != 0;
            IdxRow {
                table: t,
                index: i,
                unique: nu == 0,
                column: schemaic_core::schema::IndexColumn {
                    // A functional key part has no column name. The expression
                    // is what it is *about*, so it is what the schema tree and
                    // the designer show; `lossy` below is what stops anything
                    // trying to recreate the index from it.
                    name: c.or(expr).unwrap_or_else(|| "<expression>".to_string()),
                    // A prefix index (`KEY (bio(20))`) — recreating it without
                    // the length fails outright on a TEXT column.
                    prefix: sub.and_then(|n| u32::try_from(n).ok()),
                    // `COLLATION` is 'A' ascending, 'D' descending, NULL unsorted.
                    descending: coll.as_deref() == Some("D"),
                    expression,
                    // MySQL collates per column, not per index key.
                    collation: None,
                },
                // **Only when it isn't the default.** BTREE is, so restating it
                // everywhere would be noise in every generated statement — but
                // FULLTEXT and SPATIAL are not, and reading them as `None` is
                // what turned a recreated full-text index into a plain `KEY`
                // and broke every `MATCH … AGAINST` against the table. The
                // MySQL emitters can spell all three now
                // (`ddl::mysql_index_clause`), which is what makes reading it
                // worth anything.
                method: ty.filter(|t| !t.eq_ignore_ascii_case("BTREE") && !t.is_empty()),
                predicate: None,
                // `STATISTICS` gives the whole key — prefix, direction and now
                // the type — for an ordinary index. Two exceptions: a
                // **functional** key part, whose expression comes back as
                // MySQL 8 stored it and re-emitting it as a key part is not
                // something this model can promise; and an index the DBA has
                // **switched off** (`INVISIBLE`/`IGNORED`), which no emitter
                // here can spell. Both are marked lossy so the existing refusal
                // fires instead of a silent drop-and-recreate — and for the
                // second one that recreate brought a hidden index back *live*,
                // with the optimizer using it again and the preview silent.
                lossy: expression || disabled,
                // MySQL publishes no per-index `CREATE`; the model
                // reconstructs one from the columns it read.
                create_sql: None,
            }
        })
        .collect();

    // Views (only if the schema has any): the stored SELECT body, plus the
    // options a `CREATE OR REPLACE VIEW` **resets** when it doesn't restate them
    // — the check option, the definer, and the security type. The last of those
    // is a privilege: a view redefined without `SQL SECURITY DEFINER` starts
    // running as whoever calls it. Reading them here is what lets the schema
    // editor carry them through an edit (see `core::schema::ViewOptions`).
    let has_views = table_rows
        .iter()
        .any(|(_, ty)| ty.eq_ignore_ascii_case("VIEW"));
    let view_sql = format!(
        "SELECT CAST(TABLE_NAME AS CHAR) AS t, CAST(VIEW_DEFINITION AS CHAR) AS def, \
                CAST(CHECK_OPTION AS CHAR) AS chk, CAST(DEFINER AS CHAR) AS definer, \
                CAST(SECURITY_TYPE AS CHAR) AS sec, {} AS algo \
             FROM information_schema.VIEWS \
             WHERE TABLE_SCHEMA = ?",
        // MariaDB reports the view's ALGORITHM; MySQL 8 doesn't have the column
        // at all (only `SHOW CREATE VIEW` knows), and naming a column that
        // doesn't exist fails the whole query — so the row *shape* is held
        // steady with a NULL instead of branching the parsing.
        if mariadb {
            "CAST(ALGORITHM AS CHAR)"
        } else {
            "CAST(NULL AS CHAR)"
        }
    );
    let view_opt_rows: Vec<MyViewRow> = if has_views {
        conn.exec_map(view_sql.as_str(), (database,), |r: MyViewRow| r)
            .await
            .map_err(qerr)?
    } else {
        Vec::new()
    };
    let view_rows: Vec<(String, String)> = view_opt_rows
        .iter()
        .map(|(t, def, ..)| (t.clone(), def.clone()))
        .collect();

    // CHECK constraints. The two servers put them in different places:
    //
    // * **MySQL 8.0.16+** — `CHECK_CONSTRAINTS` carries only the clause, with no
    //   `TABLE_NAME`, so the table comes from a join onto `TABLE_CONSTRAINTS`,
    //   which is also the only place `ENFORCED` lives.
    // * **MariaDB 10.2+** — `CHECK_CONSTRAINTS` has `TABLE_NAME` itself, and
    //   there is no `NOT ENFORCED` to report.
    //
    // Anything older has no check constraints *and* no `CHECK_CONSTRAINTS` table
    // — MySQL 5.7 parsed the clause and threw it away. Naming a missing table
    // fails the query, so *that* error degrades to "no checks" rather than
    // taking the whole schema fetch down with it.
    //
    // Only that error. A blanket `unwrap_or_default` would turn a typo in the
    // query above into every table quietly reporting no constraints, which is
    // this feature's own bug wearing a disguise: the designer would then build a
    // `CREATE TABLE` that drops checks the server really has.
    //
    // `LEVEL` is MariaDB's alone and is not cosmetic: a `Column` check is part
    // of the column definition `MODIFY COLUMN` replaces, so the emitter has to
    // restate it or the server deletes it (see `CheckInfo::column_level`). MySQL
    // has no such thing — it rewrites the same syntax into a table constraint at
    // `CREATE` time — so that branch reports `Table` for every row.
    let check_sql = if mariadb {
        "SELECT CAST(TABLE_NAME AS CHAR) AS t, CAST(CONSTRAINT_NAME AS CHAR) AS cn, \
                CAST(CHECK_CLAUSE AS CHAR) AS cc, 'YES' AS enforced, \
                CAST(LEVEL AS CHAR) AS lvl \
         FROM information_schema.CHECK_CONSTRAINTS \
         WHERE CONSTRAINT_SCHEMA = ?"
    } else {
        "SELECT CAST(tc.TABLE_NAME AS CHAR) AS t, CAST(cc.CONSTRAINT_NAME AS CHAR) AS cn, \
                CAST(cc.CHECK_CLAUSE AS CHAR) AS cc, CAST(tc.ENFORCED AS CHAR) AS enforced, \
                'Table' AS lvl \
         FROM information_schema.CHECK_CONSTRAINTS cc \
         JOIN information_schema.TABLE_CONSTRAINTS tc \
           ON tc.CONSTRAINT_SCHEMA = cc.CONSTRAINT_SCHEMA \
          AND tc.CONSTRAINT_NAME = cc.CONSTRAINT_NAME \
          AND tc.CONSTRAINT_TYPE = 'CHECK' \
         WHERE cc.CONSTRAINT_SCHEMA = ?"
    };
    // MariaDB grew `LEVEL` in 10.5; 10.2-10.4 have the table without it. Losing
    // that column must not cost the whole database its check constraints, so a
    // missing-column error retries without it — those servers then behave as
    // Schemaic did before the column was read at all.
    let check_fallback = "SELECT CAST(TABLE_NAME AS CHAR) AS t, CAST(CONSTRAINT_NAME AS CHAR) AS cn, \
                CAST(CHECK_CLAUSE AS CHAR) AS cc, 'YES' AS enforced, 'Table' AS lvl \
         FROM information_schema.CHECK_CONSTRAINTS \
         WHERE CONSTRAINT_SCHEMA = ?";
    let check_rows: Vec<MyCheckRow> = match conn
        .exec_map(check_sql, (database,), |r: MyCheckRow| r)
        .await
    {
        Ok(rows) => rows,
        // 1109 `ER_UNKNOWN_TABLE` / 1146 `ER_NO_SUCH_TABLE`: the server predates
        // check constraints, so there are none to report.
        Err(mysql_async::Error::Server(e)) if e.code == 1109 || e.code == 1146 => Vec::new(),
        // 1054 `ER_BAD_FIELD_ERROR`: a MariaDB too old for `LEVEL`.
        Err(mysql_async::Error::Server(e)) if e.code == 1054 && mariadb => conn
            .exec_map(check_fallback, (database,), |r: MyCheckRow| r)
            .await
            .map_err(qerr)?,
        Err(e) => return Err(qerr(e)),
    };

    // Triggers. `information_schema.TRIGGERS` has been there since MySQL 5.0 and
    // `ACTION_ORDER` since 5.7.2 / MariaDB 10.2.3, both well below anything this
    // app connects to — so unlike CHECK_CONSTRAINTS there is no missing-table
    // case to degrade for, and a failure here is a real failure.
    let trigger_rows: Vec<MyTriggerRow> = conn
        .exec_map(
            "SELECT CAST(EVENT_OBJECT_TABLE AS CHAR) AS t, CAST(TRIGGER_NAME AS CHAR) AS n, \
                    CAST(ACTION_TIMING AS CHAR) AS ti, CAST(EVENT_MANIPULATION AS CHAR) AS ev, \
                    CAST(ACTION_STATEMENT AS CHAR) AS st, CAST(DEFINER AS CHAR) AS df, \
                    COALESCE(ACTION_ORDER, 0) AS ord \
             FROM information_schema.TRIGGERS \
             WHERE TRIGGER_SCHEMA = ?",
            (database,),
            |r: MyTriggerRow| r,
        )
        .await
        .map_err(qerr)?;

    // Stored routines. `information_schema.ROUTINES` and `PARAMETERS` have both
    // been there since 5.0, so — as with `TRIGGERS` — there is no
    // missing-table case to degrade for and a failure here is a real failure.
    //
    // `ORDINAL_POSITION = 0` is a *function's return value*, not a parameter;
    // left in, every function's rendered signature would open with its return
    // type. `PARAMETER_MODE` is reported as `IN` for a **function's** parameters
    // too, where `CREATE FUNCTION` has no grammar for it — which is
    // [`mysql_parameters`]' first job, and why the mode is not simply joined in
    // here.
    //
    // `SQL_MODE`/`CHARACTER_SET_CLIENT`/`COLLATION_CONNECTION` are the session
    // state the recreate has to restore, and the catalogue carries the same
    // values `SHOW CREATE` prints. Read here so a draft is never without them:
    // the editor's lazy `SHOW CREATE` corrects the *body*, and a keystroke that
    // lands first must not be able to strip the wrapper off a `CREATE` whose
    // `DROP` has already committed.
    let routine_rows: Vec<MyRoutineRow> = conn
        .exec_map(MY_ROUTINES_SQL, (database,), |r: Row| my_routine_row(&r))
        .await
        .map_err(qerr)?;
    let param_rows: Vec<MyParamRow> = conn
        .exec_map(
            "SELECT CAST(SPECIFIC_NAME AS CHAR) AS n, CAST(ROUTINE_TYPE AS CHAR) AS ty, \
                    CAST(COALESCE(PARAMETER_MODE, '') AS CHAR) AS mode, \
                    CAST(COALESCE(PARAMETER_NAME, '') AS CHAR) AS pname, \
                    CAST(DTD_IDENTIFIER AS CHAR) AS dtd, \
                    CAST(CHARACTER_SET_NAME AS CHAR) AS cs, \
                    CAST(COLLATION_NAME AS CHAR) AS coll \
             FROM information_schema.PARAMETERS \
             WHERE SPECIFIC_SCHEMA = ? AND ORDINAL_POSITION > 0 \
             ORDER BY SPECIFIC_NAME, ORDINAL_POSITION",
            (database,),
            |r: MyParamRow| r,
        )
        .await
        .map_err(qerr)?;
    let params = mysql_parameters(&param_rows);

    // Scheduled events. `information_schema.EVENTS` has been there since MySQL
    // 5.1 and MariaDB 5.1, but unlike `TRIGGERS` this **degrades** rather than
    // failing the whole read: the MySQL-protocol servers that aren't MySQL
    // (TiDB, Vitess and friends) are exactly the ones that may not implement the
    // scheduler, and a database whose tables can't be browsed because it has no
    // events table is a far worse outcome than one whose Events folder is empty.
    // The same call `CHECK_CONSTRAINTS` above makes, and the same two codes.
    let event_rows: Vec<MyEventRow> = match conn
        .exec_map(MY_EVENTS_SQL, (database,), |r: Row| my_event_row(&r))
        .await
    {
        Ok(rows) => rows,
        // 1109 `ER_UNKNOWN_TABLE` / 1146 `ER_NO_SUCH_TABLE`: no such catalogue,
        // so there are no events to report.
        Err(mysql_async::Error::Server(e)) if e.code == 1109 || e.code == 1146 => Vec::new(),
        Err(e) => return Err(qerr(e)),
    };

    let mut schema = assemble_schema(
        // MySQL: the database is the namespace, so tables carry none.
        None,
        &table_rows,
        &col_rows,
        &fk_col_rows,
        &idx_rows,
        &view_rows,
    );
    schema.routines = mysql_routines(&routine_rows, &params)
        .into_iter()
        .map(std::sync::Arc::new)
        .collect();
    schema.events = mysql_events(&event_rows)
        .into_iter()
        .map(std::sync::Arc::new)
        .collect();
    apply_table_options(&mut schema, &table_opt_rows);
    apply_view_options(&mut schema, &view_opt_rows);
    apply_fk_rules(&mut schema, &fk_rule_rows);
    apply_check_constraints(&mut schema, &check_rows, mariadb);
    apply_triggers(&mut schema, mysql_triggers(&trigger_rows));
    // The flavour was computed at the top of this function and then thrown
    // away, so the emitter — which is where MySQL and MariaDB actually diverge
    // — had no way to ask. It rides on the schema now, and it is the *same*
    // value the branches above asked rather than one rebuilt from a `bool`.
    schema.flavour = flavour;
    // And where it was read from, for the same kind of reason: a foreign key's
    // `REFERENCED_TABLE_SCHEMA` and a view's rewritten `VIEW_DEFINITION` both
    // name this database, and the one reader that compares two databases has to
    // subtract it. See `DbSchema::database`.
    schema.database = Some(database.to_string());
    Ok(schema)
}

/// The `ALGORITHM` of a `SHOW CREATE VIEW` body, or `None` when it doesn't name
/// one (which is what `UNDEFINED` means, and what the emitter leaves unwritten).
///
/// Pure, because the shape it reads is narrow and positional: the clause is
/// always `CREATE ALGORITHM=… DEFINER=…`, before the definer and before any
/// user-controlled text, so scanning to the first `ALGORITHM=` can't be led
/// astray by a view *body* that happens to contain the word. Anchored to the
/// leading `CREATE` for the same reason.
fn view_algorithm_of(create_sql: &str) -> Option<String> {
    let head = create_sql.trim_start();
    let rest = head
        .strip_prefix("CREATE")
        .or_else(|| head.strip_prefix("create"))?;
    // Only the clause immediately after `CREATE` — `DEFINER` follows it, and
    // everything past that is the user's own SQL.
    let rest = rest.trim_start();
    let rest = rest
        .get(..9)
        .filter(|p| p.eq_ignore_ascii_case("ALGORITHM"))
        .map(|_| &rest[9..])?;
    let value = rest.trim_start().strip_prefix('=')?.trim_start();
    let end = value
        .find(|c: char| c.is_whitespace())
        .unwrap_or(value.len());
    let algo = value[..end].trim().to_ascii_uppercase();
    // `UNDEFINED` is the default the emitter deliberately doesn't restate.
    (!algo.is_empty() && algo != "UNDEFINED").then_some(algo)
}

/// One `SHOW CREATE TRIGGER` row: `(Trigger, sql_mode, SQL Original Statement,
/// character_set_client, collation_connection, Database Collation, Created)`.
///
/// `Created` is nullable — MySQL only started recording it in 5.7.2, and a
/// trigger made before an upgrade still has none.
type MyShowCreateTriggerRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
);

/// The **body** of a `SHOW CREATE TRIGGER` statement — everything after
/// `FOR EACH ROW` and any `FOLLOWS`/`PRECEDES` clause.
///
/// Positional, like [`view_algorithm_of`], but it cannot be a plain `find`: the
/// text before the body is server-generated, yet it contains *identifiers*, and
/// a table or trigger named `` `x FOR EACH ROW y` `` is legal. So the scan goes
/// through [`sql::skip_noncode`] on the MySQL dialect, which steps over a
/// backtick-quoted name whole. Everything after the anchor is the user's own
/// SQL and is returned untouched — including any `FOR EACH ROW` inside it,
/// which is why the **first** anchor is the right one.
///
/// The ordering clause is dropped rather than kept: `TriggerInfo::order` is
/// reconstructed from `information_schema.ACTION_ORDER` and the emitter writes
/// it back, so carrying it in the body too would emit it twice.
fn trigger_body_of(create_sql: &str) -> Option<String> {
    const ANCHOR: &str = "FOR EACH ROW";
    let b = create_sql.as_bytes();
    let mut i = 0usize;
    let after = loop {
        if i >= b.len() {
            return None;
        }
        if let Some(j) = sql::skip_noncode(b, i, SqlDialect::MySql) {
            i = j;
            continue;
        }
        if b[i..].len() >= ANCHOR.len()
            && b[i..i + ANCHOR.len()].eq_ignore_ascii_case(ANCHOR.as_bytes())
        {
            break i + ANCHOR.len();
        }
        i += 1;
    };
    let rest = create_sql.get(after..)?.trim_start();
    // An ordering clause, if the server printed one: the keyword, then one
    // identifier (which may be backtick-quoted and hold anything).
    //
    // **A word boundary after the keyword, for the same reason the anchor above
    // goes through `skip_noncode`.** A prefix match alone is not the question: a
    // labelled compound statement is legal SQL and a label is any identifier, so
    // a body opening `followsx: BEGIN … END followsx` matched `FOLLOWS`, lost
    // one identifier's worth to the clause, and came back without its first
    // token — into the editable draft *and* the diff baseline, where `validate`
    // parses no body and says nothing, so an unrelated edit emitted
    // `DROP TRIGGER` and then an invalid `CREATE`. Measured on MariaDB
    // 10.11.14: `ERROR 1064`, trigger gone.
    for kw in ["FOLLOWS", "PRECEDES"] {
        if rest.len() >= kw.len()
            && rest.as_bytes()[..kw.len()].eq_ignore_ascii_case(kw.as_bytes())
            && rest
                .as_bytes()
                .get(kw.len())
                .is_none_or(|c| !sql::is_word_byte(*c))
        {
            let after_kw = rest[kw.len()..].trim_start();
            let nb = after_kw.as_bytes();
            let end = match sql::skip_noncode(nb, 0, SqlDialect::MySql) {
                Some(j) => j,
                None => nb
                    .iter()
                    .position(|&c| !sql::is_word_byte(c))
                    .unwrap_or(nb.len()),
            };
            return Some(after_kw[end..].trim().to_string());
        }
    }
    Some(rest.trim().to_string())
}

/// One `SHOW CREATE {PROCEDURE|FUNCTION}` row: `(name, sql_mode, Create …,
/// character_set_client, collation_connection, Database Collation)`.
///
/// The `Create` column is **nullable**, and that is not a corner case: MySQL
/// returns NULL there for a routine the connected account may not see the
/// definition of (it needs `SHOW_ROUTINE`, or to be the definer). A `None`
/// leaves the editor on what the schema fetch already carried.
type MyShowCreateRoutineRow = (String, String, Option<String>, String, String, String);

/// One `SHOW CREATE EVENT` row: `(name, sql_mode, time_zone, Create Event,
/// character_set_client, collation_connection, Database Collation)`.
///
/// Seven columns rather than a routine's six, and the extra one is `time_zone` —
/// which is why an event needs its own row type rather than reusing that alias.
/// The `Create Event` column is **nullable** for the same reason a routine's is.
type MyShowCreateEventRow = (
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    String,
);

/// Does this `SHOW CREATE` text declare a MariaDB **aggregate** function?
///
/// The one fact about a routine that `information_schema.ROUTINES` does not
/// publish — verified live on MariaDB 10.11.14, no column in that table names
/// it — while `SHOW CREATE FUNCTION` prints
/// ``CREATE DEFINER=`a`@`b` AGGREGATE FUNCTION `f`(…)``. Losing it destroyed the
/// function: the recreate's `CREATE` came back `ERROR 4105 (Aggregate specific
/// instruction (FETCH GROUP NEXT ROW) used in a wrong context)` after the
/// `DROP` had committed, and the catalogue was then empty for the name.
///
/// Only the **header** is read — everything before the parameter list — so a
/// body that mentions aggregates says nothing, and the scan goes through
/// [`sql::skip_noncode`] so a routine *named* `` `aggregate` `` is a quoted
/// identifier the scan steps over rather than the keyword. This is the same
/// span [`routine_body_of`] walks to find the parameter list and discards.
fn routine_is_aggregate(create_sql: &str) -> bool {
    let b = create_sql.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(j) = sql::skip_noncode(b, i, SqlDialect::MySql) {
            i = j.max(i + 1);
            continue;
        }
        // The parameter list: past here is the routine, not its header.
        if b[i] == b'(' {
            return false;
        }
        if sql::is_word_start(b[i]) {
            let mut j = i + 1;
            while j < b.len() && sql::is_word_byte(b[j]) {
                j += 1;
            }
            if create_sql[i..j].eq_ignore_ascii_case("AGGREGATE") {
                return true;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    false
}

/// The **body** of a `SHOW CREATE {PROCEDURE|FUNCTION}` statement — everything
/// after the parameter list and the characteristics that follow it.
///
/// The same shape as [`trigger_body_of`] and for the same reason, but it cannot
/// anchor on a keyword: a routine has no `FOR EACH ROW`, and what separates the
/// header from the body is *running out of characteristics*. So the parameter
/// list is skipped as a balanced group (through [`sql::balanced_paren_span`], so
/// a default or a type inside it can hold a paren in a string), and then the
/// clauses MySQL prints between it and the body are consumed by keyword.
///
/// **Greedy consumption is safe because the two vocabularies are disjoint.** The
/// characteristic words are `COMMENT`, `LANGUAGE`, `NOT`, `DETERMINISTIC`,
/// `CONTAINS`, `NO`, `READS`, `MODIFIES`, `SQL`, `DATA`, `SECURITY`, `DEFINER`,
/// `INVOKER` and `RETURNS`; no MySQL statement — and therefore no routine body —
/// begins with any of them. The first word that isn't one of them starts the
/// body, which is returned untouched.
///
/// `RETURNS` is the one clause with an argument that isn't a single token: the
/// type may carry a length (`VARCHAR(10)`) and trailing modifiers, so the word
/// after it takes an optional balanced group and then any of the type-modifier
/// words with it.
///
/// `None` when there is no parameter list to anchor on, or nothing after the
/// characteristics — both of which mean this didn't understand the text, and a
/// caller that gets `None` keeps the body it already had rather than blanking it.
fn routine_body_of(create_sql: &str) -> Option<String> {
    const CHARACTERISTIC: &[&str] = &[
        "NOT",
        "DETERMINISTIC",
        "CONTAINS",
        "NO",
        "READS",
        "MODIFIES",
        "SQL",
        "DATA",
        "SECURITY",
        "DEFINER",
        "INVOKER",
    ];
    // Words that may trail a return type on their own:
    // `RETURNS DECIMAL(10,2) UNSIGNED`. Each takes no argument.
    const TYPE_FLAG: &[&str] = &[
        "UNSIGNED", "SIGNED", "ZEROFILL", "BINARY", "ASCII", "UNICODE",
    ];
    // …and the two that take a **name** with them: `CHARSET utf8mb4`,
    // `COLLATE utf8mb4_bin`. `CHARACTER SET utf8mb4` is the third and is spelled
    // in two words, which is why it is matched as a pair below rather than by
    // putting a bare `SET` on either list — a bare `SET` there also swallowed
    // the first word of a body that legitimately begins `SET @x = 1`.
    const TYPE_NAMED: &[&str] = &["CHARSET", "COLLATE"];

    let b = create_sql.as_bytes();
    // The parameter list: the first parenthesis that isn't inside a quoted
    // identifier, a string or a comment. `CREATE DEFINER=`a`@`b` PROCEDURE
    // `db`.`p`(…)` has none before it, and a routine named `` `p(x)` `` would.
    let mut i = 0usize;
    let after_params = loop {
        if i >= b.len() {
            return None;
        }
        if let Some(j) = sql::skip_noncode(b, i, SqlDialect::MySql) {
            i = j;
            continue;
        }
        if b[i] == b'(' {
            break sql::balanced_paren_span(b, i, SqlDialect::MySql)? + 1;
        }
        i += 1;
    };

    let mut rest = create_sql.get(after_params..)?.trim_start();
    loop {
        let word = leading_word(rest);
        let upper = word.to_ascii_uppercase();
        if word.is_empty() {
            break;
        }
        if upper == "COMMENT" {
            // The literal that follows, skipped as a quoted run so an escaped
            // or doubled quote inside it can't end it early.
            let after_kw = rest[word.len()..].trim_start();
            let nb = after_kw.as_bytes();
            let end = sql::skip_noncode(nb, 0, SqlDialect::MySql)?;
            rest = after_kw[end..].trim_start();
            continue;
        }
        if upper == "LANGUAGE" {
            let after_kw = rest[word.len()..].trim_start();
            let lang = leading_word(after_kw);
            rest = after_kw[lang.len()..].trim_start();
            continue;
        }
        if upper == "RETURNS" {
            rest = rest[word.len()..].trim_start();
            // The type name, then its optional length/precision group.
            let name = leading_word(rest);
            rest = rest[name.len()..].trim_start();
            if rest.as_bytes().first() == Some(&b'(') {
                let end = sql::balanced_paren_span(rest.as_bytes(), 0, SqlDialect::MySql)? + 1;
                rest = rest[end..].trim_start();
            }
            // The type's trailing modifiers. **Each form takes its argument with
            // it or takes none — a keyword consumed without its value leaves
            // that value at the head of what is returned as the body**, which is
            // a `CREATE` that fails 1064 *after* the `DROP` has committed.
            loop {
                let w = leading_word(rest);
                if w.is_empty() {
                    break;
                }
                let after = || rest[w.len()..].trim_start();
                if TYPE_FLAG.iter().any(|t| w.eq_ignore_ascii_case(t)) {
                    rest = after();
                } else if TYPE_NAMED.iter().any(|t| w.eq_ignore_ascii_case(t)) {
                    let tail = after();
                    let v = leading_word(tail);
                    rest = tail[v.len()..].trim_start();
                } else if w.eq_ignore_ascii_case("CHARACTER") {
                    // `CHARACTER SET <name>` — three words, and only as a pair:
                    // a lone `CHARACTER` isn't a modifier, so an unmatched one
                    // ends the type rather than eating what follows.
                    let tail = after();
                    let set = leading_word(tail);
                    if !set.eq_ignore_ascii_case("SET") {
                        break;
                    }
                    let tail = tail[set.len()..].trim_start();
                    let v = leading_word(tail);
                    rest = tail[v.len()..].trim_start();
                } else {
                    break;
                }
            }
            continue;
        }
        if CHARACTERISTIC.iter().any(|c| *c == upper) {
            rest = rest[word.len()..].trim_start();
            continue;
        }
        break;
    }
    let body = rest.trim();
    (!body.is_empty()).then(|| body.to_string())
}

/// The identifier-shaped word `s` starts with, or `""` when it doesn't start
/// with one. On [`sql::is_word_start`]/[`sql::is_word_byte`], which is the one
/// definition of what a word is here.
fn leading_word(s: &str) -> &str {
    let b = s.as_bytes();
    if b.first().is_none_or(|c| !sql::is_word_start(*c)) {
        return "";
    }
    let end = b
        .iter()
        .position(|c| !sql::is_word_byte(*c))
        .unwrap_or(b.len());
    &s[..end]
}

/// One `information_schema.CHECK_CONSTRAINTS` row, already joined to its table:
/// `(table, constraint name, check clause, enforced, level)`.
///
/// `level` is MariaDB's `Column`/`Table`; the MySQL query reports `Table` for
/// every row, which is what that server actually stores.
type MyCheckRow = (String, String, String, String, String);

/// One `CHECK_CLAUSE` as SQL that can actually be run.
///
/// The two servers disagree, and only one of them says so. **MySQL 8 returns the
/// clause with an extra level of backslash escaping** — a predicate that reads
/// `_latin1'new'` comes back as `_latin1\'new\'`, and `'C:\\temp'` as
/// `'C:\\\\temp'` — so restating it verbatim in a `CREATE TABLE` is a syntax
/// error, not a subtly different constraint. **MariaDB returns it already
/// runnable**, byte for byte what `SHOW CREATE TABLE` prints, so unescaping there
/// would eat the backslash out of `'it\'s'` and change what the predicate means.
///
/// The rule is one level of unescaping: a backslash escapes the character after
/// it, which is emitted alone. Measured against `SHOW CREATE TABLE` on MySQL
/// 8.4 — the same authority [`mysql_column`] uses for defaults, and the same
/// class of bug, except this one fails loudly instead of writing something else.
fn mysql_check_clause(clause: &str, mariadb: bool) -> String {
    if mariadb || !clause.contains('\\') {
        return clause.to_string();
    }
    let b = clause.as_bytes();
    let mut out = String::with_capacity(clause.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            // **Decode the escape, don't just drop the backslash.** MySQL's
            // escapes are not all "the next byte literally": `\n` is a newline,
            // not the letter `n`. Dropping the backslash turned a column named
            // with an embedded newline into a different, non-existent
            // identifier — measured live on MySQL 8.4.11, `` `nl\ncol` ``
            // came back as `` `nlncol` ``.
            let decoded = match b[i + 1] {
                b'n' => Some('\n'),
                b'r' => Some('\r'),
                b't' => Some('\t'),
                b'0' => Some('\0'),
                b'b' => Some('\u{8}'),
                b'Z' => Some('\u{1a}'),
                // Everything else — `\'`, `\"`, `\\`, `\%`, `\_` — really is
                // the next character standing for itself.
                _ => None,
            };
            if let Some(c) = decoded {
                out.push(c);
                i += 2;
                continue;
            }
            i += 1;
        }
        // Copy one whole UTF-8 char: the escaped byte may begin a multi-byte one.
        let start = i;
        i += 1;
        while i < b.len() && (b[i] & 0xC0) == 0x80 {
            i += 1;
        }
        out.push_str(&clause[start..i]);
    }
    out
}

/// Fold MySQL/MariaDB's check constraints onto the assembled tables.
///
/// Kept out of [`assemble_schema`] for the reason [`apply_fk_rules`] is: it's a
/// second query's worth of rows keyed by table, not part of the row shape the
/// two engines share.
fn apply_check_constraints(schema: &mut DbSchema, rows: &[MyCheckRow], mariadb: bool) {
    // Bucketed once rather than filtered per table — see `group_by`.
    let by_table = crate::group_by(rows.iter().map(|r| (r.0.as_str(), r)));
    for t in schema.tables.iter_mut() {
        t.check_constraints = by_table
            .get(t.name.as_str())
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|(_, name, clause, enforced, level)| CheckInfo {
                name: name.clone(),
                // `CHECK_CLAUSE` is the server's re-print of the predicate,
                // parenthesised and — on MySQL 8 — escaped a second time; the
                // model stores it bare and runnable. Unescaping has to come
                // first: `check_predicate`'s paren scan reads string boundaries,
                // and `\'new\'` isn't one until the escaping is gone.
                expression: schemaic_core::ddl::check_predicate(
                    &mysql_check_clause(clause, mariadb),
                    schemaic_core::intel::SqlDialect::MySql,
                ),
                // MariaDB has no `NOT ENFORCED` and the query hardcodes `YES`
                // there, so this reads as enforced on both.
                enforced: !enforced.eq_ignore_ascii_case("NO"),
                // MariaDB's `LEVEL`. Only `Column` matters — it says the
                // constraint lives inside the column definition, so a `MODIFY`
                // that doesn't restate it deletes it.
                column_level: level.eq_ignore_ascii_case("Column"),
                // `NOT VALID` / `NO INHERIT` are PostgreSQL's; neither engine
                // here can report one, and the emitter writes neither.
                ..Default::default()
            })
            .collect();
    }
}

/// One `information_schema.TRIGGERS` row: `(table, name, timing, event,
/// statement, definer, action order)`.
type MyTriggerRow = (String, String, String, String, String, String, u64);

/// Fold MySQL's trigger rows into [`TriggerInfo`]s, per table.
///
/// **The ordering has to be reconstructed, not read.** MySQL has no
/// `FOLLOWS`/`PRECEDES` column: `ACTION_ORDER` reports a trigger's *position*
/// within its `(table, timing, event)` group, and that is all. A recreate that
/// ignored it would silently reorder triggers that write the same row, which is
/// the whole reason anyone sets an order in the first place — so position 2 and
/// up become `FOLLOWS <the row before them>`, the same chain MySQL was given.
///
/// A server too old to report the column sends `0` for every row. `0` means "no
/// ordering information", not "first", so those get no clause at all rather than
/// a fabricated chain.
///
/// **The group's leader gets a `PRECEDES`, not nothing.** `FOLLOWS <previous>`
/// covers positions 2 and up, but position 1 has no predecessor to name — and a
/// `CREATE TRIGGER` with no ordering clause makes MySQL append the trigger
/// *last*, so replacing the leader reversed the whole group. Measured on MySQL
/// 8.4.11. A positive anchor is the only clause that can express "first", so the
/// leader of a group of two or more names its successor instead. A group of one
/// still gets nothing: there is no order to preserve.
fn mysql_triggers(rows: &[MyTriggerRow]) -> Vec<TriggerInfo> {
    // Group key then position, so the previous row in iteration order *is* the
    // trigger this one follows. Name last, to keep it deterministic when a stale
    // server reports ties.
    let mut sorted: Vec<&MyTriggerRow> = rows.iter().collect();
    sorted.sort_by(|a, b| (&a.0, &a.2, &a.3, a.6, &a.1).cmp(&(&b.0, &b.2, &b.3, b.6, &b.1)));
    let mut out: Vec<TriggerInfo> = Vec::with_capacity(sorted.len());
    // (table, timing, event, the name of the last trigger emitted in that group)
    let mut prev: Option<(String, String, String, String)> = None;
    for (i, (table, name, timing, event, stmt, definer, order)) in sorted.iter().enumerate() {
        let same_group = prev
            .as_ref()
            .is_some_and(|(t, ti, e, _)| t == table && ti == timing && e == event);
        let order_clause = match (&prev, same_group, *order > 1) {
            (Some((.., last)), true, true) => Some(TriggerOrder::Follows(last.clone())),
            // The leader of a group of two or more. `*order == 1` excludes the
            // `0` "no information" case, and the successor's name is already in
            // hand — it is the next row, which is in the same group by the sort.
            _ if *order == 1 => sorted
                .get(i + 1)
                .filter(|(t, _, ti, e, ..)| t == table && ti == timing && e == event)
                .map(|(_, next, ..)| TriggerOrder::Precedes(next.clone())),
            _ => None,
        };
        out.push(TriggerInfo {
            name: name.clone(),
            // MySQL has no namespace level between database and table.
            schema: None,
            table: table.clone(),
            // An unreadable timing/event would be a server that grew a new one;
            // fall back to the model's default rather than drop the trigger, so
            // it still shows up and can still be dropped.
            timing: TriggerTiming::parse(timing).unwrap_or_default(),
            events: TriggerEvent::parse(event).into_iter().collect(),
            update_columns: Vec::new(),
            // `ACTION_ORIENTATION` is always ROW on MySQL; there is no other.
            level: schemaic_core::schema::TriggerLevel::Row,
            condition: None,
            action: TriggerAction::Body(stmt.clone()),
            definer: Some(definer.clone()).filter(|d| !d.is_empty()),
            order: order_clause,
            // `information_schema` reports none of the three, and on MySQL 8 the
            // body it *does* report is already unescaped. Both come from
            // `Db::trigger_source`, lazily, when the editor opens.
            sql_mode: None,
            charset_client: None,
            collation_connection: None,
            // All three are PostgreSQL's alone: MySQL has no transition tables
            // and no per-trigger firing mode.
            old_table: None,
            new_table: None,
            enabled: schemaic_core::schema::TriggerEnabled::Origin,
            constraint: false,
        });
        prev = Some((table.clone(), timing.clone(), event.clone(), name.clone()));
    }
    out
}

/// Hang each trigger off the table it fires on, dropping any whose table wasn't
/// in this fetch — the same rule [`assemble_schema`] applies to column rows.
fn apply_triggers(schema: &mut DbSchema, triggers: Vec<TriggerInfo>) {
    // Bucketed once rather than filtered per table — see `group_by`. A table
    // name is unique within the one database a MySQL fetch reads, so each
    // bucket is taken whole.
    let mut by_table = crate::group_by(triggers.into_iter().map(|g| (g.table.clone(), g)));
    for t in schema.tables.iter_mut() {
        t.triggers = by_table.remove(&t.name).unwrap_or_default();
    }
}

/// One [`MY_ROUTINES_SQL`] row.
///
/// A struct rather than the tuple its siblings here are, for two reasons: the
/// query selects fourteen columns, past `mysql_common`'s twelve-element
/// `FromRow` ceiling, and past the point where a positional `.6` in a test says
/// anything about which column it means.
///
/// The body is **nullable** — `ROUTINE_DEFINITION` is NULL for a routine the
/// connected account can't see the definition of — and is the one column here
/// that must not be trusted for an edit; see [`Db::routine_source`].
#[derive(Clone, Debug, Default)]
struct MyRoutineRow {
    name: String,
    /// `ROUTINE_TYPE` — `FUNCTION` or `PROCEDURE`, as the server spells it.
    kind: String,
    /// `DTD_IDENTIFIER`: a function's return type, empty for a procedure.
    returns: String,
    /// The return type's declared character set and collation, which
    /// `DTD_IDENTIFIER` does **not** carry. NULL for anything but a string type.
    returns_charset: Option<String>,
    returns_collation: Option<String>,
    body: Option<String>,
    deterministic: String,
    data_access: String,
    security: String,
    definer: String,
    comment: String,
    /// The session state the routine was created under. See
    /// [`schemaic_core::schema::RoutineSource`] for why a recreate has to
    /// restore it.
    sql_mode: Option<String>,
    charset_client: Option<String>,
    collation_connection: Option<String>,
}

/// The aliases [`MY_ROUTINES_SQL`] gives its columns, in the order it selects
/// them.
///
/// **A third statement of the list, deliberately.** The two that matter are the
/// query and [`my_routine_row_from`]'s reads, and a test that checked one
/// against the other would only be checking whether they were written by the
/// same hand. This is the oracle both are compared against, which is why it is
/// test-only and why it is written out rather than derived from either.
#[cfg(test)]
const MY_ROUTINE_COLUMNS: [&str; 14] = [
    "n", "ty", "rt", "rtcs", "rtcoll", "body", "det", "acc", "sec", "df", "cmt", "sqlmode", "cscl",
    "collconn",
];

/// The routine catalogue read, aliased column by column.
///
/// A `const` rather than a literal at the call so a test can read it: the
/// aliases are what [`my_routine_row`] binds to, and nothing else holds the
/// two in step.
const MY_ROUTINES_SQL: &str = "SELECT CAST(ROUTINE_NAME AS CHAR) AS n, \
     CAST(ROUTINE_TYPE AS CHAR) AS ty, \
     CAST(COALESCE(DTD_IDENTIFIER, '') AS CHAR) AS rt, \
     CAST(CHARACTER_SET_NAME AS CHAR) AS rtcs, \
     CAST(COLLATION_NAME AS CHAR) AS rtcoll, \
     CAST(ROUTINE_DEFINITION AS CHAR) AS body, \
     CAST(IS_DETERMINISTIC AS CHAR) AS det, \
     CAST(SQL_DATA_ACCESS AS CHAR) AS acc, \
     CAST(SECURITY_TYPE AS CHAR) AS sec, \
     CAST(DEFINER AS CHAR) AS df, \
     CAST(COALESCE(ROUTINE_COMMENT, '') AS CHAR) AS cmt, \
     CAST(SQL_MODE AS CHAR) AS sqlmode, \
     CAST(CHARACTER_SET_CLIENT AS CHAR) AS cscl, \
     CAST(COLLATION_CONNECTION AS CHAR) AS collconn \
     FROM information_schema.ROUTINES \
     WHERE ROUTINE_SCHEMA = ? ORDER BY ROUTINE_TYPE, ROUTINE_NAME";

/// Read one [`MY_ROUTINES_SQL`] row, by the aliases it gives its columns.
fn my_routine_row(r: &Row) -> MyRoutineRow {
    my_routine_row_from(|c| r.get::<Option<String>, &str>(c).flatten())
}

/// The name→field half of [`my_routine_row`], over any reader.
///
/// **By alias, not by position.** The struct replaced a tuple precisely because
/// fourteen columns is past `mysql_common`'s twelve-element `FromRow` ceiling —
/// which is the same thing as saying the compiler stopped checking the arity.
/// The reader that replaced it indexed the row `0..=13` against a `SELECT`
/// fifteen hundred lines away, with nothing but a doc comment holding the two
/// in step: insert a column at position 3 and `body` starts reading
/// `CHARACTER_SET_NAME`, `sql_mode` starts reading `ROUTINE_COMMENT`, the suite
/// stays green, and what ships is a routine whose Body field shows `utf8mb3`
/// and a recreate that `DROP`s the routine and re-`CREATE`s it from that — on
/// the engine whose `DROP` commits on its own.
///
/// Split from the `Row` so a test can supply the reader; `mysql_common`'s row
/// constructor isn't re-exported by `mysql_async`, and the decision here is the
/// mapping, not the driver.
///
/// Every column is `CAST(… AS CHAR)`, so a value that fails to convert is a
/// server this app can't read at all; it degrades to the empty string (or
/// `None`) here for the same reason the neighbouring queries `COALESCE` — a
/// missing characteristic must not cost the whole schema.
fn my_routine_row_from(mut opt: impl FnMut(&str) -> Option<String>) -> MyRoutineRow {
    MyRoutineRow {
        name: opt("n").unwrap_or_default(),
        kind: opt("ty").unwrap_or_default(),
        returns: opt("rt").unwrap_or_default(),
        returns_charset: opt("rtcs"),
        returns_collation: opt("rtcoll"),
        body: opt("body"),
        deterministic: opt("det").unwrap_or_default(),
        data_access: opt("acc").unwrap_or_default(),
        security: opt("sec").unwrap_or_default(),
        definer: opt("df").unwrap_or_default(),
        comment: opt("cmt").unwrap_or_default(),
        sql_mode: opt("sqlmode"),
        charset_client: opt("cscl"),
        collation_connection: opt("collconn"),
    }
}

/// One `information_schema.PARAMETERS` row: `(specific name, routine type, mode,
/// parameter name, DTD_IDENTIFIER, character set, collation)`.
type MyParamRow = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
);

/// Restate a declared character set and collation onto a type the catalogue
/// publishes without them.
///
/// `DTD_IDENTIFIER` renders `longtext`, never `longtext CHARACTER SET utf8mb3`,
/// and the two clauses live in their own columns. A recreate that emits only the
/// type re-declares the parameter under the *database's* default character set —
/// a silent change to what the routine accepts, with nothing on screen to say
/// so. MySQL reports both columns only for string types, so a non-NULL value is
/// exactly the case that needs restating.
fn mysql_type_with_charset(dtd: &str, charset: Option<&str>, collation: Option<&str>) -> String {
    let mut out = dtd.trim().to_string();
    if out.is_empty() {
        return out;
    }
    if let Some(cs) = charset.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str(" CHARACTER SET ");
        out.push_str(cs);
    }
    if let Some(coll) = collation.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str(" COLLATE ");
        out.push_str(coll);
    }
    out
}

/// Fold `information_schema.PARAMETERS` into the rendered parameter list each
/// routine's emitter wants, keyed by name **and kind** because a function and a
/// procedure may share a name.
///
/// **The mode is rendered only for a procedure.** The catalogue reports
/// `PARAMETER_MODE = 'IN'` for a function's parameters too, but `CREATE
/// FUNCTION`'s grammar is `param_name type` — the mode keywords belong to
/// `proc_parameter` alone, and both vendors' manuals say specifying one "is
/// valid only for a PROCEDURE". Joining the mode in for a function emitted a
/// `CREATE` the server answers 1064 to, *after* the recreate's `DROP` had
/// committed on its own: the function was destroyed and nothing replaced it.
///
/// **And the name is quoted, which is the same failure by the other half of the
/// same line.** `PARAMETER_NAME` is the *bare* name — a procedure declared
/// ``p(`order` INT)`` reports `order` — so joining it raw emitted
/// `CREATE PROCEDURE p(IN order INT)` and cost the routine in exactly the way
/// the paragraph above describes. Reproduced live on MariaDB 10.11.14 and MySQL
/// 8.4.11. Through [`export::ident_if_needed`], the project's one quoter for
/// SQL a user also reads — this string is what the editor's Parameters field
/// shows — so an ordinary lower-case name stays bare and no rendered list a
/// user is already looking at changes.
fn mysql_parameters(rows: &[MyParamRow]) -> HashMap<(String, String), Vec<String>> {
    let mut params: HashMap<(String, String), Vec<String>> = HashMap::new();
    for (name, ty, mode, pname, dtd, charset, collation) in rows {
        let kind = ty.to_ascii_uppercase();
        let mode = if kind == "PROCEDURE" { mode.trim() } else { "" };
        let dtd = mysql_type_with_charset(dtd, charset.as_deref(), collation.as_deref());
        let pname = export::ident_if_needed(pname.trim(), SqlDialect::MySql);
        let rendered = [mode, pname.as_str(), dtd.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        params
            .entry((name.clone(), kind))
            .or_default()
            .push(rendered);
    }
    params
}

/// Fold MySQL's `information_schema.ROUTINES` rows into [`RoutineInfo`]s.
///
/// `arguments` arrives separately, from `information_schema.PARAMETERS`: that
/// table has one row per parameter and MySQL has no rendered-signature column at
/// all, so the `IN a INT, OUT b TEXT` form the emitter and the tree both want is
/// rebuilt by [`mysql_parameters`].
///
/// A function's own return value is `PARAMETERS` ordinal **0**, which is why the
/// caller's query excludes it: folded in, every function's parameter list would
/// open with its return type.
fn mysql_routines(
    rows: &[MyRoutineRow],
    params: &HashMap<(String, String), Vec<String>>,
) -> Vec<RoutineInfo> {
    rows.iter()
        .map(|r| {
            let kind = schemaic_core::schema::RoutineKind::parse(&r.kind);
            let some = |s: &String| Some(s.clone()).filter(|s| !s.is_empty());
            RoutineInfo {
                name: r.name.clone(),
                // MySQL has no namespace level: the database *is* the
                // namespace, exactly as it is for a table.
                schema: None,
                kind,
                arguments: params
                    .get(&(r.name.clone(), r.kind.to_ascii_uppercase()))
                    .map(|p| p.join(", "))
                    .unwrap_or_default(),
                // A procedure's `DTD_IDENTIFIER` is NULL and arrives as the
                // empty string, which is what the model wants there.
                returns: mysql_type_with_charset(
                    &r.returns,
                    r.returns_charset.as_deref(),
                    r.returns_collation.as_deref(),
                ),
                // Everything MySQL stores is `SQL`; the column reports it and
                // the emitter never writes a `LANGUAGE` clause for it.
                language: "SQL".to_string(),
                body: r.body.clone().unwrap_or_default(),
                deterministic: r.deterministic.eq_ignore_ascii_case("YES"),
                data_access: schemaic_core::schema::SqlDataAccess::parse(&r.data_access),
                // **DEFINER is this engine's default**, the opposite of
                // PostgreSQL's — so an unreadable value must not fall to
                // `false` and quietly re-declare the routine as INVOKER.
                security_definer: !r.security.eq_ignore_ascii_case("INVOKER"),
                definer: some(&r.definer),
                comment: some(&r.comment),
                // The catalogue carries the same session state `SHOW CREATE`
                // prints, so a draft has it from the first frame and the lazy
                // read only ever corrects the *body*.
                sql_mode: r.sql_mode.clone(),
                charset_client: r.charset_client.clone(),
                collation_connection: r.collation_connection.clone(),
                // PostgreSQL's.
                ..Default::default()
            }
        })
        .collect()
}

/// One [`MY_EVENTS_SQL`] row.
///
/// A struct rather than a tuple, on the same two grounds [`MyRoutineRow`] is
/// one: sixteen columns is past `mysql_common`'s twelve-element `FromRow`
/// ceiling, and a positional `.9` would say nothing about which column it means.
///
/// The body is **nullable** for the same reason a routine's is, and carries the
/// same warning: `EVENT_DEFINITION` has had its escapes resolved, so it is what
/// the tree *reads* and never what an edit is emitted from. See
/// [`Db::event_source`].
#[derive(Clone, Debug, Default)]
struct MyEventRow {
    name: String,
    definer: String,
    /// `EVENT_TYPE` — `ONE TIME` or `RECURRING`, as the server spells it. The
    /// tag that decides which [`EventSchedule`] arm the other five columns are
    /// read into; the alternative, "is `EXECUTE_AT` NULL", is the same question
    /// asked of a column that is also NULL for a recurring event whose
    /// `INTERVAL_VALUE` failed to convert.
    kind: String,
    execute_at: Option<String>,
    interval_value: Option<String>,
    interval_field: Option<String>,
    starts: Option<String>,
    ends: Option<String>,
    status: String,
    on_completion: String,
    comment: String,
    body: Option<String>,
    /// The session state the event was created under, plus the time zone its
    /// schedule is read in. See [`schemaic_core::schema::EventInfo`] for why the
    /// fourth one is not optional decoration.
    time_zone: Option<String>,
    sql_mode: Option<String>,
    charset_client: Option<String>,
    collation_connection: Option<String>,
}

/// The aliases [`MY_EVENTS_SQL`] gives its columns, in the order it selects
/// them. A third statement of the list, for the reason
/// [`MY_ROUTINE_COLUMNS`] is one.
#[cfg(test)]
const MY_EVENT_COLUMNS: [&str; 16] = [
    "n", "df", "ty", "at", "iv", "if_", "st", "en", "stat", "oc", "cmt", "body", "tz", "sqlmode",
    "cscl", "collconn",
];

/// The scheduled-event catalogue read, aliased column by column.
const MY_EVENTS_SQL: &str = "SELECT CAST(EVENT_NAME AS CHAR) AS n, \
     CAST(DEFINER AS CHAR) AS df, \
     CAST(EVENT_TYPE AS CHAR) AS ty, \
     CAST(EXECUTE_AT AS CHAR) AS at, \
     CAST(INTERVAL_VALUE AS CHAR) AS iv, \
     CAST(INTERVAL_FIELD AS CHAR) AS if_, \
     CAST(STARTS AS CHAR) AS st, \
     CAST(ENDS AS CHAR) AS en, \
     CAST(STATUS AS CHAR) AS stat, \
     CAST(ON_COMPLETION AS CHAR) AS oc, \
     CAST(COALESCE(EVENT_COMMENT, '') AS CHAR) AS cmt, \
     CAST(EVENT_DEFINITION AS CHAR) AS body, \
     CAST(TIME_ZONE AS CHAR) AS tz, \
     CAST(SQL_MODE AS CHAR) AS sqlmode, \
     CAST(CHARACTER_SET_CLIENT AS CHAR) AS cscl, \
     CAST(COLLATION_CONNECTION AS CHAR) AS collconn \
     FROM information_schema.EVENTS \
     WHERE EVENT_SCHEMA = ? ORDER BY EVENT_NAME";

/// Read one [`MY_EVENTS_SQL`] row, by the aliases it gives its columns.
fn my_event_row(r: &Row) -> MyEventRow {
    my_event_row_from(|c| r.get::<Option<String>, &str>(c).flatten())
}

/// The name→field half of [`my_event_row`], over any reader. **By alias, not by
/// position**, for the reason [`my_routine_row_from`] spells out at length.
fn my_event_row_from(mut opt: impl FnMut(&str) -> Option<String>) -> MyEventRow {
    MyEventRow {
        name: opt("n").unwrap_or_default(),
        definer: opt("df").unwrap_or_default(),
        kind: opt("ty").unwrap_or_default(),
        execute_at: opt("at"),
        interval_value: opt("iv"),
        interval_field: opt("if_"),
        starts: opt("st"),
        ends: opt("en"),
        status: opt("stat").unwrap_or_default(),
        on_completion: opt("oc").unwrap_or_default(),
        comment: opt("cmt").unwrap_or_default(),
        body: opt("body"),
        time_zone: opt("tz"),
        sql_mode: opt("sqlmode"),
        charset_client: opt("cscl"),
        collation_connection: opt("collconn"),
    }
}

/// Fold `information_schema.EVENTS` rows into [`EventInfo`]s.
///
/// The one decision here is the schedule. `EVENT_TYPE` says which of the two
/// shapes the row is carrying, and the timestamps and the interval quantity are
/// quoted into SQL expressions on the way in — see [`event_time_expr`] for why
/// the model holds expressions rather than values.
///
/// A `RECURRING` row with no readable interval is read as `EVERY 1 DAY` rather
/// than dropped: an event Schemaic can't fully describe is still one the user
/// must be able to see, rename, disable and drop, and the schedule is the field
/// the editor shows them before anything is applied.
fn mysql_events(rows: &[MyEventRow]) -> Vec<EventInfo> {
    let d = SqlDialect::MySql;
    let some = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
    rows.iter()
        .map(|r| {
            let one_shot = r.kind.trim().eq_ignore_ascii_case("ONE TIME");
            let schedule = if one_shot {
                EventSchedule::At(
                    r.execute_at
                        .as_deref()
                        .and_then(|s| event_time_expr(s, d))
                        // **Both arms fall back, and to something legal.** An
                        // empty `AT` is what `EventDraft::validate` refuses, so
                        // it wouldn't have been an event with an unknown time —
                        // it would have been an event that cannot be renamed,
                        // disabled or commented, because Preview stays disabled
                        // while a draft is invalid.
                        //
                        // The fabricated value cannot reach the server on its
                        // own: `event_alter_clauses` restates `ON SCHEDULE` only
                        // when it *changed*, and it hasn't until the user edits
                        // it — at which point they are looking at the field.
                        // That is the same property that makes `EVERY 1 DAY`
                        // below safe.
                        .unwrap_or_else(|| "CURRENT_TIMESTAMP".to_string()),
                )
            } else {
                EventSchedule::Every {
                    value: r
                        .interval_value
                        .as_deref()
                        .map(|s| event_interval_expr(s, d))
                        .unwrap_or_else(|| "1".to_string()),
                    unit: r
                        .interval_field
                        .as_deref()
                        .map(|s| s.trim().to_ascii_uppercase())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "DAY".to_string()),
                    starts: r.starts.as_deref().and_then(|s| event_time_expr(s, d)),
                    ends: r.ends.as_deref().and_then(|s| event_time_expr(s, d)),
                }
            };
            EventInfo {
                name: r.name.clone(),
                // MySQL is the only engine with events, and a database is the
                // namespace there.
                schema: None,
                definer: some(&r.definer),
                schedule,
                // `ON_COMPLETION` reads `PRESERVE` or `NOT PRESERVE`; anything
                // else is read as the server's default, which is not to keep it.
                preserve: r.on_completion.trim().eq_ignore_ascii_case("PRESERVE"),
                status: EventStatus::parse(&r.status),
                comment: some(&r.comment),
                // `information_schema`'s copy — good enough to read and to copy
                // as DDL, and corrected by `Db::event_source` before an edit.
                body: r.body.clone().unwrap_or_default(),
                time_zone: r.time_zone.clone(),
                sql_mode: r.sql_mode.clone(),
                charset_client: r.charset_client.clone(),
                collation_connection: r.collation_connection.clone(),
            }
        })
        .collect()
}

/// The body of a `SHOW CREATE EVENT` statement — everything after its top-level
/// `DO`.
///
/// Simpler than [`routine_body_of`] because the keyword that opens the body is a
/// keyword: there is no parameter list to walk past and no characteristic list
/// to step over. What it still needs is [`sql::skip_noncode`], for the two ways
/// a bare byte scan gets this wrong — an event named `` `do` `` (a quoted
/// identifier) and a `COMMENT 'run this, do not touch'` (a string literal), both
/// of which sit before the real `DO`.
///
/// `None` when there is no top-level `DO` at all, which is a statement this
/// build doesn't understand; the caller keeps the body it already had rather
/// than blanking it.
fn event_body_of(create_sql: &str) -> Option<String> {
    let b = create_sql.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(j) = sql::skip_noncode(b, i, SqlDialect::MySql) {
            i = j.max(i + 1);
            continue;
        }
        if sql::is_word_start(b[i]) {
            let mut j = i + 1;
            while j < b.len() && sql::is_word_byte(b[j]) {
                j += 1;
            }
            if create_sql[i..j].eq_ignore_ascii_case("DO") {
                return Some(create_sql.get(j..)?.trim().to_string());
            }
            i = j;
            continue;
        }
        i += 1;
    }
    None
}

/// One `information_schema.VIEWS` row: `(name, definition, check option, definer,
/// security type, algorithm)`. The algorithm is `None` on MySQL, which doesn't
/// report it.
type MyViewRow = (String, String, String, String, String, Option<String>);

/// Fold MySQL's view options onto the assembled views. Kept out of
/// [`assemble_schema`] for the same reason [`apply_table_options`] is: half of
/// these have no PostgreSQL equivalent.
fn apply_view_options(schema: &mut DbSchema, rows: &[MyViewRow]) {
    let by_name: HashMap<&str, &MyViewRow> = rows.iter().map(|r| (r.0.as_str(), r)).collect();
    for t in schema.tables.iter_mut().filter(|t| t.is_view) {
        if let Some((_, _, check, definer, security, algorithm)) = by_name.get(t.name.as_str()) {
            t.view_options = Some(mysql_view_options(
                check,
                definer,
                security,
                algorithm.as_deref(),
            ));
        }
    }
}

/// A view's options as the catalogue reports them, in the form the emitter
/// wants: the values that *mean* "unset" (`NONE`, `UNDEFINED`, empty) become
/// `None`, so an untouched view round-trips to no change and nothing needless is
/// restated. Pure + tested — like [`mysql_column`], getting this wrong writes a
/// *different* view rather than failing.
pub(crate) fn mysql_view_options(
    check: &str,
    definer: &str,
    security: &str,
    algorithm: Option<&str>,
) -> ViewOptions {
    let set = |s: &str, unset: &str| {
        let s = s.trim();
        (!s.is_empty() && !s.eq_ignore_ascii_case(unset)).then(|| s.to_ascii_uppercase())
    };
    ViewOptions {
        check_option: set(check, "NONE"),
        // Not upper-cased: an account name is data, not a keyword.
        definer: Some(definer.trim().to_string()).filter(|d| !d.is_empty()),
        // Both values matter. `INVOKER` has to be restated or it reverts to the
        // default, and `DEFINER` is what that default *is* — restating it costs
        // nothing and keeps the emitted statement explicit.
        security: set(security, ""),
        algorithm: algorithm.and_then(|a| set(a, "UNDEFINED")),
        ..Default::default()
    }
}

/// One `information_schema.TABLES` row: `(name, type, engine, collation,
/// comment)`.
type MyTableRow = (String, String, Option<String>, Option<String>, String);

/// Fold MySQL's table-level options onto the assembled tables. Kept out of
/// [`assemble_schema`] (which both engines share) because PostgreSQL has no
/// equivalent of either the engine or the table collation.
fn apply_table_options(schema: &mut DbSchema, rows: &[MyTableRow]) {
    let by_name: HashMap<&str, &MyTableRow> = rows.iter().map(|r| (r.0.as_str(), r)).collect();
    for t in &mut schema.tables {
        let Some((.., engine, collation, comment)) = by_name.get(t.name.as_str()) else {
            continue;
        };
        t.engine = engine.clone().filter(|e| !e.is_empty());
        t.collation = collation.clone().filter(|c| !c.is_empty());
        t.comment = Some(comment.clone()).filter(|c| !c.is_empty());
    }
}

/// Attach each foreign key's referential actions. `NO ACTION` is the standard
/// default and both engines leave it unwritten, so it stays `None` — which is
/// exactly what makes an untouched key round-trip to no change at all.
fn apply_fk_rules(schema: &mut DbSchema, rows: &[(String, String, String, String)]) {
    let keep = |rule: &str| {
        let r = rule.trim();
        (!r.is_empty() && !r.eq_ignore_ascii_case("NO ACTION")).then(|| r.to_uppercase())
    };
    // Each table found by name once, not by a scan per rule — see `group_by`.
    // The first table of a name wins, as the scan's `find` did.
    let mut index: HashMap<String, usize> = HashMap::with_capacity(schema.tables.len());
    for (i, t) in schema.tables.iter().enumerate() {
        index.entry(t.name.clone()).or_insert(i);
    }
    for (table, name, on_delete, on_update) in rows {
        let Some(t) = index.get(table).map(|&i| &mut schema.tables[i]) else {
            continue;
        };
        let Some(fk) = t.foreign_keys.iter_mut().find(|f| f.name == *name) else {
            continue;
        };
        fk.on_delete = keep(on_delete);
        fk.on_update = keep(on_update);
    }
}

/// A raw `information_schema.COLUMNS` row as the MySQL fetch selects it.
pub(crate) type MyColRow = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Turn one MySQL/MariaDB catalogue row into a [`ColumnInfo`].
///
/// The whole reason this isn't a field-for-field copy is `COLUMN_DEFAULT`, where
/// the two servers genuinely disagree:
///
/// * **MariaDB** returns SQL text — a string default comes back *already quoted*
///   (`'draft'`), and an explicit `DEFAULT NULL` comes back as the four
///   characters `NULL`. It can be emitted verbatim.
/// * **MySQL** returns the *raw value* — `draft`, unquoted and indistinguishable
///   from the expression `draft`. Emitting it verbatim produces
///   `DEFAULT draft`, which is a syntax error at best and a column reference at
///   worst, so a non-expression default on a non-numeric column has to be quoted
///   here.
///
/// Expressions are told apart by `EXTRA`, which carries `DEFAULT_GENERATED` for
/// them on MySQL 8, plus the `CURRENT_TIMESTAMP` family which predates that flag.
/// Getting this wrong doesn't fail loudly — it writes a *different default* — so
/// it's normalized once, here, and the model downstream is plain SQL text.
pub(crate) fn mysql_column(r: MyColRow, mariadb: bool) -> ColRow {
    let (table, name, type_name, nullable, key, default, extra, collation, comment, generated) = r;
    let extra_lc = extra.to_ascii_lowercase();
    // **`classify_column_type`, not a prefix list.** This was eight prefixes
    // matched with `starts_with`, and MySQL's `COLUMN_TYPE` for four of its six
    // integer types begins with none of them: `bigint`, `tinyint`, `smallint`
    // and `mediumint` all fell to the quoting arm below, so on MySQL 8.4.11 a
    // `BIGINT DEFAULT 7` came back as `'7'` — and `BOOLEAN`, which is
    // `tinyint(1)`, with it. Nothing broke on apply (the server normalises the
    // quoted form back to a number), but it is the text the designer shows and
    // the text a MySQL-to-MariaDB comparison diffs, so every such column
    // reported a difference that did not exist.
    //
    // The classifier is the one place that knows a type keyword from its
    // spelling, and it is already exhaustive over both engines' vocabularies.
    //
    // **`YEAR` is the one the classifier cannot answer for**, and it is named
    // rather than inherited. It sits with `date`/`datetime`/`time` there, which
    // is right for the *icon* the classifier was written for and wrong here: a
    // `YEAR` default is a bare number on both servers while a `DATE` default is
    // a quoted string. Quoting it made a MySQL-to-MariaDB compare report the
    // column Differing, emit a `SET DEFAULT '2024'` the server normalises back,
    // and report the same difference on the next run — for ever.
    let numeric_or_bool = matches!(
        schemaic_core::schema::classify_column_type(&type_name),
        schemaic_core::schema::ColumnTypeClass::Numeric
            | schemaic_core::schema::ColumnTypeClass::Boolean
    ) || type_name
        .split(['(', ' '])
        .next()
        .is_some_and(|k| k.eq_ignore_ascii_case("year"));
    let default = default.and_then(|d| {
        if mariadb {
            // Already SQL text. MariaDB writes a *missing* default as SQL NULL
            // and an explicit `DEFAULT NULL` as the literal text — both mean "no
            // default worth emitting" on a nullable column.
            (d != "NULL").then_some(d)
        } else if extra_lc.contains("default_generated") {
            // **An expression default carries the same extra backslash level
            // MySQL 8 puts on a `CHECK_CLAUSE`**, and this is the other
            // catalogue column that has it: `DEFAULT (CONCAT('a','c'))` comes
            // back as `concat(_utf8mb3\'a\',_utf8mb3\'c\')` (HEX-verified),
            // and restating that is `ERROR 1064` rather than a subtly different
            // default. `mysql_check_clause` is the one unescaper and it
            // short-circuits on MariaDB, which returns `concat('a','c')`
            // already runnable — unescaping there would eat the backslash out
            // of `'it\'s'` and change what the default means.
            Some(mysql_check_clause(&d, mariadb))
        } else if numeric_or_bool || d.to_ascii_uppercase().starts_with("CURRENT_TIMESTAMP") {
            Some(d)
        } else {
            // This is the MySQL/MariaDB introspection path by construction, and
            // the quoting has to match what the emitter would write — otherwise
            // a backslash-bearing default is corrupted on the way *in* and
            // `TableDraft::from_table` produces a draft that differs from the
            // server without the designer showing any change.
            Some(schemaic_core::schema::ddl_string(
                &d,
                schemaic_core::intel::SqlDialect::MySql,
            ))
        }
    });
    ColRow {
        table,
        column: ColumnInfo {
            name,
            type_name,
            nullable: nullable.eq_ignore_ascii_case("YES"),
            primary_key: key == "PRI",
            default,
            auto_increment: extra_lc.contains("auto_increment"),
            // MySQL has no `GENERATED ALWAYS AS IDENTITY`: `AUTO_INCREMENT`
            // always accepts an explicit value.
            identity_always: false,
            // `GENERATION_EXPRESSION` is the empty string, not NULL, for an
            // ordinary column.
            generated: generated.filter(|g| !g.is_empty()),
            on_update: extra_lc
                .contains("on update current_timestamp")
                .then(|| "CURRENT_TIMESTAMP".to_string()),
            comment: comment.filter(|c| !c.is_empty()),
            collation,
            // MySQL reports `VIRTUAL GENERATED` / `STORED GENERATED` in `EXTRA`,
            // and its emitter restates neither — the flag is SQLite's, where the
            // rebuild has to write the word back or the column stops being
            // materialised.
            generated_stored: extra_lc.contains("stored generated"),
            // No such keyword on MySQL: `AUTO_INCREMENT` above is the whole
            // answer, and it already promises not to reuse a value.
            sqlite_autoincrement: false,
            // The fourth question `EXTRA` answers, and the one that was not
            // asked. MySQL 8.0.23+ and MariaDB 10.3+ publish `INVISIBLE` here
            // beside `auto_increment`, `on update current_timestamp` and
            // `stored generated`; with nothing to read it into, a column the
            // DBA had retired from `SELECT *` was restated visible by the dump,
            // by Copy DDL and by `create_ddl_script`.
            invisible: extra_lc.contains("invisible"),
        },
    }
}

// ── Lazily-fetched object sources ────────────────────────────────────────────
//
// Four `SHOW CREATE` reads, each answering for one object the editor is about to
// open. They are MySQL's alone — the dispatcher answers `Ok(None)` for the other
// two engines before reaching here — and they are *not* optimisations: what
// `information_schema` holds for each of these is the body with its escapes
// already resolved, so a restate built from it can be refused over a quote
// nobody typed, or can commit a `DROP` and then fail to recreate.

/// A routine's body **as written**, plus the session state it was written under.
pub(crate) async fn routine_source(
    db: &Db,
    database: Option<&str>,
    kind: schemaic_core::schema::RoutineKind,
    name: &str,
) -> Result<Option<schemaic_core::schema::RoutineSource>, DbError> {
    let mut conn = db.open(database, false).await?;
    // (Procedure|Function, sql_mode, Create …, character_set_client,
    //  collation_connection, Database Collation)
    let sql = format!("SHOW CREATE {} {}", kind.sql_keyword(), ident(name.trim()));
    let row: Option<MyShowCreateRoutineRow> = conn
        .query_first(sql.as_str())
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;
    let _ = conn.disconnect().await;
    let some = |s: String| Some(s).filter(|s| !s.is_empty());
    // **The session state survives a body this can't read.** All four values
    // come from the same row and only the body needs parsing, so folding
    // them into its success meant a routine with an unfamiliar header was
    // later recreated under whatever `sql_mode` the applying session had.
    Ok(row.map(
        |(_, mode, create, cs, coll, ..)| schemaic_core::schema::RoutineSource {
            body: create.as_deref().and_then(routine_body_of),
            sql_mode: some(mode),
            charset_client: some(cs),
            collation_connection: some(coll),
            // Read off the *header*, so it survives a body this can't parse
            // for the same reason the session state does — and the header
            // is the only place it exists at all.
            aggregate: create.as_deref().is_some_and(routine_is_aggregate),
        },
    ))
}

/// An event's body **as written**, plus the session state and the time zone.
pub(crate) async fn event_source(
    db: &Db,
    database: Option<&str>,
    name: &str,
) -> Result<Option<EventSource>, DbError> {
    let mut conn = db.open(database, false).await?;
    // (Event, sql_mode, time_zone, Create Event, character_set_client,
    //  collation_connection, Database Collation)
    let sql = format!("SHOW CREATE EVENT {}", ident(name.trim()));
    let row: Option<MyShowCreateEventRow> = conn
        .query_first(sql.as_str())
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;
    let _ = conn.disconnect().await;
    let some = |s: String| Some(s).filter(|s| !s.is_empty());
    // **The session state survives a body this can't read**, exactly as it
    // does for a routine: all five values come from one row and only the
    // body needs parsing.
    Ok(row.map(|(_, mode, tz, create, cs, coll, ..)| EventSource {
        body: create.as_deref().and_then(event_body_of),
        time_zone: some(tz),
        sql_mode: some(mode),
        charset_client: some(cs),
        collation_connection: some(coll),
    }))
}

/// A trigger's body **as written**, plus the session state it was written under.
///
/// **Both MySQL-family servers reach here, and this doc used to say MariaDB did
/// not.** `Db::trigger_source`'s only gate is `engine != Engine::MySql`, and
/// both flavours are `Engine::MySql` — so the claim that MariaDB "never reaches
/// here" was false of the code that dispatches to it, and the word-boundary
/// defect below was measured on MariaDB 10.11.14 through exactly this path.
/// MariaDB's `ACTION_STATEMENT` is indeed faithful, which is why the second
/// round trip is redundant *there* rather than absent; skipping it is a
/// behaviour change nobody has argued for, and `SHOW CREATE TRIGGER` is the
/// more faithful source of the two either way.
pub(crate) async fn trigger_source(
    db: &Db,
    database: Option<&str>,
    trigger: &str,
) -> Result<Option<TriggerSource>, DbError> {
    let mut conn = db.open(database, false).await?;
    // (Trigger, sql_mode, SQL Original Statement, character_set_client,
    //  collation_connection, Database Collation, Created)
    let sql = format!("SHOW CREATE TRIGGER {}", ident(trigger));
    let row: Option<MyShowCreateTriggerRow> = conn
        .query_first(sql.as_str())
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;
    let _ = conn.disconnect().await;
    let some = |s: String| Some(s).filter(|s| !s.is_empty());
    Ok(row.and_then(|(_, mode, create, cs, coll, ..)| {
        trigger_body_of(&create).map(|body| TriggerSource {
            body,
            sql_mode: some(mode),
            charset_client: some(cs),
            collation_connection: some(coll),
        })
    }))
}

/// A view's `ALGORITHM` clause, which is MySQL's alone.
pub(crate) async fn view_algorithm(
    db: &Db,
    database: Option<&str>,
    view: &str,
) -> Result<Option<String>, DbError> {
    let mut conn = db.open(database, false).await?;
    // `SHOW CREATE VIEW` returns (View, Create View, charset, collation).
    let sql = format!("SHOW CREATE VIEW {}", ident(view));
    let row: Option<(String, String, String, String)> = conn
        .query_first(sql.as_str())
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;
    let _ = conn.disconnect().await;
    Ok(row.and_then(|(_, create, ..)| view_algorithm_of(&create)))
}

/// The schema introspection's own tests, next to the code they guard.
///
/// **`s` and `cr` are deliberate copies of `lib.rs`'s fixtures**, not an
/// oversight: they build a `ColRow` and a `String`, and `assemble_schema` — which
/// stays in `lib.rs` because `pg.rs` calls it — keeps its own. Two four-line
/// constructors in two test modules is the cheaper of the two wrongs; the other
/// is a `pub(crate)` path from one crate's test module into another's, which
/// makes the fixtures API.
#[cfg(test)]
mod schema_tests {
    use super::*;

    fn s(x: &str) -> String {
        x.to_string()
    }

    /// A raw catalogue row, for the default-normalization tests.
    fn my_row(ty: &str, default: Option<&str>, extra: &str) -> MyColRow {
        (
            s("t"),
            s("c"),
            s(ty),
            s("YES"),
            s(""),
            default.map(s),
            s(extra),
            None,
            None,
            None,
        )
    }

    /// MySQL hands back a string default *unquoted*, so emitting it verbatim
    /// would produce `DEFAULT draft` — a column reference, not a string.
    #[test]
    fn mysql_quotes_a_raw_string_default() {
        let c = mysql_column(my_row("varchar(20)", Some("draft"), ""), false).column;
        assert_eq!(c.default.as_deref(), Some("'draft'"));
    }

    /// MySQL 8 returns `CHECK_CLAUSE` with one *extra* level of backslash
    /// escaping, so restating it verbatim is a syntax error — measured against
    /// `SHOW CREATE TABLE`, which is the runnable form.
    #[test]
    fn mysql8_check_clauses_are_unescaped_to_the_runnable_form() {
        // Each pair is (what CHECK_CONSTRAINTS returns, what SHOW CREATE TABLE
        // says) for the same constraint on MySQL 8.4.
        let cases = [
            (
                r#"(`s` <> _latin1\'C:\\\\temp\')"#,
                r#"(`s` <> _latin1'C:\\temp')"#,
            ),
            (
                r#"(`s` <> _latin1\'it\\\'s\')"#,
                r#"(`s` <> _latin1'it\'s')"#,
            ),
            (r#"(`s` <> _latin1\'a\\nb\')"#, r#"(`s` <> _latin1'a\nb')"#),
            (
                r#"(not((`s` like _latin1\'%a%\')))"#,
                r#"(not((`s` like _latin1'%a%')))"#,
            ),
            (r#"(`qty` > 0)"#, r#"(`qty` > 0)"#),
            // A **control character in an identifier**, which is where dropping
            // the backslash instead of decoding the escape went wrong: this
            // names a column whose name contains a newline, and the old code
            // produced `nlncol` — a different, non-existent column. Measured on
            // MySQL 8.4.11 (`CHECK_CLAUSE` hex `…606E6C5C6E636F6C60…`, i.e. the
            // two bytes `\` `n`, against a real 0x0A in `SHOW CREATE TABLE`).
            ("(`nl\\ncol` > 0)", "(`nl\ncol` > 0)"),
        ];
        for (raw, want) in cases {
            assert_eq!(mysql_check_clause(raw, false), want, "for {raw}");
        }
    }

    /// **Every method that opens a connection to a server the user is waiting on
    /// carries a deadline.**
    ///
    /// A host that stops answering at the packet level — a dropped VPN, a laptop
    /// off the network, a firewall `DROP` — does not refuse the connect, it
    /// swallows it, and the `open` then takes the OS TCP timeout: 21.0 s on
    /// MySQL, 63 s on PostgreSQL, this file's own measurement (see
    /// `PING_TIMEOUT`). `ping` and `fetch_databases` were bounded for exactly
    /// that; `fetch_sessions` and `kill_session` were not, and `fetch_sessions`
    /// runs **on a timer, forever**, with a de-dup guard keyed on a generation
    /// that a window-focus regain bumps before refreshing — so hung polls
    /// stacked one per alt-tab.
    ///
    /// **It is not the only one, though this doc said so.** The Live Monitor
    /// re-arms `fetch_table` every two seconds for as long as its modal is open,
    /// and that method was left out of this list because it takes a
    /// `CancellationToken` — which bounds nothing unless a caller *keeps* the
    /// token, and the monitor built one inline and cancelled it never. The list
    /// below is the set of methods a timer calls, and a method joining that set
    /// belongs in it.
    ///
    /// `fetch_table`'s deadline is a trade worth naming: a poll that cannot
    /// finish in [`PING_TIMEOUT`] is one the monitor's two-second cadence could
    /// not keep up with anyway, and reporting that is better than a modal
    /// showing the last snapshot with no error for the length of an OS connect
    /// timeout.
    ///
    /// A source gate because the failure is a host that never answers, which no
    /// unit test can stage: a closed port is *refused*, instantly, and the only
    /// way to reproduce the hang is a packet filter. What is checkable is that
    /// the deadline is applied, and that is what this reads.
    #[test]
    fn every_reachability_path_is_bounded_by_a_timeout() {
        // **The file is part of the entry now, because the bound does not always
        // live where the public method does.** Two shapes exist and both are
        // right. `fetch_sessions`, `kill_session` and `fetch_table` wrap the
        // whole dispatch in `lib.rs` — the `mysql::` arm and the `pg::` arm
        // alike — so the bound belongs to the public method. `ping` and
        // `fetch_databases` bound themselves per engine, because each reaches
        // the server differently: `pg::ping` takes the deadline as a parameter,
        // SQLite's wraps an `open` that can block on a dead network share, and
        // MySQL's wraps a `SELECT 1`. Writing the file down per method is what
        // stops this gate reading a file the method has left, which is how a
        // source gate stops testing anything without failing.
        let read = |file: &str| {
            let path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src")).join(file);
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{file}: {e}"))
        };
        // **The signature is assembled, never written out**, for the reason the
        // `mariadb` gate assembles its needle: this test's own table would
        // otherwise be the first match in its own file, and the "body" it then
        // measured would be the table. That is not a hypothetical — the first
        // version of this gate failed on `ping` for exactly that, having read
        // twenty lines of itself instead of the function.
        let sig_of = |file: &str, name: &str| match file {
            // The public dispatcher, indented inside `impl Db`.
            "lib.rs" => format!("    pub {}async fn {name}(", ""),
            // An engine module's free function.
            _ => format!("pub(crate{}) async fn {name}(", ""),
        };
        // The function body as text: from its signature to the next item at the
        // same indent. Enough to see whether a `timeout` wraps it.
        let body_of = |src: &str, sig: &str, name: &str| -> String {
            let at = src
                .find(sig)
                .unwrap_or_else(|| panic!("{name} is gone or was renamed ({sig})"));
            let rest = &src[at..];
            let stop = if sig.starts_with("    ") {
                format!("\n    pub {}async fn ", "")
            } else {
                format!("\npub(crate{}) async fn ", "")
            };
            let end = rest[1..].find(&stop).map_or(rest.len(), |i| i + 1);
            rest[..end].to_string()
        };
        for (file, name, deadline) in [
            // `ping` takes its deadline as a parameter — the callers pass
            // `PING_TIMEOUT` — so what is checked here is that it applies the
            // one it was given.
            ("mysql.rs", "ping", "timeout"),
            ("mysql.rs", "fetch_databases", "crate::PING_TIMEOUT"),
            ("lib.rs", "fetch_sessions", "PING_TIMEOUT"),
            ("lib.rs", "kill_session", "CANCEL_TIMEOUT"),
            // **The second forever-timer, which the doc above said did not
            // exist.** The Live Monitor re-arms `fetch_table` every two seconds
            // for as long as its modal is open, and the premise that admitted it
            // to the "already bounded" set — *the unbounded reads all take a
            // `CancellationToken`* — was true of the signature and false of the
            // caller, which built a token inline, stored it nowhere and
            // cancelled it never. A dark host cost the OS connect timeout on
            // every tick, under a modal still showing the last snapshot with no
            // error, beside a health check that said Disconnected at five
            // seconds.
            ("lib.rs", "fetch_table", "PING_TIMEOUT"),
        ] {
            let src = read(file);
            let body = body_of(&src, &sig_of(file, name), name);
            assert!(
                body.contains(&format!("tokio::time::timeout({deadline}")),
                "`{name}` (in {file}) opens a connection for someone who is \
                 waiting and must bound it with {deadline} — a dark host \
                 otherwise costs the OS connect timeout, and this one repeats"
            );
        }
    }

    /// **Every exit out of `import_on` says what the rollback achieved.**
    ///
    /// `Rollback::note`'s reason for existing is written at `Db::import_rows`:
    /// on a MySQL `MyISAM`/`MEMORY`/`ARCHIVE`/`CSV` target the batches already
    /// sent really are durable, so the error must say so rather than report an
    /// undo that did not happen. Four of the five exits carried it. The fifth —
    /// the final `COMMIT` — was `map_err(qerr)?`, so a connection that died
    /// there put the driver's bare *"Server has gone away"* in the modal over a
    /// table holding every row of the file.
    ///
    /// A source gate because the failure is a socket that dies between two
    /// statements, which no unit test can stage: every arm of `import_on` needs
    /// a live MySQL connection to reach at all. What is checkable is that no
    /// error leaves the function without the note, and that is what this reads.
    #[test]
    fn every_import_exit_says_what_the_rollback_achieved() {
        // **`mysql.rs`, because that is where `import_on` is.** This opened
        // `lib.rs` while the body lived there, and a gate that names a file is
        // worthless the moment its subject leaves it. What saved this one is the
        // `.expect` below: looking for a function by name and panicking when it
        // is absent fails loudly, where a scan that merely finds no offender
        // passes quietly over an empty file. Both shapes exist in this crate and
        // only one of them announces the move.
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/mysql.rs"))
            .expect("this module's own source");
        // **Assembled, so this test is not its own first match.** Now that the
        // gate and its subject share a file, a literal needle finds the line
        // below before it finds the function, and the "body" measured is twenty
        // lines of the test. The floor caught it — `only 0 notes` — which is
        // what a floor is for.
        let needle = format!("async fn import{}on(", '_');
        let at = src
            .find(&needle)
            .expect("`import_on` is gone or was renamed");
        let rest = &src[at..];
        let end = rest[1..]
            .find(
                "
/// ",
            )
            .map_or(rest.len(), |i| i + 1);
        let body = &rest[..end];

        let lines: Vec<&str> = body.lines().collect();
        let mut offenders = Vec::new();
        for (n, line) in lines.iter().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") || !code.contains("return Err(") {
                continue;
            }
            // The whole `return` statement, which may be several lines: either
            // spelling of the note counts — the explicit one, or
            // `cancelled_import`, which is a wrapper over it.
            let stmt: String = lines[n..]
                .iter()
                .take(8)
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            let says = stmt.contains("undone.note()")
                || stmt.contains("cancelled_import(")
                || stmt.contains(".note()");
            if !says {
                offenders.push(format!("import_on +{n}: {code}"));
            }
        }
        assert!(
            offenders.is_empty(),
            "an import failure that does not say what the rollback achieved              reports an undo that may not have happened, over a MySQL table that              may hold the whole file:
{}",
            offenders.join("
")
        );
        // The floor: a negative assertion over a body it could not find passes.
        assert!(
            body.matches("undone.note()").count() >= 3,
            "only {} notes in `import_on` — did they get renamed? Rewrite this              gate rather than letting it pass by finding nothing.",
            body.matches("undone.note()").count()
        );
        // And nothing propagates a failure out with `?` instead, which is what
        // the `COMMIT` exit did.
        for (n, line) in body.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            // The opening `BEGIN` is the one exception and it is not an
            // exemption: nothing has been sent yet, so there is no rollback to
            // describe and no durable batch to warn about.
            if code.contains(r#"query_drop("BEGIN")"#) {
                continue;
            }
            assert!(
                !code.contains("map_err(qerr)?"),
                "import_on +{n}: {code} — a `?` leaves without the note"
            );
        }
    }

    /// **The gate.** One place decides which MySQL-family server this is, and
    /// it is `ServerFlavour::parse_version`.
    ///
    /// `collect_schema` spelled the same test out —
    /// `version.to_ascii_lowercase().contains("mariadb")` — into a local
    /// `bool` driving five branches, then rebuilt the model's `ServerFlavour`
    /// from that `bool` at the bottom of the same function. Two spellings of
    /// one question on the path where the two servers' divergence is a
    /// data-loss class, and a local `bool` is the constant CLAUDE.md's engine
    /// rule names: greppable inside one function, invisible outside it, and
    /// with no `Unknown` arm, so "the server did not say" folded to MySQL
    /// rather than to the documented safe default.
    ///
    /// Line comments are dropped before the scan, so the paragraph in
    /// `collect_schema` that quotes the old spelling is not a hit.
    ///
    /// **It reads the whole crate's `src`, and it did not always.** While
    /// `collect_schema` lived in `lib.rs` this opened that one file by name —
    /// correct then, and silently worthless the moment the function it guards
    /// moved here: the scan would have found no offender in `lib.rs` because
    /// there was no longer any MySQL introspection in `lib.rs` to offend, and a
    /// second `contains("mariadb")` written in *this* file would have gone
    /// unnoticed. That is the failure mode a source-scanning gate has and an
    /// ordinary test does not, and the one the splitting procedure says to check
    /// for by breaking a term and watching it fail. Reading the directory also
    /// makes the gate say what it always meant: one place decides, in this whole
    /// crate, not in whichever file the rule was written down in.
    #[test]
    fn only_one_function_decides_which_mysql_family_server_this_is() {
        let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
        // Assembled, so this line is not its own first offender.
        let needle = format!("contains({}mariadb{})", '"', '"');
        let mut offenders = Vec::new();
        let mut scanned = 0;
        for entry in std::fs::read_dir(dir).expect("this crate's src") {
            let path = entry.expect("a dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("a source file");
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_string();
            scanned += 1;
            for (i, line) in src.lines().enumerate() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                if line.contains(&needle) {
                    offenders.push(format!("{name}:{}: {}", i + 1, line.trim()));
                }
            }
        }
        // The floor every source gate in this workspace carries: a moved or
        // renamed `src` would otherwise make this pass by reading nothing.
        assert!(
            scanned >= 5,
            "only {scanned} source files scanned — has `src` moved?"
        );
        assert!(
            offenders.is_empty(),
            "ask `ServerFlavour::parse_version`:\n{}",
            offenders.join("\n")
        );
    }

    /// MariaDB reports the clause already runnable — its `CHECK_CLAUSE` and its
    /// `SHOW CREATE TABLE` agree byte for byte. Unescaping it would eat the
    /// backslash out of `'it\'s'` and change the predicate.
    #[test]
    fn mariadb_check_clauses_are_left_alone() {
        let raw = r#"`s` <> 'it\'s'"#;
        assert_eq!(mysql_check_clause(raw, true), raw);
    }

    /// A trailing lone backslash has nothing to escape; dropping it would lose a
    /// character rather than an escape.
    #[test]
    fn a_dangling_backslash_survives() {
        assert_eq!(mysql_check_clause(r"a\", false), r"a\");
    }

    /// The clause MySQL 8 puts nowhere else. It sits between `CREATE` and
    /// `DEFINER`, so it's read positionally rather than searched for.
    #[test]
    fn a_views_algorithm_is_read_out_of_show_create_view() {
        assert_eq!(
            view_algorithm_of(
                "CREATE ALGORITHM=MERGE DEFINER=`root`@`localhost` SQL SECURITY DEFINER \
                 VIEW `v` AS select 1"
            )
            .as_deref(),
            Some("MERGE")
        );
        assert_eq!(
            view_algorithm_of("CREATE ALGORITHM = TEMPTABLE DEFINER=`r`@`h` VIEW `v` AS select 1")
                .as_deref(),
            Some("TEMPTABLE")
        );
    }

    /// `UNDEFINED` is the server's default and the emitter leaves it unwritten,
    /// so reading it back as a value would make every view look edited.
    #[test]
    fn an_undefined_or_absent_algorithm_is_none() {
        assert_eq!(
            view_algorithm_of("CREATE ALGORITHM=UNDEFINED DEFINER=`r`@`h` VIEW `v` AS select 1"),
            None
        );
        assert_eq!(
            view_algorithm_of("CREATE DEFINER=`r`@`h` SQL SECURITY DEFINER VIEW `v` AS select 1"),
            None
        );
        assert_eq!(view_algorithm_of(""), None);
    }

    /// The body is the user's own SQL and may say anything. Only the clause in
    /// its fixed position counts — a column called `algorithm=` in a `SELECT`
    /// must not be mistaken for one.
    #[test]
    fn the_view_body_cannot_impersonate_the_clause() {
        assert_eq!(
            view_algorithm_of(
                "CREATE DEFINER=`r`@`h` VIEW `v` AS select 'ALGORITHM=MERGE' as algorithm"
            ),
            None
        );
    }

    /// MariaDB already hands back SQL text, so quoting again would store the
    /// quotes as part of the value.
    #[test]
    fn mariadb_keeps_its_already_quoted_default() {
        let c = mysql_column(my_row("varchar(20)", Some("'draft'"), ""), true).column;
        assert_eq!(c.default.as_deref(), Some("'draft'"));
    }

    /// MariaDB writes an explicit `DEFAULT NULL` as the four characters `NULL`;
    /// that's the same as having no default worth restating.
    #[test]
    fn mariadb_null_default_is_no_default() {
        let c = mysql_column(my_row("varchar(20)", Some("NULL"), ""), true).column;
        assert_eq!(c.default, None);
    }

    /// A numeric default is a literal in both servers — quoting it would change
    /// the type of the stored default.
    ///
    /// **Every one of MySQL's numeric spellings**, because the list this used to
    /// match against covered `int` and missed the four that do not begin with
    /// it. Measured on MySQL 8.4.11: `COLUMN_TYPE` is `bigint` / `tinyint` /
    /// `smallint` / `mediumint` / `tinyint(1)`, and `COLUMN_DEFAULT` is the bare
    /// value.
    #[test]
    fn a_numeric_default_is_never_quoted() {
        for (ty, d) in [
            ("int(11)", "0"),
            ("int", "5"),
            ("bigint", "7"),
            ("tinyint", "1"),
            ("smallint", "2"),
            ("mediumint", "3"),
            // `BOOLEAN` — the catalogue calls it what it is.
            ("tinyint(1)", "1"),
            ("bigint", "-9"),
            ("bigint unsigned", "9"),
            ("decimal(10,2)", "1.50"),
            ("double", "0.5"),
            ("float", "1"),
            ("bit(1)", "b'1'"),
            // **`YEAR` is a bare number on both servers**, and the classifier
            // puts it with `date`/`datetime`/`time` — correct for the *icon* it
            // was written for, and wrong for the quoting question, which is why
            // it has to be named here rather than inherited. MariaDB returns
            // `2024`; MySQL quoted it `'2024'`, so a MySQL-to-MariaDB compare
            // reported the column Differing, emitted a `SET DEFAULT '2024'` the
            // server normalises, and reported the same difference next time —
            // for ever. Verbatim the failure `58e0bd8` fixed for five sibling
            // types, with this one left on the wrong side of the list.
            ("year", "2024"),
            ("year(4)", "0"),
        ] {
            let c = mysql_column(my_row(ty, Some(d), ""), false).column;
            assert_eq!(
                c.default.as_deref(),
                Some(d),
                "{ty} DEFAULT {d} came back quoted"
            );
        }
    }

    /// And the other direction, which is what stops the fix from being "never
    /// quote anything": a string default on MySQL arrives unquoted and
    /// indistinguishable from a column reference, so it has to be quoted here.
    #[test]
    fn a_non_numeric_default_is_still_quoted() {
        for (ty, d, want) in [
            ("varchar(20)", "draft", "'draft'"),
            ("char(2)", "gb", "'gb'"),
            ("text", "7", "'7'"),
            ("date", "2026-01-01", "'2026-01-01'"),
            ("enum('a','b')", "a", "'a'"),
        ] {
            let c = mysql_column(my_row(ty, Some(d), ""), false).column;
            assert_eq!(c.default.as_deref(), Some(want), "{ty} DEFAULT {d}");
        }
    }

    /// The two ways MySQL says "this default is an expression": the 8.0
    /// `DEFAULT_GENERATED` flag, and the `CURRENT_TIMESTAMP` family that predates
    /// it. Quoting either would turn a live expression into a constant string.
    #[test]
    fn an_expression_default_is_left_alone() {
        let c = mysql_column(my_row("timestamp", Some("CURRENT_TIMESTAMP"), ""), false).column;
        assert_eq!(c.default.as_deref(), Some("CURRENT_TIMESTAMP"));
        let c = mysql_column(
            my_row("varchar(36)", Some("(uuid())"), "DEFAULT_GENERATED"),
            false,
        )
        .column;
        assert_eq!(c.default.as_deref(), Some("(uuid())"));
    }

    /// **MySQL 8's expression default carries the same extra backslash level its
    /// `CHECK_CLAUSE` does**, and this branch used to pass it through. Measured
    /// on 8.4.11 with `HEX()` so the escaping is not a rendering artefact:
    /// `b varchar(30) DEFAULT (CONCAT('a','c'))` reports
    /// `concat(_utf8mb3\'a\',_utf8mb3\'c\')`, while `SHOW CREATE TABLE` — the
    /// runnable form — prints `concat(_utf8mb3'a',_utf8mb3'c')`.
    #[test]
    fn a_mysql8_expression_default_is_unescaped_to_the_runnable_form() {
        let c = mysql_column(
            my_row(
                "varchar(30)",
                Some(r"concat(_utf8mb3\'a\',_utf8mb3\'c\')"),
                "DEFAULT_GENERATED",
            ),
            false,
        )
        .column;
        assert_eq!(
            c.default.as_deref(),
            Some("concat(_utf8mb3'a',_utf8mb3'c')")
        );
    }

    /// The half an over-eager fix breaks: MariaDB 10.11.14 returns
    /// `concat('a','c')` already runnable (HEX-verified), so unescaping there
    /// would eat the backslash out of `'it\'s'` and change what the default is.
    #[test]
    fn a_mariadb_expression_default_is_left_exactly_as_it_came() {
        let c = mysql_column(
            my_row(
                "varchar(30)",
                Some(r"concat('it\'s','c')"),
                "DEFAULT_GENERATED",
            ),
            true,
        )
        .column;
        assert_eq!(c.default.as_deref(), Some(r"concat('it\'s','c')"));
    }

    /// **The composition, which is the only thing that says the statement runs.**
    /// The escaping fix alone still emits `DEFAULT concat(…)` bare and MySQL 8
    /// answers `ERROR 1064`; the parens fix alone still emits `\'` inside. One
    /// runnable clause needs both, so the test that guards it has to go from the
    /// catalogue row all the way to the emitted SQL — asserted against what
    /// `SHOW CREATE TABLE` prints for the same column on 8.4.11.
    #[test]
    fn a_mysql8_expression_default_survives_the_round_trip_to_emitted_ddl() {
        let c = mysql_column(
            my_row(
                "varchar(30)",
                Some(r"concat(_utf8mb3\'a\',_utf8mb3\'c\')"),
                "DEFAULT_GENERATED",
            ),
            false,
        )
        .column;
        let sql = c.definition_sql(schemaic_core::intel::SqlDialect::MySql);
        assert!(
            sql.ends_with("DEFAULT (concat(_utf8mb3'a',_utf8mb3'c'))"),
            "{sql}"
        );
        assert!(!sql.contains('\\'), "{sql}");
    }

    /// `EXTRA` is where MySQL keeps the two attributes `MODIFY COLUMN` would
    /// otherwise drop.
    #[test]
    fn extra_carries_auto_increment_and_on_update() {
        let c = mysql_column(my_row("int", None, "auto_increment"), false).column;
        assert!(c.auto_increment);
        let c = mysql_column(
            my_row(
                "timestamp",
                Some("CURRENT_TIMESTAMP"),
                "DEFAULT_GENERATED on update CURRENT_TIMESTAMP",
            ),
            false,
        )
        .column;
        assert_eq!(c.on_update.as_deref(), Some("CURRENT_TIMESTAMP"));
    }

    /// **`EXTRA` answers a fourth question, and it was not asked.** MySQL
    /// 8.0.23+ and MariaDB 10.3+ publish `INVISIBLE` in the same column as
    /// `auto_increment` and `stored generated`. Marking a column invisible is
    /// how a DBA retires one without breaking an application; with nothing
    /// reading it, every recreate path restated the column *visible* and
    /// `SELECT *` silently started returning it again.
    ///
    /// The test that names this column's contents is the one above, and its own
    /// title enumerates two of the four.
    #[test]
    fn extra_carries_an_invisible_column_too() {
        let c = mysql_column(my_row("varchar(64)", None, "INVISIBLE"), false).column;
        assert!(c.invisible, "the retirement was read as an ordinary column");
        // And an ordinary column is not marked, on either server.
        assert!(
            !mysql_column(my_row("int", None, ""), false)
                .column
                .invisible
        );
        assert!(
            !mysql_column(my_row("int", None, "auto_increment"), true)
                .column
                .invisible
        );
        // It travels with the rest: MySQL writes `INVISIBLE` after the default,
        // so a column can be both.
        let c = mysql_column(
            my_row("varchar(64)", Some("x"), "DEFAULT_GENERATED INVISIBLE"),
            true,
        )
        .column;
        assert!(c.invisible);
    }

    fn tr(table: &str, name: &str, timing: &str, event: &str, order: u64) -> MyTriggerRow {
        (
            s(table),
            s(name),
            s(timing),
            s(event),
            s("SET NEW.x = 1"),
            s("root@localhost"),
            order,
        )
    }

    /// Verbatim `SHOW CREATE TRIGGER` output from MySQL 8.4.11 — the two bodies
    /// `information_schema.ACTION_STATEMENT` corrupts.
    ///
    /// Through that column they come back as `SET NEW.a = 'C:<TAB>emp'` (the
    /// `\t` already resolved, hex `…27433A09656D7027`) and `SET NEW.b = 'it's'`
    /// (a 1064 on restate). Here they are exactly as written.
    #[test]
    fn trigger_body_survives_the_escapes_information_schema_resolves() {
        let bs = "CREATE DEFINER=`schemaic`@`%` TRIGGER `wp5_bs` BEFORE INSERT ON `wp5` \
                  FOR EACH ROW SET NEW.a = 'C:\\temp'";
        assert_eq!(
            trigger_body_of(bs).as_deref(),
            Some("SET NEW.a = 'C:\\temp'")
        );
        let q = "CREATE DEFINER=`schemaic`@`%` TRIGGER `wp5_q` BEFORE UPDATE ON `wp5` \
                 FOR EACH ROW SET NEW.b = 'it''s'";
        assert_eq!(trigger_body_of(q).as_deref(), Some("SET NEW.b = 'it''s'"));
    }

    /// The anchor is found at a *code* position, so an identifier holding the
    /// words can't be mistaken for it — a table really can be named this.
    #[test]
    fn trigger_body_anchor_ignores_the_words_inside_an_identifier() {
        let sql = "CREATE DEFINER=`root`@`%` TRIGGER `t` BEFORE INSERT ON \
                   `a FOR EACH ROW b` FOR EACH ROW SET NEW.x = 1";
        assert_eq!(trigger_body_of(sql).as_deref(), Some("SET NEW.x = 1"));
        // …and a body that mentions them is returned whole.
        let sql = "CREATE TRIGGER `t` BEFORE INSERT ON `t2` FOR EACH ROW \
                   SET NEW.note = 'FOR EACH ROW'";
        assert_eq!(
            trigger_body_of(sql).as_deref(),
            Some("SET NEW.note = 'FOR EACH ROW'")
        );
    }

    /// The ordering clause belongs to `TriggerInfo::order`, which the emitter
    /// writes back — carrying it in the body too would emit it twice.
    #[test]
    fn trigger_body_drops_the_ordering_clause() {
        for sql in [
            "CREATE TRIGGER `b` BEFORE INSERT ON `t` FOR EACH ROW FOLLOWS `a` SET NEW.x = 1",
            "CREATE TRIGGER `b` BEFORE INSERT ON `t` FOR EACH ROW PRECEDES `a` SET NEW.x = 1",
            // A quoted name holding the next keyword must be stepped over whole.
            "CREATE TRIGGER `b` BEFORE INSERT ON `t` FOR EACH ROW FOLLOWS `a SET x` SET NEW.x = 1",
        ] {
            assert_eq!(
                trigger_body_of(sql).as_deref(),
                Some("SET NEW.x = 1"),
                "{sql}"
            );
        }
    }

    /// **`FOLLOWS`/`PRECEDES` need a word boundary after them**, the way the
    /// `FOR EACH ROW` anchor twelve lines up needs one — and they were matched
    /// as a bare byte prefix.
    ///
    /// A labelled compound statement is legal SQL and a label may be any
    /// identifier, so a body opening `followsx: BEGIN … END followsx` starts
    /// with the six bytes `FOLLOW` plus `S`. The prefix matched, one
    /// identifier's worth was eaten as the ordering clause, and what came back
    /// was a body missing its first token — into **both** the editable draft and
    /// the diff baseline, where `TriggerDraft::validate` parses no body and so
    /// says nothing. Applying any unrelated edit to that trigger then emits
    /// `DROP TRIGGER` followed by an invalid `CREATE`, run in sequence with no
    /// transaction: measured on MariaDB 10.11.14 as `ERROR 1064` with the
    /// trigger gone and unrepairable in-app.
    ///
    /// The last two cases are the ones that must keep working, so the boundary
    /// test cannot be satisfied by simply never matching.
    #[test]
    fn an_ordering_keyword_is_matched_on_a_word_boundary() {
        for (sql, body) in [
            // A label that merely starts with the keyword: the whole body,
            // including the label.
            (
                "CREATE TRIGGER `b` BEFORE INSERT ON `t` FOR EACH ROW \
                 followsx: BEGIN SET NEW.x = 1; END followsx",
                "followsx: BEGIN SET NEW.x = 1; END followsx",
            ),
            (
                "CREATE TRIGGER `b` BEFORE INSERT ON `t` FOR EACH ROW \
                 precedes_it: BEGIN SET NEW.x = 1; END precedes_it",
                "precedes_it: BEGIN SET NEW.x = 1; END precedes_it",
            ),
            // A bare statement that happens to start with the letters.
            (
                "CREATE TRIGGER `b` BEFORE INSERT ON `t` FOR EACH ROW SET NEW.followsme = 1",
                "SET NEW.followsme = 1",
            ),
            // …and the real clause, which still has to be dropped.
            (
                "CREATE TRIGGER `b` BEFORE INSERT ON `t` FOR EACH ROW FOLLOWS `a` SET NEW.x = 1",
                "SET NEW.x = 1",
            ),
            (
                "CREATE TRIGGER `b` BEFORE INSERT ON `t` FOR EACH ROW PRECEDES `a` SET NEW.x = 1",
                "SET NEW.x = 1",
            ),
        ] {
            assert_eq!(trigger_body_of(sql).as_deref(), Some(body), "{sql}");
        }
    }

    #[test]
    fn trigger_body_is_none_without_an_anchor() {
        assert!(trigger_body_of("CREATE TRIGGER `t` BEFORE INSERT ON `t2`").is_none());
        assert!(trigger_body_of("").is_none());
    }

    /// The ordering is the point: MySQL reports positions, and only a
    /// reconstructed FOLLOWS chain recreates the order it was given.
    #[test]
    fn mysql_triggers_rebuild_the_follows_chain_from_action_order() {
        let rows = [
            tr("orders", "third", "BEFORE", "INSERT", 3),
            tr("orders", "first", "BEFORE", "INSERT", 1),
            tr("orders", "second", "BEFORE", "INSERT", 2),
        ];
        let out = mysql_triggers(&rows);
        let by = |n: &str| out.iter().find(|t| t.name == n).unwrap().order.clone();
        // Position 1 anchors the chain from the front. This used to assert
        // `None` — which is the defect, not the contract: a `CREATE TRIGGER`
        // with no ordering clause is appended *last* by MySQL, so recreating
        // the leader reversed the group.
        assert_eq!(by("first"), Some(TriggerOrder::Precedes(s("second"))));
        assert_eq!(by("second"), Some(TriggerOrder::Follows(s("first"))));
        assert_eq!(by("third"), Some(TriggerOrder::Follows(s("second"))));
    }

    #[test]
    fn mysql_triggers_do_not_chain_across_groups() {
        // Same table, different event — and same event, different table. Neither
        // is a group, so neither may produce a FOLLOWS.
        let rows = [
            tr("orders", "a", "BEFORE", "INSERT", 1),
            tr("orders", "b", "BEFORE", "UPDATE", 1),
            tr("orders", "c", "AFTER", "INSERT", 1),
            tr("lines", "d", "BEFORE", "INSERT", 1),
        ];
        assert!(mysql_triggers(&rows).iter().all(|t| t.order.is_none()));
    }

    /// The **leading** trigger of a group needs an anchor too.
    ///
    /// It recorded none, because `ACTION_ORDER > 1` was the whole condition. So
    /// replacing it emitted a `CREATE TRIGGER` with no ordering clause, MySQL
    /// appended it **last**, and the group's firing order silently reversed —
    /// every later write computing from a chain that now runs backwards, with
    /// no error and nothing in the preview. Measured on MySQL 8.4.11: a group
    /// `[a, b]` came back `[b, a]` after replacing `a`, and `PRECEDES b`
    /// restored it.
    ///
    /// A group of one needs nothing: there is no order to lose.
    #[test]
    fn mysql_triggers_anchor_the_leading_trigger_of_a_group() {
        let rows = [
            tr("orders", "a", "BEFORE", "INSERT", 1),
            tr("orders", "b", "BEFORE", "INSERT", 2),
            tr("orders", "c", "BEFORE", "INSERT", 3),
        ];
        let out = mysql_triggers(&rows);
        assert_eq!(
            out[0].order,
            Some(TriggerOrder::Precedes(s("b"))),
            "the leader must name its successor"
        );
        assert_eq!(out[1].order, Some(TriggerOrder::Follows(s("a"))));
        assert_eq!(out[2].order, Some(TriggerOrder::Follows(s("b"))));

        // A lone trigger in its group has no order to preserve.
        let out = mysql_triggers(&[tr("orders", "only", "BEFORE", "INSERT", 1)]);
        assert!(out[0].order.is_none());
    }

    /// **And the composition, which is where the leader's anchor became a
    /// defect.** `Precedes` is the right answer for the caller that *replaces*
    /// one trigger inside a group that already exists — the test above is that
    /// caller, and it is why testing `mysql_triggers` alone cannot see this. The
    /// other caller replays the whole set into nothing, and there the leader's
    /// clause names a trigger no statement has created yet: measured, MySQL
    /// 8.4.11 answers `ERROR 3011` and MariaDB 10.11.14 `ERROR 4031`,
    /// *"Referenced trigger … for the given action time and event type does not
    /// exist"*. A structure dump of any table with two triggers in one group
    /// died on its first `CREATE TRIGGER`, after the `DROP TABLE` above it had
    /// already run against the target.
    #[test]
    fn a_trigger_set_never_names_a_trigger_a_later_statement_creates() {
        let rows = [
            tr("orders", "a", "BEFORE", "INSERT", 1),
            tr("orders", "b", "BEFORE", "INSERT", 2),
            tr("orders", "c", "BEFORE", "INSERT", 3),
        ];
        let triggers = mysql_triggers(&rows);
        // The premise: the model really does anchor the leader forwards, so this
        // cannot pass because there was nothing to fix.
        assert_eq!(
            triggers[0].order,
            Some(TriggerOrder::Precedes(s("b"))),
            "{:?}",
            triggers[0].order
        );

        let stmts = schemaic_core::schema::TriggerInfo::create_set_sql(
            &triggers,
            schemaic_core::intel::SqlDialect::MySql,
        );
        assert_eq!(stmts.len(), 3);
        for (i, stmt) in stmts.iter().enumerate() {
            for later in &triggers[i + 1..] {
                for kw in ["FOLLOWS", "PRECEDES"] {
                    assert!(
                        !stmt.contains(&format!("{kw} `{}`", later.name)),
                        "statement {i} names {} before it exists:\n{stmt}",
                        later.name
                    );
                }
            }
        }
        // And the chain the file *does* carry is enough to rebuild the order:
        // every non-leader still follows its predecessor.
        assert!(!stmts[0].contains("PRECEDES"), "{}", stmts[0]);
        assert!(stmts[1].contains("FOLLOWS `a`"), "{}", stmts[1]);
        assert!(stmts[2].contains("FOLLOWS `b`"), "{}", stmts[2]);
    }

    #[test]
    fn mysql_triggers_treat_zero_action_order_as_no_information() {
        // A server too old to report the column sends 0 for every row. Inventing
        // a chain there would order triggers the server never ordered.
        let rows = [
            tr("orders", "a", "BEFORE", "INSERT", 0),
            tr("orders", "b", "BEFORE", "INSERT", 0),
        ];
        assert!(mysql_triggers(&rows).iter().all(|t| t.order.is_none()));
    }

    #[test]
    fn mysql_triggers_carry_definer_timing_event_and_body() {
        let out = mysql_triggers(&[tr("orders", "a", "AFTER", "DELETE", 1)]);
        let t = &out[0];
        assert_eq!(t.timing, TriggerTiming::After);
        assert_eq!(t.events, vec![TriggerEvent::Delete]);
        assert_eq!(t.action, TriggerAction::Body(s("SET NEW.x = 1")));
        assert_eq!(t.definer.as_deref(), Some("root@localhost"));
        assert!(t.schema.is_none()); // MySQL has no namespace level
        assert_eq!(t.enabled, schemaic_core::schema::TriggerEnabled::Origin);
    }

    #[test]
    fn apply_triggers_drops_rows_for_tables_not_in_this_fetch() {
        let tables = [(s("orders"), s("BASE TABLE"))];
        let mut schema = assemble_schema(None, &tables, &[], &[], &[], &[]);
        let triggers = mysql_triggers(&[
            tr("orders", "keep", "BEFORE", "INSERT", 1),
            tr("ghost", "drop", "BEFORE", "INSERT", 1),
        ]);
        apply_triggers(&mut schema, triggers);
        assert_eq!(schema.tables[0].triggers.len(), 1);
        assert_eq!(schema.tables[0].triggers[0].name, "keep");
    }

    // ── Stored routines ──────────────────────────────────────────────────
    //
    // `routine_body_of` is the half `information_schema` cannot answer: every
    // MySQL routine edit is a `DROP` plus a `CREATE`, so a body that came back
    // with its escapes resolved fails *after* the only copy is gone. Every
    // input below is a real `SHOW CREATE` shape.

    /// The plain case: no characteristics at all between the parameter list and
    /// the body, which is what a bare `CREATE PROCEDURE` produces.
    #[test]
    fn routine_body_starts_after_the_parameter_list() {
        let sql = "CREATE DEFINER=`root`@`localhost` PROCEDURE `restock`(IN sku VARCHAR(20))\n\
                   BEGIN\n  UPDATE stock SET n = n + 1;\nEND";
        assert_eq!(
            routine_body_of(sql).as_deref(),
            Some("BEGIN\n  UPDATE stock SET n = n + 1;\nEND")
        );
    }

    /// Every characteristic MySQL prints, in the order it prints them — and the
    /// body still starts where the last one ends.
    #[test]
    fn routine_body_skips_every_characteristic_clause() {
        let sql = "CREATE DEFINER=`root`@`localhost` FUNCTION `label`(n INT) \
                   RETURNS varchar(20) CHARSET utf8mb4 COLLATE utf8mb4_general_ci\n\
                       DETERMINISTIC\n    NO SQL\n    SQL SECURITY INVOKER\n\
                       COMMENT 'names a number'\n\
                   BEGIN\n  RETURN 'x';\nEND";
        assert_eq!(
            routine_body_of(sql).as_deref(),
            Some("BEGIN\n  RETURN 'x';\nEND")
        );
    }

    /// The characteristic vocabulary and the statement vocabulary are disjoint,
    /// which is what makes consuming greedily safe — a body that begins with a
    /// bare statement survives, and so does one that *mentions* the words.
    #[test]
    fn routine_body_may_be_a_bare_statement() {
        let sql = "CREATE DEFINER=`a`@`b` PROCEDURE `p`() \
                   MODIFIES SQL DATA SELECT 'contains sql' AS note";
        assert_eq!(
            routine_body_of(sql).as_deref(),
            Some("SELECT 'contains sql' AS note")
        );
    }

    /// The escapes `information_schema.ROUTINE_DEFINITION` resolves — the whole
    /// reason this path exists. Through that column the second comes back as
    /// `'it's'`, a 1064 on restate.
    #[test]
    fn routine_body_survives_the_escapes_information_schema_resolves() {
        let bs = "CREATE DEFINER=`a`@`b` PROCEDURE `p`() SET @x = 'C:\\temp'";
        assert_eq!(routine_body_of(bs).as_deref(), Some("SET @x = 'C:\\temp'"));
        let q = "CREATE DEFINER=`a`@`b` PROCEDURE `p`() SET @x = 'it''s'";
        assert_eq!(routine_body_of(q).as_deref(), Some("SET @x = 'it''s'"));
    }

    /// The parameter list is found at a *code* position and skipped as a
    /// balanced group, so neither a routine named with a paren nor a default
    /// holding one can end it early.
    #[test]
    fn routine_body_is_not_confused_by_a_paren_in_a_name_or_a_literal() {
        let sql = "CREATE PROCEDURE `p(x)`(IN a VARCHAR(9) ) BEGIN SELECT 1; END";
        assert_eq!(routine_body_of(sql).as_deref(), Some("BEGIN SELECT 1; END"));
        // A `COMMENT` holding the word the loop would otherwise stop on.
        let sql = "CREATE PROCEDURE `p`() COMMENT 'BEGIN here' BEGIN SELECT 1; END";
        assert_eq!(routine_body_of(sql).as_deref(), Some("BEGIN SELECT 1; END"));
    }

    /// **A return type's modifiers each take their argument with them.** A
    /// keyword consumed without its value leaves that value at the head of what
    /// is returned as the body — and since every MySQL edit is a `DROP` plus a
    /// `CREATE`, that is a 1064 *after* the only copy is gone.
    #[test]
    fn routine_body_survives_every_return_type_modifier() {
        for ty in [
            "varchar(20) CHARSET utf8mb4",
            "varchar(20) CHARACTER SET utf8mb4",
            "varchar(20) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin",
            "varchar(20) CHARSET utf8mb4 COLLATE utf8mb4_bin",
            "decimal(10,2) UNSIGNED",
            "int UNSIGNED ZEROFILL",
            "char(1) BINARY",
            "int",
        ] {
            let sql = format!(
                "CREATE DEFINER=`a`@`b` FUNCTION `f`(n INT) RETURNS {ty}\n\
                 DETERMINISTIC\nBEGIN\n  RETURN 'x';\nEND"
            );
            assert_eq!(
                routine_body_of(&sql).as_deref(),
                Some("BEGIN\n  RETURN 'x';\nEND"),
                "RETURNS {ty}"
            );
        }
    }

    /// A bare `SET` is **not** a type modifier — it is only ever the second word
    /// of `CHARACTER SET`. On the trailer list it swallowed the first word of a
    /// body that legitimately begins `SET @x = 1`, which is a valid routine body
    /// on its own.
    #[test]
    fn routine_body_beginning_with_set_is_not_eaten_as_a_type_modifier() {
        let sql = "CREATE DEFINER=`a`@`b` FUNCTION `f`() RETURNS INT SET @x = 1";
        assert_eq!(routine_body_of(sql).as_deref(), Some("SET @x = 1"));
    }

    /// No parameter list, or nothing after the characteristics: this didn't
    /// understand the text, and says so rather than handing back a fragment the
    /// caller would restate.
    #[test]
    fn routine_body_is_none_when_the_text_is_not_understood() {
        assert_eq!(routine_body_of("not a create statement"), None);
        assert_eq!(routine_body_of("CREATE PROCEDURE `p`() NO SQL"), None);
        // Nothing at all after the keyword: the `COMMENT` arm used to index
        // byte 0 of an empty slice and take the process down rather than
        // degrade to the body it already had.
        assert_eq!(routine_body_of("CREATE PROCEDURE `p`() COMMENT"), None);
    }

    /// **Every field reads the column the query aliases, and it reads all of
    /// them.** The reader used to index the row `0..=13` against a `SELECT`
    /// fifteen hundred lines away — the struct replaced a tuple exactly because
    /// fourteen columns is past `mysql_common`'s `FromRow` ceiling, which is
    /// the same thing as saying the compiler stopped checking. Insert a column
    /// at position 3 and `body` read `CHARACTER_SET_NAME` and `sql_mode` read
    /// `ROUTINE_COMMENT`, with nothing failing; the three tests that exercise
    /// `mysql_routines` build a `MyRoutineRow` literal and never come through
    /// here.
    ///
    /// The reader now asks by name, so this is the pin on the two remaining
    /// ways it can drift: a field bound to the wrong alias, and an alias the
    /// query does not actually declare (which reads as NULL — silently empty,
    /// not loud).
    #[test]
    fn every_routine_field_reads_the_column_the_query_aliases() {
        // Hand each read back its own column name, and record what was asked.
        let mut asked: Vec<String> = Vec::new();
        let r = my_routine_row_from(|c| {
            asked.push(c.to_string());
            Some(c.to_string())
        });
        assert_eq!(r.name, "n");
        assert_eq!(r.kind, "ty");
        assert_eq!(r.returns, "rt");
        assert_eq!(r.returns_charset.as_deref(), Some("rtcs"));
        assert_eq!(r.returns_collation.as_deref(), Some("rtcoll"));
        assert_eq!(r.body.as_deref(), Some("body"));
        assert_eq!(r.deterministic, "det");
        assert_eq!(r.data_access, "acc");
        assert_eq!(r.security, "sec");
        assert_eq!(r.definer, "df");
        assert_eq!(r.comment, "cmt");
        assert_eq!(r.sql_mode.as_deref(), Some("sqlmode"));
        assert_eq!(r.charset_client.as_deref(), Some("cscl"));
        assert_eq!(r.collation_connection.as_deref(), Some("collconn"));
        assert_eq!(asked, MY_ROUTINE_COLUMNS, "one read per declared column");

        // …and the query really declares each of them, as a whole token, in
        // this order.
        let mut rest = MY_ROUTINES_SQL;
        for c in MY_ROUTINE_COLUMNS {
            let needle = format!(" AS {c}");
            let at = rest
                .find(&needle)
                .unwrap_or_else(|| panic!("the routine query aliases no column `{c}`"));
            rest = &rest[at + needle.len()..];
            assert!(
                matches!(rest.chars().next(), Some(',') | Some(' ')),
                "`{c}` is only the prefix of the alias the query declares"
            );
        }
    }

    /// The same pin, for the event read: a field bound to the wrong alias, and
    /// an alias the query does not actually declare (which reads as NULL —
    /// silently empty, not loud). Sixteen columns is well past the point where
    /// the compiler checks anything about this mapping.
    #[test]
    fn every_event_field_reads_the_column_the_query_aliases() {
        let mut asked: Vec<String> = Vec::new();
        let r = my_event_row_from(|c| {
            asked.push(c.to_string());
            Some(c.to_string())
        });
        assert_eq!(r.name, "n");
        assert_eq!(r.definer, "df");
        assert_eq!(r.kind, "ty");
        assert_eq!(r.execute_at.as_deref(), Some("at"));
        assert_eq!(r.interval_value.as_deref(), Some("iv"));
        assert_eq!(r.interval_field.as_deref(), Some("if_"));
        assert_eq!(r.starts.as_deref(), Some("st"));
        assert_eq!(r.ends.as_deref(), Some("en"));
        assert_eq!(r.status, "stat");
        assert_eq!(r.on_completion, "oc");
        assert_eq!(r.comment, "cmt");
        assert_eq!(r.body.as_deref(), Some("body"));
        assert_eq!(r.time_zone.as_deref(), Some("tz"));
        assert_eq!(r.sql_mode.as_deref(), Some("sqlmode"));
        assert_eq!(r.charset_client.as_deref(), Some("cscl"));
        assert_eq!(r.collation_connection.as_deref(), Some("collconn"));
        assert_eq!(asked, MY_EVENT_COLUMNS, "one read per declared column");

        let mut rest = MY_EVENTS_SQL;
        for c in MY_EVENT_COLUMNS {
            let needle = format!(" AS {c}");
            let at = rest
                .find(&needle)
                .unwrap_or_else(|| panic!("the event query aliases no column `{c}`"));
            rest = &rest[at + needle.len()..];
            assert!(
                matches!(rest.chars().next(), Some(',') | Some(' ')),
                "`{c}` is only the prefix of the alias the query declares"
            );
        }
    }

    fn er(name: &str, kind: &str) -> MyEventRow {
        MyEventRow {
            name: s(name),
            definer: s("root@localhost"),
            kind: s(kind),
            status: s("ENABLED"),
            on_completion: s("NOT PRESERVE"),
            body: Some(s("DELETE FROM t")),
            time_zone: Some(s("SYSTEM")),
            ..Default::default()
        }
    }

    /// A recurring row becomes an `EVERY` schedule, and the two catalogue
    /// columns that make it are quoted into SQL on the way in — the quantity
    /// bare because it is digits, the bounds as literals because they are
    /// datetimes.
    #[test]
    fn mysql_events_read_a_recurring_schedule() {
        let mut row = er("nightly", "RECURRING");
        row.interval_value = Some(s("1"));
        row.interval_field = Some(s("day"));
        row.starts = Some(s("2026-01-01 03:00:00"));
        let e = mysql_events(&[row]).remove(0);
        assert_eq!(e.name, "nightly");
        assert_eq!(e.schema, None);
        assert_eq!(e.definer.as_deref(), Some("root@localhost"));
        assert_eq!(
            e.schedule,
            EventSchedule::Every {
                value: s("1"),
                // Uppercased, so the emitted clause reads the way `SHOW CREATE`
                // prints it whatever case the catalogue used.
                unit: s("DAY"),
                starts: Some(s("'2026-01-01 03:00:00'")),
                ends: None,
            }
        );
        assert!(!e.preserve);
        assert_eq!(e.status, EventStatus::Enabled);
        assert_eq!(e.time_zone.as_deref(), Some("SYSTEM"));
    }

    /// A one-time row becomes an `AT` schedule, read off `EXECUTE_AT` — and
    /// `EVENT_TYPE` is what decides, not "is the interval NULL".
    #[test]
    fn mysql_events_read_a_one_time_schedule() {
        let mut row = er("once", "ONE TIME");
        row.execute_at = Some(s("2026-06-01 00:00:00"));
        row.on_completion = s("PRESERVE");
        row.status = s("SLAVESIDE_DISABLED");
        let e = mysql_events(&[row]).remove(0);
        assert_eq!(e.schedule, EventSchedule::At(s("'2026-06-01 00:00:00'")));
        assert!(e.preserve);
        assert_eq!(e.status, EventStatus::SlavesideDisabled);
    }

    /// A compound interval — `EVERY '1:30' HOUR_MINUTE` — keeps its quotes, and
    /// a `RECURRING` row the server reported nothing readable for still becomes
    /// a browsable event rather than being dropped from the list.
    #[test]
    fn mysql_events_survive_an_interval_they_cannot_read() {
        let mut row = er("compound", "RECURRING");
        row.interval_value = Some(s("1:30"));
        row.interval_field = Some(s("HOUR_MINUTE"));
        let e = mysql_events(&[row]).remove(0);
        assert_eq!(
            e.schedule,
            EventSchedule::Every {
                value: s("'1:30'"),
                unit: s("HOUR_MINUTE"),
                starts: None,
                ends: None,
            }
        );

        let e = mysql_events(&[er("mystery", "RECURRING")]).remove(0);
        assert_eq!(e.name, "mystery");
        assert_eq!(
            e.schedule,
            EventSchedule::Every {
                value: s("1"),
                unit: s("DAY"),
                starts: None,
                ends: None,
            }
        );
    }

    /// **The one-shot arm falls back too, and the fallback has to be legal.**
    /// An empty `AT` is exactly what `EventDraft::validate` refuses, so a
    /// `ONE TIME` row whose `EXECUTE_AT` came back empty would have opened an
    /// editor with Preview permanently disabled — an event that cannot be
    /// renamed, disabled or commented, which is the opposite of what the
    /// recurring arm's fallback is for.
    #[test]
    fn a_one_time_event_with_no_readable_time_is_still_editable() {
        let e = mysql_events(&[er("mystery_once", "ONE TIME")]).remove(0);
        assert_eq!(e.schedule, EventSchedule::At(s("CURRENT_TIMESTAMP")));
        assert!(
            schemaic_core::ddl::EventDraft::from_info(&e)
                .validate()
                .is_empty(),
            "the draft has to be one Preview will act on"
        );
        // And the fabricated value stays put: the schedule clause is restated
        // only on a change, so an edit that touches something else emits no
        // `ON SCHEDULE` at all.
        let mut d = schemaic_core::ddl::EventDraft::from_info(&e);
        d.info.comment = Some(s("paused"));
        let sql = schemaic_core::ddl::diff_event(&e, &d, SqlDialect::MySql).emit();
        assert!(!sql.iter().any(|s| s.contains("ON SCHEDULE")), "{sql:?}");
    }

    /// **The reader's output, put through the emitter.** Both halves of this seam
    /// were tested only against themselves: the tests above assert the *model* a
    /// catalogue row folds into and stop, and every `EventInfo` fixture in
    /// `schemaic-core` hand-writes `starts: Some("'2026-01-01 03:00:00'")` — i.e.
    /// hand-writes the answer `event_time_expr` is supposed to produce. So the
    /// quoting is asserted twice against one assumption and never end to end,
    /// which is what would let a second `ddl_string` downstream
    /// (`'''2026-01-01 03:00:00'''`) or a reader that stopped quoting
    /// (`STARTS 2026-01-01 03:00:00`) through.
    #[test]
    fn a_catalogue_row_emits_a_statement_with_its_datetimes_quoted_once() {
        use schemaic_core::ddl::{EventDraft, create_event, diff_event};

        // The columns exactly as `information_schema.EVENTS` reports them: bare,
        // unquoted.
        let mut row = er("nightly", "RECURRING");
        row.interval_value = Some(s("1"));
        row.interval_field = Some(s("day"));
        row.starts = Some(s("2026-01-01 03:00:00"));
        row.ends = Some(s("2027-01-01 03:00:00"));
        row.comment = s("nightly purge");
        let e = mysql_events(&[row]).remove(0);

        let sql = create_event(&EventDraft::from_info(&e), SqlDialect::MySql).emit();
        let create = sql
            .iter()
            .find(|s| s.starts_with("CREATE DEFINER"))
            .expect("one CREATE EVENT");
        assert!(
            create.contains("STARTS '2026-01-01 03:00:00'"),
            "one pair of quotes, not three and not none: {create}"
        );
        assert!(create.contains("ENDS '2027-01-01 03:00:00'"), "{create}");
        assert!(create.contains("COMMENT 'nightly purge'"), "{create}");

        // …and the round-trip gate holds across the seam: what the reader
        // produced diffs to nothing against its own draft, so opening the editor
        // on an untouched event says "No changes".
        assert!(diff_event(&e, &EventDraft::from_info(&e), SqlDialect::MySql).is_empty());

        // The one-time shape too — a different column (`EXECUTE_AT`) through a
        // different arm.
        let mut row = er("once", "ONE TIME");
        row.execute_at = Some(s("2026-06-01 00:00:00"));
        let e = mysql_events(&[row]).remove(0);
        let sql = create_event(&EventDraft::from_info(&e), SqlDialect::MySql).emit();
        assert!(
            sql.iter()
                .any(|s| s.contains("ON SCHEDULE AT '2026-06-01 00:00:00'")),
            "{sql:?}"
        );
        assert!(diff_event(&e, &EventDraft::from_info(&e), SqlDialect::MySql).is_empty());

        // **And the fabricated fallback reaching `create_sql`**, which is the
        // half `mysql_events`' own comment does not cover: the copy path restates
        // every clause unconditionally, so a row the server reported nothing
        // readable for emits the fallback rather than an empty clause. It has to
        // be legal SQL, which is why the fallback is a keyword and not "".
        let e = mysql_events(&[er("mystery", "RECURRING")]).remove(0);
        let sql = create_event(&EventDraft::from_info(&e), SqlDialect::MySql).emit();
        assert!(
            sql.iter().any(|s| s.contains("ON SCHEDULE EVERY 1 DAY")),
            "{sql:?}"
        );
        let e = mysql_events(&[er("mystery_once", "ONE TIME")]).remove(0);
        let sql = create_event(&EventDraft::from_info(&e), SqlDialect::MySql).emit();
        assert!(
            sql.iter()
                .any(|s| s.contains("ON SCHEDULE AT CURRENT_TIMESTAMP")),
            "an unquoted keyword, not a literal: {sql:?}"
        );
    }

    /// The body is everything after the **top-level** `DO`, and the two things
    /// that sit before it and spell it are stepped over rather than matched: a
    /// quoted identifier, and a string literal.
    #[test]
    fn the_show_create_event_body_starts_after_its_do() {
        assert_eq!(
            event_body_of(
                "CREATE DEFINER=`root`@`localhost` EVENT `nightly` ON SCHEDULE EVERY 1 DAY \
                 ON COMPLETION NOT PRESERVE ENABLE DO DELETE FROM sessions"
            )
            .as_deref(),
            Some("DELETE FROM sessions")
        );
        // A compound body, kept whole — the `;` inside it are the body's own.
        assert_eq!(
            event_body_of("CREATE EVENT `e` ON SCHEDULE EVERY 1 DAY DO BEGIN SELECT 1; END")
                .as_deref(),
            Some("BEGIN SELECT 1; END")
        );
        // An event *named* `do`: a quoted identifier, which `skip_noncode`
        // steps over rather than reading as the keyword.
        assert_eq!(
            event_body_of("CREATE EVENT `do` ON SCHEDULE EVERY 1 DAY DO SELECT 1").as_deref(),
            Some("SELECT 1")
        );
        // …and a `COMMENT` whose text says `do`, which is a string literal.
        assert_eq!(
            event_body_of(
                "CREATE EVENT `e` ON SCHEDULE EVERY 1 DAY COMMENT 'do not touch' DO SELECT 1"
            )
            .as_deref(),
            Some("SELECT 1")
        );
        // Not a statement this build understands: the caller keeps the body it
        // already had rather than blanking it.
        assert_eq!(
            event_body_of("CREATE EVENT `e` ON SCHEDULE EVERY 1 DAY"),
            None
        );
    }

    fn rr(name: &str, ty: &str, returns: &str) -> MyRoutineRow {
        MyRoutineRow {
            name: s(name),
            kind: s(ty),
            returns: s(returns),
            body: Some(s("BEGIN SELECT 1; END")),
            deterministic: s("NO"),
            data_access: s("READS_SQL_DATA"),
            security: s("DEFINER"),
            definer: s("root@localhost"),
            comment: s("hello"),
            ..Default::default()
        }
    }

    /// A `PARAMETERS` row as the server sends one. The mode is what the server
    /// states, **including for a function** — see
    /// [`mysql_parameters_drop_the_mode_from_a_functions_parameters`].
    fn pr(name: &str, ty: &str, mode: &str, pname: &str, dtd: &str) -> MyParamRow {
        (s(name), s(ty), s(mode), s(pname), s(dtd), None, None)
    }

    /// The rendered parameter list is rebuilt from `PARAMETERS`, because MySQL
    /// publishes no signature column — and it is keyed by name **and kind**, so
    /// a procedure and a function of the same name don't take each other's.
    #[test]
    fn mysql_routines_render_their_parameter_lists_per_kind() {
        let params = mysql_parameters(&[
            pr("go", "PROCEDURE", "IN", "sku", "VARCHAR(20)"),
            pr("go", "PROCEDURE", "OUT", "n", "INT"),
            pr("go", "FUNCTION", "IN", "n", "INT"),
        ]);
        let out = mysql_routines(
            &[rr("go", "PROCEDURE", ""), rr("go", "FUNCTION", "int")],
            &params,
        );
        assert_eq!(out[0].kind, schemaic_core::schema::RoutineKind::Procedure);
        assert_eq!(out[0].arguments, "IN sku VARCHAR(20), OUT n INT");
        assert!(out[0].returns.is_empty());
        assert_eq!(out[1].kind, schemaic_core::schema::RoutineKind::Function);
        assert_eq!(out[1].arguments, "n INT");
        assert_eq!(out[1].returns, "int");
    }

    /// The catalogue reports `PARAMETER_MODE = 'IN'` for a **function's**
    /// parameters — measured on MariaDB 10.11 — and `CREATE FUNCTION` has no
    /// grammar for it. Emitting it cost the routine: the recreate's `DROP` had
    /// already committed when the `CREATE` came back 1064.
    #[test]
    fn mysql_parameters_drop_the_mode_from_a_functions_parameters() {
        let params = mysql_parameters(&[
            pr("f", "FUNCTION", "IN", "n", "INT"),
            pr("p", "PROCEDURE", "IN", "n", "INT"),
            pr("p", "PROCEDURE", "INOUT", "acc", "DECIMAL(9,2)"),
        ]);
        assert_eq!(params[&(s("f"), s("FUNCTION"))], vec![s("n INT")]);
        assert_eq!(
            params[&(s("p"), s("PROCEDURE"))],
            vec![s("IN n INT"), s("INOUT acc DECIMAL(9,2)")]
        );
    }

    /// **`PARAMETER_NAME` is the bare name, and the rebuilt list has to quote
    /// it.**
    ///
    /// A procedure declared ``p(`order` INT)`` comes back from
    /// `information_schema.PARAMETERS` as `order`, and the list was joined raw
    /// — so the recreate emitted `CREATE PROCEDURE p(IN order INT)` and the
    /// server answered 1064 **after** the `DROP` had committed on its own.
    /// Reproduced live on MariaDB 10.11.14 and MySQL 8.4.11: the procedure was
    /// gone and nothing replaced it, and the backticked form restores it.
    ///
    /// This is the same statement, the same commit and the same stated failure
    /// as the parameter-mode fix beside it, which fixed the mode half and left
    /// the quoting half.
    #[test]
    fn mysql_parameters_quote_a_name_that_needs_quoting() {
        let params = mysql_parameters(&[
            pr("p", "PROCEDURE", "IN", "order", "INT"),
            pr("p", "PROCEDURE", "IN", "first name", "INT"),
            pr("p", "PROCEDURE", "IN", "amount", "DECIMAL(9,2)"),
            pr("f", "FUNCTION", "IN", "rank", "INT"),
        ]);
        assert_eq!(
            params[&(s("p"), s("PROCEDURE"))],
            vec![
                s("IN `order` INT"),
                s("IN `first name` INT"),
                // An ordinary name stays bare, so no rendered list a user is
                // already reading changes.
                s("IN amount DECIMAL(9,2)"),
            ]
        );
        assert_eq!(params[&(s("f"), s("FUNCTION"))], vec![s("`rank` INT")]);
    }

    /// **`AGGREGATE` is printed by `SHOW CREATE` and by nothing else.**
    ///
    /// MariaDB's `information_schema.ROUTINES` has no column that distinguishes
    /// an aggregate function — verified live, zero columns matching `%AGG%` —
    /// so the header of the `SHOW CREATE` text is the only place it can be
    /// learned. Dropping it cost the function: the recreate came back
    /// `ERROR 4105` after the `DROP` committed.
    #[test]
    fn the_show_create_header_reports_an_aggregate_function() {
        assert!(routine_is_aggregate(
            "CREATE DEFINER=`schemaic`@`%` AGGREGATE FUNCTION `f_agg`(x int(11)) RETURNS int(11) \
             BEGIN LOOP FETCH GROUP NEXT ROW; END LOOP; END"
        ));
        assert!(routine_is_aggregate(
            "create aggregate function f(x int) returns int RETURN 1"
        ));
        assert!(!routine_is_aggregate(
            "CREATE DEFINER=`schemaic`@`%` FUNCTION `f`(x int(11)) RETURNS int(11) RETURN 1"
        ));
        assert!(!routine_is_aggregate(
            "CREATE DEFINER=`a`@`b` PROCEDURE `p`(IN `n` INT) BEGIN SELECT 1; END"
        ));
        // The scan stops at the parameter list, so a body that talks about
        // aggregates says nothing about the header…
        assert!(!routine_is_aggregate(
            "CREATE FUNCTION `f`() RETURNS int BEGIN /* aggregate */ RETURN 1; END"
        ));
        // …and a routine *named* `aggregate` is a quoted identifier, which
        // `skip_noncode` steps over rather than reading as the keyword.
        assert!(!routine_is_aggregate(
            "CREATE DEFINER=`a`@`b` FUNCTION `aggregate`(x int) RETURNS int RETURN 1"
        ));
    }

    /// `DTD_IDENTIFIER` renders `longtext`, never the character set the
    /// parameter was declared with — that lives in its own column. A recreate
    /// that dropped it re-declared the parameter under the database default.
    #[test]
    fn mysql_routines_keep_a_parameters_character_set() {
        let params = mysql_parameters(&[(
            s("execute_prepared_stmt"),
            s("PROCEDURE"),
            s("IN"),
            s("in_query"),
            s("longtext"),
            Some(s("utf8mb3")),
            Some(s("utf8mb3_general_ci")),
        )]);
        assert_eq!(
            params[&(s("execute_prepared_stmt"), s("PROCEDURE"))],
            vec![s(
                "IN in_query longtext CHARACTER SET utf8mb3 COLLATE utf8mb3_general_ci"
            )]
        );

        // The same column pair on `ROUTINES` is a function's *return* type.
        let mut row = rr("extract_schema", "FUNCTION", "varchar(64)");
        row.returns_charset = Some(s("utf8mb3"));
        row.returns_collation = Some(s("utf8mb3_general_ci"));
        let out = mysql_routines(&[row], &HashMap::new());
        assert_eq!(
            out[0].returns,
            "varchar(64) CHARACTER SET utf8mb3 COLLATE utf8mb3_general_ci"
        );

        // NULL for every non-string type, and a procedure has no return type at
        // all — neither may grow a dangling clause.
        assert_eq!(mysql_type_with_charset("int", None, None), "int");
        assert_eq!(mysql_type_with_charset("", Some("utf8mb4"), None), "");
    }

    /// The session state a recreate has to restore comes from the catalogue, so
    /// a draft carries it from the first frame — the editor's lazy
    /// `SHOW CREATE` only ever corrects the body.
    #[test]
    fn mysql_routines_carry_the_session_state_a_recreate_restores() {
        let mut row = rr("rewards_report", "PROCEDURE", "");
        row.sql_mode = Some(s("STRICT_TRANS_TABLES,TRADITIONAL"));
        row.charset_client = Some(s("utf8mb3"));
        row.collation_connection = Some(s("utf8mb3_general_ci"));
        let out = mysql_routines(&[row], &HashMap::new());
        assert_eq!(
            out[0].sql_mode.as_deref(),
            Some("STRICT_TRANS_TABLES,TRADITIONAL")
        );
        assert_eq!(out[0].charset_client.as_deref(), Some("utf8mb3"));
        assert_eq!(
            out[0].collation_connection.as_deref(),
            Some("utf8mb3_general_ci")
        );
    }

    /// Every characteristic the emitter has to restate is read, and the
    /// security type falls to **DEFINER** — this engine's default, and the
    /// opposite of PostgreSQL's, so an unreadable value must not read as
    /// INVOKER and quietly widen what the routine may do.
    #[test]
    fn mysql_routines_carry_the_characteristics_a_recreate_would_reset() {
        let out = mysql_routines(&[rr("p", "PROCEDURE", "")], &HashMap::new());
        let r = &out[0];
        assert!(!r.deterministic);
        assert_eq!(
            r.data_access,
            schemaic_core::schema::SqlDataAccess::ReadsSqlData
        );
        assert!(r.security_definer);
        assert_eq!(r.definer.as_deref(), Some("root@localhost"));
        assert_eq!(r.comment.as_deref(), Some("hello"));
        // MySQL has no namespace level, and reports `SQL` for everything.
        assert!(r.schema.is_none());
        assert_eq!(r.language, "SQL");

        let mut invoker = rr("p", "PROCEDURE", "");
        invoker.security = s("INVOKER");
        assert!(!mysql_routines(&[invoker], &HashMap::new())[0].security_definer);
    }

    /// A routine whose definition the account may not read arrives with a NULL
    /// body, which is an empty one here rather than a panic.
    #[test]
    fn mysql_routines_tolerate_an_unreadable_body() {
        let mut row = rr("p", "PROCEDURE", "");
        row.body = None;
        assert!(mysql_routines(&[row], &HashMap::new())[0].body.is_empty());
    }

    /// The values that mean "nothing to restate" have to fold to `None`, or the
    /// schema editor opens on a phantom change; the ones that mean something
    /// have to survive, or a replace quietly resets them.
    #[test]
    fn mysql_view_options_keeps_only_what_is_set() {
        let o = mysql_view_options("NONE", "root@localhost", "DEFINER", Some("UNDEFINED"));
        assert_eq!(o.check_option, None);
        assert_eq!(o.definer.as_deref(), Some("root@localhost"));
        assert_eq!(o.security.as_deref(), Some("DEFINER"));
        assert_eq!(o.algorithm, None);

        let o = mysql_view_options("cascaded", "app@10.0.0.1", "INVOKER", Some("merge"));
        assert_eq!(o.check_option.as_deref(), Some("CASCADED"));
        assert_eq!(o.security.as_deref(), Some("INVOKER"));
        assert_eq!(o.algorithm.as_deref(), Some("MERGE"));
        // PostgreSQL's half of the struct stays empty on MySQL.
        assert!(o.storage.is_empty() && !o.materialized);

        // MySQL 8 reports no algorithm at all.
        assert_eq!(
            mysql_view_options("NONE", "", "", None),
            ViewOptions::default()
        );
    }
}

/// Run the (unprepared, text-protocol) statement, stopping at the row cap, and
/// materialize it into a [`ResultSet`]. When `early_stop` is true, the row
/// stream is abandoned as soon as the cap is hit (the caller tears the
/// connection down); when false, the rest is drained so the connection stays
/// reusable for the next statement in a batch.
pub(crate) async fn collect_rows(
    conn: &mut Conn,
    sql: &str,
    dest: &mut RowDest,
    early_stop: bool,
) -> Result<ResultSet, DbError> {
    let row_cap = dest.cap();
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());
    let start = std::time::Instant::now();

    let mut result = conn.query_iter(sql).await.map_err(qerr)?;

    // Column metadata arrives before any rows, and is present even for a
    // zero-row SELECT. A statement that returns no result set (DML/DDL) has no
    // columns — that's how we tell a grid apart from an affected-rows outcome.
    let columns: Vec<Column> = result.columns_ref().iter().map(map_column).collect();

    if columns.is_empty() {
        let affected = result.affected_rows();
        // Drain the (empty) result so the connection is clean.
        let _ = result.collect::<Row>().await;
        return Ok(
            ResultSet::affected_rows(columns, affected).with_elapsed(start.elapsed().as_millis())
        );
    }

    // Which columns hold raw bytes, answered **once for the result** rather than
    // once per cell. `Column::is_binary` splits a type name and walks a keyword
    // list; at the 200k-row cap on a wide result that is tens of millions of
    // calls in the row loop, for an answer that cannot change between rows.
    let binary: Vec<bool> = columns.iter().map(Column::is_binary).collect();
    // Every value here arrives as `Bytes`, so nothing reads this — hoisted with
    // its siblings because `convert_row` is one function and a caller that
    // *could* hit a typed arm must not be the one that forgot to supply it.
    let scale = fractional_scales(result.columns_ref());
    // Likewise unread on this path — the padding is already in the bytes the
    // server sent — and supplied for the same reason: one function, one set of
    // per-column facts, whichever protocol the caller is on.
    let zerofill = zerofill_widths(result.columns_ref());
    // Hoisted for the same reason, and asked of the type name only: a bit-field's
    // bytes are a number, and nothing but the column says so.
    let bit: Vec<bool> = columns
        .iter()
        .map(|c| schemaic_core::model::type_is_bit(&c.type_name))
        .collect();
    // And how each column's text parses, for the same reason and with the
    // same lifetime — see `convert_row`.
    let kinds: Vec<NumKind> = columns.iter().map(|c| num_kind(&c.type_name)).collect();
    // Assemble the result columnar, one row at a time, so we never hold a
    // row-major `Vec<Vec<Value>>` copy alongside the final storage.
    let chunk_capacity = dest.chunk_capacity();
    let mut builder = ResultBuilder::with_capacity(columns, chunk_capacity);
    let mut truncated = false;
    if let Some(mut stream) = result.stream::<Row>().await.map_err(qerr)? {
        while let Some(row) = stream.next().await {
            let row = row.map_err(qerr)?;
            if builder.row_count() < row_cap {
                let cells = convert_row(
                    &row,
                    builder.columns(),
                    &binary,
                    &bit,
                    &scale,
                    &kinds,
                    &zerofill,
                );
                builder.push_row(&cells);
                // A stream hands the block over here and keeps reading into an
                // empty builder; a capped read never fills a chunk, so this is
                // dead weight for it and nothing more.
                if dest.chunk_full(builder.row_count(), builder.text_bytes()) {
                    dest.flush(&mut builder, chunk_capacity).await?;
                }
            } else {
                // A row beyond the cap exists → the result is truncated.
                truncated = true;
                if early_stop {
                    break;
                }
                // else: keep draining (discarding) to leave the conn clean.
            }
        }
    }

    // The tail: a stream's last block is usually short and may be empty, and the
    // export needs that last block even when it is — the columns for its header
    // come from the first chunk, and a table with no rows has only this one. Not
    // reached by a statement that returns no columns at all, which returned
    // above; `Db::stream_query` refuses those rather than letting the writer see
    // an empty stream and call the file finished.
    dest.flush(&mut builder, 0).await?;
    builder.set_truncated(truncated);
    builder.set_elapsed(start.elapsed().as_millis());
    Ok(builder.finish())
}

/// Map a wire column definition to our [`Column`], capturing its origin
/// (real database/table/column + key flags) when the server reports one.
/// Expression/aggregate/literal columns carry an empty `org_table`, which we
/// surface as `origin: None` — the signal that such a column is not editable.
pub(crate) fn map_column(c: &MyColumn) -> Column {
    let type_name = type_name_of(c);
    let binary = is_binary_data_type(&type_name);
    let f = c.flags();
    let flags = CoreColFlags {
        primary_key: f.contains(ColumnFlags::PRI_KEY_FLAG),
        unique_key: f.contains(ColumnFlags::UNIQUE_KEY_FLAG),
        not_null: f.contains(ColumnFlags::NOT_NULL_FLAG),
        auto_increment: f.contains(ColumnFlags::AUTO_INCREMENT_FLAG),
        no_default: f.contains(ColumnFlags::NO_DEFAULT_VALUE_FLAG),
    };
    let origin = column_origin(
        &c.schema_str(),
        &c.org_table_str(),
        &c.org_name_str(),
        flags,
        binary,
    );
    Column {
        name: c.name_str().to_string(),
        type_name,
        origin,
    }
}

/// Is the resolved SQL type a *binary-data* column (raw bytes), not merely
/// "binary charset"? Numeric / temporal columns also report charset 63, so this
/// keys off the resolved type name. Such values can't round-trip through the
/// text protocol losslessly, so the editing system treats them as read-only.
///
/// The list itself lives in `core::model::type_is_binary`, which is the same
/// question the export path asks of a column with no wire provenance to consult
/// — a second copy here is how the two would come to disagree.
fn is_binary_data_type(type_name: &str) -> bool {
    schemaic_core::model::type_is_binary(type_name)
}

/// Build a column's [`ColumnOrigin`] from its wire provenance, or `None` when
/// `org_table` is empty — an expression/aggregate/literal with no single base
/// column, the signal that such a column is not editable.
fn column_origin(
    schema: &str,
    org_table: &str,
    org_name: &str,
    flags: CoreColFlags,
    binary: bool,
) -> Option<ColumnOrigin> {
    if org_table.is_empty() {
        return None;
    }
    Some(ColumnOrigin {
        database: schema.to_string(),
        // MySQL has no namespace between database and table — `schema` here is
        // the wire protocol's `org_schema`, i.e. the database.
        schema: None,
        table: org_table.to_string(),
        column: org_name.to_string(),
        flags,
        binary,
        // MySQL's own row identity is always a column of the table; it has no
        // analogue of SQLite's `rowid`.
        implicit_key: false,
    })
}

/// Reconstruct a human SQL type name (`VARCHAR`, `INT UNSIGNED`, `DATETIME`, …)
/// from the wire column type + flags + charset — matching what the old sqlx
/// `type_info().name()` produced, so `parse_typed` and the UI keep behaving.
fn type_name_of(c: &MyColumn) -> String {
    resolve_type_name(
        c.column_type(),
        c.flags().contains(ColumnFlags::UNSIGNED_FLAG),
        c.character_set() == BINARY_CHARSET,
        c.flags().contains(ColumnFlags::ZEROFILL_FLAG),
    )
}

/// Pure core of [`type_name_of`]: map a wire column type + UNSIGNED flag + binary
/// charset to a human SQL type name. Split out so the mapping (which drives
/// `parse_typed` and editability) is unit-tested without a wire column object.
///
/// **`ZEROFILL` is here because it changes what a value *is* over the wire**,
/// not merely how it was declared: the server renders such a column padded to
/// its display width (`0007`), and the type name is the only thing
/// [`num_kind`] is given to decide whether the cell's text is the value.
pub(crate) fn resolve_type_name(
    ct: ColumnType,
    unsigned: bool,
    binary: bool,
    zerofill: bool,
) -> String {
    let base = match ct {
        ColumnType::MYSQL_TYPE_TINY => "TINYINT",
        ColumnType::MYSQL_TYPE_SHORT => "SMALLINT",
        ColumnType::MYSQL_TYPE_INT24 => "MEDIUMINT",
        ColumnType::MYSQL_TYPE_LONG => "INT",
        ColumnType::MYSQL_TYPE_LONGLONG => "BIGINT",
        ColumnType::MYSQL_TYPE_FLOAT => "FLOAT",
        ColumnType::MYSQL_TYPE_DOUBLE => "DOUBLE",
        ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => "DECIMAL",
        ColumnType::MYSQL_TYPE_YEAR => "YEAR",
        ColumnType::MYSQL_TYPE_BIT => "BIT",
        ColumnType::MYSQL_TYPE_TIMESTAMP | ColumnType::MYSQL_TYPE_TIMESTAMP2 => "TIMESTAMP",
        ColumnType::MYSQL_TYPE_DATE | ColumnType::MYSQL_TYPE_NEWDATE => "DATE",
        ColumnType::MYSQL_TYPE_TIME | ColumnType::MYSQL_TYPE_TIME2 => "TIME",
        ColumnType::MYSQL_TYPE_DATETIME | ColumnType::MYSQL_TYPE_DATETIME2 => "DATETIME",
        ColumnType::MYSQL_TYPE_JSON => "JSON",
        ColumnType::MYSQL_TYPE_ENUM => "ENUM",
        ColumnType::MYSQL_TYPE_SET => "SET",
        ColumnType::MYSQL_TYPE_GEOMETRY => "GEOMETRY",
        ColumnType::MYSQL_TYPE_VAR_STRING | ColumnType::MYSQL_TYPE_VARCHAR => {
            if binary {
                "VARBINARY"
            } else {
                "VARCHAR"
            }
        }
        ColumnType::MYSQL_TYPE_STRING => {
            if binary {
                "BINARY"
            } else {
                "CHAR"
            }
        }
        ColumnType::MYSQL_TYPE_TINY_BLOB => {
            if binary {
                "TINYBLOB"
            } else {
                "TINYTEXT"
            }
        }
        ColumnType::MYSQL_TYPE_MEDIUM_BLOB => {
            if binary {
                "MEDIUMBLOB"
            } else {
                "MEDIUMTEXT"
            }
        }
        ColumnType::MYSQL_TYPE_LONG_BLOB => {
            if binary {
                "LONGBLOB"
            } else {
                "LONGTEXT"
            }
        }
        ColumnType::MYSQL_TYPE_BLOB => {
            if binary {
                "BLOB"
            } else {
                "TEXT"
            }
        }
        ColumnType::MYSQL_TYPE_NULL => "NULL",
        _ => "UNKNOWN",
    };
    // MySQL reports UNSIGNED only for the numeric types.
    let numeric = matches!(
        ct,
        ColumnType::MYSQL_TYPE_TINY
            | ColumnType::MYSQL_TYPE_SHORT
            | ColumnType::MYSQL_TYPE_INT24
            | ColumnType::MYSQL_TYPE_LONG
            | ColumnType::MYSQL_TYPE_LONGLONG
            | ColumnType::MYSQL_TYPE_FLOAT
            | ColumnType::MYSQL_TYPE_DOUBLE
            | ColumnType::MYSQL_TYPE_DECIMAL
            | ColumnType::MYSQL_TYPE_NEWDECIMAL
    );
    let mut name = base.to_string();
    if numeric && unsigned {
        name.push_str(" UNSIGNED");
    }
    if numeric && zerofill {
        name.push_str(" ZEROFILL");
    }
    name
}

/// Convert one wire row into our typed cells. Over the text protocol every
/// non-NULL value arrives as `Bytes` (its textual form), so we parse it with the
/// column's type exactly as the old code did; the typed arms cover the binary
/// protocol defensively.
///
/// `binary[i]` is whether column `i` holds raw bytes, computed **once for the
/// result** by the caller: `Column::is_binary` splits a type name and walks a
/// keyword list, which is not an answer to re-derive per cell in a loop that
/// runs up to the row cap times the column count.
///
/// **A raw-bytes column is the exception, and it used to be a data bug.** A
/// BLOB/BINARY/BIT value arrives as its literal bytes, and
/// `from_utf8_lossy`-ing those produced mojibake that *looks like data* — so a
/// CSV or `INSERT` export wrote the replacement characters as the value and
/// re-imported as the wrong bytes. It renders as `binary_display` now, the same
/// `<n bytes>` SQLite and PostgreSQL show, which says what it is and cannot be
/// mistaken for the value.
pub(crate) fn convert_row(
    row: &Row,
    columns: &[Column],
    binary: &[bool],
    bit: &[bool],
    scale: &[u32],
    kinds: &[NumKind],
    zerofill: &[Option<usize>],
) -> Vec<Value> {
    (0..columns.len())
        .map(|i| match row.as_ref(i) {
            None | Some(MyValue::NULL) => Value::Null,
            Some(MyValue::Bytes(b)) if binary.get(i).copied().unwrap_or(false) => {
                Value::Str(binary_display(b.len()))
            }
            // **A bit-field arrives as bytes and is a number.** Nothing in the
            // value says so — only the column's type does — and lossy-decoding
            // those bytes as text is how a `BIT(8)` holding 10 became a newline
            // character. `bit_value` reads them the way MySQL wrote them and the
            // way it takes them back.
            //
            // `UInt`, not `Str`: the number is the value, and a `Value::Str`
            // carries a *quoted* literal into every export. `'10'` assigned to a
            // `BIT` column is the raw bits of its two bytes — 12594 on a
            // `BIT(16)`, "Data too long" on a `BIT(8)` — so the round trip that
            // taking `BIT` off the binary list was meant to enable was writing
            // wrong data instead of withholding it. The grid shows the same
            // digits either way.
            Some(MyValue::Bytes(b)) if bit.get(i).copied().unwrap_or(false) => {
                schemaic_core::model::bit_cell(b)
            }
            // **`parse_as` with a kind computed once, not `parse_typed`.**
            // `parse_typed` is `parse_as(num_kind(type_name), s)`, and
            // `num_kind` opens by uppercasing the type name — a heap
            // allocation — then walks up to eight `starts_with` scans and a
            // `contains`, for an answer that is a property of the *column*
            // and cannot vary between rows. Both docs say so: `num_kind`'s
            // reads "Called once per column", and `parse_typed`'s says "What
            // a row loop should call is `parse_as` with a kind it computed
            // once". This is the row loop. `binary` and `bit` above are
            // hoisted for exactly this reason, with a comment pricing it at
            // "tens of millions of calls in the row loop" at the 200k cap on
            // a wide result.
            Some(MyValue::Bytes(b)) => parse_as(
                kinds.get(i).copied().unwrap_or(NumKind::Text),
                String::from_utf8_lossy(b).into_owned(),
            ),
            // **The one integer the binary protocol renders differently.** A
            // `ZEROFILL` column arrives here as a bare number while the text
            // protocol sent it padded, so it is re-rendered to the column's
            // display width — see `zerofill_value`. Every other column has
            // `None` and passes straight through.
            Some(MyValue::Int(n)) => zerofill_value(
                Value::Int(*n),
                zerofill.get(i).copied().flatten(),
                scale.get(i).copied().unwrap_or(0),
            ),
            Some(MyValue::UInt(n)) => zerofill_value(
                Value::UInt(*n),
                zerofill.get(i).copied().flatten(),
                scale.get(i).copied().unwrap_or(0),
            ),
            // A `ZEROFILL` double is padded here for the same reason an integer
            // is — see `zerofill_value`. Without the width it is the bare float,
            // exactly as before.
            Some(MyValue::Double(f)) => zerofill_value(
                Value::Float(*f),
                zerofill.get(i).copied().flatten(),
                scale.get(i).copied().unwrap_or(0),
            ),
            // A `ZEROFILL` `FLOAT`, for the same reason as the `DOUBLE` above.
            // Without the width this falls through to `binary_as_text`, whose
            // `f32` rendering is deliberate and unchanged — which is why the
            // arm is guarded rather than unconditional.
            Some(MyValue::Float(f)) if zerofill.get(i).copied().flatten().is_some() => {
                zerofill_value(
                    Value::Float(f64::from(*f)),
                    zerofill.get(i).copied().flatten(),
                    scale.get(i).copied().unwrap_or(0),
                )
            }
            // **The binary protocol's own shapes, rendered the way the text
            // protocol renders the same column.** See `binary_as_text`; the
            // catch-all below is `as_sql`, which is a *SQL literal* and not what
            // the load produced.
            Some(other) => match binary_as_text(
                other,
                &columns[i].type_name,
                scale.get(i).copied().unwrap_or(0),
            ) {
                Some(text) => parse_typed(text, &columns[i].type_name),
                None => Value::Str(other.as_sql(false).trim_matches('\'').to_string()),
            },
        })
        .collect()
}

/// One binary-protocol value as the **text protocol's** rendering of the same
/// column — or `None` for a shape that needs no translation.
///
/// **The two protocols are two different readings of one row, and this app uses
/// both.** `collect_rows` loads a result with `query_iter` (text), where every
/// value arrives as `Bytes` and `parse_typed` keeps the server's own characters;
/// `refetch_on` re-reads one row with `exec_iter` (binary, because it is a
/// prepared statement with the key bound), where MySQL sends `DATETIME` as
/// `Date`, `TIME` as `Time` and `FLOAT` as an `f32`. Those fell to `convert_row`'s
/// catch-all, `MyValue::as_sql`, which renders a **SQL literal** rather than the
/// text form: `mysql_common` prints a `Date` with a zero time as `'YYYY-MM-DD'`
/// and a `Time` as `'{:03}:{:02}:{:02}'`.
///
/// So on MariaDB 10.11.14 and MySQL 8.4.11, editing one column of
/// `(1, 'a', '2024-01-15 00:00:00', '10:30:00', 3.14)` and committing spliced
/// the row back with `2024-01-15`, `010:30:00` and `3.140000104904175` in three
/// cells the user never touched — measured. Re-running the query restored them,
/// so the grid disagreed with itself about the same row, and a CSV, clipboard or
/// `INSERT` export taken in between wrote the wrong text out.
///
/// The fraction follows the **column's declared precision**, which is what the
/// server's own text form does: a `DATETIME(3)` reads `…:00.000` and a bare
/// `DATETIME` reads `…:00`. An `f32` goes through its shortest round-tripping
/// text, which is what MySQL prints for a `FLOAT` and what `3.14f32 as f64`
/// destroys.
pub(crate) fn binary_as_text(v: &MyValue, type_name: &str, scale: u32) -> Option<String> {
    let scale = scale.min(6);
    let frac = |us: u32| match scale {
        0 => String::new(),
        n => format!(".{:0>width$}", us / 10u32.pow(6 - n), width = n as usize),
    };
    match v {
        // A bare `DATE` column has no time to print; every other temporal does,
        // zero or not.
        MyValue::Date(y, m, d, h, mi, sec, us) => {
            Some(if type_name.trim().eq_ignore_ascii_case("date") {
                format!("{y:04}-{m:02}-{d:02}")
            } else {
                format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{sec:02}{}", frac(*us))
            })
        }
        // `TIME` is a *duration*: it runs past 24 hours and can be negative, so
        // the day part folds into the hours rather than being dropped.
        MyValue::Time(neg, days, h, mi, sec, us) => {
            let hours = u32::from(*h) + days * 24;
            Some(format!(
                "{}{hours:02}:{mi:02}:{sec:02}{}",
                if *neg { "-" } else { "" },
                frac(*us)
            ))
        }
        // `{}` on an `f32` is its shortest round-tripping form, which is what the
        // server prints. Widening to `f64` first is what produced
        // `3.140000104904175`.
        MyValue::Float(f) => Some(f.to_string()),
        _ => None,
    }
}

/// The fractional-seconds precision of each column, off the **wire** rather than
/// off the type name.
///
/// The column-definition packet carries it (`decimals`), and the resolved type
/// name does not: `type_name_of` builds `DATETIME` from the type code with no
/// precision in it, so a `DATETIME(3)` and a bare `DATETIME` are indistinguishable
/// by name. The binary protocol always sends microseconds, so without this a
/// `DATETIME(3)` holding `.120` came back with no fraction at all and a bare
/// `DATETIME` would have grown six zeroes — both a cell the user never edited,
/// changing under them.
pub(crate) fn fractional_scales(columns: &[MyColumn]) -> Vec<u32> {
    columns.iter().map(|c| u32::from(c.decimals())).collect()
}

/// Run one statement on its own connection and stream the rows into `dest`.
///
/// **`early_stop` is true here and false in a batch**, which is the whole
/// difference between this and [`crate::Db::run_batch`]'s arm: this connection is
/// torn down on the next line, so the row stream can be abandoned at the cap
/// rather than drained, while a batch's connection has to stay reusable for the
/// statement after it.
///
/// The connection id is captured before the `select!` so the cancel arm can
/// `KILL QUERY` the read that is already running — dropping the future stops
/// this side only, and the server would carry on.
pub(crate) async fn fetch_query(
    db: &Db,
    database: Option<&str>,
    sql: &str,
    dest: &mut RowDest,
    cancel: CancellationToken,
    enforce: Option<crate::Enforce>,
) -> Result<ResultSet, DbError> {
    let mut conn = db.open(database, false).await?;
    if let Some(enforce) = enforce
        && let Err(e) = enforce_session(&mut conn, enforce).await
    {
        let _ = conn.disconnect().await;
        return Err(e);
    }
    // The connection id, so a second connection can KILL its in-flight query.
    let conn_id = conn.id();

    let outcome = tokio::select! {
        r = collect_rows(&mut conn, sql, dest, true) => r,
        _ = cancel.cancelled() => {
            kill_query(db, conn_id).await;
            Err(DbError::Cancelled)
        }
    };

    let _ = conn.disconnect().await;
    outcome
}

/// Put a fresh connection in the state [`crate::Enforce`] asks for, before the
/// gated statement is sent.
///
/// **The mode is read back, not trusted.** The pin removes names from the
/// server's own mode (`sql::mysql_mode_lexed_like_the_gate`), and a server that
/// puts one back — a combination mode the list does not know — must fail the
/// statement rather than run it under a lexer the gate did not use. Sent as a
/// bound parameter, so nothing the server listed is spliced into SQL text.
///
/// `SET SESSION TRANSACTION READ ONLY` covers the autocommit statement that
/// follows, which is a transaction of its own; a stored function cannot lift it
/// from inside, because a transaction's access mode cannot change while it runs.
async fn enforce_session(conn: &mut Conn, enforce: crate::Enforce) -> Result<(), DbError> {
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());
    let mode: Option<String> = conn
        .query_first("SELECT @@SESSION.sql_mode")
        .await
        .map_err(qerr)?;
    let pinned = schemaic_core::sql::mysql_mode_lexed_like_the_gate(mode.as_deref().unwrap_or(""));
    conn.exec_drop("SET SESSION sql_mode = ?", (pinned,))
        .await
        .map_err(qerr)?;
    let back: Option<String> = conn
        .query_first("SELECT @@SESSION.sql_mode")
        .await
        .map_err(qerr)?;
    if !schemaic_core::sql::mysql_mode_is_lexed_like_the_gate(back.as_deref().unwrap_or("")) {
        return Err(DbError::Query(format!(
            "refused: the server's sql_mode ({}) reads quotes differently from the \
             statement check, and could not be changed for this session",
            back.unwrap_or_default()
        )));
    }
    if enforce == crate::Enforce::ReadOnly {
        conn.query_drop("SET SESSION TRANSACTION READ ONLY")
            .await
            .map_err(qerr)?;
    }
    Ok(())
}

/// `DbError` isn't `Clone`; this reproduces one for the "connect failed"
/// fan-out below.
///
/// **Here rather than in the dispatcher**, where it sat until its only caller
/// moved: `lib.rs`'s own rule is that what stays there is what more than one
/// engine reads, and this is read by one. It was widened to `pub(crate)` to
/// survive the extraction instead, which is the shape that rule exists to
/// refuse. Private again, and the compiler proves that is complete.
fn err_clone(e: &DbError) -> DbError {
    match e {
        DbError::Connect(s) => DbError::Connect(s.clone()),
        DbError::Query(s) => DbError::Query(s.clone()),
        DbError::Cancelled => DbError::Cancelled,
    }
}

/// Run `stmts` in order on **one** connection, reporting each result as it lands.
///
/// **It takes `scope` where [`crate::pg::run_batch`] does not**, and the extra
/// parameter is the asymmetry rather than an oversight: `USE` is MySQL's, so
/// only this arm can move the batch's current database mid-run, and the label a
/// later statement's result carries has to follow it. An interface that is a
/// naming convention rather than a trait is what lets the one engine with the
/// statement take the one argument for it.
///
/// Eight arguments, which is one past clippy's bar and is the honest count: six
/// are `pg::run_batch`'s, and the two extra are the `USE` bookkeeping above.
/// Bundling them into a struct would name the pair something, and the only
/// honest name for it is "what MySQL needs and the others do not".
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_batch(
    db: &Db,
    database: Option<&str>,
    stmts: &[String],
    row_cap: usize,
    cancel: CancellationToken,
    mut on_result: impl FnMut(usize, Result<ResultSet, DbError>),
    scope: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    dialect: SqlDialect,
) {
    let mut conn = match db.open(database, false).await {
        Ok(c) => c,
        Err(e) => {
            // Couldn't even connect: fail the first statement, cancel the rest.
            for (i, _) in stmts.iter().enumerate() {
                on_result(
                    i,
                    if i == 0 {
                        Err(err_clone(&e))
                    } else {
                        Err(DbError::Cancelled)
                    },
                );
            }
            return;
        }
    };
    let conn_id = conn.id();

    let mut stopped = false;
    for (i, sql) in stmts.iter().enumerate() {
        if stopped || cancel.is_cancelled() {
            on_result(i, Err(DbError::Cancelled));
            continue;
        }
        let mut dest = RowDest::Capped(row_cap);
        let outcome = tokio::select! {
            // `early_stop = false`: the connection is reused for the next
            // statement, so a truncated result must be drained fully to leave
            // the connection clean.
            r = collect_rows(&mut conn, sql, &mut dest, false) => r,
            _ = cancel.cancelled() => {
                kill_query(db, conn_id).await;
                Err(DbError::Cancelled)
            }
        };
        if outcome.is_err() {
            stopped = true;
        }
        // Before the sink, so a `USE` labels its own (empty) result with the
        // database it moved to — which is what the statement did.
        if outcome.is_ok()
            && schemaic_core::sql::leading_keyword(sql, dialect).as_deref() == Some("USE")
            && let Ok(mut scope) = scope.lock()
        {
            *scope = schemaic_core::sql::use_target(sql, dialect);
        }
        on_result(i, outcome);
    }

    let _ = conn.disconnect().await;
}

// ── EXPLAIN ──────────────────────────────────────────────────────────────────

/// The plan for `sql`, or the measured plan when `analyze`.
///
/// **Two servers, two spellings, and the second is only reachable by trying the
/// first**: MySQL has `EXPLAIN ANALYZE` and MariaDB has `ANALYZE <stmt>`, and
/// neither accepts the other's. Asking the server which it is would be a round
/// trip to avoid a round trip, so the fallback runs on the refusal.
pub(crate) async fn explain(
    db: &Db,
    database: Option<&str>,
    sql: &str,
    analyze: bool,
    cancel: CancellationToken,
) -> Result<ResultSet, DbError> {
    let (primary, fallback) = explain_commands(sql, analyze);
    if !analyze {
        return db
            .fetch_query(database, &primary, EXPLAIN_ROW_CAP, cancel)
            .await;
    }
    match explain_in_rolled_back_tx(db, database, &primary, cancel.clone()).await {
        // MariaDB: `EXPLAIN ANALYZE` is invalid — retry with `ANALYZE <stmt>`.
        Err(DbError::Query(_)) if fallback.is_some() => {
            explain_in_rolled_back_tx(db, database, &fallback.unwrap(), cancel).await
        }
        other => other,
    }
}

/// Run one analyzing-EXPLAIN command on a single connection, wrapped in a
/// transaction that is always rolled back. Separate from [`fetch_query`]
/// because that opens a fresh connection per call, which would put the
/// `BEGIN`, the measurement and the `ROLLBACK` on three different sessions.
async fn explain_in_rolled_back_tx(
    db: &Db,
    database: Option<&str>,
    cmd: &str,
    cancel: CancellationToken,
) -> Result<ResultSet, DbError> {
    let mut conn = db.open(database, false).await?;
    let conn_id = conn.id();
    if let Err(e) = conn.query_drop("BEGIN").await {
        let _ = conn.disconnect().await;
        return Err(DbError::Query(e.to_string()));
    }

    let mut dest = RowDest::Capped(EXPLAIN_ROW_CAP);
    let outcome = tokio::select! {
        r = collect_rows(&mut conn, cmd, &mut dest, true) => r,
        _ = cancel.cancelled() => {
            kill_query(db, conn_id).await;
            Err(DbError::Cancelled)
        }
    };

    // Unconditional. Dropping the connection would roll back too, but saying
    // so explicitly is what makes the guarantee readable at the call site.
    let _ = conn.query_drop("ROLLBACK").await;
    let _ = conn.disconnect().await;
    outcome
}
/// The `EXPLAIN`/`ANALYZE` command(s) for `sql`: the statement is trimmed of a
/// trailing `;`, then wrapped. Returns `(primary, fallback)` — for `analyze` the
/// fallback is MariaDB's `ANALYZE <stmt>` (MySQL uses `EXPLAIN ANALYZE`); plain
/// `EXPLAIN` has no fallback. Pure so the wrapping/fallback logic is unit-tested.
fn explain_commands(sql: &str, analyze: bool) -> (String, Option<String>) {
    let stmt = sql.trim().trim_end_matches(';').trim_end();
    if analyze {
        (
            format!("EXPLAIN ANALYZE {stmt}"),
            Some(format!("ANALYZE {stmt}")),
        )
    } else {
        (format!("EXPLAIN {stmt}"), None)
    }
}

// ── Reachability and small reads ─────────────────────────────────────────────

/// Is the server answering?
///
/// **The bound is here, not around the dispatch**, unlike [`fetch_sessions`]:
/// each engine bounds its own reachability check because each reaches the server
/// differently — `pg::ping` takes the deadline as a parameter and SQLite's wraps
/// an `open` that can block on a dead network share. `every_reachability_path_is_
/// bounded_by_a_timeout` names the file each one lives in for exactly that
/// reason.
pub(crate) async fn ping(db: &Db, timeout: std::time::Duration) -> Result<(), DbError> {
    let check = async {
        let mut conn = db.open(None, false).await?;
        let r = conn
            .query_drop("SELECT 1")
            .await
            .map_err(|e| DbError::Query(e.to_string()));
        let _ = conn.disconnect().await;
        r
    };
    tokio::time::timeout(timeout, check)
        .await
        .map_err(|_| DbError::Connect("timed out".to_string()))?
}

/// The user databases on this server, system schemas excluded.
pub(crate) async fn fetch_databases(db: &Db) -> Result<Vec<String>, DbError> {
    let listing = async {
        let mut conn = db.open(None, false).await?;
        let out = conn
            .query_map(
                "SELECT CAST(SCHEMA_NAME AS CHAR) AS n FROM information_schema.SCHEMATA \
                 WHERE SCHEMA_NAME NOT IN \
                   ('information_schema','mysql','performance_schema','sys') \
                 ORDER BY SCHEMA_NAME",
                |n: String| n,
            )
            .await
            .map_err(|e| DbError::Query(e.to_string()));
        let _ = conn.disconnect().await;
        out
    };
    tokio::time::timeout(crate::PING_TIMEOUT, listing)
        .await
        .map_err(|_| DbError::Connect("timed out".to_string()))?
}

/// `SELECT COUNT(*)` — the exact count, killable from a second connection.
pub(crate) async fn count_rows(
    db: &Db,
    database: &str,
    sql: &str,
    cancel: CancellationToken,
) -> Result<u64, DbError> {
    let mut conn = db.open(Some(database), false).await?;
    // The connection id, so a second connection can KILL the scan — the same
    // shape `fetch_query` uses, and the only thing that actually stops work
    // already running on the server.
    let conn_id = conn.id();
    let out = tokio::select! {
        r = conn.query_first::<u64, _>(sql) => r
            .map_err(|e| DbError::Query(e.to_string()))
            .and_then(|n| n.ok_or_else(|| DbError::Query("COUNT(*) returned no row".into()))),
        _ = cancel.cancelled() => {
            kill_query(db, conn_id).await;
            Err(DbError::Cancelled)
        }
    };
    let _ = conn.disconnect().await;
    out
}

/// One table's first `limit` rows, for the Live Monitor.
///
/// The caller bounds this — [`crate::Db::fetch_table`] wraps the whole dispatch,
/// because the Monitor re-arms every couple of seconds and a dark host would
/// otherwise cost the OS connect timeout on every tick.
pub(crate) async fn fetch_table(
    db: &Db,
    database: &str,
    schema: Option<&str>,
    table: &str,
    order_by: Option<&[String]>,
    limit: usize,
    cancel: CancellationToken,
) -> Result<ResultSet, DbError> {
    // MySQL has no namespace level — the database already is one.
    debug_assert!(schema.is_none(), "MySQL tables carry no namespace");
    let sql = format!(
        "SELECT * FROM {}.{}{} LIMIT {}",
        ident(database),
        ident(table),
        order_by_clause(order_by, ident),
        limit
    );
    db.fetch_query(Some(database), &sql, limit, cancel).await
}

/// Validate `stmt` by preparing it and throwing the prepared statement away.
pub(crate) async fn prepare_check(
    db: &Db,
    database: Option<&str>,
    stmt: &str,
) -> Result<(), DbError> {
    let mut conn = db.open(database, false).await?;
    let result = match conn.prep(stmt).await {
        Ok(prepared) => {
            let _ = conn.close(prepared).await;
            Ok(())
        }
        Err(e) => {
            let msg = e.to_string();
            // 1295 = "not supported in the prepared statement protocol": we
            // can't validate it, so don't flag a false error.
            if msg.contains("1295")
                || msg
                    .to_ascii_lowercase()
                    .contains("prepared statement protocol")
            {
                Ok(())
            } else {
                Err(DbError::Query(msg))
            }
        }
    };
    let _ = conn.disconnect().await;
    result
}

/// The wire decoders' own tests.
///
/// **The rest of this family is still in `lib.rs`, and deliberately so for one
/// more step**: the tests for `binary_as_text`, the fractional scales and the
/// zerofill padding sit interleaved with tests for `value_to_param`,
/// `build_refetch_sql` and `build_blob_select`, which are write-back builders
/// that have not moved yet. Splitting that block by hand now would be the
/// error-prone half of a move done twice; it comes across whole with the write
/// paths. `lib.rs` names the three decoders it still reaches for in an import
/// written to fail loudly when they go.
#[cfg(test)]
mod decode_tests {
    use super::*;

    #[test]
    fn resolve_type_name_maps_common_types() {
        let non_binary = false;
        let plain = false;
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_LONG, false, non_binary, plain),
            "INT"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_LONGLONG, false, non_binary, plain),
            "BIGINT"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_NEWDECIMAL, false, non_binary, plain),
            "DECIMAL"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_DATETIME, false, non_binary, plain),
            "DATETIME"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_JSON, false, non_binary, plain),
            "JSON"
        );
    }

    #[test]
    fn resolve_type_name_binary_charset_flips_string_and_blob_types() {
        // charset 63 (binary) turns text types into their binary counterparts.
        let plain = false;
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_VAR_STRING, false, true, plain),
            "VARBINARY"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_VAR_STRING, false, false, plain),
            "VARCHAR"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_STRING, false, true, plain),
            "BINARY"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_STRING, false, false, plain),
            "CHAR"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_BLOB, false, true, plain),
            "BLOB"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_BLOB, false, false, plain),
            "TEXT"
        );
    }

    #[test]
    fn resolve_type_name_unsigned_only_on_numeric_types() {
        let plain = false;
        // UNSIGNED suffix appended for numerics…
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_LONG, true, false, plain),
            "INT UNSIGNED"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_NEWDECIMAL, true, false, plain),
            "DECIMAL UNSIGNED"
        );
        // …but never for non-numeric types, even if the flag is set.
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_DATETIME, true, false, plain),
            "DATETIME"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_VAR_STRING, true, false, plain),
            "VARCHAR"
        );
    }

    #[test]
    fn is_binary_data_type_flags_only_raw_byte_types() {
        for t in [
            "VARBINARY",
            "BINARY",
            "BLOB",
            "TINYBLOB",
            "LONGBLOB",
            "GEOMETRY",
        ] {
            assert!(is_binary_data_type(t), "{t} should be binary data");
        }
        // Temporal/numeric report charset 63 too, but aren't binary DATA — and
        // `BIT` is in that company rather than with the blobs: it arrives as
        // bytes and is a *number*, which `convert_row` reads with `bit_display`
        // (see `core::model::type_is_bit`). Being on the list above made a
        // `BIT(8)` read `<1 bytes>`, kept it out of the CSV and JSON exports, and
        // made the column read-only.
        for t in [
            "DATETIME", "INT", "VARCHAR", "TEXT", "JSON", "DECIMAL", "BIT",
        ] {
            assert!(!is_binary_data_type(t), "{t} should not be binary data");
        }
    }

    #[test]
    fn column_origin_none_for_empty_org_table() {
        let flags = CoreColFlags::default();
        // Expression/aggregate/literal → empty org_table → not editable.
        assert!(column_origin("db", "", "expr", flags, false).is_none());
    }

    #[test]
    fn column_origin_some_carries_provenance_and_flags() {
        let flags = CoreColFlags {
            primary_key: true,
            not_null: true,
            ..Default::default()
        };
        let o = column_origin("shop", "users", "id", flags, false).expect("has base table");
        assert_eq!(o.database, "shop");
        assert_eq!(o.table, "users");
        assert_eq!(o.column, "id");
        assert!(o.flags.primary_key);
        assert!(o.flags.not_null);
        assert!(!o.binary);
    }
}

#[cfg(test)]
mod explain_tests {
    use super::*;

    #[test]
    fn explain_commands_plain_has_no_fallback() {
        let (primary, fallback) = explain_commands("SELECT * FROM t", false);
        assert_eq!(primary, "EXPLAIN SELECT * FROM t");
        assert!(fallback.is_none());
    }

    #[test]
    fn explain_commands_analyze_offers_mariadb_fallback() {
        let (primary, fallback) = explain_commands("SELECT 1", true);
        assert_eq!(primary, "EXPLAIN ANALYZE SELECT 1");
        assert_eq!(fallback.as_deref(), Some("ANALYZE SELECT 1"));
    }

    #[test]
    fn explain_commands_strips_trailing_semicolon_and_space() {
        let (primary, _) = explain_commands("  SELECT 1 ;  ", false);
        assert_eq!(primary, "EXPLAIN SELECT 1");
    }
}
/// What a cancelled import reports, given what its `ROLLBACK` achieved.
///
/// **Cancelling is a write-path exit like any other, so it reports what the
/// rollback achieved.** It used to `kill_query` and disconnect, and the modal
/// then said, unconditionally, "the transaction rolled back, so nothing was
/// written" — which on `MyISAM`/`MEMORY`/`ARCHIVE`/`CSV` is false: every batch
/// already executed is durable there, so the user re-ran the import and doubled
/// the rows it had already loaded.
///
/// [`DbError::Cancelled`] is what the modal renders as "nothing was written",
/// so it is only for [`Rollback::Complete`]; anything else carries
/// [`Rollback::note`], which says the rows may still be there.
fn cancelled_import(undone: Rollback) -> DbError {
    match undone {
        Rollback::Complete => DbError::Cancelled,
        undone => DbError::Query(format!("Import cancelled{}", undone.note())),
    }
}

/// [`cancelled_import`]'s twin for a cancelled **grid write-back**, and the
/// same rule for the same reason.
///
/// Separate only because the sentence names a different act: "Import cancelled"
/// is wrong over a Commit, and the whole point of the non-`Complete` arm is
/// that the user reads it and knows what may still be in the table.
fn cancelled_write(undone: Rollback) -> DbError {
    match undone {
        Rollback::Complete => DbError::Cancelled,
        undone => DbError::Query(format!("Commit cancelled{}", undone.note())),
    }
}

/// The MySQL half of [`Db::import_rows`], on an already-open connection.
async fn import_on(
    db: &Db,
    conn: &mut Conn,
    conn_id: u32,
    dialect: schemaic_core::intel::SqlDialect,
    target: &ImportTarget<'_>,
    rows: RowSource<'_>,
    cancel: &CancellationToken,
) -> Result<u64, DbError> {
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());
    let cols: Vec<&str> = target.columns.iter().map(String::as_str).collect();
    conn.query_drop("BEGIN").await.map_err(qerr)?;

    let mut total: u64 = 0;
    // The row the byte ceiling held back from the previous batch — see
    // `next_batch`. It lives here so it cannot be lost between two of them.
    let mut held: Option<Vec<Value>> = None;
    loop {
        // **Between batches**, where the connection is idle and a `ROLLBACK`
        // is the connection's own next statement. A Stop pressed while the
        // reader is pulling rows lands here — `next_batch_off_executor` blocks
        // the task, so nothing could have observed the token earlier anyway.
        if cancel.is_cancelled() {
            return Err(cancelled_import(rollback(conn, "ROLLBACK").await));
        }
        // A reader error (a bad record, a value that wouldn't coerce) has to undo
        // the transaction too — returning straight out would leave it open until
        // the connection drops, which is a lock held for no reason.
        let batch = match next_batch_off_executor(rows, &mut held) {
            Ok(Some(b)) => b,
            Ok(None) => break,
            Err(e) => {
                let undone = rollback(conn, "ROLLBACK").await;
                return Err(match (e, undone) {
                    // Only worth saying when it isn't what the message implies.
                    (e, Rollback::Complete) => e,
                    (DbError::Query(msg), undone) => {
                        DbError::Query(format!("{msg}{}", undone.note()))
                    }
                    (e, _) => e,
                });
            }
        };
        let Some(sql) = schemaic_core::import::build_insert(
            target.database,
            target.schema,
            target.table,
            &cols,
            &batch,
            dialect,
        ) else {
            continue;
        };
        // **The killed statement is awaited, not dropped** — the same rule
        // `run_script_mysql` states, and for a sharper reason here: dropping it
        // desynchronises the result stream, and everything after that
        // (`ROLLBACK`, the `SHOW WARNINGS` behind `Rollback`) then reads
        // somebody else's reply. Scoped so the borrow ends before the rollback.
        let step = {
            let mut fut = std::pin::pin!(conn.query_drop(&sql));
            let raced = tokio::select! {
                r = fut.as_mut() => Some(r),
                _ = cancel.cancelled() => None,
            };
            match raced {
                Some(r) => Some(r),
                None => {
                    kill_query(db, conn_id).await;
                    let _ = fut.await;
                    None
                }
            }
        };
        match step {
            Some(Ok(())) => {}
            Some(Err(e)) => {
                let msg = e.to_string();
                let undone = rollback(conn, "ROLLBACK").await;
                return Err(DbError::Query(format!("{msg}{}", undone.note())));
            }
            None => return Err(cancelled_import(rollback(conn, "ROLLBACK").await)),
        }
        let affected = conn.affected_rows();
        if affected != batch.len() as u64 {
            let n = batch.len();
            let undone = rollback(conn, "ROLLBACK").await;
            return Err(DbError::Query(format!(
                "a batch of {n} rows inserted {affected}{}",
                undone.note()
            )));
        }
        total += affected;
    }

    // A Stop pressed after the last batch and before the commit is still a
    // Stop: committing here would land the whole import the user just stopped.
    if cancel.is_cancelled() {
        return Err(cancelled_import(rollback(conn, "ROLLBACK").await));
    }
    // **The fifth exit, and the one that had no `Rollback::note`.** Every other
    // failure here says what the rollback achieved; this one returned the
    // driver's bare sentence. On a MySQL `MyISAM`/`MEMORY`/`ARCHIVE`/`CSV`
    // target — the tables this whole apparatus exists for — a connection that
    // dies during the `COMMIT` left every row of the file durable in the table
    // while the modal showed "Server has gone away" and nothing else. Asking
    // for the rollback is what produces the answer: on a dead socket it fails
    // too, and `Rollback::note` is what turns that into a sentence about the
    // data rather than about the socket.
    if let Err(e) = conn.query_drop("COMMIT").await {
        let msg = qerr(e).to_string();
        let undone = rollback(conn, "ROLLBACK").await;
        return Err(DbError::Query(format!("{msg}{}", undone.note())));
    }
    Ok(total)
}

/// Apply a staged batch of grid mutations on an already-open connection:
/// deletes → updates → inserts, each required to affect exactly one row, the
/// whole batch rolled back if any doesn't. `scope` decides whether that
/// atomicity comes from a transaction of its own or a nested savepoint.
///
/// Deletes run first so "delete a row, then insert one with the same unique key"
/// works. The caller is responsible for `client_found_rows` being on — the guard
/// counts *matched* rows, not *changed* ones.
///
/// **`cancel` is handled *here*, not raced around this future.** That is the
/// rule [`Db::import_rows`] states with its measurement: a `tokio::select!`
/// around the whole write drops it mid-statement, and a dropped `mysql_async`
/// statement leaves the connection's result stream desynchronised — after which
/// the `ROLLBACK` and the `SHOW WARNINGS` behind [`Rollback`] read replies that
/// are not their own and `Complete` is claimed off garbage. Inside, the batch
/// stops between statements, or after *awaiting* a statement it killed, so the
/// rollback goes out on an intact protocol and [`cancelled_write`] can be
/// believed.
///
/// `None` for a caller that does not cancel, or one for which a `Rollback`
/// verdict read off *this* connection is not the answer:
/// [`crate::session::Session::commit_writes`] passes `None` because its batch is
/// a savepoint inside the user's transaction and `classify_isolated` is what
/// says what survived.
///
/// **That is a statement about who answers, not a licence to drop the future.**
/// This sentence used to read as the latter, and `Session::commit_writes` raced
/// `write_on` against its token and dropped it — a *transaction-state* argument
/// used to settle a *protocol* question, which is how the pinned connection came
/// to be left desynchronised. It goes through [`cancel_awaited`] now, which
/// kills the statement and awaits it; `None` here still means only that the
/// batch does not read its own rollback.
pub(crate) async fn write_on(
    conn: &mut Conn,
    write: &GridWrite,
    scope: TxScope,
    cancel: Option<(&Db, u32, &CancellationToken)>,
) -> Result<u64, DbError> {
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());
    conn.query_drop(scope.begin_sql()).await.map_err(qerr)?;

    // One statement + its 1-row check. On a miss the batch is undone and the
    // error describes what happened, in the caller's terms — the verdict and its
    // wording are `one_row_verdict`, shared with the PostgreSQL executor, and
    // what the rollback *achieved* is asked of the server rather than assumed.
    async fn one(
        conn: &mut Conn,
        scope: TxScope,
        sql: String,
        params: Params,
        step: WriteStep<'_>,
        cancel: Option<(&Db, u32, &CancellationToken)>,
    ) -> Result<u64, DbError> {
        // **The killed statement is awaited, not dropped** — see this
        // function's doc, and `import_on`, which states the rule and the
        // measurement behind it. Scoped so the borrow ends before the rollback.
        let sent = match cancel {
            None => Some(conn.exec_drop(sql, params).await),
            Some((db, conn_id, token)) => {
                let mut fut = std::pin::pin!(conn.exec_drop(sql, params));
                let raced = tokio::select! {
                    r = fut.as_mut() => Some(r),
                    _ = token.cancelled() => None,
                };
                match raced {
                    Some(r) => Some(r),
                    None => {
                        kill_query(db, conn_id).await;
                        let _ = fut.await;
                        None
                    }
                }
            }
        };
        let Some(sent) = sent else {
            return Err(cancelled_write(rollback(conn, scope.rollback_sql()).await));
        };
        if let Err(e) = sent {
            let msg = e.to_string();
            let undone = rollback(conn, scope.rollback_sql()).await;
            return Err(DbError::Query(format!("{msg}{}", undone.note())));
        }
        let affected = conn.affected_rows();
        if let Err(msg) = one_row_verdict(step, affected) {
            let undone = rollback(conn, scope.rollback_sql()).await;
            return Err(DbError::Query(format!("{msg}{}", undone.note())));
        }
        Ok(affected)
    }

    let mut total: u64 = 0;
    // Deletes → updates → inserts, ordered by `GridWrite::plan` rather than by
    // three loops each engine has to keep in step.
    for step in write.plan() {
        // Between statements, where the protocol is whole.
        if cancel.is_some_and(|(_, _, t)| t.is_cancelled()) {
            return Err(cancelled_write(rollback(conn, scope.rollback_sql()).await));
        }
        let (sql, params) = match step {
            WriteStep::Delete(del) => build_delete(del),
            WriteStep::Update(edit) => build_update(edit),
            WriteStep::Insert(ins) => build_insert(ins),
        };
        total += one(conn, scope, sql, params, step, cancel).await?;
    }

    if let Err(e) = conn.query_drop(scope.commit_sql()).await {
        let msg = e.to_string();
        let undone = rollback(conn, scope.rollback_sql()).await;
        return Err(DbError::Query(format!("{msg}{}", undone.note())));
    }
    Ok(total)
}

/// Roll back, and find out from the server whether it worked.
///
/// MySQL's `ROLLBACK` **succeeds** when the transaction touched a
/// non-transactional table (`MyISAM`, `MEMORY`, `ARCHIVE`, `CSV`) and raises
/// warning **1196** — *"Some non-transactional changed tables couldn't be rolled
/// back"* — instead. Every rollback on this path used to be `let _ =
/// conn.query_drop(…)`, discarding the result *and* the server's own statement
/// that the undo was partial, so the write path promised an atomicity the engine
/// had just said it couldn't provide.
///
/// `SHOW WARNINGS` is read immediately after, since the next statement clears
/// it. Anything unreadable resolves to [`Rollback::Unknown`] — the write path
/// must not claim more than it knows.
///
/// **`Unknown`, not `Incomplete`, and the difference is a sentence the user
/// acts on.** `Incomplete` says *this table's storage engine is not
/// transactional, so the rows already written remain* — a claim about the
/// engine, which only warning 1196 establishes. The other two routes here
/// establish nothing: a `ROLLBACK` sent down a socket that is already gone never
/// reached a server, and the server had almost certainly rolled the transaction
/// back itself when the connection dropped. Told `Incomplete`, the user audits
/// an InnoDB table that is exactly as they left it.
///
/// The unreadable-warnings route was worse than misdescribed — it was silently
/// `Complete`. `unwrap_or_default()` turns a failed `SHOW WARNINGS` into an
/// empty vector, no 1196 is found, and the write path then promised a clean
/// rollback on the strength of a query that did not run. The paragraph above
/// has claimed otherwise since it was written.
async fn rollback(conn: &mut Conn, sql: &str) -> Rollback {
    /// `ER_WARNING_NOT_COMPLETE_ROLLBACK`.
    const INCOMPLETE_ROLLBACK: u32 = 1196;
    if conn.query_drop(sql).await.is_err() {
        return Rollback::Unknown;
    }
    let Ok(warnings) = conn
        .query::<(String, u32, String), _>("SHOW WARNINGS")
        .await
    else {
        return Rollback::Unknown;
    };
    if warnings
        .iter()
        .any(|(_, code, _)| *code == INCOMPLETE_ROLLBACK)
    {
        Rollback::Incomplete
    } else {
        Rollback::Complete
    }
}

/// [`Db::fetch_blob`]'s MySQL body, on an already-open connection — so the
/// pinned connection of a manual-transaction tab can run the same read and see
/// its own uncommitted bytes.
pub(crate) async fn blob_on(conn: &mut Conn, r: &BlobRef) -> Result<Option<BlobValue>, DbError> {
    let (sql, params) = build_blob_select(r);
    let row: Option<mysql_async::Row> = conn
        .exec_first(sql, params)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;
    let Some(row) = row else { return Ok(None) };
    // `OCTET_LENGTH(NULL)` is NULL, which is how a NULL cell arrives here.
    let Some(len) = row.get::<Option<u64>, _>(0).flatten() else {
        return Ok(None);
    };
    let bytes = row
        .get::<Option<Vec<u8>>, _>(1)
        .flatten()
        .unwrap_or_default();
    Ok(Some(BlobValue { bytes, len }))
}

/// The most this connection can be asked for, in the server's own words.
///
/// **A row has to fit in one `max_allowed_packet`, and on MySQL the blob really
/// does cross the wire** — the row is not spared the way PostgreSQL's and
/// SQLite's are. MariaDB ships that setting at 16 MiB, a **quarter** of
/// [`FETCH_CAP`], so asking for the full 64 MiB of a large value did not fail
/// politely: measured against MariaDB 10.11 with a 20 MiB `LONGBLOB`, the
/// server dropped the connection mid-row (`ERROR 2013`), taking a
/// manual-transaction tab's pinned session and its uncommitted work with it.
///
/// Read from `@@max_allowed_packet` **inside the statement** rather than in a
/// round trip of its own, so there is no window for the setting to change
/// between the asking and the reading, and no second query on a path that is
/// one click.
///
/// The 1 MiB of headroom is for everything else in the packet — the
/// `OCTET_LENGTH` column, the row and column framing, the protocol's own
/// bookkeeping — and is deliberately generous, because being 100 bytes over
/// costs the whole connection while being 1 MiB under costs a truncation the
/// panel already knows how to describe. `GREATEST` keeps the arithmetic
/// positive on a server configured below the headroom (MariaDB's floor is
/// 1 KiB), and the `CAST` keeps the subtraction from wrapping in unsigned.
const PACKET_ROOM: &str = "GREATEST(1024, CAST(@@max_allowed_packet AS SIGNED) - 1048576)";

/// Build the `SELECT OCTET_LENGTH(c), SUBSTRING(c, 1, LEAST(?, …)) … WHERE
/// <key> <=> ? … LIMIT 1` behind [`blob_on`].
///
/// **The length and the bytes come from one row of one statement**, not two
/// queries: asked separately they can straddle another session's `UPDATE`, and
/// the pair is what [`BlobValue::truncated`] reads to decide whether saving the
/// buffer would write a file that is not the data.
///
/// `SUBSTRING` on a binary string is byte-indexed in MySQL (it is
/// character-indexed only for a character string), so the cap really is octets.
/// **Two caps, and the smaller wins**: ours ([`FETCH_CAP`], bound) and the
/// server's ([`PACKET_ROOM`], read live). Neither subsumes the other — a small
/// `max_allowed_packet` bounds a large blob, and a large one leaves `FETCH_CAP`
/// the operative limit — and a value cut by either arrives with its true
/// `OCTET_LENGTH` beside it, so [`BlobValue::truncated`] answers `true` and the
/// panel shows the prefix and refuses to save it. That is the whole point of
/// capping rather than erroring: 16 MiB of a 20 MiB value, honestly labelled,
/// beats a dropped connection.
///
/// The WHERE is `build_update`'s, NULL-safe `<=>` and all — the identity of a
/// row is one thing on this path, whether it is being written or read.
fn build_blob_select(r: &BlobRef) -> (String, Params) {
    let mut params: Vec<MyValue> = Vec::with_capacity(r.key.len() + 1);
    params.push(MyValue::UInt(FETCH_CAP as u64));
    let where_sql = r
        .key
        .iter()
        .map(|(col, val)| {
            params.push(value_to_param(val));
            format!("{} <=> ?", ident(col))
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let col = ident(&r.column);
    let sql = format!(
        "SELECT OCTET_LENGTH({col}), SUBSTRING({col}, 1, LEAST(?, {PACKET_ROOM})) \
         FROM {}.{} WHERE {where_sql} LIMIT 1",
        ident(&r.database),
        ident(&r.table),
    );
    (sql, Params::Positional(params))
}

/// Re-`SELECT` just-edited rows on an already-open connection. Read-only, so it
/// is safe both on a fresh connection and inside an open transaction — and
/// inside one it is *required*, since only that connection can see the
/// uncommitted rows it just wrote.
pub(crate) async fn refetch_on(
    conn: &mut Conn,
    template: &RefetchTemplate,
    rows: &[RefetchRow],
) -> Result<Vec<(usize, Vec<Value>)>, DbError> {
    let sql = build_refetch_sql(template);
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let params: Vec<MyValue> = row.key.iter().map(value_to_param).collect();
        let mut result = conn
            .exec_iter(sql.as_str(), Params::Positional(params))
            .await
            .map_err(qerr)?;
        // Column metadata (owned) before consuming the result stream.
        let columns: Vec<Column> = result.columns_ref().iter().map(map_column).collect();
        // **Before the collect**, which consumes the result and empties
        // `columns_ref`. The declared fractional precision is only on the wire.
        let scale = fractional_scales(result.columns_ref());
        // **This is the path that needs it.** A prepared statement sends a
        // `ZEROFILL` column as a bare integer, so the splice would otherwise
        // paint `7` over the `0007` the load read — see `zerofill_value`.
        let zerofill = zerofill_widths(result.columns_ref());
        let fetched: Vec<Row> = result.collect::<Row>().await.map_err(qerr)?;
        if let Some(r) = fetched.first() {
            let binary: Vec<bool> = columns.iter().map(Column::is_binary).collect();
            let bit: Vec<bool> = columns
                .iter()
                .map(|c| schemaic_core::model::type_is_bit(&c.type_name))
                .collect();
            let kinds: Vec<NumKind> = columns.iter().map(|c| num_kind(&c.type_name)).collect();
            out.push((
                row.data_row,
                convert_row(r, &columns, &binary, &bit, &scale, &kinds, &zerofill),
            ));
        }
    }
    Ok(out)
}

/// One staged cell value as a MySQL bound parameter.
///
/// `Text` and `Bytes` both become `MyValue::Bytes` — the wire has one
/// length-prefixed octet-string and the server coerces it to the column type —
/// but they arrive there by different routes and only one of them is reversible:
/// `Text` is the user's characters encoded as UTF-8, `Bytes` is the octets
/// themselves, unencoded. Collapsing the two at the *call site* is what would
/// hurt, because `String::into_bytes` on a lossily-decoded blob is not the blob.
fn cell_param(v: &CellEdit) -> MyValue {
    match v {
        CellEdit::Text(t) => MyValue::Bytes(t.clone().into_bytes()),
        CellEdit::Bytes(b) => MyValue::Bytes(b.to_vec()),
        CellEdit::Null => MyValue::NULL,
    }
}

/// Build a parameterized `UPDATE db.table SET … WHERE …` for one row edit.
/// Identifiers are backtick-escaped; every value is a bound parameter.
fn build_update(edit: &RowEdit) -> (String, Params) {
    let mut params: Vec<MyValue> = Vec::with_capacity(edit.set.len() + edit.key.len());
    let set_sql = edit
        .set
        .iter()
        .map(|(col, val)| {
            params.push(cell_param(val));
            format!("{} = ?", ident(col))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let where_sql = edit
        .key
        .iter()
        .map(|(col, val)| {
            params.push(value_to_param(val));
            // NULL-safe equality so a NULL key value matches (plain `= NULL`
            // never does). Float/binary keys are excluded upstream in
            // `resolve_key`, where they can't be matched reliably at all.
            format!("{} <=> ?", ident(col))
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let sql = format!(
        "UPDATE {}.{} SET {set_sql} WHERE {where_sql}",
        ident(&edit.database),
        ident(&edit.table),
    );
    (sql, Params::Positional(params))
}

/// Build a parameterized `INSERT INTO db.table (cols) VALUES (?, …)` for one new
/// row. Identifiers are backtick-escaped; every value is a bound parameter — see
/// [`cell_param`]. Columns not listed take their server default — with none
/// listed, `() VALUES ()` inserts an all-defaults row.
fn build_insert(ins: &RowInsert) -> (String, Params) {
    let mut params: Vec<MyValue> = Vec::with_capacity(ins.cols.len());
    let cols_sql = ins
        .cols
        .iter()
        .map(|(col, val)| {
            params.push(cell_param(val));
            ident(col)
        })
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = vec!["?"; ins.cols.len()].join(", ");
    let sql = format!(
        "INSERT INTO {}.{} ({cols_sql}) VALUES ({placeholders})",
        ident(&ins.database),
        ident(&ins.table),
    );
    (sql, Params::Positional(params))
}

/// Build a parameterized `DELETE FROM db.table WHERE …` for one row, keyed by its
/// identity (NULL-safe `<=>` per key column, like `build_update`'s WHERE). Every
/// value is a bound parameter.
fn build_delete(del: &RowDelete) -> (String, Params) {
    let mut params: Vec<MyValue> = Vec::with_capacity(del.key.len());
    let where_sql = del
        .key
        .iter()
        .map(|(col, val)| {
            params.push(value_to_param(val));
            format!("{} <=> ?", ident(col))
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let sql = format!(
        "DELETE FROM {}.{} WHERE {where_sql}",
        ident(&del.database),
        ident(&del.table),
    );
    (sql, Params::Positional(params))
}

/// Build the `SELECT … WHERE <key> <=> ? … LIMIT 1` used to re-fetch one edited
/// row after a commit. Identifiers are backtick-escaped; the key columns become
/// positional NULL-safe placeholders (bound by the caller from each row's key,
/// in `template.key_cols` order). Pure so the SQL shape is unit-tested.
fn build_refetch_sql(template: &RefetchTemplate) -> String {
    let cols_sql = template
        .columns
        .iter()
        .map(|c| ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    // Key first, then the confirming columns — the order `edit::refetch_key`
    // builds the values in, and the reason they are here at all is
    // `RefetchTemplate::confirm_cols`: the write's own `WHERE` carries them and
    // this copy dropped them.
    let where_sql = template
        .key_cols
        .iter()
        .chain(template.confirm_cols.iter())
        .map(|&kci| format!("{} <=> ?", ident(&template.columns[kci])))
        .collect::<Vec<_>>()
        .join(" AND ");
    format!(
        "SELECT {cols_sql} FROM {}.{} WHERE {where_sql} LIMIT 1",
        ident(&template.database),
        ident(&template.table),
    )
}

/// Backtick-quote an identifier, doubling any embedded backtick.
///
/// The one identifier-quoting rule, pinned to this path's only engine — these
/// statements are built for MySQL by construction (the PostgreSQL write path is
/// `pg.rs`'s).
pub(crate) fn ident(name: &str) -> String {
    schemaic_core::export::ident_sql(name, schemaic_core::intel::SqlDialect::MySql)
}

/// Convert a typed cell value into a bound parameter for a `WHERE` comparison.
fn value_to_param(v: &Value) -> MyValue {
    match v {
        Value::Null => MyValue::NULL,
        Value::Int(i) => MyValue::Int(*i),
        Value::UInt(u) => MyValue::UInt(*u),
        Value::Float(f) => MyValue::Double(*f),
        Value::Str(s) => MyValue::Bytes(s.clone().into_bytes()),
    }
}

/// Re-render a **binary-protocol** number the way the text protocol would have
/// sent it, for a `ZEROFILL` column — `width` is the column's display width, or
/// `None` for every other column, which passes through untouched. `scale` is the
/// column's declared decimals, which a float needs and an integer ignores.
///
/// **The two protocols disagree about these cells and only one of them is the
/// user's answer.** A prepared statement — which is what the post-commit
/// re-fetch runs — sends `Int(7)` for the cell `SELECT` sent as `0007`
/// (measured on MariaDB 10.11.14 and MySQL 8.4). Without this the splice would
/// paint `7` over the padded value the load put in the grid, and the column
/// would disagree with itself about a cell the user never edited.
///
/// **Floats are `ZEROFILL` columns too**, and covering the integer arms alone
/// left exactly that disagreement on them. MySQL's numeric-type syntax admits
/// the attribute on `FLOAT[(M,D)]` and `DOUBLE[(M,D)]`, and `zerofill_widths`
/// answers `Some` for every numeric column carrying the flag — so the width was
/// computed, carried into `convert_row`, and consumed by two of the arms that
/// could use it. Measured on MariaDB 10.11.14 **and** MySQL 8.4.11, where the
/// form is deprecated but still accepted: `DOUBLE(10,2) UNSIGNED ZEROFILL`
/// holding `123.45` reads back `0000123.45` over the text protocol, and
/// `FLOAT(8,2) UNSIGNED ZEROFILL` holding `12.5` reads `00012.50`. The splice
/// painted `123.45` over the first of those, in a cell the user never touched,
/// and an export taken before the next full re-run wrote the unpadded text out.
///
/// The scale is part of it: the server pads to `width` *including* the decimal
/// point and the fraction, so `{n:0w$.p$}` is the same rendering.
///
/// Padding only, never truncation: a value wider than the declared width is the
/// server's to render, and `format!` leaves it alone.
pub(crate) fn zerofill_value(v: Value, width: Option<usize>, scale: u32) -> Value {
    let Some(w) = width else {
        return v;
    };
    let p = scale as usize;
    match v {
        Value::Int(n) => Value::Str(format!("{n:0w$}")),
        Value::UInt(n) => Value::Str(format!("{n:0w$}")),
        // **`NOT_FIXED_DEC` is not a scale.** A `FLOAT`/`DOUBLE` declared
        // without `(M,D)` reports `decimals = 31`, and formatting to
        // thirty-one places would invent digits the server never sent. There
        // the shortest round-tripping form is what the text protocol prints,
        // padded — the same reading `binary_as_text` takes for an `f32`.
        Value::Float(f) if p >= 31 => Value::Str(format!("{:0>w$}", f.to_string())),
        Value::Float(f) => Value::Str(format!("{f:0w$.p$}")),
        other => other,
    }
}

/// Per-column display width for the `ZEROFILL` columns of a result, `None` for
/// every other column — computed **once for the result**, like
/// [`fractional_scales`] beside it, because it is a fact about the column.
pub(crate) fn zerofill_widths(columns: &[MyColumn]) -> Vec<Option<usize>> {
    columns
        .iter()
        .map(|c| {
            c.flags()
                .contains(ColumnFlags::ZEROFILL_FLAG)
                .then(|| c.column_length() as usize)
        })
        .collect()
}

// ── DDL, scripts, imports and write-back ─────────────────────────────────────

/// Run a reviewed plan against one database.
///
/// The `fail` shape is the caller's — see [`crate::Db::run_ddl`] for why a
/// half-applied plan reports *which* statement stopped it and how many are
/// already in effect rather than pretending to roll back.
pub(crate) async fn run_ddl(
    db: &Db,
    database: &str,
    stmts: &[String],
    cancel: CancellationToken,
    fail: impl Fn(usize, usize, DbError) -> DdlError,
) -> Result<(), DdlError> {
    let mut conn = db
        .open(Some(database), false)
        .await
        .map_err(|e| fail(0, 0, e))?;
    let conn_id = conn.id();
    // Best-effort: a server old enough not to have the variable keeps its own
    // default rather than failing the plan over the bound.
    let _ = conn.query_drop(lock_wait_sql(db.engine)).await;
    // Best-effort too, and for the same reason the dump writes it into the
    // file: every literal in `stmts` was written by `export::sql_literal`,
    // which doubles a backslash because that is what MySQL does with one by
    // default — and on a session carrying `NO_BACKSLASH_ESCAPES` the doubled
    // literal stores two. On a `CREATE USER … IDENTIFIED BY` that is an
    // account nobody can log in to — repairable now that the browser offers
    // a password reset, but through the same `ddl_string` this would have
    // got wrong, so the reset would store the same mangled value and the
    // escape has to be right here rather than fixable afterwards.
    //
    // **Scoped to the plan, never to the connection.** A user who sets that
    // mode means it for the SQL they *type*, and pinning it at connect time
    // would quietly change what their own statements mean. This connection
    // runs one reviewed plan and is disconnected on the way out, per the
    // one-connection-per-operation rule, so nothing here outlives the call.
    if let Some(sql) = schemaic_core::export::literal_mode_sql(db.engine.dialect()) {
        let _ = conn.query_drop(sql).await;
    }
    let dialect = db.engine.dialect();
    let mut out = Ok(());
    for (i, sql) in stmts.iter().enumerate() {
        let step = tokio::select! {
            r = conn.query_drop(sql) => r.map_err(|e| DbError::Query(e.to_string())),
            _ = cancel.cancelled() => {
                kill_query(db, conn_id).await;
                Err(DbError::Cancelled)
            }
        };
        if let Err(e) = step {
            // **What applied, not what succeeded.** A routine, trigger or
            // event edit is emitted wrapped in a session guard, and those
            // `SET`s succeed against session variables on a connection this
            // function disconnects four lines down — nothing about them
            // outlives the call. Counting them made a rejected `ALTER EVENT`
            // report "2 earlier statements already applied and cannot be
            // rolled back" over a plan that had changed nothing, on the app's
            // only disclosure of a genuinely half-applied migration.
            //
            // The decision is `ddl::applied_count`'s, and it is there rather
            // than a counter here because it is a decision about emitted SQL
            // and this loop has only strings — which is how the scaffolding
            // came to be counted in the first place.
            out = Err(fail(
                i,
                schemaic_core::ddl::applied_count(stmts, i, dialect),
                e,
            ));
            break;
        }
    }
    let _ = conn.disconnect().await;
    out
}

/// Run a plan about a **container** — a database or a schema — on a connection
/// attached to no database.
pub(crate) async fn run_server_ddl(
    db: &Db,
    stmts: &[String],
    cancel: CancellationToken,
    fail: impl Fn(usize, usize, DbError) -> DdlError,
) -> Result<(), DdlError> {
    // **Serverless, not `open(None)`.** `avoid` names the database this
    // plan is about to drop or create, and `open(None)` fills an unnamed
    // database in from the connection's own — so `DROP DATABASE shop` on a
    // connection configured for `shop` ran on a session pointed at its
    // target. The comment here used to claim the opposite, and was true
    // until the connection gained a configured database. (PostgreSQL still
    // reads `avoid` in its own arm above: it must connect to *some*
    // database, so it picks one that is not the target. MySQL needs none.)
    let mut conn = db.open_serverless(false).await.map_err(|e| fail(0, 0, e))?;
    let conn_id = conn.id();
    let _ = conn.query_drop(lock_wait_sql(db.engine)).await;
    // The same literal-mode pin `run_ddl` sets, for the same reason: a
    // container plan carries literals too (a `CREATE DATABASE`'s comment,
    // a collation name), and this connection is as short-lived as that one.
    if let Some(sql) = schemaic_core::export::literal_mode_sql(db.engine.dialect()) {
        let _ = conn.query_drop(sql).await;
    }
    let mut out = Ok(());
    for (i, sql) in stmts.iter().enumerate() {
        let step = tokio::select! {
            r = conn.query_drop(sql) => r.map_err(|e| DbError::Query(e.to_string())),
            _ = cancel.cancelled() => {
                kill_query(db, conn_id).await;
                Err(DbError::Cancelled)
            }
        };
        if let Err(e) = step {
            // No session-guard scaffolding on this path — every statement
            // here is one the user reviewed — so what applied is simply how
            // many ran, and `ddl::applied_count` has nothing to discount.
            out = Err(fail(i, i, e));
            break;
        }
    }
    let _ = conn.disconnect().await;
    out
}

/// Bulk-insert `rows` into `target`, in batches, cancellable between them.
pub(crate) async fn import_rows(
    db: &Db,
    target: ImportTarget<'_>,
    rows: RowSource<'_>,
    cancel: CancellationToken,
) -> Result<u64, DbError> {
    let mut conn = db.open(Some(target.database), false).await?;
    let conn_id = conn.id();
    // **The cancel is inside the loop, not a race around it.** This was
    // `tokio::select!` over the whole of `import_on`, and the cancel arm
    // then *dropped* that future mid-statement — which leaves a
    // `mysql_async` connection's result stream desynchronised. Measured on
    // MariaDB 10.11.14 and MySQL 8.4.11: after the drop, `SELECT
    // CONNECTION_ID()` came back `Ok(None)`, the following `ROLLBACK`
    // "succeeded" and `SHOW WARNINGS` was empty — so the rollback below
    // classified `Complete` off a reply that was not its own, and a
    // cancelled import into a `MyISAM` table reported `DbError::Cancelled`
    // (the variant the modal renders as *"nothing was written"*) over 1,000
    // rows that were permanently there. That is the exact failure this
    // path's `Rollback::note()` was added to prevent, still live on the one
    // exit that dropped the connection out from under it.
    //
    // `import_on` now owns the token and stops at a point where the
    // protocol is intact: between batches, or after awaiting a statement it
    // killed — the same "the killed statement is awaited, not dropped"
    // rule `run_script_mysql` states.
    let outcome = import_on(
        db,
        &mut conn,
        conn_id,
        db.engine.dialect(),
        &target,
        rows,
        &cancel,
    )
    .await;
    let _ = conn.disconnect().await;
    outcome
}

/// The write paths and the wire decoders, tested beside what they build.
#[cfg(test)]
mod write_tests {
    use super::*;
    // `the_write_paths_quote_identifiers_the_way_core_does` asks about both
    // quoters at once, which is the point of it: the test is that each engine's
    // statements are quoted the way `core::export` would. SQLite's lives in
    // `lib.rs` beside `sqlite.rs`'s use of it.
    use crate::ident_sqlite;

    #[test]
    fn build_insert_sql_shapes() {
        // Normal insert: listed columns → backtick-quoted names + placeholders.
        let ins = RowInsert {
            database: "db".to_string(),
            schema: None,
            table: "users".to_string(),
            cols: vec![
                ("name".to_string(), CellEdit::Text("Ada".to_string())),
                ("email".to_string(), CellEdit::Null), // explicit NULL
            ],
        };
        let (sql, _) = build_insert(&ins);
        assert_eq!(
            sql,
            "INSERT INTO `db`.`users` (`name`, `email`) VALUES (?, ?)"
        );

        // All-defaults insert (no columns set) → `() VALUES ()`.
        let empty = RowInsert {
            database: "db".to_string(),
            schema: None,
            table: "t".to_string(),
            cols: vec![],
        };
        let (sql, _) = build_insert(&empty);
        assert_eq!(sql, "INSERT INTO `db`.`t` () VALUES ()");

        // Identifiers with backticks are doubled.
        let weird = RowInsert {
            database: "d`b".to_string(),
            schema: None,
            table: "t".to_string(),
            cols: vec![("a`b".to_string(), CellEdit::Text("x".to_string()))],
        };
        let (sql, _) = build_insert(&weird);
        assert_eq!(sql, "INSERT INTO `d``b`.`t` (`a``b`) VALUES (?)");
    }

    #[test]
    fn build_delete_sql_shape() {
        // NULL-safe equality per key column (composite key joins with AND).
        let del = RowDelete {
            database: "db".to_string(),
            schema: None,
            table: "users".to_string(),
            key: vec![
                ("id".to_string(), Value::Int(7)),
                ("tenant".to_string(), Value::Str("acme".to_string())),
            ],
        };
        let (sql, _) = build_delete(&del);
        assert_eq!(
            sql,
            "DELETE FROM `db`.`users` WHERE `id` <=> ? AND `tenant` <=> ?"
        );
    }

    fn positional(p: &Params) -> &[MyValue] {
        match p {
            Params::Positional(v) => v.as_slice(),
            _ => panic!("expected positional params"),
        }
    }

    #[test]
    fn build_update_sql_and_param_order() {
        // SET params come first (in column order), then WHERE key params.
        let edit = RowEdit {
            database: "db".to_string(),
            schema: None,
            table: "users".to_string(),
            set: vec![
                ("name".to_string(), CellEdit::Text("Ada".to_string())),
                ("nickname".to_string(), CellEdit::Null), // set to NULL
            ],
            key: vec![("id".to_string(), Value::Int(7))],
        };
        let (sql, params) = build_update(&edit);
        assert_eq!(
            sql,
            "UPDATE `db`.`users` SET `name` = ?, `nickname` = ? WHERE `id` <=> ?"
        );
        let p = positional(&params);
        assert_eq!(p.len(), 3);
        assert!(matches!(&p[0], MyValue::Bytes(b) if b == b"Ada"));
        assert!(matches!(p[1], MyValue::NULL));
        assert!(matches!(p[2], MyValue::Int(7)));
    }

    /// **Bytes bind as bytes, and the two shapes are not the same param.**
    /// `MyValue::Bytes` is the wire shape both take, which is exactly why this
    /// is worth pinning: `Text` reaches it through `String::into_bytes` (UTF-8
    /// encoding the user's characters) and `Bytes` reaches it unencoded, so the
    /// two agree on every ASCII fixture and diverge on the first byte a blob
    /// actually contains. The fixture is a PNG header for that reason — `0x89`
    /// is not valid UTF-8 on its own, so a `Bytes` value that had gone through
    /// the text arm could not have arrived intact.
    #[test]
    fn build_update_binds_bytes_unencoded_next_to_a_text_column() {
        let png = vec![0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let edit = RowEdit {
            database: "sakila".to_string(),
            schema: None,
            table: "staff".to_string(),
            set: vec![
                ("first_name".to_string(), CellEdit::Text("Ada".to_string())),
                ("picture".to_string(), CellEdit::bytes(png.clone())),
                ("last_name".to_string(), CellEdit::Null),
            ],
            key: vec![("staff_id".to_string(), Value::Int(1))],
        };
        let (sql, params) = build_update(&edit);
        assert_eq!(
            sql,
            "UPDATE `sakila`.`staff` SET `first_name` = ?, `picture` = ?, `last_name` = ? \
             WHERE `staff_id` <=> ?"
        );
        let p = positional(&params);
        assert_eq!(p.len(), 4);
        assert!(matches!(&p[0], MyValue::Bytes(b) if b == b"Ada"));
        assert!(
            matches!(&p[1], MyValue::Bytes(b) if *b == png),
            "the blob's own octets, not a re-encoding of them"
        );
        assert!(matches!(p[2], MyValue::NULL));
        assert!(matches!(p[3], MyValue::Int(1)));
    }

    /// The same for an `INSERT` — a new row can carry a file too, and the
    /// `VALUES` list binds in column order.
    #[test]
    fn build_insert_binds_bytes_in_column_order() {
        let ins = RowInsert {
            database: "db".to_string(),
            schema: None,
            table: "docs".to_string(),
            cols: vec![
                (
                    "payload".to_string(),
                    CellEdit::bytes(vec![0xFF, 0x00, 0xFE]),
                ),
                ("title".to_string(), CellEdit::Text("x".to_string())),
            ],
        };
        let (sql, params) = build_insert(&ins);
        assert_eq!(
            sql,
            "INSERT INTO `db`.`docs` (`payload`, `title`) VALUES (?, ?)"
        );
        let p = positional(&params);
        assert!(matches!(&p[0], MyValue::Bytes(b) if *b == vec![0xFFu8, 0x00, 0xFE]));
        assert!(matches!(&p[1], MyValue::Bytes(b) if b == b"x"));
    }

    /// An empty file is a value, not an absence: zero bytes bind as a zero-length
    /// param, which MySQL stores as an empty blob. `NULL` is the other thing, and
    /// the two must not collapse — a `NOT NULL BLOB` column accepts the first and
    /// rejects the second.
    #[test]
    fn zero_bytes_is_an_empty_blob_and_not_null() {
        let ins = RowInsert {
            database: "db".to_string(),
            schema: None,
            table: "docs".to_string(),
            cols: vec![("payload".to_string(), CellEdit::bytes(Vec::new()))],
        };
        let (_, params) = build_insert(&ins);
        let p = positional(&params);
        assert!(matches!(&p[0], MyValue::Bytes(b) if b.is_empty()));
        assert!(!matches!(p[0], MyValue::NULL));
    }

    #[test]
    fn build_update_escapes_backtick_identifiers() {
        let edit = RowEdit {
            database: "d`b".to_string(),
            schema: None,
            table: "t`t".to_string(),
            set: vec![("a`b".to_string(), CellEdit::Text("x".to_string()))],
            key: vec![("k`k".to_string(), Value::Int(1))],
        };
        let (sql, _) = build_update(&edit);
        assert_eq!(
            sql,
            "UPDATE `d``b`.`t``t` SET `a``b` = ? WHERE `k``k` <=> ?"
        );
    }

    #[test]
    fn ident_doubles_embedded_backticks() {
        assert_eq!(ident("plain"), "`plain`");
        assert_eq!(ident("a`b"), "`a``b`");
        // Two backticks → each doubled (four), wrapped → six backticks.
        assert_eq!(ident("``"), "`".repeat(6));
    }

    #[test]
    fn value_to_param_maps_each_variant() {
        assert!(matches!(value_to_param(&Value::Null), MyValue::NULL));
        assert!(matches!(value_to_param(&Value::Int(-3)), MyValue::Int(-3)));
        assert!(matches!(value_to_param(&Value::UInt(3)), MyValue::UInt(3)));
        assert!(matches!(value_to_param(&Value::Float(1.5)), MyValue::Double(f) if f == 1.5));
        assert!(matches!(value_to_param(&Value::Str("s".into())), MyValue::Bytes(b) if b == b"s"));
    }

    // ── the two protocols, and the one row read through both ────────────

    /// **The re-fetch reads the same row over a different protocol**, and its
    /// answer is spliced straight over the cells on screen. `collect_rows` uses
    /// `query_iter` (text), where every value is `Bytes`; `refetch_on` uses
    /// `exec_iter` (binary, the statement being prepared with the key bound),
    /// where MySQL sends `DATETIME` as `Date`, `TIME` as `Time` and `FLOAT` as an
    /// `f32`. Those fell to `convert_row`'s catch-all, `MyValue::as_sql` — a
    /// **SQL literal**, not the text form — which prints a `Date` with a zero
    /// time as `'YYYY-MM-DD'` and a `Time` as `'{:03}:{:02}:{:02}'`.
    ///
    /// Measured on MariaDB 10.11.14 and MySQL 8.4.11: editing one column of
    /// `(1, 'a', '2024-01-15 00:00:00', '10:30:00', 3.14)` spliced the row back
    /// with `2024-01-15`, `010:30:00` and `3.140000104904175` in three cells
    /// nobody touched. Re-running the query restored them, so the grid disagreed
    /// with itself about one row, and any export taken in between wrote the wrong
    /// text.
    #[test]
    fn a_binary_temporal_reads_back_as_the_text_protocol_wrote_it() {
        // The zero time a `DATETIME` carries and `as_sql` drops.
        assert_eq!(
            binary_as_text(&MyValue::Date(2024, 1, 15, 0, 0, 0, 0), "DATETIME", 0).as_deref(),
            Some("2024-01-15 00:00:00")
        );
        // A bare `DATE` has no time to print, and must not grow one.
        assert_eq!(
            binary_as_text(&MyValue::Date(2024, 1, 15, 0, 0, 0, 0), "DATE", 0).as_deref(),
            Some("2024-01-15")
        );
        // `TIME` is a duration: `as_sql`'s `{:03}` made this `010:30:00`.
        assert_eq!(
            binary_as_text(&MyValue::Time(false, 0, 10, 30, 0, 0), "TIME", 0).as_deref(),
            Some("10:30:00")
        );
        // …which runs past a day, and backwards.
        assert_eq!(
            binary_as_text(&MyValue::Time(false, 3, 2, 0, 0, 0), "TIME", 0).as_deref(),
            Some("74:00:00")
        );
        assert_eq!(
            binary_as_text(&MyValue::Time(true, 0, 1, 2, 3, 0), "TIME", 0).as_deref(),
            Some("-01:02:03")
        );
        // An `f32` widened to `f64` is what produced `3.140000104904175`; the
        // value here is one whose widening is visible without being a constant
        // clippy recognises — `0.1f32 as f64` is `0.10000000149011612`.
        assert_eq!(
            binary_as_text(&MyValue::Float(0.1), "FLOAT", 0).as_deref(),
            Some("0.1")
        );
        assert_ne!(
            (0.1f32 as f64).to_string(),
            "0.1",
            "if this ever holds, the widening was never the bug"
        );
        // A `DOUBLE` was already right and stays out of this.
        assert_eq!(binary_as_text(&MyValue::Double(1.25), "DOUBLE", 0), None);
        assert_eq!(binary_as_text(&MyValue::Int(7), "INT", 0), None);
    }

    /// **The declared precision, and only that.** A `DATETIME(3)` reads
    /// `…:00.120` and a bare `DATETIME` reads `…:00`, so the fraction cannot come
    /// from the value — the binary protocol always sends microseconds, and
    /// `.120` trimmed of trailing zeros would be `.12`. It comes off the wire's
    /// column definition, which is also the only place it is: `type_name_of`
    /// builds `DATETIME` from the type code with no precision in it.
    #[test]
    fn the_fraction_follows_the_columns_declared_precision() {
        let at = |scale| {
            binary_as_text(
                &MyValue::Date(2024, 1, 15, 8, 9, 10, 120_000),
                "DATETIME",
                scale,
            )
            .expect("a temporal")
        };
        assert_eq!(at(0), "2024-01-15 08:09:10");
        assert_eq!(at(3), "2024-01-15 08:09:10.120");
        assert_eq!(at(6), "2024-01-15 08:09:10.120000");
        // MySQL's own maximum, so a server answering more does not widen it.
        assert_eq!(at(9), "2024-01-15 08:09:10.120000");
        assert_eq!(
            binary_as_text(&MyValue::Time(false, 0, 10, 30, 0, 500_000), "TIME", 1).as_deref(),
            Some("10:30:00.5")
        );
    }

    /// **A hoisted kind has to answer what the per-cell call answered.**
    ///
    /// `convert_row` called `parse_typed` per cell, which is
    /// `parse_as(num_kind(type_name), s)` — and `num_kind` opens by
    /// uppercasing the type name, then walks up to eight `starts_with` scans
    /// and a `contains`, for a property of the *column*. Both docs already
    /// said so ("Called once per column"; "What a row loop should call is
    /// `parse_as` with a kind it computed once"), and the two sibling
    /// classifications beside it in the row loop were hoisted on exactly that
    /// reasoning.
    ///
    /// The answer must not move, so this is the equivalence: over every type
    /// spelling the mapping distinguishes, `parse_as(num_kind(t), s)` is
    /// `parse_typed(s, t)`.
    #[test]
    fn a_kind_computed_once_parses_a_cell_the_way_the_per_cell_call_did() {
        let types = [
            "TINYINT",
            "SMALLINT",
            "MEDIUMINT",
            "INT",
            "BIGINT",
            "YEAR",
            "INT UNSIGNED",
            "BIGINT UNSIGNED",
            "tinyint unsigned",
            "FLOAT",
            "DOUBLE",
            "DECIMAL(10,2)",
            "VARCHAR(255)",
            "TEXT",
            "DATETIME",
            "",
        ];
        let cells = ["42", "-1", "0", "3.5", "18446744073709551615", "abc", ""];
        for t in types {
            let kind = num_kind(t);
            for c in cells {
                assert_eq!(
                    parse_as(kind, c.to_string()),
                    parse_typed(c.to_string(), t),
                    "{t:?} / {c:?}"
                );
            }
        }
    }

    /// A column index past the end falls back to `Text`, which is what a
    /// value with no column to describe it has to be — the row loop indexes
    /// `kinds` the same way it indexes `binary` and `bit`, both of which take
    /// the same defensive default.
    #[test]
    fn a_missing_kind_is_text() {
        let kinds: Vec<NumKind> = vec![NumKind::Int];
        assert_eq!(
            kinds.get(9).copied().unwrap_or(NumKind::Text),
            NumKind::Text
        );
        assert!(matches!(
            parse_as(NumKind::Text, "42".to_string()),
            Value::Str(_)
        ));
    }

    /// **A `ZEROFILL` column's padding is the server's own rendering, and it is
    /// the value the user sees in `mysql` and in DataGrip.** `INT(4) UNSIGNED
    /// ZEROFILL` holding 7 arrives over the text protocol as `0007`
    /// (**measured** on MariaDB 10.11.14 and MySQL 8.4 — both send the padded
    /// bytes and set `ZEROFILL_FLAG` with the display width in
    /// `column_length`), and parsing it as a number threw the padding away in
    /// the grid and in every export, on the path whose doc promises the cell
    /// keeps its exact text.
    ///
    /// `Text` costs nothing here that matters: `Column::is_numeric` reads the
    /// **leading** type token, so the column still right-aligns, and a
    /// fixed-width zero-padded unsigned sorts identically as text.
    #[test]
    fn a_zerofill_column_keeps_the_padding_the_server_sent() {
        assert_eq!(num_kind("INT(4) UNSIGNED ZEROFILL"), NumKind::Text);
        assert_eq!(num_kind("BIGINT(8) UNSIGNED ZEROFILL"), NumKind::Text);
        assert!(matches!(
            parse_typed("0007".into(), "INT(4) UNSIGNED ZEROFILL"),
            Value::Str(s) if s == "0007"
        ));
        // Without the attribute nothing changes: an ordinary unsigned integer is
        // still a number, which is what the hoisted kind array is mostly for.
        assert_eq!(num_kind("INT UNSIGNED"), NumKind::UInt);
        assert_eq!(num_kind("INT"), NumKind::Int);
    }

    /// The wire flag has to reach the type name, or `num_kind` above cannot see
    /// it — the type name is the only thing it is given.
    #[test]
    fn resolve_type_name_carries_the_zerofill_attribute() {
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_LONG, true, false, true),
            "INT UNSIGNED ZEROFILL"
        );
        // ZEROFILL implies UNSIGNED on the server, but the flags are
        // independent on the wire; the name is built from what arrived.
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_LONGLONG, false, false, true),
            "BIGINT ZEROFILL"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_LONG, true, false, false),
            "INT UNSIGNED"
        );
        // Non-numeric types never carry it, the way they never carry UNSIGNED.
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_VAR_STRING, true, false, true),
            "VARCHAR"
        );
    }

    /// **The other protocol, which sends no padding at all.** The post-commit
    /// re-fetch runs a *prepared* statement, and the binary protocol carries an
    /// integer: `Int(7)` for the same cell the text protocol sent as `0007`
    /// (measured on both engines). Left alone, the splice would paint `7` over
    /// the `0007` the load put there and the grid would disagree with itself
    /// about a cell nobody edited — so the re-fetch re-renders it the way the
    /// server would have, from the display width the wire also carries.
    #[test]
    fn a_zerofill_cell_from_the_binary_protocol_is_padded_back() {
        assert!(matches!(
            zerofill_value(Value::Int(7), Some(4), 0),
            Value::Str(s) if s == "0007"
        ));
        assert!(matches!(
            zerofill_value(Value::UInt(42), Some(8), 0),
            Value::Str(s) if s == "00000042"
        ));
        // A value already at or over the width is untouched by the padding.
        assert!(matches!(
            zerofill_value(Value::UInt(12345), Some(4), 0),
            Value::Str(s) if s == "12345"
        ));
        // No width means no ZEROFILL: every other column passes through whole.
        assert!(matches!(
            zerofill_value(Value::Int(7), None, 0),
            Value::Int(7)
        ));
        assert!(matches!(
            zerofill_value(Value::Str("0007".into()), Some(4), 0),
            Value::Str(s) if s == "0007"
        ));
        assert!(matches!(
            zerofill_value(Value::Null, Some(4), 0),
            Value::Null
        ));
    }

    /// **A float is a `ZEROFILL` column too**, and the first spelling of this
    /// covered the integer arms alone — so `zerofill_widths`, which answers
    /// `Some` for every numeric column carrying the flag, computed a width that
    /// two of the three arms that could use it threw away. The splice then
    /// painted `123.45` over the `0000123.45` the text load had put in the grid,
    /// in a cell the user never touched, and an export taken before the next
    /// full re-run wrote the unpadded text out.
    ///
    /// Measured on MariaDB 10.11.14 **and** MySQL 8.4.11, where the `(M,D)` form
    /// is deprecated and still accepted: `DOUBLE(10,2) UNSIGNED ZEROFILL`
    /// holding `123.45` reads back `0000123.45`, and `FLOAT(8,2) UNSIGNED
    /// ZEROFILL` holding `12.5` reads `00012.50`.
    #[test]
    fn a_zerofill_float_is_padded_to_its_declared_scale() {
        assert!(matches!(
            zerofill_value(Value::Float(123.45), Some(10), 2),
            Value::Str(s) if s == "0000123.45"
        ));
        // The scale is part of the rendering: a trailing zero the server prints
        // is not noise, it is the column's declared precision.
        assert!(matches!(
            zerofill_value(Value::Float(12.5), Some(8), 2),
            Value::Str(s) if s == "00012.50"
        ));
        // **`NOT_FIXED_DEC` is not a scale.** A float declared without `(M,D)`
        // reports `decimals = 31`; formatting to thirty-one places would invent
        // digits the server never sent, so the shortest round-tripping form is
        // padded instead.
        assert!(matches!(
            zerofill_value(Value::Float(1.5), Some(6), 31),
            Value::Str(s) if s == "0001.5"
        ));
        // And no width is still no ZEROFILL.
        assert!(matches!(
            zerofill_value(Value::Float(1.5), None, 2),
            Value::Float(f) if f == 1.5
        ));
    }

    #[test]
    fn parse_typed_integers_unsigned_floats_and_fallback() {
        // Signed integer types.
        assert!(matches!(parse_typed("42".into(), "INT"), Value::Int(42)));
        assert!(matches!(parse_typed("-1".into(), "BIGINT"), Value::Int(-1)));
        assert!(matches!(
            parse_typed("2024".into(), "YEAR"),
            Value::Int(2024)
        ));
        // Unsigned.
        assert!(matches!(
            parse_typed("42".into(), "INT UNSIGNED"),
            Value::UInt(42)
        ));
        // A negative into an UNSIGNED column can't parse → lossless string fallback.
        assert!(matches!(
            parse_typed("-1".into(), "INT UNSIGNED"),
            Value::Str(s) if s == "-1"
        ));
        // Floats.
        assert!(matches!(parse_typed("1.5".into(), "DOUBLE"), Value::Float(f) if f == 1.5));
        assert!(matches!(parse_typed("3.0".into(), "FLOAT"), Value::Float(f) if f == 3.0));
        // DECIMAL stays an exact string (never a lossy float).
        assert!(matches!(
            parse_typed("1.10".into(), "DECIMAL(10,2)"),
            Value::Str(s) if s == "1.10"
        ));
        // Non-numeric type → string.
        assert!(matches!(
            parse_typed("hi".into(), "VARCHAR(20)"),
            Value::Str(s) if s == "hi"
        ));
        // Unparseable integer → string fallback, never a panic.
        assert!(matches!(
            parse_typed("NaN".into(), "INT"),
            Value::Str(s) if s == "NaN"
        ));
    }

    #[test]
    fn build_refetch_sql_single_key() {
        let t = RefetchTemplate {
            database: "db".to_string(),
            schema: None,
            table: "users".to_string(),
            columns: vec!["id".to_string(), "name".to_string()],
            key_cols: vec![0],
            confirm_cols: Vec::new(),
        };
        assert_eq!(
            build_refetch_sql(&t),
            "SELECT `id`, `name` FROM `db`.`users` WHERE `id` <=> ? LIMIT 1"
        );
    }

    #[test]
    fn build_refetch_sql_composite_key_joins_with_and() {
        let t = RefetchTemplate {
            database: "db".to_string(),
            schema: None,
            table: "t".to_string(),
            columns: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            key_cols: vec![0, 2],
            confirm_cols: Vec::new(),
        };
        assert_eq!(
            build_refetch_sql(&t),
            "SELECT `a`, `b`, `c` FROM `db`.`t` WHERE `a` <=> ? AND `c` <=> ? LIMIT 1"
        );
    }

    /// **The confirming half of the `WHERE`, which no test reached.**
    /// `confirm_cols` is populated only by SQLite's implicit-rowid key
    /// (`sqlite.rs`'s `implicit_key`), so on this builder it is empty in every
    /// round trip the suite runs and the `.chain(confirm_cols)` above could be
    /// deleted with the whole suite green. The chain is still this builder's
    /// contract — the placeholders it writes are bound in
    /// `edit::refetch_key`'s order, key first — so it is asserted here
    /// directly, with a template that has both halves.
    #[test]
    fn build_refetch_sql_confirms_with_the_columns_after_the_key() {
        let t = RefetchTemplate {
            database: "db".to_string(),
            schema: None,
            table: "t".to_string(),
            columns: vec!["rowid".to_string(), "a".to_string(), "b".to_string()],
            key_cols: vec![0],
            confirm_cols: vec![1, 2],
        };
        assert_eq!(
            build_refetch_sql(&t),
            "SELECT `rowid`, `a`, `b` FROM `db`.`t` \
             WHERE `rowid` <=> ? AND `a` <=> ? AND `b` <=> ? LIMIT 1"
        );
    }

    #[test]
    fn build_refetch_sql_escapes_identifiers() {
        let t = RefetchTemplate {
            database: "d`b".to_string(),
            schema: None,
            table: "t`t".to_string(),
            columns: vec!["a`b".to_string()],
            key_cols: vec![0],
            confirm_cols: Vec::new(),
        };
        assert_eq!(
            build_refetch_sql(&t),
            "SELECT `a``b` FROM `d``b`.`t``t` WHERE `a``b` <=> ? LIMIT 1"
        );
    }

    fn blob_ref(key: &[(&str, Value)]) -> BlobRef {
        BlobRef {
            database: "db".to_string(),
            schema: None,
            table: "staff".to_string(),
            column: "picture".to_string(),
            key: key
                .iter()
                .map(|(c, v)| (c.to_string(), v.clone()))
                .collect(),
        }
    }

    /// **The cap binds before the key.** The `SUBSTRING` placeholder sits in the
    /// select list and every key placeholder in the `WHERE` after it, so the
    /// parameter vector has to be built in that order — reversed, MySQL reads
    /// the row's id as a byte count and the key as a length, and the statement
    /// still runs.
    #[test]
    fn build_blob_select_binds_the_cap_first_then_the_key() {
        let (sql, params) = build_blob_select(&blob_ref(&[("staff_id", Value::UInt(1))]));
        assert_eq!(
            sql,
            "SELECT OCTET_LENGTH(`picture`), SUBSTRING(`picture`, 1, \
             LEAST(?, GREATEST(1024, CAST(@@max_allowed_packet AS SIGNED) - 1048576))) \
             FROM `db`.`staff` WHERE `staff_id` <=> ? LIMIT 1"
        );
        let Params::Positional(p) = params else {
            panic!("positional params expected");
        };
        assert_eq!(p.len(), 2);
        assert_eq!(p[0], MyValue::UInt(FETCH_CAP as u64));
        assert_eq!(p[1], MyValue::UInt(1));
    }

    /// A composite key joins with `AND`, in `row_key` order — the same WHERE
    /// `build_update` builds, because it is the same row identity.
    #[test]
    fn build_blob_select_joins_a_composite_key_with_and() {
        let (sql, params) = build_blob_select(&blob_ref(&[
            ("a", Value::Int(1)),
            ("b", Value::Str("x".to_string())),
        ]));
        assert!(
            sql.ends_with("WHERE `a` <=> ? AND `b` <=> ? LIMIT 1"),
            "{sql}"
        );
        let Params::Positional(p) = params else {
            panic!("positional params expected");
        };
        assert_eq!(p.len(), 3, "cap + two key values");
    }

    /// A NULL key value still compares, because the WHERE is NULL-safe — a
    /// plain `= NULL` would silently match no row and report the cell empty.
    #[test]
    fn build_blob_select_keeps_the_null_safe_comparison() {
        let (sql, _) = build_blob_select(&blob_ref(&[("k", Value::Null)]));
        assert!(sql.contains("`k` <=> ?"), "{sql}");
    }

    #[test]
    fn build_blob_select_escapes_every_identifier() {
        let r = BlobRef {
            database: "d`b".to_string(),
            schema: None,
            table: "t`t".to_string(),
            column: "c`c".to_string(),
            key: vec![("k`k".to_string(), Value::Int(1))],
        };
        let (sql, _) = build_blob_select(&r);
        assert_eq!(
            sql,
            "SELECT OCTET_LENGTH(`c``c`), SUBSTRING(`c``c`, 1, \
             LEAST(?, GREATEST(1024, CAST(@@max_allowed_packet AS SIGNED) - 1048576))) \
             FROM `d``b`.`t``t` WHERE `k``k` <=> ? LIMIT 1"
        );
    }

    /// **The server's limit is the other cap, and the smaller of the two wins.**
    /// `FETCH_CAP` stays bound as a parameter — the statement must not carry a
    /// second literal — while `max_allowed_packet` is read live inside the
    /// statement, because it is the server's answer and it can change under a
    /// long-lived connection.
    ///
    /// Asking for the full 64 MiB unconditionally is not a polite failure on
    /// MySQL: measured against MariaDB 10.11 (`max_allowed_packet` = 16 MiB) a
    /// 20 MiB `LONGBLOB` dropped the connection mid-row. The cap turns that into
    /// a truncated read the panel already describes.
    #[test]
    fn build_blob_select_bounds_the_read_by_the_servers_packet_limit() {
        let (sql, params) = build_blob_select(&blob_ref(&[("id", Value::Int(1))]));
        assert!(
            sql.contains(
                "LEAST(?, GREATEST(1024, CAST(@@max_allowed_packet AS SIGNED) - 1048576))"
            ),
            "the two caps must both be in the length, smaller winning: {sql}"
        );
        assert!(
            !sql.contains(&FETCH_CAP.to_string()),
            "our cap is bound, not written into the statement: {sql}"
        );
        let Params::Positional(p) = params else {
            panic!("positional params expected");
        };
        assert_eq!(
            p[0],
            MyValue::UInt(FETCH_CAP as u64),
            "and it is still the first parameter, ahead of the key"
        );
    }

    /// This crate's three identifier quoters answer to `core`'s, so the SQL a
    /// write path builds can't drift from the SQL the export and DDL paths
    /// **A cancelled write reports `Cancelled` only when the rollback really
    /// undid it**, which is the same rule `cancelled_import` states and the
    /// grid's commit did not follow.
    #[test]
    fn a_cancelled_write_says_so_only_when_the_rollback_was_complete() {
        assert!(matches!(
            cancelled_write(Rollback::Complete),
            DbError::Cancelled
        ));
        match cancelled_write(Rollback::Incomplete) {
            DbError::Query(msg) => {
                assert!(msg.starts_with("Commit cancelled"), "{msg}");
                assert!(
                    msg.contains("remain"),
                    "the note that says the rows may still be there is missing: {msg}"
                );
            }
            other => panic!("an incomplete undo reported as {other:?}"),
        }
        // And it names the right act: "Import cancelled" over a Commit is the
        // sentence `cancelled_import` would have given.
        assert!(
            !format!("{:?}", cancelled_write(Rollback::Incomplete)).contains("Import"),
            "a cancelled commit reported as a cancelled import"
        );
    }

    /// **And the composition, which the test above cannot reach.**
    ///
    /// `DbError::Cancelled` is what the modal renders as "nothing was written",
    /// so the whole question is whether the cancel arm *asks*. It did not: it
    /// killed the query, disconnected, and returned `Cancelled` on the strength
    /// of the connection drop undoing the transaction — true on InnoDB and
    /// false on `MyISAM`/`MEMORY`/`ARCHIVE`/`CSV`, where a cancelled commit of
    /// three staged `INSERT`s that got two in reported "nothing was written",
    /// left all three staged, and a second Commit landed the two again.
    ///
    /// A source gate for `import_on`'s reason one function up: every arm of
    /// `commit_writes` needs a live MySQL connection to reach, and a unit test
    /// of `cancelled_write` alone is green against the defect — the seam is
    /// between the predicate and its caller, which is the shape CLAUDE.md's
    /// testing section names.
    ///
    /// **And the half that gate could not see: *where* the cancel happens.**
    /// Asking `rollback` is worth nothing if the connection cannot hear the
    /// answer, and a `tokio::select!` around the whole write is exactly that —
    /// the cancel arm runs only after the branch futures are dropped, which is
    /// why its `&mut conn` re-borrow compiles at all, so the killed statement is
    /// dropped rather than awaited and the stream is torn. `import_rows` removed
    /// that construct three hundred lines up for this reason and recorded the
    /// measurement; `commit_writes` kept it and then put a `ROLLBACK` on the
    /// torn connection, which is how `Complete` came to be claimed off a reply
    /// that was not its own.
    #[test]
    fn the_grid_commits_cancel_arm_goes_through_a_rollback() {
        // **Two files now, and the split is the whole repair.** `commit_writes`
        // is a dispatcher in `lib.rs`; `write_on` is the body, here. The first
        // half used to read a fixed 2400-byte slice off `commit_writes` — which
        // once covered the write loop and now covers three match arms and the
        // start of the next method, so it would have kept passing while
        // asserting nothing about the code it names. It is measured to the end
        // of the method instead.
        let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
            .expect("the dispatcher's source");
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/mysql.rs"))
            .expect("this module's own source");
        // Assembled, so this test is not its own first match — see
        // `every_import_exit_says_what_the_rollback_achieved`.
        let sig = format!("    pub async fn commit{}writes(", '_');
        let at = lib
            .find(&sig)
            .expect("`commit_writes` is gone or was renamed");
        let rest = &lib[at..];
        let end = rest[1..]
            .find("\n    pub async fn ")
            .map_or(rest.len(), |i| i + 1);
        let body = &rest[..end];
        assert!(
            body.contains("mysql::commit_writes("),
            "the dispatcher no longer reaches this module's `commit_writes`, so \
             the rest of this gate is asking about a path it does not take"
        );
        // The write itself is not raced. Assembled, so this paragraph is not the
        // hit.
        let raced = format!("{}! {{", "tokio::select");
        assert!(
            !body.contains(&raced),
            "`commit_writes` races the write future again: the cancel arm drops \
             it mid-statement, which desynchronises the connection's result \
             stream, and the `ROLLBACK` below then reads somebody else's reply. \
             Hand the token into `write_on`, the way `import_rows` does."
        );
        // …and the cancel still leaves through a rollback whose outcome is asked
        // for, which is now `write_on`'s job.
        // **Measured to the end of the function, not to a fixed 3,000 bytes.**
        // That is the same defect the first half of this gate was repaired for
        // one commit earlier and this half kept: the window already stopped
        // eleven lines short of `write_on`'s end, so a cancel path added below
        // it was invisible, and a `write_on` that grew would have silently
        // narrowed the gate further. `R2.2-L6-06`.
        let wo_sig = format!("pub(crate) async fn write{}on(", '_');
        let wo = src
            .find(&wo_sig)
            .expect("`write_on` is gone or was renamed");
        let rest = &src[wo..];
        let wob = match rest.find("\n}\n") {
            Some(i) => &rest[..i],
            None => rest,
        };
        assert!(
            wob.len() > 3000,
            "the `write_on` window is shorter than the fixed slice it replaced, \
             so the end marker matched something inside the function"
        );
        assert!(
            wob.contains("rollback(") && wob.contains("cancelled_write("),
            "the cancel path returns without asking what the rollback achieved, \
             so it claims 'nothing was written' about a MySQL table that may \
             hold half the batch"
        );
        assert!(
            wob.contains("kill_query(db, conn_id).await") && wob.contains("let _ = fut.await;"),
            "the killed statement is not awaited, so the next statement reads \
             its reply — see `import_on`, which states the rule"
        );
    }

    /// build. Each is engine-fixed by construction — `pg.rs` only ever emits
    /// PostgreSQL, `sqlite.rs`'s statements only ever SQLite, this module's
    /// remaining builders only ever MySQL — which is why they take no dialect and
    /// why the binding has to be asserted rather than typed.
    #[test]
    fn the_write_paths_quote_identifiers_the_way_core_does() {
        use schemaic_core::export::ident_sql;
        use schemaic_core::intel::SqlDialect;
        for name in [
            "plain",
            "MixedCase",
            "with space",
            "a`b",
            "a\"b",
            "both`and\"",
            "sélect",
            "",
        ] {
            assert_eq!(ident(name), ident_sql(name, SqlDialect::MySql), "{name:?}");
            assert_eq!(
                crate::pg::pg_ident_for_test(name),
                ident_sql(name, SqlDialect::Postgres),
                "{name:?}"
            );
            assert_eq!(
                ident_sqlite(name),
                ident_sql(name, SqlDialect::Sqlite),
                "{name:?}"
            );
        }
    }
}

/// Run a whole `.sql` file on **one** connection.
///
/// The one-connection-per-operation invariant's second stated exception, and
/// the reason is session state: a dump's `SET FOREIGN_KEY_CHECKS`, its own
/// `BEGIN`, its `DELIMITER` all have to outlive the statement that set them.
pub(crate) async fn run_script(
    db: &Db,
    database: &str,
    mut rx: tokio::sync::mpsc::Receiver<schemaic_core::script::Statement>,
    cancel: CancellationToken,
) -> (schemaic_core::script::ExecEnd, usize) {
    use schemaic_core::script::ExecEnd;
    let mut conn = match db.open(Some(database), false).await {
        Ok(c) => c,
        Err(e) => return (ExecEnd::Connect(e.to_string()), 0),
    };
    let conn_id = conn.id();
    // **No lock bound is set here.** See `pg::run_script` for the whole
    // reasoning: `DDL_LOCK_WAIT_SECS` is documented for the Apply modal's
    // short reviewed plan, and a restore that dies at statement N with N−1
    // applied and no transaction of ours to roll back is worse than
    // waiting. `mysql <` sets nothing either, and Stop here kills the
    // running statement server-side.
    let mut ran = 0usize;
    let end = loop {
        // Cancel has to be reachable **while waiting for the next
        // statement**, not only while one is running. A load stalled on a
        // slow disk spends most of its life here, and a Stop that only
        // landed between statements would look ignored.
        let next = tokio::select! {
            s = rx.recv() => s,
            _ = cancel.cancelled() => break ExecEnd::Cancelled,
        };
        let Some(st) = next else { break ExecEnd::Done };
        // **The killed statement is awaited, not dropped.** `KILL QUERY` is
        // a request — MySQL documents that it may be ignored during an
        // online `ALTER`'s commit phase — so throwing the future away left
        // `ran` a floor while the panel presents it as the count. Scoped so
        // the borrow of `st.sql` ends before the `Failed` arm moves it.
        let step = {
            let mut fut = std::pin::pin!(conn.query_drop(&st.sql));
            let raced = tokio::select! {
                r = fut.as_mut() => Some(r),
                _ = cancel.cancelled() => None,
            };
            match raced {
                Some(Ok(())) => pg::ScriptStep::Ran,
                Some(Err(e)) => pg::ScriptStep::Failed(e.to_string()),
                None => {
                    kill_query(db, conn_id).await;
                    pg::ScriptStep::Cancelled {
                        ran: fut.await.is_ok(),
                    }
                }
            }
        };
        match step {
            pg::ScriptStep::Ran => ran += 1,
            pg::ScriptStep::Cancelled { ran: landed } => {
                if landed {
                    ran += 1;
                }
                break ExecEnd::Cancelled;
            }
            pg::ScriptStep::Failed(message) => {
                break ExecEnd::Failed {
                    message,
                    sql: st.sql,
                    line: st.line,
                };
            }
        }
    };
    let _ = conn.disconnect().await;
    (end, ran)
}

/// Apply a staged batch of grid mutations in one transaction.
///
/// **Named for the entry point, not for the body it calls.** `write_on` does the
/// work and `session.rs` calls that directly for a pinned manual-transaction
/// connection; this is the fresh-connection door, and it exists under this name
/// because `ENGINE_ENTRY_POINTS` is the interface and
/// `every_engine_module_answers_the_whole_interface` is what holds a module to
/// it. Both convention tests found all three of these missing the moment MySQL
/// joined the list they check.
pub(crate) async fn commit_writes(
    db: &Db,
    write: &GridWrite,
    cancel: CancellationToken,
) -> Result<u64, DbError> {
    // `client_found_rows` so the 1-row guard counts matches, not changes.
    let mut conn = db.open(None, true).await?;
    let conn_id = conn.id();

    // **The cancel is inside `write_on`, not a race around it** — the same
    // rule `import_rows` states three hundred lines up, for the same
    // measured reason. A `tokio::select!` over the whole write dropped that
    // future *mid-statement* (the `&mut conn` re-borrow in the cancel arm
    // only compiles because the branch futures are dropped first), which
    // leaves a `mysql_async` connection's result stream desynchronised:
    // measured there on MariaDB 10.11.14 and MySQL 8.4.11, after the drop
    // the following `ROLLBACK` "succeeded" and `SHOW WARNINGS` was empty.
    // So the `ROLLBACK` this method's doc is about classified `Complete`
    // off a reply that was not its own, and a cancelled Commit of three
    // staged `INSERT`s into a `MyISAM` table reported "nothing was written"
    // over two rows that were permanently there — the exact failure the
    // `ROLLBACK` was added to prevent, one exit further along.
    let outcome = write_on(&mut conn, write, TxScope::Own, Some((db, conn_id, &cancel))).await;

    let _ = conn.disconnect().await;
    outcome
}

/// Re-read the rows a write just touched, so the grid shows what landed.
pub(crate) async fn refetch_rows(
    db: &Db,
    template: &RefetchTemplate,
    rows: &[RefetchRow],
    cancel: CancellationToken,
) -> Result<Vec<(usize, Vec<Value>)>, DbError> {
    let mut conn = db.open(None, false).await?;
    let conn_id = conn.id();
    let outcome = tokio::select! {
        r = refetch_on(&mut conn, template, rows) => r,
        _ = cancel.cancelled() => {
            kill_query(db, conn_id).await;
            Err(DbError::Cancelled)
        }
    };
    let _ = conn.disconnect().await;
    outcome
}

/// Read one binary cell's bytes, bounded by what a packet holds.
pub(crate) async fn fetch_blob(
    db: &Db,
    r: &BlobRef,
    cancel: CancellationToken,
) -> Result<Option<BlobValue>, DbError> {
    // `None`, not the target database: `build_blob_select` qualifies the
    // table as `db`.`table` itself, so the session default is never
    // consulted and a `USE` would be a round trip that decides nothing —
    // the same shape `refetch_rows` below already has.
    let mut conn = db.open(None, false).await?;
    let conn_id = conn.id();
    let outcome = tokio::select! {
        res = blob_on(&mut conn, r) => res,
        _ = cancel.cancelled() => {
            kill_query(db, conn_id).await;
            Err(DbError::Cancelled)
        }
    };
    let _ = conn.disconnect().await;
    outcome
}
