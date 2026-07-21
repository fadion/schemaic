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

pub mod ssh;

use std::collections::{HashMap, HashSet};

use futures_util::StreamExt;
use mysql_async::consts::{ColumnFlags, ColumnType};
use mysql_async::prelude::Queryable;
use mysql_async::{Column as MyColumn, Conn, Row, Value as MyValue};
use mysql_async::{OptsBuilder, Params, TxOpts};
use schemaic_core::model::{
    Column, ColumnFlags as CoreColFlags, ColumnOrigin, GridWrite, RefetchRow, RefetchTemplate,
    ResultSet, RowDelete, RowEdit, RowInsert, Value,
};
use schemaic_core::schema::{ColumnInfo, DbSchema, IndexInfo, TableInfo};
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
    host: String,
    port: u16,
    user: String,
    pass: String,
}

impl Db {
    /// Resolve a saved connection into a `Db`. For an SSH connection, pass the
    /// established tunnel's local port and the target is rewritten to
    /// `127.0.0.1:<port>`. Infallible — no URL is parsed.
    pub fn connect(conn: &schemaic_core::connection::Connection, tunnel_port: Option<u16>) -> Db {
        match tunnel_port {
            Some(port) => Db {
                host: "127.0.0.1".to_string(),
                port,
                user: conn.user.clone(),
                pass: conn.password.clone(),
            },
            None => Db {
                host: conn.host.clone(),
                port: conn.port,
                user: conn.user.clone(),
                pass: conn.password.clone(),
            },
        }
    }

    /// Reconstruct from raw parts — used by the MCP subprocess, which receives
    /// the (already-tunnelled) endpoint over its environment.
    pub fn from_parts(host: String, port: u16, user: String, pass: String) -> Db {
        Db {
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
    async fn open(&self, database: Option<&str>, found_rows: bool) -> Result<Conn, DbError> {
        Conn::new(self.opts(database, found_rows))
            .await
            .map_err(|e| DbError::Connect(e.to_string()))
    }

    /// Best-effort server-side cancel: connect afresh and `KILL QUERY <id>`.
    async fn kill_query(&self, conn_id: u32) {
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
        let stmt = sql.trim().trim_end_matches(';').trim_end();
        if analyze {
            let primary = format!("EXPLAIN ANALYZE {stmt}");
            match self
                .fetch_query(database, &primary, EXPLAIN_ROW_CAP, cancel.clone())
                .await
            {
                Err(DbError::Query(_)) => {
                    // MariaDB: `EXPLAIN ANALYZE` is invalid — it uses `ANALYZE <stmt>`.
                    let alt = format!("ANALYZE {stmt}");
                    self.fetch_query(database, &alt, EXPLAIN_ROW_CAP, cancel)
                        .await
                }
                other => other,
            }
        } else {
            let q = format!("EXPLAIN {stmt}");
            self.fetch_query(database, &q, EXPLAIN_ROW_CAP, cancel)
                .await
        }
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
        let mut conn = self.open(None, false).await?;
        let out = collect_schema(&mut conn, database).await;
        let _ = conn.disconnect().await;
        out
    }
}

async fn collect_schema(conn: &mut Conn, database: &str) -> Result<DbSchema, DbError> {
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());

    // Tables, ordered. `TABLE_TYPE` flags views ('VIEW') vs base tables so the
    // tree can render them distinctly.
    let table_rows: Vec<(String, String)> = conn
        .exec_map(
            "SELECT CAST(TABLE_NAME AS CHAR) AS t, CAST(TABLE_TYPE AS CHAR) AS ty \
             FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
            (database,),
            |(t, ty): (String, String)| (t, ty),
        )
        .await
        .map_err(qerr)?;

    let mut tables: Vec<TableInfo> = Vec::with_capacity(table_rows.len());
    let mut index: HashMap<String, usize> = HashMap::with_capacity(table_rows.len());
    for (name, ty) in &table_rows {
        index.insert(name.clone(), tables.len());
        tables.push(TableInfo {
            name: name.clone(),
            columns: Vec::new(),
            indexes: Vec::new(),
            is_view: ty.eq_ignore_ascii_case("VIEW"),
            view_definition: None,
        });
    }

    // Columns for the whole schema in one pass, grouped back onto their tables.
    let col_rows: Vec<(String, String, String, String, String)> = conn
        .exec_map(
            "SELECT CAST(TABLE_NAME AS CHAR) AS t, \
                    CAST(COLUMN_NAME AS CHAR) AS c, \
                    CAST(COLUMN_TYPE AS CHAR) AS ty, \
                    CAST(IS_NULLABLE AS CHAR) AS nullable, \
                    CAST(COLUMN_KEY AS CHAR) AS ck \
             FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = ? \
             ORDER BY TABLE_NAME, ORDINAL_POSITION",
            (database,),
            |(t, c, ty, nullable, ck): (String, String, String, String, String)| {
                (t, c, ty, nullable, ck)
            },
        )
        .await
        .map_err(qerr)?;
    for (t, c, ty, nullable, key) in &col_rows {
        let Some(&ti) = index.get(t) else { continue };
        tables[ti].columns.push(ColumnInfo {
            name: c.clone(),
            type_name: ty.clone(),
            nullable: nullable.eq_ignore_ascii_case("YES"),
            primary_key: key == "PRI",
        });
    }

    // Foreign-key constraint names — MySQL auto-creates an index sharing the
    // constraint's name, so we tag those indexes as FOREIGN below.
    let fk_rows: Vec<(String, String)> = conn
        .exec_map(
            "SELECT CAST(TABLE_NAME AS CHAR) AS t, CAST(CONSTRAINT_NAME AS CHAR) AS n \
             FROM information_schema.TABLE_CONSTRAINTS \
             WHERE TABLE_SCHEMA = ? AND CONSTRAINT_TYPE = 'FOREIGN KEY'",
            (database,),
            |(t, n): (String, String)| (t, n),
        )
        .await
        .map_err(qerr)?;
    let fks: HashSet<(String, String)> = fk_rows.into_iter().collect();

    // Indexes: one row per (index, key-column); fold consecutive columns into
    // the same index, preserving `SEQ_IN_INDEX` order.
    let idx_rows: Vec<(String, String, i64, String)> = conn
        .exec_map(
            "SELECT CAST(TABLE_NAME AS CHAR) AS t, \
                    CAST(INDEX_NAME AS CHAR) AS i, \
                    CAST(NON_UNIQUE AS SIGNED) AS nu, \
                    CAST(COLUMN_NAME AS CHAR) AS c \
             FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = ? \
             ORDER BY TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX",
            (database,),
            |(t, i, nu, c): (String, String, i64, String)| (t, i, nu, c),
        )
        .await
        .map_err(qerr)?;
    for (t, iname, non_unique, col) in &idx_rows {
        let Some(&ti) = index.get(t) else { continue };
        let table = &mut tables[ti];
        if let Some(existing) = table.indexes.iter_mut().find(|x| &x.name == iname) {
            existing.columns.push(col.clone());
        } else {
            let foreign = fks.contains(&(t.clone(), iname.clone()));
            table.indexes.push(IndexInfo {
                name: iname.clone(),
                columns: vec![col.clone()],
                unique: *non_unique == 0,
                foreign,
            });
        }
    }

    // View definitions (only if the schema has any views) — the stored SELECT
    // body, attached to each view's `TableInfo` for `CREATE VIEW` DDL.
    if tables.iter().any(|t| t.is_view) {
        let view_rows: Vec<(String, String)> = conn
            .exec_map(
                "SELECT CAST(TABLE_NAME AS CHAR) AS t, CAST(VIEW_DEFINITION AS CHAR) AS def \
                 FROM information_schema.VIEWS \
                 WHERE TABLE_SCHEMA = ?",
                (database,),
                |(t, def): (String, String)| (t, def),
            )
            .await
            .map_err(qerr)?;
        for (t, def) in &view_rows {
            let Some(&ti) = index.get(t) else { continue };
            if !def.is_empty() {
                tables[ti].view_definition = Some(def.clone());
            }
        }
    }

    Ok(DbSchema { tables })
}

/// Run the (unprepared, text-protocol) statement, stopping at the row cap, and
/// materialize it into a [`ResultSet`]. When `early_stop` is true, the row
/// stream is abandoned as soon as the cap is hit (the caller tears the
/// connection down); when false, the rest is drained so the connection stays
/// reusable for the next statement in a batch.
async fn collect_rows(
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
        return Ok(ResultSet {
            columns,
            rows: Vec::new(),
            elapsed_ms: start.elapsed().as_millis(),
            truncated: false,
            affected: Some(affected),
        });
    }

    let mut rows: Vec<Vec<Value>> = Vec::new();
    let mut truncated = false;
    if let Some(mut stream) = result.stream::<Row>().await.map_err(qerr)? {
        while let Some(row) = stream.next().await {
            let row = row.map_err(qerr)?;
            if rows.len() < row_cap {
                rows.push(convert_row(&row, &columns));
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

    Ok(ResultSet {
        columns,
        rows,
        elapsed_ms: start.elapsed().as_millis(),
        truncated,
        affected: None,
    })
}

/// Map a wire column definition to our [`Column`], capturing its origin
/// (real database/table/column + key flags) when the server reports one.
/// Expression/aggregate/literal columns carry an empty `org_table`, which we
/// surface as `origin: None` — the signal that such a column is not editable.
fn map_column(c: &MyColumn) -> Column {
    let type_name = type_name_of(c);
    // A *binary-data* column (raw bytes), not merely "binary charset": numeric /
    // temporal columns also report charset 63, so key off the resolved type
    // name instead. These can't round-trip through the text protocol losslessly.
    let binary = matches!(
        type_name.as_str(),
        "VARBINARY"
            | "BINARY"
            | "TINYBLOB"
            | "BLOB"
            | "MEDIUMBLOB"
            | "LONGBLOB"
            | "BIT"
            | "GEOMETRY"
    );
    let org_table = c.org_table_str();
    let origin = if org_table.is_empty() {
        None
    } else {
        let f = c.flags();
        Some(ColumnOrigin {
            database: c.schema_str().to_string(),
            table: org_table.to_string(),
            column: c.org_name_str().to_string(),
            flags: CoreColFlags {
                primary_key: f.contains(ColumnFlags::PRI_KEY_FLAG),
                unique_key: f.contains(ColumnFlags::UNIQUE_KEY_FLAG),
                not_null: f.contains(ColumnFlags::NOT_NULL_FLAG),
                auto_increment: f.contains(ColumnFlags::AUTO_INCREMENT_FLAG),
                no_default: f.contains(ColumnFlags::NO_DEFAULT_VALUE_FLAG),
            },
            binary,
        })
    };
    Column {
        name: c.name_str().to_string(),
        type_name,
        origin,
    }
}

/// Reconstruct a human SQL type name (`VARCHAR`, `INT UNSIGNED`, `DATETIME`, …)
/// from the wire column type + flags + charset — matching what the old sqlx
/// `type_info().name()` produced, so `parse_typed` and the UI keep behaving.
fn type_name_of(c: &MyColumn) -> String {
    let ct = c.column_type();
    let flags = c.flags();
    let binary = c.character_set() == BINARY_CHARSET;
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
    if numeric && flags.contains(ColumnFlags::UNSIGNED_FLAG) {
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
        // `client_found_rows` so the 1-row guard counts matches, not changes.
        let mut conn = self.open(None, true).await?;
        let conn_id = conn.id();

        let run = async {
            let mut tx = conn
                .start_transaction(TxOpts::default())
                .await
                .map_err(|e| DbError::Query(e.to_string()))?;
            let mut total: u64 = 0;
            // Deletes first (so "delete a row then insert one with the same unique
            // key" works), then updates, then inserts — all in the one transaction.
            for del in &write.deletes {
                let (sql, params) = build_delete(del);
                tx.exec_drop(sql, params)
                    .await
                    .map_err(|e| DbError::Query(e.to_string()))?;
                let affected = tx.affected_rows();
                if affected != 1 {
                    let _ = tx.rollback().await;
                    return Err(DbError::Query(format!(
                        "delete on `{}`.`{}` matched {affected} rows (expected exactly 1) — \
                     rolled back all changes",
                        del.database, del.table
                    )));
                }
                total += affected;
            }
            // Then updates, then inserts.
            for edit in &write.updates {
                let (sql, params) = build_update(edit);
                tx.exec_drop(sql, params)
                    .await
                    .map_err(|e| DbError::Query(e.to_string()))?;
                let affected = tx.affected_rows();
                if affected != 1 {
                    // Roll back everything: the identity wasn't unique / current.
                    let _ = tx.rollback().await;
                    return Err(DbError::Query(format!(
                        "update on `{}`.`{}` matched {affected} rows (expected exactly 1) — \
                     rolled back all changes",
                        edit.database, edit.table
                    )));
                }
                total += affected;
            }
            for ins in &write.inserts {
                let (sql, params) = build_insert(ins);
                tx.exec_drop(sql, params)
                    .await
                    .map_err(|e| DbError::Query(e.to_string()))?;
                let affected = tx.affected_rows();
                if affected != 1 {
                    let _ = tx.rollback().await;
                    return Err(DbError::Query(format!(
                        "insert into `{}`.`{}` added {affected} rows (expected exactly 1) — \
                     rolled back all changes",
                        ins.database, ins.table
                    )));
                }
                total += affected;
            }
            tx.commit()
                .await
                .map_err(|e| DbError::Query(e.to_string()))?;
            Ok(total)
        };

        let outcome = tokio::select! {
            r = run => r,
            _ = cancel.cancelled() => {
                self.kill_query(conn_id).await;
                Err(DbError::Cancelled)
            }
        };

        let _ = conn.disconnect().await;
        outcome
    }
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
        let sql = format!(
            "SELECT {cols_sql} FROM {}.{} WHERE {where_sql} LIMIT 1",
            ident(&template.database),
            ident(&template.table),
        );

        let mut conn = self.open(None, false).await?;
        let conn_id = conn.id();
        let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());
        let run = async {
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
        };
        let outcome = tokio::select! {
            r = run => r,
            _ = cancel.cancelled() => {
                self.kill_query(conn_id).await;
                Err(DbError::Cancelled)
            }
        };
        let _ = conn.disconnect().await;
        outcome
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
fn parse_typed(s: String, type_name: &str) -> Value {
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
            table: "t".to_string(),
            cols: vec![],
        };
        let (sql, _) = build_insert(&empty);
        assert_eq!(sql, "INSERT INTO `db`.`t` () VALUES ()");

        // Identifiers with backticks are doubled.
        let weird = RowInsert {
            database: "d`b".to_string(),
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
}
