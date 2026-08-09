//! Database access for Schemaic.
//!
//! Connect, run a statement over the MySQL **text protocol**, and stop at a row
//! cap (ARCHITECTURE §8). The query runs on a **dedicated connection** whose id
//! we capture up front, so it can be cancelled server-side with `KILL QUERY`
//! from a second connection (ARCHITECTURE §7).
//!
//! Built on [`mysql_async`] (not sqlx): we need the per-column wire metadata —
//! `org_table` / `org_name` / key flags — that the MySQL protocol sends in every
//! column-definition packet, which is the foundation of the editing system.
//! sqlx's MySQL driver parses that packet but keeps only the alias name + type,
//! so it can't tell which real table/column a result cell came from.

pub mod pg;
pub mod session;
pub mod ssh;

pub use session::{Outcome, Session};

use std::collections::HashMap;

use futures_util::StreamExt;
use mysql_async::consts::{ColumnFlags, ColumnType};
use mysql_async::prelude::Queryable;
use mysql_async::{Column as MyColumn, Conn, Row, Value as MyValue};
use mysql_async::{OptsBuilder, Params};
use schemaic_core::model::{
    Column, ColumnFlags as CoreColFlags, ColumnOrigin, GridWrite, RefetchRow, RefetchTemplate,
    ResultBuilder, ResultSet, RowDelete, RowEdit, RowInsert, Value,
};
use schemaic_core::schema::{ColumnInfo, DbSchema, ForeignKeyInfo, IndexInfo, TableInfo};
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("connection failed: {0}")]
    Connect(String),
    #[error("query failed: {0}")]
    Query(String),
    #[error("query cancelled")]
    Cancelled,
}

/// The binary collation id (`binary`) — a column with this charset holds raw
/// bytes (BLOB/BINARY/VARBINARY) rather than text.
const BINARY_CHARSET: u16 = 63;

/// Which database engine a [`Db`] speaks. Selected from the saved connection's
/// `db_type` at [`Db::connect`] time; each public method dispatches to the
/// engine-specific backend (MySQL bodies inline here, Postgres in [`pg`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Engine {
    #[default]
    MySql,
    Postgres,
}

impl Engine {
    /// The SQL dialect this engine speaks — the quoting and escaping rules any
    /// generated statement has to follow.
    pub fn dialect(self) -> schemaic_core::intel::SqlDialect {
        match self {
            Engine::MySql => schemaic_core::intel::SqlDialect::MySql,
            Engine::Postgres => schemaic_core::intel::SqlDialect::Postgres,
        }
    }

    /// A stable lowercase tag for this engine — used to serialize the engine into
    /// the MCP endpoint JSON (round-trips through [`Engine::from_db_type`]).
    pub fn as_str(self) -> &'static str {
        match self {
            Engine::MySql => "mysql",
            Engine::Postgres => "postgres",
        }
    }

    /// Map a saved connection's `db_type` label to an engine. Anything that isn't
    /// recognizably Postgres falls back to MySQL (the historical default), so old
    /// saved connections and the "MySQL"/"MariaDB" labels keep working.
    pub fn from_db_type(db_type: &str) -> Engine {
        let t = db_type.trim();
        if t.eq_ignore_ascii_case("postgresql")
            || t.eq_ignore_ascii_case("postgres")
            || t.eq_ignore_ascii_case("pg")
        {
            Engine::Postgres
        } else {
            Engine::MySql
        }
    }
}

/// A resolved connection target — server coordinates + credentials, already
/// pointed through any established SSH tunnel. Built once from a saved
/// [`Connection`]; every operation derives a fresh `mysql_async` connection from
/// it.
///
/// This is the app's single connection *identity* (review §3.1): the app threads
/// a `Db` (or a connection id resolving to one), never a `mysql://user:pass@…`
/// URL string. Credentials go to the driver through `OptsBuilder`, not a URL, so
/// a password containing `@ / # ? % :` needs no percent-encoding and can't break
/// parsing (review B7), and no plaintext URL is embedded anywhere as identity or
/// leaked on a command line (review C6).
#[derive(Clone, Debug)]
pub struct Db {
    pub(crate) engine: Engine,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) user: String,
    pub(crate) pass: String,
}

impl Db {
    /// Resolve a saved connection into a `Db`. For an SSH connection, pass the
    /// established tunnel's local port and the target is rewritten to
    /// `127.0.0.1:<port>`. Infallible — no URL is parsed. The engine is derived
    /// from the connection's `db_type` (MySQL/MariaDB vs PostgreSQL).
    pub fn connect(conn: &schemaic_core::connection::Connection, tunnel_port: Option<u16>) -> Db {
        let engine = Engine::from_db_type(&conn.db_type);
        match tunnel_port {
            Some(port) => Db {
                engine,
                host: "127.0.0.1".to_string(),
                port,
                user: conn.user.clone(),
                pass: conn.password.clone(),
            },
            None => Db {
                engine,
                host: conn.host.clone(),
                port: conn.port,
                user: conn.user.clone(),
                pass: conn.password.clone(),
            },
        }
    }

    /// Reconstruct from raw parts + engine — used by the MCP subprocess, which
    /// receives the (already-tunnelled) endpoint (incl. engine) over its
    /// environment, so AI queries run against the right driver.
    pub fn from_parts(engine: Engine, host: String, port: u16, user: String, pass: String) -> Db {
        Db {
            engine,
            host,
            port,
            user,
            pass,
        }
    }

    /// Borrow the endpoint parts `(host, port, user, pass)` — used to serialize
    /// the endpoint for the MCP subprocess handoff.
    pub fn parts(&self) -> (&str, u16, &str, &str) {
        (&self.host, self.port, &self.user, &self.pass)
    }

    /// The engine this handle speaks.
    pub fn engine(&self) -> Engine {
        self.engine
    }

    /// Build connection options for a fresh connection, optionally with a default
    /// database (`USE`d on connect so unqualified names resolve) and
    /// `CLIENT_FOUND_ROWS` (so `affected_rows()` counts *matched* rows, not
    /// *changed* ones — the commit path's exactly-one-row guard relies on it).
    fn opts(&self, database: Option<&str>, found_rows: bool) -> OptsBuilder {
        let mut b = OptsBuilder::default()
            .ip_or_hostname(self.host.clone())
            .tcp_port(self.port)
            .user(Some(self.user.clone()))
            .pass(Some(self.pass.clone()))
            .client_found_rows(found_rows);
        if let Some(db) = database {
            b = b.db_name(Some(db));
        }
        b
    }

    /// Open one connection to this endpoint (optionally scoped to a database).
    pub(crate) async fn open(
        &self,
        database: Option<&str>,
        found_rows: bool,
    ) -> Result<Conn, DbError> {
        Conn::new(self.opts(database, found_rows))
            .await
            .map_err(|e| DbError::Connect(e.to_string()))
    }

    /// Best-effort server-side cancel: connect afresh and `KILL QUERY <id>`.
    pub(crate) async fn kill_query(&self, conn_id: u32) {
        if let Ok(mut killer) = self.open(None, false).await {
            let _ = killer.query_drop(format!("KILL QUERY {conn_id}")).await;
            let _ = killer.disconnect().await;
        }
    }
}

impl Db {
    /// Connect (scoped to `database`), run `sql` (up to `row_cap` rows), and
    /// return the result. If `cancel` fires first, the running query is killed
    /// server-side and `DbError::Cancelled` is returned.
    pub async fn fetch_query(
        &self,
        database: Option<&str>,
        sql: &str,
        row_cap: usize,
        cancel: CancellationToken,
    ) -> Result<ResultSet, DbError> {
        if self.engine == Engine::Postgres {
            return pg::fetch_query(self, database, sql, row_cap, cancel).await;
        }
        let mut conn = self.open(database, false).await?;
        // The connection id, so a second connection can KILL its in-flight query.
        let conn_id = conn.id();

        let outcome = tokio::select! {
            // `early_stop`: this connection is torn down right after, so we can bail
            // out of the row stream at the cap without draining the rest.
            r = collect_rows(&mut conn, sql, row_cap, true) => r,
            _ = cancel.cancelled() => {
                self.kill_query(conn_id).await;
                Err(DbError::Cancelled)
            }
        };

        let _ = conn.disconnect().await;
        outcome
    }

    /// Fetch up to `limit` rows of a single table for the Live Monitor:
    /// `SELECT * FROM `db`.`table` LIMIT n`. Bounded by construction — the monitor
    /// never polls an unbounded table. Column provenance is populated as for any
    /// query, so the caller derives the row-identity key via `analyze_edit`.
    pub async fn fetch_table(
        &self,
        database: &str,
        schema: Option<&str>,
        table: &str,
        limit: usize,
        cancel: CancellationToken,
    ) -> Result<ResultSet, DbError> {
        if self.engine == Engine::Postgres {
            return pg::fetch_table(self, database, schema, table, limit, cancel).await;
        }
        // MySQL has no namespace level — the database already is one.
        debug_assert!(schema.is_none(), "MySQL tables carry no namespace");
        let sql = format!(
            "SELECT * FROM {}.{} LIMIT {}",
            ident(database),
            ident(table),
            limit
        );
        self.fetch_query(Some(database), &sql, limit, cancel).await
    }
}

/// A plan's row count is tiny (classic EXPLAIN) or one big row (tree-format
/// `EXPLAIN ANALYZE`); this cap is only a backstop.
const EXPLAIN_ROW_CAP: usize = 10_000;

impl Db {
    /// Run `EXPLAIN sql` (or `EXPLAIN ANALYZE sql`) and return the plan as a
    /// result set (the caller parses it with `schemaic_core::plan`).
    ///
    /// Plain `EXPLAIN` only *plans* the statement — it never executes it, so it's
    /// safe even for `UPDATE`/`DELETE`. `analyze` is different: it **executes** the
    /// statement to measure it, so callers must gate it to read-only statements.
    ///
    /// MariaDB spells the analyzing form `ANALYZE <stmt>`, not `EXPLAIN ANALYZE`
    /// (which it rejects as a syntax error *before* running anything), so when the
    /// `EXPLAIN ANALYZE` attempt fails we retry with `ANALYZE`. On MySQL the reverse
    /// (`ANALYZE <select>`) is itself a syntax error, so the two servers never both
    /// match — the fallback can't double-execute.
    pub async fn explain(
        &self,
        database: Option<&str>,
        sql: &str,
        analyze: bool,
        cancel: CancellationToken,
    ) -> Result<ResultSet, DbError> {
        if self.engine == Engine::Postgres {
            return pg::explain(self, database, sql, analyze, cancel).await;
        }
        let (primary, fallback) = explain_commands(sql, analyze);
        match self
            .fetch_query(database, &primary, EXPLAIN_ROW_CAP, cancel.clone())
            .await
        {
            // MariaDB: `EXPLAIN ANALYZE` is invalid — retry with `ANALYZE <stmt>`.
            Err(DbError::Query(_)) if fallback.is_some() => {
                self.fetch_query(database, &fallback.unwrap(), EXPLAIN_ROW_CAP, cancel)
                    .await
            }
            other => other,
        }
    }

    /// Validate `sql` against the server **without executing it**: prepare it via
    /// the binary protocol (`PREPARE`), then deallocate. The server checks syntax,
    /// object names, and types but runs nothing — safe even for `UPDATE`/`DELETE`.
    /// Returns the server's error text on failure, `Ok(())` on a clean prepare.
    ///
    /// Statements the prepared-statement protocol doesn't support (server error
    /// 1295 — e.g. some `SHOW`/admin forms) can't be validated this way, so they're
    /// treated as `Ok` rather than surfacing a spurious error. A trailing `;` is
    /// trimmed (the protocol prepares a single statement).
    pub async fn prepare_check(&self, database: Option<&str>, sql: &str) -> Result<(), DbError> {
        if self.engine == Engine::Postgres {
            return pg::prepare_check(self, database, sql).await;
        }
        let stmt = sql.trim().trim_end_matches(';').trim_end();
        if stmt.is_empty() {
            return Ok(());
        }
        let mut conn = self.open(database, false).await?;
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

/// Run several statements in order on ONE connection, so session state (`USE`,
/// `SET`, temp tables, transactions) carries across them exactly as a SQL script
/// would — unlike calling [`fetch_query`] per statement, which reconnects each
/// time. Each statement's outcome is delivered through `on_result(index, …)` as
/// soon as it completes, so the UI can fill result tabs progressively.
///
/// Execution stops at the first failing statement (its index reports the error);
/// every statement after it reports [`DbError::Cancelled`], matching DataGrip's
/// default "stop on error". `cancel` is honored both between and during
/// statements (a mid-flight statement is killed server-side, as in `fetch_query`).
impl Db {
    pub async fn run_batch(
        &self,
        database: Option<&str>,
        stmts: &[String],
        row_cap: usize,
        cancel: CancellationToken,
        mut on_result: impl FnMut(usize, Result<ResultSet, DbError>),
    ) {
        if self.engine == Engine::Postgres {
            pg::run_batch(self, database, stmts, row_cap, cancel, on_result).await;
            return;
        }
        let mut conn = match self.open(database, false).await {
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
            let outcome = tokio::select! {
                // `early_stop = false`: the connection is reused for the next
                // statement, so a truncated result must be drained fully to leave
                // the connection clean.
                r = collect_rows(&mut conn, sql, row_cap, false) => r,
                _ = cancel.cancelled() => {
                    self.kill_query(conn_id).await;
                    Err(DbError::Cancelled)
                }
            };
            if outcome.is_err() {
                stopped = true;
            }
            on_result(i, outcome);
        }

        let _ = conn.disconnect().await;
    }
}

/// `DbError` isn't `Clone`; this reproduces one for the "connect failed" fan-out.
fn err_clone(e: &DbError) -> DbError {
    match e {
        DbError::Connect(s) => DbError::Connect(s.clone()),
        DbError::Query(s) => DbError::Query(s.clone()),
        DbError::Cancelled => DbError::Cancelled,
    }
}

impl Db {
    /// Lightweight reachability check: connect and run `SELECT 1`, all bounded by
    /// `timeout` so a dead host/tunnel can't hang the caller. `Ok(())` means the
    /// server answered.
    pub async fn ping(&self, timeout: std::time::Duration) -> Result<(), DbError> {
        if self.engine == Engine::Postgres {
            return pg::ping(self, timeout).await;
        }
        let check = async {
            let mut conn = self.open(None, false).await?;
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

    /// List the user databases on a server (excludes the built-in system schemas),
    /// sorted by name. Connects at the server level (no specific database needed).
    pub async fn fetch_databases(&self) -> Result<Vec<String>, DbError> {
        if self.engine == Engine::Postgres {
            return pg::fetch_databases(self).await;
        }
        let mut conn = self.open(None, false).await?;
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
    }

    /// Introspect one database's schema (tables → columns + indexes) via
    /// `information_schema` (ARCHITECTURE §11). Everything is `CAST` to a known type
    /// so the protocol never surprises us with a width mismatch.
    pub async fn fetch_schema(&self, database: &str) -> Result<DbSchema, DbError> {
        if self.engine == Engine::Postgres {
            return pg::fetch_schema(self, database).await;
        }
        let mut conn = self.open(None, false).await?;
        let out = collect_schema(&mut conn, database).await;
        let _ = conn.disconnect().await;
        out
    }
}

async fn collect_schema(conn: &mut Conn, database: &str) -> Result<DbSchema, DbError> {
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());

    // Tables, ordered. `TABLE_TYPE` flags views ('VIEW') vs base tables so the
    // tree can render them distinctly; the engine/collation/comment behind it are
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
    let mariadb: bool = conn
        .query_first::<String, _>("SELECT VERSION()")
        .await
        .map_err(qerr)?
        .is_some_and(|v| v.to_ascii_lowercase().contains("mariadb"));

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
    let idx_rows: Vec<IdxRow> = conn
        .exec_map(
            "SELECT CAST(TABLE_NAME AS CHAR) AS t, \
                    CAST(INDEX_NAME AS CHAR) AS i, \
                    CAST(NON_UNIQUE AS SIGNED) AS nu, \
                    CAST(COLUMN_NAME AS CHAR) AS c, \
                    CAST(SUB_PART AS SIGNED) AS sub, \
                    CAST(COLLATION AS CHAR) AS coll \
             FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = ? \
             ORDER BY TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX",
            (database,),
            |r: (String, String, i64, String, Option<i64>, Option<String>)| r,
        )
        .await
        .map_err(qerr)?
        .into_iter()
        .map(|(t, i, nu, c, sub, coll)| IdxRow {
            table: t,
            index: i,
            unique: nu == 0,
            column: schemaic_core::schema::IndexColumn {
                name: c,
                // A prefix index (`KEY (bio(20))`) — recreating it without the
                // length fails outright on a TEXT column.
                prefix: sub.and_then(|n| u32::try_from(n).ok()),
                // `COLLATION` is 'A' ascending, 'D' descending, NULL unsorted.
                descending: coll.as_deref() == Some("D"),
            },
            // MySQL's index type is only worth restating when it isn't the
            // default; BTREE is, so emitting `USING BTREE` everywhere would be
            // noise in every generated statement.
            method: None,
            predicate: None,
        })
        .collect();

    // View definitions (only if the schema has any views) — the stored SELECT
    // body, attached to each view's `TableInfo` for `CREATE VIEW` DDL.
    let has_views = table_rows
        .iter()
        .any(|(_, ty)| ty.eq_ignore_ascii_case("VIEW"));
    let view_rows: Vec<(String, String)> = if has_views {
        conn.exec_map(
            "SELECT CAST(TABLE_NAME AS CHAR) AS t, CAST(VIEW_DEFINITION AS CHAR) AS def \
                 FROM information_schema.VIEWS \
                 WHERE TABLE_SCHEMA = ?",
            (database,),
            |(t, def): (String, String)| (t, def),
        )
        .await
        .map_err(qerr)?
    } else {
        Vec::new()
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
    apply_table_options(&mut schema, &table_opt_rows);
    apply_fk_rules(&mut schema, &fk_rule_rows);
    Ok(schema)
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
    let numeric_or_bool = {
        let t = type_name.to_ascii_lowercase();
        [
            "int", "dec", "num", "float", "double", "real", "bit", "bool",
        ]
        .iter()
        .any(|p| t.starts_with(p))
    };
    let default = default.and_then(|d| {
        if mariadb {
            // Already SQL text. MariaDB writes a *missing* default as SQL NULL
            // and an explicit `DEFAULT NULL` as the literal text — both mean "no
            // default worth emitting" on a nullable column.
            (d != "NULL").then_some(d)
        } else if extra_lc.contains("default_generated")
            || numeric_or_bool
            || d.to_ascii_uppercase().starts_with("CURRENT_TIMESTAMP")
        {
            Some(d)
        } else {
            Some(schemaic_core::schema::ddl_string(&d))
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
            // `GENERATION_EXPRESSION` is the empty string, not NULL, for an
            // ordinary column.
            generated: generated.filter(|g| !g.is_empty()),
            on_update: extra_lc
                .contains("on update current_timestamp")
                .then(|| "CURRENT_TIMESTAMP".to_string()),
            comment: comment.filter(|c| !c.is_empty()),
            collation,
        },
    }
}

/// One introspected column, already turned into the model. A struct rather than
/// a widening tuple because a column now carries nine fields, and
/// `(String, String, String, String, String, Option<String>, bool, …)` at the
/// call site is unreadable and trivially mis-ordered.
///
/// Each engine's fetch does its own normalization (see
/// [`ColumnInfo::default`](schemaic_core::schema::ColumnInfo)) and hands the
/// finished column here, so [`assemble_schema`] stays pure grouping.
#[derive(Clone)]
pub(crate) struct ColRow {
    pub table: String,
    pub column: ColumnInfo,
}

/// One key-column of one index, plus the index-level attributes carried on every
/// row of it (they repeat per key column; the first row wins).
#[derive(Clone)]
pub(crate) struct IdxRow {
    pub table: String,
    pub index: String,
    pub unique: bool,
    pub column: schemaic_core::schema::IndexColumn,
    /// Access method, when the engine names one worth emitting.
    pub method: Option<String>,
    /// Partial-index predicate (PostgreSQL).
    pub predicate: Option<String>,
}

/// One `KEY_COLUMN_USAGE` row for a foreign key: `(table, constraint, column,
/// ref_schema, ref_table, ref_column)`. The referenced fields are `Option` since
/// the column is nullable in the catalogue (though non-null for the FK rows we
/// select). Aliased to keep [`assemble_schema`]'s signature readable.
pub(crate) type FkColRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Assemble the fetched `information_schema` rows into a [`DbSchema`]: group
/// columns onto their tables, fold each index's key columns (in `SEQ_IN_INDEX`
/// order) into one [`IndexInfo`], flag an index FOREIGN when its name matches a
/// FK constraint, mark views, and attach view definitions. Pure — the async
/// `collect_schema` just runs the queries and hands the rows here — so the
/// key/uniqueness/foreign detection that drives editing + DDL is unit-tested.
///
/// Rows referencing a table not in `table_rows` are dropped. `idx_rows` and
/// `col_rows` are consumed in order, so callers must sort by
/// `TABLE_NAME, SEQ_IN_INDEX` / `ORDINAL_POSITION` as the queries do.
///
/// All rows must belong to **one** namespace, which is stamped onto every
/// produced table as [`TableInfo::schema`]: MySQL passes `None` (a database *is*
/// its namespace), and PostgreSQL calls this once per schema and concatenates,
/// since every row here is keyed by table name alone and two schemas may hold
/// same-named tables.
pub(crate) fn assemble_schema(
    schema: Option<&str>,
    table_rows: &[(String, String)],
    col_rows: &[ColRow],
    fk_col_rows: &[FkColRow],
    idx_rows: &[IdxRow],
    view_rows: &[(String, String)],
) -> DbSchema {
    let mut tables: Vec<TableInfo> = Vec::with_capacity(table_rows.len());
    let mut index: HashMap<String, usize> = HashMap::with_capacity(table_rows.len());
    for (name, ty) in table_rows {
        index.insert(name.clone(), tables.len());
        tables.push(TableInfo {
            schema: schema.map(str::to_string),
            name: name.clone(),
            is_view: ty.eq_ignore_ascii_case("VIEW"),
            ..Default::default()
        });
    }

    for c in col_rows {
        let Some(&ti) = index.get(&c.table) else {
            continue;
        };
        tables[ti].columns.push(c.column.clone());
    }

    // Fold the FK key-column rows into one `ForeignKeyInfo` per (table,
    // constraint), preserving column order. Rows missing a referenced table/
    // column are skipped (can't form a usable target).
    let mut fk_slot: HashMap<(usize, String), usize> = HashMap::new();
    for (t, cn, col, rs, rt, rc) in fk_col_rows {
        let Some(&ti) = index.get(t) else { continue };
        let (Some(rt), Some(rc)) = (rt.as_ref(), rc.as_ref()) else {
            continue;
        };
        match fk_slot.get(&(ti, cn.clone())) {
            Some(&fi) => {
                let fk = &mut tables[ti].foreign_keys[fi];
                fk.columns.push(col.clone());
                fk.ref_columns.push(rc.clone());
            }
            None => {
                let fi = tables[ti].foreign_keys.len();
                tables[ti].foreign_keys.push(ForeignKeyInfo {
                    name: cn.clone(),
                    columns: vec![col.clone()],
                    ref_schema: rs.clone(),
                    ref_table: rt.clone(),
                    ref_columns: vec![rc.clone()],
                    ..Default::default()
                });
                fk_slot.insert((ti, cn.clone()), fi);
            }
        }
    }

    for r in idx_rows {
        let Some(&ti) = index.get(&r.table) else {
            continue;
        };
        let table = &mut tables[ti];
        if let Some(existing) = table.indexes.iter_mut().find(|x| x.name == r.index) {
            existing.columns.push(r.column.clone());
        } else {
            table.indexes.push(IndexInfo {
                name: r.index.clone(),
                columns: vec![r.column.clone()],
                unique: r.unique,
                foreign: false, // set by the column-match pass below
                method: r.method.clone(),
                predicate: r.predicate.clone(),
                // Constraint-backed indexes are tagged by the engine's own fetch
                // afterwards (PostgreSQL only); the catalogue rows folded here
                // don't carry it.
                constraint: None,
            });
        }
    }

    // Tag each index FOREIGN when its columns are exactly a FK's referencing
    // columns — matched by *columns*, not name. A FK's backing index is often
    // named after the column (e.g. classicmodels `customerNumber`), not the
    // constraint (`orders_ibfk_1`), so a name match misses it. Done after folding
    // so an index's full column list is known.
    for table in tables.iter_mut() {
        let fk_cols: Vec<&[String]> = table
            .foreign_keys
            .iter()
            .map(|fk| fk.columns.as_slice())
            .filter(|cols| !cols.is_empty())
            .collect();
        for ix in table.indexes.iter_mut() {
            let names: Vec<&str> = ix.column_names().collect();
            ix.foreign = fk_cols
                .iter()
                .any(|&cols| cols.len() == names.len() && cols.iter().eq(names.iter()));
        }
    }

    for (t, def) in view_rows {
        let Some(&ti) = index.get(t) else { continue };
        if !def.is_empty() {
            tables[ti].view_definition = Some(def.clone());
        }
    }

    DbSchema { tables }
}

/// Run the (unprepared, text-protocol) statement, stopping at the row cap, and
/// materialize it into a [`ResultSet`]. When `early_stop` is true, the row
/// stream is abandoned as soon as the cap is hit (the caller tears the
/// connection down); when false, the rest is drained so the connection stays
/// reusable for the next statement in a batch.
pub(crate) async fn collect_rows(
    conn: &mut Conn,
    sql: &str,
    row_cap: usize,
    early_stop: bool,
) -> Result<ResultSet, DbError> {
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

    // Assemble the result columnar, one row at a time, so we never hold a
    // row-major `Vec<Vec<Value>>` copy alongside the final storage.
    let mut builder = ResultBuilder::new(columns);
    let mut truncated = false;
    if let Some(mut stream) = result.stream::<Row>().await.map_err(qerr)? {
        while let Some(row) = stream.next().await {
            let row = row.map_err(qerr)?;
            if builder.row_count() < row_cap {
                let cells = convert_row(&row, builder.columns());
                builder.push_row(&cells);
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

    builder.set_truncated(truncated);
    builder.set_elapsed(start.elapsed().as_millis());
    Ok(builder.finish())
}

/// Map a wire column definition to our [`Column`], capturing its origin
/// (real database/table/column + key flags) when the server reports one.
/// Expression/aggregate/literal columns carry an empty `org_table`, which we
/// surface as `origin: None` — the signal that such a column is not editable.
fn map_column(c: &MyColumn) -> Column {
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
fn is_binary_data_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "VARBINARY"
            | "BINARY"
            | "TINYBLOB"
            | "BLOB"
            | "MEDIUMBLOB"
            | "LONGBLOB"
            | "BIT"
            | "GEOMETRY"
    )
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
    )
}

/// Pure core of [`type_name_of`]: map a wire column type + UNSIGNED flag + binary
/// charset to a human SQL type name. Split out so the mapping (which drives
/// `parse_typed` and editability) is unit-tested without a wire column object.
fn resolve_type_name(ct: ColumnType, unsigned: bool, binary: bool) -> String {
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
    if numeric && unsigned {
        format!("{base} UNSIGNED")
    } else {
        base.to_string()
    }
}

/// Convert one wire row into our typed cells. Over the text protocol every
/// non-NULL value arrives as `Bytes` (its textual form), so we parse it with the
/// column's type exactly as the old code did; the typed arms cover the binary
/// protocol defensively.
fn convert_row(row: &Row, columns: &[Column]) -> Vec<Value> {
    (0..columns.len())
        .map(|i| match row.as_ref(i) {
            None | Some(MyValue::NULL) => Value::Null,
            Some(MyValue::Bytes(b)) => parse_typed(
                String::from_utf8_lossy(b).into_owned(),
                &columns[i].type_name,
            ),
            Some(MyValue::Int(n)) => Value::Int(*n),
            Some(MyValue::UInt(n)) => Value::UInt(*n),
            Some(MyValue::Float(f)) => Value::Float(*f as f64),
            Some(MyValue::Double(f)) => Value::Float(*f),
            Some(other) => Value::Str(other.as_sql(false).trim_matches('\'').to_string()),
        })
        .collect()
}

/// Why a DDL run stopped, and — the part that matters — **how much of it
/// already happened**.
///
/// The two engines differ in a way no amount of wrapping can hide. PostgreSQL
/// has transactional DDL, so a failure rolls the whole plan back and `applied`
/// is 0. MySQL commits implicitly around every DDL statement, so a plan that
/// fails halfway has genuinely half-applied — and the honest thing is to say
/// which statement failed and how many are already in effect, not to pretend the
/// table is untouched.
#[derive(Debug, Clone)]
pub struct DdlError {
    pub message: String,
    /// 0-based index of the statement that failed.
    pub at: usize,
    /// Statements that are in effect on the server despite the failure. Always 0
    /// on PostgreSQL.
    pub applied: usize,
}

impl std::fmt::Display for DdlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "statement {} failed: {}", self.at + 1, self.message)?;
        if self.applied > 0 {
            write!(
                f,
                " — {} earlier statement{} already applied and cannot be rolled back",
                self.applied,
                if self.applied == 1 { "" } else { "s" }
            )?;
        }
        Ok(())
    }
}

impl Db {
    /// Run a generated DDL plan against `database`.
    ///
    /// The statements come from [`ChangeSet::emit`](schemaic_core::ddl::ChangeSet::emit),
    /// which has already put them in an order that works; this only decides how
    /// much atomicity the engine can actually give:
    ///
    /// * **PostgreSQL** — one transaction. `ALTER TABLE`/`CREATE INDEX` are
    ///   transactional there, so a failure anywhere leaves nothing behind.
    /// * **MySQL** — sequential, stopping at the first failure. Every DDL
    ///   statement commits implicitly, so a transaction here would be theatre
    ///   (`tx::implicit_commit` models the same truth for the manual-transaction
    ///   path). The caller is told which statement failed and how many stuck.
    ///
    /// Runs on a fresh connection, like every other operation — a designer's
    /// Apply must not be blocked behind, or ride inside, a tab's transaction.
    pub async fn run_ddl(
        &self,
        database: &str,
        stmts: &[String],
        cancel: CancellationToken,
    ) -> Result<(), DdlError> {
        let fail = |at: usize, applied: usize, e: DbError| DdlError {
            message: e.to_string(),
            at,
            applied,
        };
        if stmts.is_empty() {
            return Ok(());
        }
        if self.engine == Engine::Postgres {
            return pg::run_ddl(self, database, stmts, cancel).await;
        }
        let mut conn = self
            .open(Some(database), false)
            .await
            .map_err(|e| fail(0, 0, e))?;
        let conn_id = conn.id();
        let mut applied = 0usize;
        let mut out = Ok(());
        for (i, sql) in stmts.iter().enumerate() {
            let step = tokio::select! {
                r = conn.query_drop(sql) => r.map_err(|e| DbError::Query(e.to_string())),
                _ = cancel.cancelled() => {
                    self.kill_query(conn_id).await;
                    Err(DbError::Cancelled)
                }
            };
            match step {
                Ok(()) => applied += 1,
                Err(e) => {
                    out = Err(fail(i, applied, e));
                    break;
                }
            }
        }
        let _ = conn.disconnect().await;
        out
    }
}

/// Where an import is writing, and what its `INSERT`s name.
///
/// `columns` are bare names (unquoted); the quoting is the export path's, applied
/// when the statement is built, so it can't drift from the SQL export that reads
/// the same table back out.
pub struct ImportTarget<'a> {
    pub database: &'a str,
    /// The PostgreSQL namespace, when the table has one. On MySQL a database *is*
    /// the namespace, so this is `None`.
    pub schema: Option<&'a str>,
    pub table: &'a str,
    pub columns: &'a [String],
}

/// A source of rows to import. Errors are the reader's — a malformed record or a
/// value that wouldn't coerce — and abort the transaction.
///
/// `Send` because the import runs on the tokio runtime: the reader is pulled
/// between `await`s, so it crosses whatever thread the task resumes on.
pub type RowSource<'a> = &'a mut (dyn Iterator<Item = Result<Vec<Value>, String>> + Send);

impl Db {
    /// Bulk-load rows into one table in a single transaction, as batched
    /// multi-row `INSERT`s.
    ///
    /// Deliberately **not** [`Db::commit_writes`], though it borrows its
    /// discipline. That path runs one statement per row with an exactly-one-row
    /// check, which is right for a handful of grid edits and ruinous for a file:
    /// 100k rows would be 100k round-trips inside one transaction. Here each
    /// statement carries up to `import::INSERT_BATCH_ROWS` rows and the check
    /// becomes "this batch affected exactly as many rows as it had" — same
    /// guarantee that nothing landed half-applied, at a thousandth of the
    /// round-trips.
    ///
    /// All-or-nothing: any reader error, any batch whose count doesn't match, or
    /// a cancellation rolls the whole thing back. Rows are pulled from `rows` in
    /// batches between statements, so the file is never held in memory — and
    /// since a reader that parses a batch does so between two awaits, the work it
    /// does there should stay small.
    pub async fn import_rows(
        &self,
        target: ImportTarget<'_>,
        rows: RowSource<'_>,
        cancel: CancellationToken,
    ) -> Result<u64, DbError> {
        if target.columns.is_empty() {
            return Err(DbError::Query("No columns to import into".to_string()));
        }
        if self.engine == Engine::Postgres {
            return pg::import_rows(self, target, rows, cancel).await;
        }
        let mut conn = self.open(Some(target.database), false).await?;
        let conn_id = conn.id();
        let outcome = tokio::select! {
            r = import_on(&mut conn, self.engine.dialect(), &target, rows) => r,
            _ = cancel.cancelled() => {
                self.kill_query(conn_id).await;
                Err(DbError::Cancelled)
            }
        };
        let _ = conn.disconnect().await;
        outcome
    }
}

/// Pull the next batch of rows from the source. `Ok(None)` at the end.
fn next_batch(rows: RowSource<'_>) -> Result<Option<Vec<Vec<Value>>>, DbError> {
    let mut batch = Vec::with_capacity(schemaic_core::import::INSERT_BATCH_ROWS);
    // `next()` in a loop rather than `by_ref().take(..)`: `by_ref` isn't callable
    // on a `dyn Iterator`, and the source has to stay borrowed for the next batch.
    while batch.len() < schemaic_core::import::INSERT_BATCH_ROWS {
        match rows.next() {
            Some(row) => batch.push(row.map_err(DbError::Query)?),
            None => break,
        }
    }
    Ok(if batch.is_empty() { None } else { Some(batch) })
}

/// The MySQL half of [`Db::import_rows`], on an already-open connection.
async fn import_on(
    conn: &mut Conn,
    dialect: schemaic_core::intel::SqlDialect,
    target: &ImportTarget<'_>,
    rows: RowSource<'_>,
) -> Result<u64, DbError> {
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());
    let cols: Vec<&str> = target.columns.iter().map(String::as_str).collect();
    conn.query_drop("BEGIN").await.map_err(qerr)?;

    let mut total: u64 = 0;
    loop {
        // A reader error (a bad record, a value that wouldn't coerce) has to undo
        // the transaction too — returning straight out would leave it open until
        // the connection drops, which is a lock held for no reason.
        let batch = match next_batch(rows) {
            Ok(Some(b)) => b,
            Ok(None) => break,
            Err(e) => {
                let _ = conn.query_drop("ROLLBACK").await;
                return Err(e);
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
        if let Err(e) = conn.query_drop(&sql).await {
            let _ = conn.query_drop("ROLLBACK").await;
            return Err(qerr(e));
        }
        let affected = conn.affected_rows();
        if affected != batch.len() as u64 {
            let _ = conn.query_drop("ROLLBACK").await;
            return Err(DbError::Query(format!(
                "a batch of {} rows inserted {affected} — rolled back the whole import",
                batch.len()
            )));
        }
        total += affected;
    }

    conn.query_drop("COMMIT").await.map_err(qerr)?;
    Ok(total)
}

/// Apply a batch of staged grid mutations — `UPDATE`s then `INSERT`s — in a
/// single transaction. Every statement must affect **exactly one row**; if any
/// affects zero or more than one (a stale/ambiguous UPDATE identity, or an
/// INSERT that didn't add exactly one row), the whole transaction is rolled back
/// and an error returned, so nothing is half-applied. On success the transaction
/// commits and the total number of affected rows is returned.
///
/// UPDATE identity comes from each edit's `key` (typically the primary key);
/// INSERT columns not listed take their server default (auto-increment /
/// `DEFAULT` / `NULL`). All values are bound parameters, coerced by the server to
/// the column type. Cancellation kills the in-flight statement server-side; the
/// open transaction is then rolled back when the connection drops.
impl Db {
    pub async fn commit_writes(
        &self,
        write: &GridWrite,
        cancel: CancellationToken,
    ) -> Result<u64, DbError> {
        if write.is_empty() {
            return Ok(0);
        }
        if self.engine == Engine::Postgres {
            return pg::commit_writes(self, write, cancel).await;
        }
        // `client_found_rows` so the 1-row guard counts matches, not changes.
        let mut conn = self.open(None, true).await?;
        let conn_id = conn.id();

        let outcome = tokio::select! {
            r = write_on(&mut conn, write, TxScope::Own) => r,
            _ = cancel.cancelled() => {
                self.kill_query(conn_id).await;
                Err(DbError::Cancelled)
            }
        };

        let _ = conn.disconnect().await;
        outcome
    }
}

/// How a batch of writes gets its atomicity.
///
/// A fresh connection owns the whole transaction ([`TxScope::Own`]). Inside a
/// user's **manual** transaction the batch must be atomic *without* ending that
/// transaction, so it nests under a savepoint ([`TxScope::Savepoint`]) — the
/// 1-row guard then rolls back only its own batch and leaves the surrounding
/// transaction intact and usable. (On PostgreSQL the savepoint is what makes a
/// failed batch recoverable at all: a bare error aborts the whole transaction.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TxScope {
    Own,
    Savepoint,
}

/// The savepoint is named `schemaic_w` throughout. A fixed name is safe because
/// batches never overlap on one connection — the session serialises them behind
/// its mutex — and it keeps these strings `&'static`.
impl TxScope {
    pub(crate) fn begin_sql(self) -> &'static str {
        match self {
            TxScope::Own => "BEGIN",
            TxScope::Savepoint => "SAVEPOINT schemaic_w",
        }
    }

    /// Make the batch permanent — for a savepoint that means releasing it, which
    /// merges it into the enclosing transaction rather than committing anything.
    pub(crate) fn commit_sql(self) -> &'static str {
        match self {
            TxScope::Own => "COMMIT",
            TxScope::Savepoint => "RELEASE SAVEPOINT schemaic_w",
        }
    }

    /// Undo the batch, and nothing beyond it.
    pub(crate) fn rollback_sql(self) -> &'static str {
        match self {
            TxScope::Own => "ROLLBACK",
            TxScope::Savepoint => "ROLLBACK TO SAVEPOINT schemaic_w",
        }
    }
}

/// Apply a staged batch of grid mutations on an already-open connection:
/// deletes → updates → inserts, each required to affect exactly one row, the
/// whole batch rolled back if any doesn't. `scope` decides whether that
/// atomicity comes from a transaction of its own or a nested savepoint.
///
/// Deletes run first so "delete a row, then insert one with the same unique key"
/// works. The caller is responsible for `client_found_rows` being on — the guard
/// counts *matched* rows, not *changed* ones.
pub(crate) async fn write_on(
    conn: &mut Conn,
    write: &GridWrite,
    scope: TxScope,
) -> Result<u64, DbError> {
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());
    conn.query_drop(scope.begin_sql()).await.map_err(qerr)?;

    // One statement + its 1-row check. On a miss the batch is undone and the
    // error describes what happened, in the caller's terms.
    #[allow(clippy::too_many_arguments)]
    async fn one(
        conn: &mut Conn,
        scope: TxScope,
        sql: String,
        params: Params,
        action: &str,
        verb: &str,
        database: &str,
        table: &str,
    ) -> Result<u64, DbError> {
        let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());
        if let Err(e) = conn.exec_drop(sql, params).await {
            let _ = conn.query_drop(scope.rollback_sql()).await;
            return Err(qerr(e));
        }
        let affected = conn.affected_rows();
        if affected != 1 {
            let _ = conn.query_drop(scope.rollback_sql()).await;
            return Err(DbError::Query(format!(
                "{action} `{database}`.`{table}` {verb} {affected} rows (expected exactly 1) — \
                 rolled back all changes"
            )));
        }
        Ok(affected)
    }

    let mut total: u64 = 0;
    for del in &write.deletes {
        let (sql, params) = build_delete(del);
        total += one(
            conn,
            scope,
            sql,
            params,
            "delete on",
            "matched",
            &del.database,
            &del.table,
        )
        .await?;
    }
    for edit in &write.updates {
        let (sql, params) = build_update(edit);
        total += one(
            conn,
            scope,
            sql,
            params,
            "update on",
            "matched",
            &edit.database,
            &edit.table,
        )
        .await?;
    }
    for ins in &write.inserts {
        let (sql, params) = build_insert(ins);
        total += one(
            conn,
            scope,
            sql,
            params,
            "insert into",
            "added",
            &ins.database,
            &ins.table,
        )
        .await?;
    }

    if let Err(e) = conn.query_drop(scope.commit_sql()).await {
        let _ = conn.query_drop(scope.rollback_sql()).await;
        return Err(qerr(e));
    }
    Ok(total)
}

impl Db {
    /// Re-`SELECT` the given just-edited rows by their (post-edit) key, so the
    /// grid can splice DB truth back in without re-running the whole query. Runs
    /// one `SELECT … LIMIT 1` per row on a fresh connection — the commit already
    /// committed, so a new connection sees the new data. Rows that no longer match
    /// (e.g. concurrently deleted) are silently skipped. Returns `(data_row,
    /// cells)` pairs, the cells aligned to `template.columns` (i.e. the result
    /// columns). Never mutates data — read-only, so it's safe outside the
    /// transactional write path.
    pub async fn refetch_rows(
        &self,
        template: &RefetchTemplate,
        rows: &[RefetchRow],
        cancel: CancellationToken,
    ) -> Result<Vec<(usize, Vec<Value>)>, DbError> {
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        if self.engine == Engine::Postgres {
            return pg::refetch_rows(self, template, rows, cancel).await;
        }
        let mut conn = self.open(None, false).await?;
        let conn_id = conn.id();
        let outcome = tokio::select! {
            r = refetch_on(&mut conn, template, rows) => r,
            _ = cancel.cancelled() => {
                self.kill_query(conn_id).await;
                Err(DbError::Cancelled)
            }
        };
        let _ = conn.disconnect().await;
        outcome
    }
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
        let fetched: Vec<Row> = result.collect::<Row>().await.map_err(qerr)?;
        if let Some(r) = fetched.first() {
            out.push((row.data_row, convert_row(r, &columns)));
        }
    }
    Ok(out)
}

/// Build a parameterized `UPDATE db.table SET … WHERE …` for one row edit.
/// Identifiers are backtick-escaped; every value is a bound parameter.
fn build_update(edit: &RowEdit) -> (String, Params) {
    let mut params: Vec<MyValue> = Vec::with_capacity(edit.set.len() + edit.key.len());
    let set_sql = edit
        .set
        .iter()
        .map(|(col, val)| {
            params.push(match val {
                Some(v) => MyValue::Bytes(v.clone().into_bytes()),
                None => MyValue::NULL,
            });
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
/// row. Identifiers are backtick-escaped; every value is a bound parameter
/// (`Some` → string param coerced by the server, `None` → SQL `NULL`). Columns
/// not listed take their server default — with none listed, `() VALUES ()`
/// inserts an all-defaults row.
fn build_insert(ins: &RowInsert) -> (String, Params) {
    let mut params: Vec<MyValue> = Vec::with_capacity(ins.cols.len());
    let cols_sql = ins
        .cols
        .iter()
        .map(|(col, val)| {
            params.push(match val {
                Some(v) => MyValue::Bytes(v.clone().into_bytes()),
                None => MyValue::NULL,
            });
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
    let where_sql = template
        .key_cols
        .iter()
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
fn ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
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

/// Parse a text-protocol cell into a typed [`Value`] using the column's SQL
/// type. Integers/floats become compact numeric variants; anything else stays
/// an exact string. Any parse failure falls back to the string — never lossy.
pub(crate) fn parse_typed(s: String, type_name: &str) -> Value {
    let t = type_name.to_ascii_uppercase();
    let is_integer = ["TINYINT", "SMALLINT", "MEDIUMINT", "INT", "BIGINT", "YEAR"]
        .iter()
        .any(|k| t.starts_with(k));
    let is_float = t.starts_with("FLOAT") || t.starts_with("DOUBLE");

    if is_integer {
        if t.contains("UNSIGNED") {
            return s.parse::<u64>().map(Value::UInt).unwrap_or(Value::Str(s));
        }
        return s.parse::<i64>().map(Value::Int).unwrap_or(Value::Str(s));
    }
    if is_float {
        return s.parse::<f64>().map(Value::Float).unwrap_or(Value::Str(s));
    }
    Value::Str(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_insert_sql_shapes() {
        // Normal insert: listed columns → backtick-quoted names + placeholders.
        let ins = RowInsert {
            database: "db".to_string(),
            schema: None,
            table: "users".to_string(),
            cols: vec![
                ("name".to_string(), Some("Ada".to_string())),
                ("email".to_string(), None), // explicit NULL
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
            cols: vec![("a`b".to_string(), Some("x".to_string()))],
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
                ("name".to_string(), Some("Ada".to_string())),
                ("nickname".to_string(), None), // set to NULL
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

    #[test]
    fn build_update_escapes_backtick_identifiers() {
        let edit = RowEdit {
            database: "d`b".to_string(),
            schema: None,
            table: "t`t".to_string(),
            set: vec![("a`b".to_string(), Some("x".to_string()))],
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
    fn db_connect_rewrites_endpoint_for_tunnel() {
        let conn = schemaic_core::connection::Connection {
            id: 1,
            name: "c".to_string(),
            db_type: "MySQL".to_string(),
            host: "remote.example".to_string(),
            port: 3306,
            user: "u".to_string(),
            password: "p".to_string(),
            ssh: Default::default(),
            color: None,
            prominent_color: false,
            read_only: false,
            environment: Default::default(),
        };
        // No tunnel → direct host/port passthrough.
        let direct = Db::connect(&conn, None);
        assert_eq!(direct.parts(), ("remote.example", 3306, "u", "p"));
        // Tunnel → rewritten to 127.0.0.1:<local port>, credentials preserved.
        let tunneled = Db::connect(&conn, Some(55001));
        assert_eq!(tunneled.parts(), ("127.0.0.1", 55001, "u", "p"));
    }

    #[test]
    fn db_from_parts_roundtrips() {
        let db = Db::from_parts(
            Engine::Postgres,
            "h".into(),
            3307,
            "user".into(),
            "pass".into(),
        );
        assert_eq!(db.parts(), ("h", 3307, "user", "pass"));
        assert_eq!(db.engine(), Engine::Postgres);
    }

    #[test]
    fn build_refetch_sql_single_key() {
        let t = RefetchTemplate {
            database: "db".to_string(),
            schema: None,
            table: "users".to_string(),
            columns: vec!["id".to_string(), "name".to_string()],
            key_cols: vec![0],
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
        };
        assert_eq!(
            build_refetch_sql(&t),
            "SELECT `a`, `b`, `c` FROM `db`.`t` WHERE `a` <=> ? AND `c` <=> ? LIMIT 1"
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
        };
        assert_eq!(
            build_refetch_sql(&t),
            "SELECT `a``b` FROM `d``b`.`t``t` WHERE `a``b` <=> ? LIMIT 1"
        );
    }

    #[test]
    fn resolve_type_name_maps_common_types() {
        let non_binary = false;
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_LONG, false, non_binary),
            "INT"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_LONGLONG, false, non_binary),
            "BIGINT"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_NEWDECIMAL, false, non_binary),
            "DECIMAL"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_DATETIME, false, non_binary),
            "DATETIME"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_JSON, false, non_binary),
            "JSON"
        );
    }

    #[test]
    fn resolve_type_name_binary_charset_flips_string_and_blob_types() {
        // charset 63 (binary) turns text types into their binary counterparts.
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_VAR_STRING, false, true),
            "VARBINARY"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_VAR_STRING, false, false),
            "VARCHAR"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_STRING, false, true),
            "BINARY"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_STRING, false, false),
            "CHAR"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_BLOB, false, true),
            "BLOB"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_BLOB, false, false),
            "TEXT"
        );
    }

    #[test]
    fn resolve_type_name_unsigned_only_on_numeric_types() {
        // UNSIGNED suffix appended for numerics…
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_LONG, true, false),
            "INT UNSIGNED"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_NEWDECIMAL, true, false),
            "DECIMAL UNSIGNED"
        );
        // …but never for non-numeric types, even if the flag is set.
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_DATETIME, true, false),
            "DATETIME"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_VAR_STRING, true, false),
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
            "BIT",
            "GEOMETRY",
        ] {
            assert!(is_binary_data_type(t), "{t} should be binary data");
        }
        // Temporal/numeric report charset 63 too, but aren't binary DATA.
        for t in ["DATETIME", "INT", "VARCHAR", "TEXT", "JSON", "DECIMAL"] {
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

    fn s(x: &str) -> String {
        x.to_string()
    }

    /// A plain introspected column, for the grouping tests.
    fn cr(table: &str, name: &str, ty: &str, nullable: bool, pk: bool) -> ColRow {
        ColRow {
            table: s(table),
            column: ColumnInfo {
                name: s(name),
                type_name: s(ty),
                nullable,
                primary_key: pk,
                ..Default::default()
            },
        }
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
    #[test]
    fn a_numeric_default_is_never_quoted() {
        let c = mysql_column(my_row("int(11)", Some("0"), ""), false).column;
        assert_eq!(c.default.as_deref(), Some("0"));
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

    /// One key column of an index, as the MySQL fetch produces it (`non_unique`
    /// is the catalogue's sense: 1 means not unique).
    fn ir(table: &str, index: &str, non_unique: i64, col: &str) -> IdxRow {
        IdxRow {
            table: s(table),
            index: s(index),
            unique: non_unique == 0,
            column: schemaic_core::schema::IndexColumn::plain(col),
            method: None,
            predicate: None,
        }
    }

    #[test]
    fn assemble_schema_groups_columns_and_flags_pk() {
        let tables = [(s("users"), s("BASE TABLE"))];
        let cols = [
            cr("users", "id", "int", false, true),
            cr("users", "email", "varchar(255)", true, false),
        ];
        let schema = assemble_schema(None, &tables, &cols, &[], &[], &[]);
        assert_eq!(schema.tables.len(), 1);
        let t = &schema.tables[0];
        assert!(!t.is_view);
        assert_eq!(t.columns.len(), 2);
        assert!(t.columns[0].primary_key);
        assert!(!t.columns[0].nullable); // IS_NULLABLE = "NO"
        assert!(!t.columns[1].primary_key);
        assert!(t.columns[1].nullable); // "YES"
    }

    #[test]
    fn assemble_schema_stamps_the_namespace_on_every_table() {
        let tables = [(s("orders"), s("BASE TABLE")), (s("v"), s("VIEW"))];
        // MySQL: no namespace level at all.
        let mysql = assemble_schema(None, &tables, &[], &[], &[], &[]);
        assert!(mysql.tables.iter().all(|t| t.schema.is_none()));
        // Postgres: every table in the batch belongs to the one namespace it was
        // fetched for — views included.
        let pg = assemble_schema(Some("sales"), &tables, &[], &[], &[], &[]);
        assert!(
            pg.tables
                .iter()
                .all(|t| t.schema.as_deref() == Some("sales"))
        );
    }

    #[test]
    fn assemble_schema_folds_composite_index_in_order() {
        let tables = [(s("t"), s("BASE TABLE"))];
        // Two rows for the same index name → one IndexInfo, columns in row order.
        let idx = [
            ir("t", "idx_ab", 1, "a"),
            ir("t", "idx_ab", 1, "b"),
            ir("t", "PRIMARY", 0, "id"),
        ];
        let schema = assemble_schema(None, &tables, &[], &[], &idx, &[]);
        let t = &schema.tables[0];
        assert_eq!(t.indexes.len(), 2);
        let ab = t.indexes.iter().find(|i| i.name == "idx_ab").unwrap();
        assert_eq!(ab.column_names().collect::<Vec<_>>(), vec!["a", "b"]);
        assert!(!ab.unique); // NON_UNIQUE = 1
        let pk = t.indexes.iter().find(|i| i.name == "PRIMARY").unwrap();
        assert!(pk.unique); // NON_UNIQUE = 0
        assert!(pk.is_primary());
    }

    #[test]
    fn assemble_schema_flags_foreign_index_by_columns_not_name() {
        let tables = [(s("orders"), s("BASE TABLE"))];
        // The FK's backing index is named after the column (`customerNumber`), not
        // the constraint (`orders_ibfk_1`) — the classicmodels case. Matching by
        // name misses it; matching by columns flags it.
        let idx = [
            ir("orders", "customerNumber", 1, "customerNumber"),
            ir("orders", "idx_plain", 1, "total"),
        ];
        let fks = [(
            s("orders"),
            s("orders_ibfk_1"),
            s("customerNumber"),
            Some(s("shop")),
            Some(s("customers")),
            Some(s("customerNumber")),
        )];
        let schema = assemble_schema(None, &tables, &[], &fks, &idx, &[]);
        let t = &schema.tables[0];
        assert!(
            t.indexes
                .iter()
                .find(|i| i.name == "customerNumber")
                .unwrap()
                .foreign,
            "FK-backing index flagged FOREIGN by columns despite name != constraint"
        );
        assert!(
            !t.indexes
                .iter()
                .find(|i| i.name == "idx_plain")
                .unwrap()
                .foreign
        );
    }

    #[test]
    fn assemble_schema_builds_foreign_keys_with_targets() {
        let tables = [(s("orders"), s("BASE TABLE"))];
        // One single-column FK and one composite FK (two ordered rows).
        let fks = [
            (
                s("orders"),
                s("fk_customer"),
                s("customer_id"),
                Some(s("shop")),
                Some(s("customers")),
                Some(s("id")),
            ),
            (
                s("orders"),
                s("fk_line"),
                s("order_id"),
                Some(s("shop")),
                Some(s("lines")),
                Some(s("order_id")),
            ),
            (
                s("orders"),
                s("fk_line"),
                s("line_no"),
                Some(s("shop")),
                Some(s("lines")),
                Some(s("no")),
            ),
        ];
        let schema = assemble_schema(None, &tables, &[], &fks, &[], &[]);
        let t = &schema.tables[0];
        assert_eq!(t.foreign_keys.len(), 2);

        let single = t.fk_for_column("customer_id").unwrap();
        assert_eq!(single.ref_table, "customers");
        assert_eq!(single.ref_schema.as_deref(), Some("shop"));
        assert_eq!(single.ref_columns, vec!["id".to_string()]);

        // Composite FK: both columns fold into one FK, in ORDINAL_POSITION order.
        let composite = t.fk_for_column("line_no").unwrap();
        assert_eq!(composite.ref_table, "lines");
        assert_eq!(
            composite.columns,
            vec!["order_id".to_string(), "line_no".to_string()]
        );
        assert_eq!(
            composite.ref_columns,
            vec!["order_id".to_string(), "no".to_string()]
        );
    }

    #[test]
    fn assemble_schema_marks_views_and_attaches_definition() {
        let tables = [(s("v"), s("VIEW")), (s("base"), s("BASE TABLE"))];
        let views = [(s("v"), s("SELECT 1"))];
        let schema = assemble_schema(None, &tables, &[], &[], &[], &views);
        let v = schema.tables.iter().find(|t| t.name == "v").unwrap();
        assert!(v.is_view);
        assert_eq!(v.view_definition.as_deref(), Some("SELECT 1"));
        let base = schema.tables.iter().find(|t| t.name == "base").unwrap();
        assert!(!base.is_view);
        assert!(base.view_definition.is_none());
    }

    #[test]
    fn assemble_schema_drops_rows_for_unknown_tables() {
        let tables = [(s("known"), s("BASE TABLE"))];
        // Column/index rows referencing a table absent from `tables` are ignored.
        let cols = [cr("ghost", "x", "int", false, true)];
        let idx = [ir("ghost", "idx", 1, "x")];
        let schema = assemble_schema(None, &tables, &cols, &[], &idx, &[]);
        assert_eq!(schema.tables.len(), 1);
        assert!(schema.tables[0].columns.is_empty());
        assert!(schema.tables[0].indexes.is_empty());
    }

    #[test]
    fn assemble_schema_empty_view_definition_stays_none() {
        // A view whose VIEW_DEFINITION came back empty (e.g. privileges) → None,
        // so create_ddl falls back to its placeholder.
        let tables = [(s("v"), s("VIEW"))];
        let views = [(s("v"), s(""))];
        let schema = assemble_schema(None, &tables, &[], &[], &[], &views);
        assert!(schema.tables[0].view_definition.is_none());
    }

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
