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
//! `the_dispatcher_calls_both_engine_modules_for_every_entry_point` — could only
//! ever check two engines out of three for the same reason.
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

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Row};
use schemaic_core::activity::{self, KillKind, SessionInfo};
use schemaic_core::intel::SqlDialect;
use schemaic_core::schema::{
    CheckInfo, ColumnInfo, DbSchema, EventInfo, EventSchedule, EventSource, EventStatus,
    RoutineInfo, TableInfo, TriggerAction, TriggerEvent, TriggerInfo, TriggerOrder, TriggerSource,
    TriggerTiming, ViewOptions, event_interval_expr, event_time_expr,
};
use schemaic_core::stats::{Freshness, IndexStats, SchemaStats, TableStats};
use schemaic_core::users::{self, Grants, MyUserRow, Principal};
use schemaic_core::{export, sql};
use tokio_util::sync::CancellationToken;

use crate::{ColRow, Db, DbError, FkColRow, IdxRow, assemble_schema, ident};

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
/// does not exist there.
const MY_USERS_MYSQL_SQL: &str = "SELECT CAST(User AS CHAR), CAST(Host AS CHAR), \
            CAST(plugin AS CHAR), CAST(password_expired AS CHAR), CAST(account_locked AS CHAR) \
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

/// One `mysql.user` row as the two wide queries project it.
type MyUserTuple = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// The MySQL/MariaDB half of [`Db::fetch_principals`]: four queries, of which
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
            &my_user_rows(rows, true),
        )));
    }
    if let Ok(rows) = conn.query_map(MY_USERS_MYSQL_SQL, |r: MyUserTuple| r).await {
        return Ok(users::Principals::complete(users::from_mysql_rows(
            &my_user_rows(rows, false),
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

/// Slot the fifth column into whichever field this server's spelling meant it
/// for. The fold that reads it — `users::from_mysql_rows` — is where every
/// decision about what a [`Principal`] *says* lives, and it needs the two
/// columns kept apart rather than merged into a "flag" it would have to
/// re-interpret.
fn my_user_rows(rows: Vec<MyUserTuple>, mariadb: bool) -> Vec<MyUserRow> {
    rows.into_iter()
        .map(|(user, host, plugin, expired, fifth)| MyUserRow {
            user,
            host,
            plugin,
            password_expired: expired,
            is_role: if mariadb { fifth.clone() } else { None },
            account_locked: if mariadb { None } else { fifth },
        })
        .collect()
}

/// [`MY_USERS_ROLE_SQL`]'s three columns, as rows.
///
/// A named function rather than a closure inside the ladder, for the reason
/// [`my_user_rows`] is one: the mapping is the whole content of a rung, and a
/// rung whose mapping is inline is a rung no test can reach.
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
    use super::{MyUserRow, my_role_rows, my_user_rows};

    /// **Which column the fifth one is.** Two bare boolean literals twelve lines
    /// apart decide it, and nothing in any tier asserted the result: transposing
    /// them makes every locked MySQL account a `Role`, drops its host, and
    /// `DROP USER "app"` then resolves to a *different* account. The live role
    /// test finds its role by name and never asks what kind it is.
    #[test]
    fn the_fifth_column_lands_in_the_field_this_servers_spelling_meant() {
        let row = |fifth: &str| {
            vec![(
                "app".to_string(),
                "%".to_string(),
                Some("plugin".to_string()),
                Some("N".to_string()),
                Some(fifth.to_string()),
            )]
        };
        // MariaDB's fifth column is `is_role`…
        let maria = my_user_rows(row("Y"), true);
        assert_eq!(maria[0].is_role.as_deref(), Some("Y"));
        assert_eq!(maria[0].account_locked, None);
        // …and MySQL 8's is `account_locked`, which does not make a role.
        let mysql = my_user_rows(row("Y"), false);
        assert_eq!(mysql[0].is_role, None);
        assert_eq!(mysql[0].account_locked.as_deref(), Some("Y"));
        // The other four columns are the same either way.
        assert_eq!(maria[0].user, mysql[0].user);
        assert_eq!(maria[0].host, mysql[0].host);
        assert_eq!(maria[0].plugin, mysql[0].plugin);
        assert_eq!(maria[0].password_expired, mysql[0].password_expired);
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
            db.kill_query(conn_id).await;
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
    for kw in ["FOLLOWS", "PRECEDES"] {
        if rest.len() >= kw.len() && rest.as_bytes()[..kw.len()].eq_ignore_ascii_case(kw.as_bytes())
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
    for t in schema.tables.iter_mut() {
        t.check_constraints = rows
            .iter()
            .filter(|(table, ..)| *table == t.name)
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
    for t in schema.tables.iter_mut() {
        t.triggers = triggers
            .iter()
            .filter(|g| g.table == t.name)
            .cloned()
            .collect();
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
    for (table, name, on_delete, on_update) in rows {
        let Some(t) = schema.tables.iter_mut().find(|t| t.name == *table) else {
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
/// MariaDB returns a faithful `ACTION_STATEMENT` already and never reaches here
/// — the dispatcher's engine check is not the whole gate, so see
/// [`crate::Db::trigger_source`] for which servers ask.
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
        // **`src/lib.rs` is the right file to read even as the MySQL bodies
        // leave it**, because the deadline is not what moves: each of these
        // wraps the whole dispatch — the `mysql::` arm and the `pg::` arm alike
        // — so the bound is a property of the public method rather than of one
        // engine's tail. `fetch_sessions` and `kill_session` have already had
        // their MySQL halves lifted into `mysql.rs` and this still reads the
        // timeout, which is the point. A step that moves a `tokio::time::timeout`
        // into an engine module has bounded one engine and left the others
        // open, and would have to answer here first.
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
            .expect("this module's own source");
        // The method body as text: from its signature to the next one at the
        // same indent. Enough to see whether a `timeout` wraps it.
        let body_of = |name: &str| -> String {
            let at = src
                .find(&format!("    pub async fn {name}("))
                .unwrap_or_else(|| panic!("{name} is gone or was renamed"));
            let rest = &src[at..];
            let end = rest[1..]
                .find("\n    pub async fn ")
                .map_or(rest.len(), |i| i + 1);
            rest[..end].to_string()
        };
        for (name, deadline) in [
            // `ping` takes its deadline as a parameter — the callers pass
            // `PING_TIMEOUT` — so what is checked here is that it applies the
            // one it was given.
            ("ping", "timeout"),
            ("fetch_databases", "PING_TIMEOUT"),
            ("fetch_sessions", "PING_TIMEOUT"),
            ("kill_session", "CANCEL_TIMEOUT"),
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
            ("fetch_table", "PING_TIMEOUT"),
        ] {
            let body = body_of(name);
            assert!(
                body.contains(&format!("tokio::time::timeout({deadline}")),
                "`{name}` opens a connection for someone who is waiting and must \
                 bound it with {deadline} — a dark host otherwise costs the OS \
                 connect timeout, and this one repeats"
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
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
            .expect("this module's own source");
        let at = src
            .find("async fn import_on(")
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
