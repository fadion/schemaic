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

use mysql_async::Conn;
use mysql_async::prelude::Queryable;
use schemaic_core::activity::{self, KillKind, SessionInfo};
use schemaic_core::intel::SqlDialect;
use schemaic_core::stats::{Freshness, IndexStats, SchemaStats, TableStats};
use schemaic_core::users::{self, Grants, MyUserRow, Principal};

use crate::{Db, DbError};

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
