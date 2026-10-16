//! **The write log** — an append-only record of every write the app and the
//! CLI executed: when, on which connection and database, what ran, how many
//! rows it touched and how it ended. One JSON object a line in
//! `write_log.jsonl` under the config directory ([`FILE`]).
//!
//! **Append-only and never cleared by Schemaic.** Nothing in the app truncates,
//! rewrites, prunes or rotates it — deleting a connection leaves its lines, and
//! there is no *Clear* — because a record the tool can quietly drop is not one.
//! It is the user's file: they may archive or delete it themselves, and the
//! next write starts a fresh one. It is opened per entry in append mode
//! (`persist::open_private_append`, so 0600 on Unix), and each line goes out
//! in one `write`, so the GUI and a `schemaic exec` in another process do not
//! interleave inside a line.
//!
//! **What counts as a write** is decided by the caller's path: the editor and
//! `schemaic exec` log a statement [`is_logged`] answers for — one
//! `sql::contains_write` calls a write, or one the read-only gate will not
//! call a read, so a `SELECT` of a writing function is logged too — and the
//! grid commit, the DDL preview, the import and a `.sql` file run log
//! whatever they ran. *Restore from file* hands the file to the engine's own
//! client in the terminal, so it is logged as it is launched, its outcome
//! unknown ([`Source::Restore`]). The MCP server has no write tool, so it has
//! nothing to log. **How a statement ended** is [`Entry::landed`]'s, from a
//! [`Ran`] each front end maps its own state onto, and which statements of a
//! Run All get a line is [`batch_logged`]'s — decisions that lived in an app
//! closure and a CLI function, untested, until a typed `COMMIT` of an aborted
//! PostgreSQL transaction was logged `ok`.
//!
//! **Values are masked by default, because the SQL is the user's data.** An
//! `UPDATE … SET email = 'ann@example.com'` carries the very value a log should
//! not keep in the clear for years. [`mask_values`] replaces every string and
//! numeric literal with `?` and drops comments — the statement's shape (which
//! table, which columns, which `WHERE`) stays, the data goes — and the engine's
//! error is passed through `prompt::redact_engine_error`, which strips the
//! values a uniqueness or not-null report quotes back. The user may keep
//! values (`UiState::write_log_values`), **but never a credential's**: a
//! statement `sql::carries_credential` flags is masked whatever that setting
//! says, as `history.json` drops such a statement altogether.

use serde::{Deserialize, Serialize};

use crate::intel::SqlDialect;
use crate::sql::{is_word_byte, skip_comment, skip_noncode};

/// The log's file name under `persist::config_dir`.
pub const FILE: &str = "write_log.jsonl";

/// Which path executed the write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// An editor run, Run All, or a Manual tab's commit or rollback.
    Editor,
    /// A `.sql` file run as a script.
    Script,
    /// The result grid's staged edits, written back.
    Grid,
    /// A previewed DDL plan — the designers, the account editor, a drop.
    Ddl,
    /// Rows imported from a file.
    Import,
    /// Rows copied from another table by *Copy to* (its `CREATE` is a `Ddl`).
    Copy,
    /// `schemaic exec`.
    Cli,
    /// *Restore from file*, launched in the terminal by the engine's own
    /// client (`psql`, `pg_restore`, `mysql`). Logged when it starts, with
    /// the file in `detail`: the client, not Schemaic, sees how it ends, so
    /// its outcome is [`Outcome::Unknown`].
    Restore,
}

/// How the write ended, as far as the client can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    Failed,
    Cancelled,
    /// The client lost track of it — a timeout after the statement was sent —
    /// so it may or may not have taken effect.
    Unknown,
}

/// One write, as its path reports it — before masking ([`line()`] masks).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Unix time in milliseconds.
    pub ts: u64,
    pub source: Source,
    /// The connection's name as the user saved it.
    pub connection: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// What ran: the statement(s). Empty for a path that has no statement to
    /// name — an import, a file that ran to the end — whose `detail` says what
    /// happened instead.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sql: String,
    /// What the path says about the write beyond its SQL — the file a script
    /// ran from and how many statements ran, an import's target. Schemaic's
    /// own words over names (a path, a table), so never masked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Rows affected, where the engine said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<u64>,
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Whether `sql` and `error` had their values masked. Written so a reader
    /// knows a `?` was a value, not the statement's own placeholder.
    #[serde(default)]
    pub masked: bool,
}

impl Entry {
    /// An entry stamped now.
    pub fn now(source: Source, connection: &str, database: Option<&str>, sql: &str) -> Entry {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        Entry {
            ts,
            source,
            connection: connection.to_string(),
            database: database.filter(|d| !d.is_empty()).map(str::to_string),
            sql: sql.to_string(),
            detail: None,
            rows: None,
            outcome: Outcome::Ok,
            error: None,
            masked: false,
        }
    }

    /// The same entry with `detail` said.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Entry {
        self.detail = Some(detail.into());
        self
    }

    /// The same entry with its outcome: `Ok(rows)` or `Err(message)`.
    pub fn ended(mut self, result: Result<Option<u64>, String>) -> Entry {
        match result {
            Ok(rows) => {
                self.rows = rows;
                self.outcome = Outcome::Ok;
            }
            Err(e) => {
                self.outcome = Outcome::Failed;
                self.error = Some(e);
            }
        }
        self
    }

    /// The same entry, ended with `outcome` and no row count — a cancel, or a
    /// write the client lost track of.
    pub fn ended_as(mut self, outcome: Outcome, error: Option<String>) -> Entry {
        self.outcome = outcome;
        self.error = error;
        self
    }

    /// The same entry ended as `ran` says, or `None` while it has no verdict.
    ///
    /// `aborted_before` is whether the statement ran inside a transaction a
    /// failure had already aborted (`tx::TxState::Poisoned`). There a typed
    /// `COMMIT` is a `ROLLBACK` on PostgreSQL, which reports it as a success,
    /// so the line says `failed` and why rather than `ok` — the Commit
    /// button's `aborted_commit` refusal, for the statement typed by hand.
    ///
    /// **`PREPARE TRANSACTION` is the third such spelling**: PostgreSQL ends
    /// an aborted transaction it is typed into with a rollback and the tag
    /// `ROLLBACK`, reported as a success, as it does a `COMMIT` or `END`.
    pub fn landed(self, ran: Ran, aborted_before: bool, dialect: SqlDialect) -> Option<Entry> {
        let words = crate::sql::leading_words(&self.sql, 2, dialect);
        let ends = match words
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["COMMIT" | "END", ..] => Some("COMMIT"),
            ["PREPARE", "TRANSACTION"] => Some("PREPARE TRANSACTION"),
            _ => None,
        };
        Some(match ran {
            Ran::Rows(_) if aborted_before && ends.is_some() => self.ended(Err(format!(
                "the transaction was already aborted, so {} rolled it back — nothing was \
                 committed",
                ends.unwrap_or("COMMIT")
            ))),
            Ran::Rows(rows) => self.ended(Ok(rows)),
            Ran::Failed(m) => self.ended(Err(m)),
            Ran::Cancelled => self.ended_as(Outcome::Cancelled, None),
            Ran::Unknown(m) => self.ended_as(Outcome::Unknown, m),
            Ran::Pending => return None,
        })
    }
}

/// How one statement's run ended, as a front end saw it — the input to
/// [`Entry::landed`] and [`batch_logged`], each front end mapping its own
/// state onto it in one thin match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ran {
    /// It ran; rows affected where the engine said.
    Rows(Option<u64>),
    /// The engine refused it, in its words.
    Failed(String),
    /// Stopped, or never dispatched (a batch statement after a stop or a
    /// failure).
    Cancelled,
    /// Sent, and then the client lost track of it — a timeout.
    Unknown(Option<String>),
    /// No verdict yet.
    Pending,
}

/// Does running `sql` get a line in the log? A statement
/// `sql::contains_write` calls a write, **or one the read-only gate
/// (`sql::read_only_reason`) will not call a read** — `SELECT
/// archive_and_purge(2023)` deletes rows behind a read head, and the gate's
/// PostgreSQL and SQL Server function allowlists fail closed on a call they do
/// not know where `contains_write` fails open. Logging a read is the
/// acceptable direction; missing a write is not.
pub fn is_logged(sql: &str, dialect: SqlDialect) -> bool {
    crate::sql::contains_write(sql, dialect) || crate::sql::read_only_reason(sql, dialect).is_err()
}

/// Which statements of a Run All get a line, by index, given how each ended
/// and whether the user pressed Stop. A `Cancelled` statement was never
/// dispatched — except the one in flight at Stop, the first without a result
/// after the run of successes, which may have written (a statement already
/// sent is not unsent). One with no verdict yet gets none.
pub fn batch_logged(ran: &[Ran], stopped: bool) -> Vec<bool> {
    let in_flight = stopped
        .then(|| ran.iter().position(|r| !matches!(r, Ran::Rows(_))))
        .flatten();
    ran.iter()
        .enumerate()
        .map(|(i, r)| match r {
            Ran::Cancelled => in_flight == Some(i),
            Ran::Pending => false,
            Ran::Rows(_) | Ran::Failed(_) | Ran::Unknown(_) => true,
        })
        .collect()
}

/// For each statement of a batch, whether it ran inside a transaction a
/// failure had already aborted — [`Entry::landed`]'s `aborted_before` — given
/// the tab's state at launch (`start`) and each statement with its outcome
/// (`None`: never dispatched, which moves nothing).
///
/// **Read before the statement's own outcome is folded in**, in order, since
/// one batch can poison its transaction and then `COMMIT` it: the `COMMIT` is
/// the rollback, the statement that failed ran in a healthy transaction.
pub fn batch_aborted(
    start: crate::tx::TxState,
    engine: crate::tx::TxEngine,
    steps: &[(&str, Option<crate::tx::StmtOutcome>)],
) -> Vec<bool> {
    let mut tx = start;
    steps
        .iter()
        .map(|(sql, outcome)| {
            let aborted = matches!(tx, crate::tx::TxState::Poisoned { .. });
            if let Some(o) = outcome {
                tx = tx.on_statement(engine, sql, *o);
            }
            aborted
        })
        .collect()
}

/// The line for a Manual tab's open transaction that Schemaic dropped rather
/// than ended — its connection edited or deleted — which the server rolls
/// back on the disconnect. Without it the log kept the transaction's writes
/// as `ok` with no `COMMIT` or `ROLLBACK` after them, and that end line is
/// the only one that says whether they stayed. `why` is the detail.
pub fn abandoned(database: Option<&str>, why: &str) -> Entry {
    Entry::now(Source::Editor, "", database, "ROLLBACK")
        .with_detail(why)
        .ended(Ok(None))
}

/// `entry` as the line the log holds — masked unless `keep_values`, and
/// masked regardless when it carries a credential — without the newline.
pub fn line(entry: &Entry, dialect: SqlDialect, keep_values: bool) -> String {
    let mut e = entry.clone();
    let credential = crate::sql::carries_credential(&e.sql, dialect);
    if !keep_values || credential {
        e.sql = mask_values(&e.sql, dialect);
        e.error = e.error.map(|m| crate::prompt::redact_engine_error(&m));
        e.masked = true;
    }
    serde_json::to_string(&e).unwrap_or_default()
}

/// Append `entry` to the log under the config directory. An error is the
/// caller's to report or drop: the write it records has already happened, and
/// failing to log it must not be mistaken for the write failing.
pub fn append(entry: &Entry, dialect: SqlDialect, keep_values: bool) -> std::io::Result<()> {
    use std::io::Write;
    let dir = crate::persist::config_dir()
        .ok_or_else(|| std::io::Error::other("no configuration directory"))?;
    std::fs::create_dir_all(&dir)?;
    let mut text = line(entry, dialect, keep_values);
    text.push('\n');
    let mut f = crate::persist::open_private_append(&dir.join(FILE))?;
    // One `write_all` of the whole line, so two processes appending at once
    // cannot interleave inside it.
    f.write_all(text.as_bytes())
}

/// A grid commit as the log names it: each staged step in the order the
/// executor runs it (`GridWrite::plan`), as a statement with its values
/// written in as literals (`export::sql_literal`) — the executor binds them,
/// so this is the record's spelling, not what went over the wire. A byte
/// value is written as `<binary, N bytes>`, never its bytes; a column set to
/// its default as `DEFAULT`.
pub fn grid_sql(write: &crate::model::GridWrite, dialect: SqlDialect) -> String {
    use crate::export::{ident_sql, sql_literal};
    use crate::model::{CellEdit, Value, WriteStep};
    let table = |database: &str, schema: &Option<String>, table: &str| {
        let ns = schema.as_deref().unwrap_or(database);
        if ns.is_empty() {
            ident_sql(table, dialect)
        } else {
            format!("{}.{}", ident_sql(ns, dialect), ident_sql(table, dialect))
        }
    };
    let cell = |c: &CellEdit| match c {
        CellEdit::Null => "NULL".to_string(),
        CellEdit::Text(t) => sql_literal(&Value::Str(t.clone()), dialect),
        CellEdit::Bytes(b) => format!("<binary, {} bytes>", b.len()),
        CellEdit::Default => "DEFAULT".to_string(),
    };
    let key = |k: &[(String, Value)]| {
        k.iter()
            .map(|(c, v)| match v {
                Value::Null => format!("{} IS NULL", ident_sql(c, dialect)),
                v => format!("{} = {}", ident_sql(c, dialect), sql_literal(v, dialect)),
            })
            .collect::<Vec<_>>()
            .join(" AND ")
    };
    write
        .plan()
        .iter()
        .map(|step| match step {
            WriteStep::Delete(d) => format!(
                "DELETE FROM {} WHERE {};",
                table(&d.database, &d.schema, &d.table),
                key(&d.key)
            ),
            WriteStep::Update(u) => format!(
                "UPDATE {} SET {} WHERE {};",
                table(&u.database, &u.schema, &u.table),
                u.set
                    .iter()
                    .map(|(c, v)| format!("{} = {}", ident_sql(c, dialect), cell(v)))
                    .collect::<Vec<_>>()
                    .join(", "),
                key(&u.key)
            ),
            WriteStep::Insert(i) => format!(
                "INSERT INTO {} ({}) VALUES ({});",
                table(&i.database, &i.schema, &i.table),
                i.cols
                    .iter()
                    .map(|(c, _)| ident_sql(c, dialect))
                    .collect::<Vec<_>>()
                    .join(", "),
                i.cols
                    .iter()
                    .map(|(_, v)| cell(v))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `sql` with every **value** replaced by `?`: string literals (with any
/// prefix — `N'…'`, `E'…'`, `X'…'`, `_utf8mb4'…'`), a dialect's double-quoted
/// strings, and numeric literals; comments are dropped, since a comment is
/// prose the user may have put a value in. Identifiers, keywords and bind
/// placeholders (`?`, `$1`, `:name`, `@p`) stay, so the shape of what ran is
/// still readable. A dollar-quoted body is code (a function's), so it is
/// masked in turn rather than swallowed whole.
///
/// Boundaries come from [`skip_noncode`], the one SQL boundary lexer.
pub fn mask_values(sql: &str, dialect: SqlDialect) -> String {
    let b = sql.as_bytes();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0usize;
    while i < b.len() {
        if let Some(j) = skip_comment(b, i, dialect) {
            // One space keeps the words either side apart — and only one, so
            // `a /* x */ b` reads `a b`.
            i = j;
            if out.ends_with([' ', '\t']) {
                while i < b.len() && matches!(b[i], b' ' | b'\t') {
                    i += 1;
                }
            } else if !out.is_empty() && !out.ends_with(char::is_whitespace) {
                out.push(' ');
            }
            continue;
        }
        if let Some(j) = skip_noncode(b, i, dialect) {
            match b[i] {
                b'\'' => out.push('?'),
                b'"' if !dialect.double_quote_is_ident() => out.push('?'),
                b'$' => out.push_str(&mask_dollar_body(&sql[i..j], dialect)),
                _ => out.push_str(&sql[i..j]),
            }
            i = j;
            continue;
        }
        if is_word_byte(b[i]) {
            let start = i;
            while i < b.len() && is_word_byte(b[i]) {
                i += 1;
            }
            let word = &sql[start..i];
            let prev = start.checked_sub(1).map(|p| b[p]);
            let placeholder = matches!(prev, Some(b'$' | b':' | b'@' | b'?'));
            let prefix =
                (word.len() == 1 && b"EeNnXxBbUu".contains(&b[start])) || word.starts_with('_');
            if b.get(i) == Some(&b'\'') && prefix && !placeholder {
                // A string's prefix or introducer: the literal that follows
                // becomes the `?`.
                continue;
            }
            if b[start].is_ascii_digit() && !placeholder && !word_continues_name(b, start) {
                // A decimal's fraction is part of the number.
                if b.get(i) == Some(&b'.') && b.get(i + 1).is_some_and(u8::is_ascii_digit) {
                    i += 1;
                    while i < b.len() && is_word_byte(b[i]) {
                        i += 1;
                    }
                }
                out.push('?');
            } else {
                out.push_str(word);
            }
            continue;
        }
        let ch = sql[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&sql[i..i + ch]);
        i += ch;
    }
    out
}

/// Is the digit-led word at `start` the tail of a qualified name (`t.1col` —
/// rare, but a name), rather than a number? Only a `.` directly before it that
/// itself follows a word says so; `.5` alone is a number.
fn word_continues_name(b: &[u8], start: usize) -> bool {
    start >= 2
        && b[start - 1] == b'.'
        && is_word_byte(b[start - 2])
        && !b[start - 2].is_ascii_digit()
}

/// A dollar-quoted span `$tag$ … $tag$` with its body masked as SQL.
fn mask_dollar_body(span: &str, dialect: SqlDialect) -> String {
    // The opening tag runs to the second `$`.
    let Some(open_end) = span[1..].find('$').map(|p| p + 2) else {
        return "?".into();
    };
    let tag = &span[..open_end];
    let body_end = if span.len() >= 2 * tag.len() && span.ends_with(tag) {
        span.len() - tag.len()
    } else {
        span.len()
    };
    if body_end < open_end {
        return "?".into();
    }
    let body = &span[open_end..body_end];
    format!("{tag}{}{}", mask_values(body, dialect), &span[body_end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    const MY: SqlDialect = SqlDialect::MySql;
    const PG: SqlDialect = SqlDialect::Postgres;
    const MS: SqlDialect = SqlDialect::MsSql;

    #[test]
    fn strings_and_numbers_become_placeholders_and_names_stay() {
        assert_eq!(
            mask_values(
                "UPDATE users SET email = 'ann@example.com', age = 41 WHERE id = 7",
                MY
            ),
            "UPDATE users SET email = ?, age = ? WHERE id = ?"
        );
        assert_eq!(
            mask_values("INSERT INTO t1 (c2) VALUES (3.25), (-0.5)", PG),
            "INSERT INTO t1 (c2) VALUES (?), (-?)"
        );
    }

    #[test]
    fn a_prefixed_or_introduced_string_loses_its_prefix_too() {
        assert_eq!(mask_values("SELECT N'Zoë', X'0F'", MS), "SELECT ?, ?");
        assert_eq!(mask_values("SELECT E'a\\'b'", PG), "SELECT ?");
        assert_eq!(mask_values("SELECT _utf8mb4'x'", MY), "SELECT ?");
    }

    #[test]
    fn a_double_quote_is_masked_only_where_it_is_a_string() {
        assert_eq!(
            mask_values(r#"UPDATE t SET a = "v""#, MY),
            "UPDATE t SET a = ?"
        );
        assert_eq!(
            mask_values(r#"UPDATE "T" SET "a" = 'v'"#, PG),
            r#"UPDATE "T" SET "a" = ?"#
        );
    }

    #[test]
    fn placeholders_and_quoted_names_are_left_alone() {
        assert_eq!(
            mask_values(
                "UPDATE `t` SET a = ? WHERE b = $1 AND c = :c AND d = @p1",
                MY
            ),
            "UPDATE `t` SET a = ? WHERE b = $1 AND c = :c AND d = @p1"
        );
        assert_eq!(
            mask_values("DELETE FROM [o'k] WHERE x = 1", MS),
            "DELETE FROM [o'k] WHERE x = ?"
        );
    }

    #[test]
    fn comments_are_dropped_since_a_value_can_hide_in_one() {
        assert_eq!(
            mask_values("DELETE FROM t -- bob@example.com asked\nWHERE id = 9", PG),
            "DELETE FROM t \nWHERE id = ?"
        );
        assert_eq!(
            mask_values("DELETE /* 555-1234 */ FROM t", MY),
            "DELETE FROM t"
        );
    }

    #[test]
    fn a_dollar_quoted_body_is_masked_as_code_not_swallowed() {
        assert_eq!(
            mask_values(
                "CREATE FUNCTION f() RETURNS int AS $$ SELECT 42 WHERE 'x' = 'y' $$ LANGUAGE sql",
                PG
            ),
            "CREATE FUNCTION f() RETURNS int AS $$ SELECT ? WHERE ? = ? $$ LANGUAGE sql"
        );
        // An unclosed string inside the body runs to its end — still masked.
        assert_eq!(mask_values("SELECT $q$it's$q$", PG), "SELECT $q$it?$q$");
        // A word that merely touches a quote is not a prefix.
        assert_eq!(
            mask_values("UPDATE t SET a='x' WHERE b IN('y')", MY),
            "UPDATE t SET a=? WHERE b IN(?)"
        );
    }

    #[test]
    fn a_line_masks_by_default_and_says_so() {
        let e = Entry::now(
            Source::Editor,
            "prod",
            Some("shop"),
            "UPDATE c SET n = 'Ann' WHERE id = 3",
        )
        .ended(Err("Duplicate entry 'Ann' for key 'n'".into()));
        let v: serde_json::Value = serde_json::from_str(&line(&e, MY, false)).unwrap();
        assert_eq!(v["sql"], "UPDATE c SET n = ? WHERE id = ?");
        assert_eq!(v["masked"], true);
        assert_eq!(v["outcome"], "failed");
        assert_eq!(v["source"], "editor");
        assert_eq!(v["database"], "shop");
        assert!(
            !v["error"].as_str().unwrap().contains("Ann"),
            "{}",
            v["error"]
        );
        assert!(!line(&e, MY, false).contains('\n'));
    }

    #[test]
    fn keeping_values_keeps_them_but_never_a_credentials() {
        let e = Entry::now(Source::Editor, "c", None, "UPDATE c SET n = 'Ann'").ended(Ok(Some(1)));
        let kept: serde_json::Value = serde_json::from_str(&line(&e, MY, true)).unwrap();
        assert_eq!(kept["sql"], "UPDATE c SET n = 'Ann'");
        assert_eq!(kept["masked"], false);
        assert_eq!(kept["rows"], 1);
        assert!(kept.get("database").is_none());
        let pw = Entry::now(
            Source::Ddl,
            "c",
            None,
            "CREATE USER 'u'@'%' IDENTIFIED BY 'hunter2'",
        );
        let l = line(&pw, MY, true);
        assert!(!l.contains("hunter2"), "{l}");
        assert!(l.contains(r#""masked":true"#), "{l}");
    }

    #[test]
    fn a_grid_commit_is_named_step_by_step_in_the_executors_order() {
        use crate::model::{CellEdit, GridWrite, RowDelete, RowEdit, RowInsert, Value};
        let write = GridWrite {
            updates: vec![RowEdit {
                database: "shop".into(),
                schema: None,
                table: "users".into(),
                set: vec![
                    ("email".into(), CellEdit::Text("o'k@x.io".into())),
                    ("avatar".into(), CellEdit::Bytes(vec![1u8, 2, 3].into())),
                ],
                key: vec![("id".into(), Value::Int(7))],
            }],
            inserts: vec![RowInsert {
                database: "shop".into(),
                schema: None,
                table: "users".into(),
                cols: vec![
                    ("name".into(), CellEdit::Null),
                    ("made".into(), CellEdit::Default),
                ],
            }],
            deletes: vec![RowDelete {
                database: "shop".into(),
                schema: None,
                table: "users".into(),
                key: vec![("id".into(), Value::Int(9)), ("t".into(), Value::Null)],
            }],
        };
        let sql = grid_sql(&write, MY);
        assert_eq!(
            sql,
            "DELETE FROM `shop`.`users` WHERE `id` = 9 AND `t` IS NULL;\n\
             UPDATE `shop`.`users` SET `email` = 'o''k@x.io', `avatar` = <binary, 3 bytes> WHERE `id` = 7;\n\
             INSERT INTO `shop`.`users` (`name`, `made`) VALUES (NULL, DEFAULT);"
        );
        // And masked as a line would be, the email goes.
        assert!(!mask_values(&sql, MY).contains("x.io"));
    }

    #[test]
    fn a_postgres_grid_write_is_qualified_by_its_schema() {
        use crate::model::{GridWrite, RowDelete, Value};
        let write = GridWrite {
            deletes: vec![RowDelete {
                database: "app".into(),
                schema: Some("public".into()),
                table: "t".into(),
                key: vec![("id".into(), Value::Int(1))],
            }],
            ..GridWrite::default()
        };
        assert_eq!(
            grid_sql(&write, PG),
            r#"DELETE FROM "public"."t" WHERE "id" = 1;"#
        );
    }

    #[test]
    fn an_empty_database_is_no_database() {
        assert_eq!(Entry::now(Source::Cli, "c", Some(""), "x").database, None);
    }

    #[test]
    fn a_line_reads_back_as_the_entry() {
        let e = Entry::now(Source::Grid, "c", Some("d"), "UPDATE t SET a = ?").ended(Ok(Some(2)));
        let back: Entry = serde_json::from_str(&line(&e, PG, true)).unwrap();
        assert_eq!(back, e);
    }

    fn entry(sql: &str) -> Entry {
        Entry::now(Source::Editor, "c", None, sql)
    }

    #[test]
    fn each_way_a_run_ends_maps_to_its_outcome() {
        let ok = entry("DELETE FROM t")
            .landed(Ran::Rows(Some(3)), false, PG)
            .unwrap();
        assert_eq!((ok.outcome, ok.rows), (Outcome::Ok, Some(3)));
        let bad = entry("DELETE FROM t")
            .landed(Ran::Failed("no".into()), false, PG)
            .unwrap();
        assert_eq!(
            (bad.outcome, bad.error.as_deref()),
            (Outcome::Failed, Some("no"))
        );
        let stop = entry("DELETE FROM t")
            .landed(Ran::Cancelled, false, PG)
            .unwrap();
        assert_eq!(stop.outcome, Outcome::Cancelled);
        assert!(
            entry("DELETE FROM t")
                .landed(Ran::Pending, false, PG)
                .is_none()
        );
    }

    /// The CLI's timeout: sent, then lost track of — neither ok nor failed.
    #[test]
    fn an_indeterminate_run_is_unknown() {
        let e = entry("DELETE FROM t")
            .landed(Ran::Unknown(Some("timed out".into())), false, PG)
            .unwrap();
        assert_eq!(e.outcome, Outcome::Unknown);
        assert_eq!(e.error.as_deref(), Some("timed out"));
        assert_eq!(e.rows, None);
    }

    /// S1.1-L1-02: PostgreSQL answers a `COMMIT` of an aborted block with a
    /// ROLLBACK it reports as success; the line must not say `ok`.
    #[test]
    fn a_commit_typed_into_an_aborted_transaction_is_logged_failed() {
        for sql in ["COMMIT", "commit;", "END", "COMMIT AND CHAIN"] {
            let e = entry(sql).landed(Ran::Rows(None), true, PG).unwrap();
            assert_eq!(e.outcome, Outcome::Failed, "{sql}");
            assert!(
                e.error.as_deref().is_some_and(|m| m.contains("rolled")),
                "{sql}"
            );
        }
        // A rollback of it is what it says, and a commit of a healthy
        // transaction is a commit.
        let rb = entry("ROLLBACK").landed(Ran::Rows(None), true, PG).unwrap();
        assert_eq!(rb.outcome, Outcome::Ok);
        let ok = entry("COMMIT").landed(Ran::Rows(None), false, PG).unwrap();
        assert_eq!(ok.outcome, Outcome::Ok);
    }

    /// **`PREPARE TRANSACTION` is the third spelling**: in an aborted
    /// PostgreSQL transaction it is a rollback too, answered with success
    /// and the tag `ROLLBACK` (measured on PG 16), and it was logged `ok`.
    #[test]
    fn a_prepare_typed_into_an_aborted_transaction_is_logged_failed() {
        for sql in ["PREPARE TRANSACTION 'batch42'", "prepare transaction 'x';"] {
            let e = entry(sql).landed(Ran::Rows(None), true, PG).unwrap();
            assert_eq!(e.outcome, Outcome::Failed, "{sql}");
            assert!(
                e.error
                    .as_deref()
                    .is_some_and(|m| m.contains("PREPARE TRANSACTION rolled")),
                "{sql}: {:?}",
                e.error
            );
        }
        let ok = entry("PREPARE TRANSACTION 'x'")
            .landed(Ran::Rows(None), false, PG)
            .unwrap();
        assert_eq!(ok.outcome, Outcome::Ok);
        // A prepared statement is not a transaction's end.
        let stmt = entry("PREPARE q AS SELECT 1")
            .landed(Ran::Rows(None), true, PG)
            .unwrap();
        assert_eq!(stmt.outcome, Outcome::Ok);
    }

    /// **Where each statement of a batch ran is folded before it runs, in
    /// order** — `aborted_before` is the state *ahead* of the statement, so a
    /// batch that poisons its transaction and then commits it logs the
    /// `COMMIT` as the rollback it is, and the statement that failed as
    /// having run in a healthy one. The app folded this inline, a caller
    /// seam no test saw (R2-L6-02).
    #[test]
    fn a_batch_reads_each_statement_against_the_state_before_it() {
        use crate::tx::{StmtOutcome, TxEngine, TxState};
        let steps = [
            ("BEGIN", Some(StmtOutcome::Ok)),
            ("SELECT 1/0", Some(StmtOutcome::Failed)),
            ("COMMIT", Some(StmtOutcome::Ok)),
        ];
        assert_eq!(
            batch_aborted(TxState::Idle, TxEngine::Postgres, &steps),
            [false, false, true]
        );
        // Opened earlier in the tab and already aborted: every one of them.
        let poisoned = TxState::Idle
            .on_statement(TxEngine::Postgres, "BEGIN", StmtOutcome::Ok)
            .on_statement(TxEngine::Postgres, "SELECT 1/0", StmtOutcome::Failed);
        assert_eq!(
            batch_aborted(poisoned, TxEngine::Postgres, &steps[2..]),
            [true]
        );
        // A statement with no outcome (never dispatched) moves nothing.
        let held = [
            ("BEGIN", Some(StmtOutcome::Ok)),
            ("SELECT 1/0", None),
            ("COMMIT", Some(StmtOutcome::Ok)),
        ];
        assert_eq!(
            batch_aborted(TxState::Idle, TxEngine::Postgres, &held),
            [false, false, false]
        );
    }

    #[test]
    fn a_stopped_batch_logs_the_in_flight_statement_and_not_the_undispatched() {
        let ran = [Ran::Rows(Some(1)), Ran::Cancelled, Ran::Cancelled];
        assert_eq!(batch_logged(&ran, true), [true, true, false]);
    }

    #[test]
    fn a_failed_batch_logs_nothing_after_the_failure() {
        let ran = [
            Ran::Rows(Some(1)),
            Ran::Failed("dup".into()),
            Ran::Cancelled,
        ];
        assert_eq!(batch_logged(&ran, false), [true, true, false]);
        // Stopped after a failure: nothing was in flight past it.
        assert_eq!(batch_logged(&ran, true), [true, true, false]);
        assert_eq!(batch_logged(&[Ran::Pending], false), [false]);
    }

    /// S1.1-L5-04: a write made by calling a function from a `SELECT` is
    /// still a write. The log asks the read-only gate too, which fails closed
    /// on a call it does not know.
    #[test]
    fn a_write_through_a_function_call_is_logged() {
        assert!(is_logged("SELECT archive_and_purge(2023)", PG));
        assert!(is_logged("UPDATE t SET a = 1", PG));
        assert!(!is_logged("SELECT 1", PG));
        assert!(!is_logged("SELECT * FROM t WHERE a = 1", MY));
    }

    #[test]
    fn a_dropped_transaction_is_logged_as_the_rollback_the_server_made() {
        let e = abandoned(Some("shop"), "the connection was deleted");
        assert_eq!(
            (e.sql.as_str(), e.outcome, e.detail.as_deref()),
            ("ROLLBACK", Outcome::Ok, Some("the connection was deleted"))
        );
        assert_eq!(e.database.as_deref(), Some("shop"));
    }
}
