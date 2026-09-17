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

use mysql_async::Conn;
use mysql_async::prelude::Queryable;
use schemaic_core::intel::SqlDialect;
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
