//! **The agent log** — a record of every call an AI agent made to Schemaic's
//! MCP server: which tool, on which connection and database, with what SQL,
//! and how it ended. One JSON object a line in `agent_log.jsonl` under the
//! config directory ([`FILE`]), beside the write log.
//!
//! **A separate file from the write log, in its format.** The write log is a
//! record of writes, and the MCP server writes nothing; what an agent *looked
//! at* on a production connection is the question this answers, and mixing
//! its reads into the writes would bury the record that file exists for. The
//! lines are [`write_log::Entry`] — [`Source::Agent`], the tool and the host
//! in `detail` — and go through [`write_log::line`], so the values in a
//! `run_query`'s SQL are masked as a write's are, and a credential's always.
//!
//! **Capped, unlike the write log.** An agent calls a tool many times a
//! minute, and nothing would ever clear it; past [`ROLL_OVER_BYTES`] the file
//! becomes [`ROLLED`] — replacing the previous one — and a fresh one starts.
//! The most recent stretch is kept whole; anything older than two files' worth
//! is gone, which is the trade for a log that is on by default.
//!
//! [`write_log::Entry`]: crate::write_log::Entry
//! [`write_log::line`]: crate::write_log::line
//! [`Source::Agent`]: crate::write_log::Source::Agent

use crate::intel::SqlDialect;
use crate::write_log::{Entry, Outcome, Source};

/// The log's file name under `persist::config_dir`.
pub const FILE: &str = "agent_log.jsonl";

/// What [`FILE`] becomes when it rolls over.
pub const ROLLED: &str = "agent_log.jsonl.1";

/// Past this size the file rolls over. Several thousand calls' worth: a long
/// day of an agent's work, kept whole twice over with [`ROLLED`].
pub const ROLL_OVER_BYTES: u64 = 8 << 20;

/// Which front end served the call — said in the line, since the panel's
/// assistant and an outside client are different answers to "who asked?".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caller {
    /// The app's AI panel (`--mcp-serve`).
    Panel,
    /// `schemaic mcp`, spawned by an MCP client of the user's choosing.
    Standalone,
}

impl Caller {
    fn word(self) -> &'static str {
        match self {
            Caller::Panel => "panel",
            Caller::Standalone => "mcp",
        }
    }
}

/// How long an error's text may run in a line. An error is Schemaic's
/// sentence or the engine's — never rows — but a long one is no clearer.
pub const ERROR_CHARS: usize = 500;

/// How long a call's SQL may run in a line — far under [`ROLL_OVER_BYTES`],
/// so no one call can roll the file: two agent-sized statements used to roll
/// it twice and leave both files holding only themselves.
pub const SQL_CHARS: usize = 32 * 1024;

/// One call, as the log records it.
///
/// `sql` is the statement the tool was given — `run_query`'s, `explain`'s,
/// `propose_sql`'s — and empty for a tool that takes none. `subject` is what
/// else the call named: a table, a search, the other side of a comparison.
/// `refused_or_failed` carries the tool's own answer when it was an error,
/// which is Schemaic's sentence or the engine's, never rows; `rows` how many
/// rows an answer showed the agent, where it showed any — the count, as the
/// write log keeps a write's.
#[allow(clippy::too_many_arguments)]
pub fn entry(
    caller: Caller,
    connection: &str,
    database: Option<&str>,
    tool: &str,
    sql: &str,
    subject: Option<&str>,
    refused_or_failed: Option<&str>,
    rows: Option<u64>,
) -> Entry {
    let detail = match subject {
        Some(s) if !s.is_empty() => format!("{tool} {s} ({})", caller.word()),
        _ => format!("{tool} ({})", caller.word()),
    };
    let e = Entry::now(Source::Agent, connection, database, sql).with_detail(detail);
    match refused_or_failed {
        None => e.ended(Ok(rows)),
        Some(why) => e.ended_as(Outcome::Failed, Some(why.to_string())),
    }
}

/// `entry` as a line holds it: masked as the write log masks
/// ([`crate::write_log::masked`]), **then** cut — the SQL to [`SQL_CHARS`],
/// the error to [`ERROR_CHARS`], each saying what was cut.
///
/// **The order is the point.** The redactor finds a rejected value by its
/// closing quote; cut first, a value long enough to lose it was written
/// whole up to the cut, under `"masked": true`.
pub fn line_entry(entry: &Entry, dialect: SqlDialect, keep_values: bool) -> Entry {
    let mut e = crate::write_log::masked(entry, dialect, keep_values);
    e.sql = cut(&e.sql, SQL_CHARS);
    e.error = e.error.map(|m| cut(&m, ERROR_CHARS));
    e
}

/// [`line_entry`], serialised — without the newline.
pub fn line(entry: &Entry, dialect: SqlDialect, keep_values: bool) -> String {
    serde_json::to_string(&line_entry(entry, dialect, keep_values)).unwrap_or_default()
}

/// `text` to its first `max` characters, and `…` with how many more there
/// were when there were any.
fn cut(text: &str, max: usize) -> String {
    let total = text.chars().count();
    if total <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max).collect();
    format!("{kept}… ({} characters more)", total - max)
}

/// The lock a roll-over is taken under — see [`append`].
const ROLL_LOCK: &str = "agent_log.jsonl.lock";

/// Past this age a lock is a crashed process's, not a roll in progress — a
/// rename takes milliseconds — and is taken over rather than leaving the log
/// never to roll again.
const STALE_LOCK: std::time::Duration = std::time::Duration::from_secs(60);

/// Is a lock last touched `age` ago abandoned?
fn lock_is_stale(age: std::time::Duration) -> bool {
    age > STALE_LOCK
}

/// The roll-over lock, held while it lives: a file created exclusively, and
/// removed on drop.
struct RollLock(std::path::PathBuf);

impl RollLock {
    /// The lock at `path`, or `None` while another process holds it.
    fn take(path: &std::path::Path) -> Option<RollLock> {
        let create = || {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
        };
        match create() {
            Ok(_) => Some(RollLock(path.to_path_buf())),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let age = std::fs::metadata(path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok());
                if !age.is_some_and(lock_is_stale) {
                    return None;
                }
                let _ = std::fs::remove_file(path);
                create().ok().map(|_| RollLock(path.to_path_buf()))
            }
            Err(_) => None,
        }
    }
}

impl Drop for RollLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Does a file of `len` bytes roll over before `adding` more go in?
///
/// **An empty file never does**, however long the line: a line larger than
/// the cap would otherwise roll every time and leave each file holding only
/// itself.
pub fn rolls_over(len: u64, adding: usize) -> bool {
    len > 0 && len.saturating_add(adding as u64) > ROLL_OVER_BYTES
}

/// Append `entry` under the config directory, rolling the file over first
/// when [`rolls_over`] says so. An error is the caller's to drop: the call it
/// records has already been answered, and a log that could not be written
/// must not turn into a failed tool call.
pub fn append(entry: &Entry, dialect: SqlDialect, keep_values: bool) -> std::io::Result<()> {
    use std::io::Write;
    let dir = crate::persist::config_dir()
        .ok_or_else(|| std::io::Error::other("no configuration directory"))?;
    std::fs::create_dir_all(&dir)?;
    let mut text = line(entry, dialect, keep_values);
    text.push('\n');
    let path = dir.join(FILE);
    let size = || std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    // **The roll is taken under a lock, and the size asked again inside it.**
    // The panel's server and a `schemaic mcp` both append here; two of them
    // seeing the full file at once each renamed it, the second renaming the
    // fresh one-line file over the 8 MiB one the first had just rolled. One
    // that finds the lock taken skips the roll and appends: whoever holds it
    // is rolling, and one line past the cap is no loss.
    if rolls_over(size(), text.len())
        && let Some(_lock) = RollLock::take(&dir.join(ROLL_LOCK))
        && rolls_over(size(), text.len())
    {
        // A failed rename leaves the file growing past the cap, which is
        // better than a call nobody recorded.
        let _ = std::fs::rename(&path, dir.join(ROLLED));
    }
    let mut f = crate::persist::open_private_append(&path)?;
    // One `write_all` of the whole line, as the write log does, so the panel's
    // server and a `schemaic mcp` appending at once cannot interleave inside it.
    f.write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lock is a roll in progress for a minute, and a crashed process's
    /// after it — never a log that cannot roll again.
    #[test]
    fn a_roll_lock_goes_stale_after_a_minute() {
        use std::time::Duration;
        assert!(!lock_is_stale(Duration::from_millis(5)));
        assert!(!lock_is_stale(STALE_LOCK));
        assert!(lock_is_stale(STALE_LOCK + Duration::from_secs(1)));
    }

    #[test]
    fn an_answered_call_names_its_tool_subject_and_caller() {
        let e = entry(
            Caller::Panel,
            "prod",
            Some("shop"),
            "describe_table",
            "",
            Some("orders"),
            None,
            None,
        );
        assert_eq!(e.source, Source::Agent);
        assert_eq!(e.connection, "prod");
        assert_eq!(e.database.as_deref(), Some("shop"));
        assert_eq!(e.detail.as_deref(), Some("describe_table orders (panel)"));
        assert_eq!(e.outcome, Outcome::Ok);
        assert!(e.sql.is_empty());
    }

    #[test]
    fn a_query_keeps_its_sql_and_a_refusal_its_reason() {
        let e = entry(
            Caller::Standalone,
            "prod",
            None,
            "run_query",
            "DELETE FROM t",
            None,
            Some("not a read"),
            None,
        );
        assert_eq!(e.sql, "DELETE FROM t");
        assert_eq!(e.detail.as_deref(), Some("run_query (mcp)"));
        assert_eq!(e.outcome, Outcome::Failed);
        assert_eq!(e.error.as_deref(), Some("not a read"));
    }

    /// The SQL an agent sent is the user's data as much as a write's is, so
    /// its values are masked by the write log's own `line`.
    #[test]
    fn a_querys_values_are_masked_as_a_writes_are() {
        let e = entry(
            Caller::Panel,
            "prod",
            None,
            "run_query",
            "SELECT * FROM users WHERE email = 'ann@example.com'",
            None,
            None,
            None,
        );
        let line = line(&e, SqlDialect::MySql, false);
        assert!(!line.contains("ann@example.com"), "{line}");
        assert!(line.contains("\"source\":\"agent\""), "{line}");
    }

    /// **Masked first, cut second.** A PostgreSQL rejected value long enough
    /// to lose its closing quote to the cut was written unmasked: the
    /// redactor needs both quotes, and it ran on the cut text.
    #[test]
    fn a_long_error_is_redacted_before_it_is_cut() {
        let value = "S".repeat(600);
        let error = format!("Query error: invalid input syntax for type integer: \"{value}\"");
        let e = entry(
            Caller::Standalone,
            "prod",
            None,
            "run_query",
            "SELECT t::int FROM x",
            None,
            Some(&error),
            None,
        );
        let line = line(&e, SqlDialect::Postgres, false);
        assert!(!line.contains("SSSSSSSSSS"), "{line}");
        // Kept values are still cut to the cap.
        let kept = line_entry(&e, SqlDialect::Postgres, true);
        let kept = kept.error.unwrap();
        assert!(kept.starts_with("Query error") && kept.ends_with("characters more)"));
        assert!(kept.chars().count() < ERROR_CHARS + 40, "{kept}");
    }

    /// **One call cannot roll the file**: the logged SQL is capped, so two
    /// oversized calls no longer erase both files' worth of record.
    #[test]
    fn an_oversized_statement_is_cut_well_under_the_roll_over() {
        let sql = format!("SELECT {} FROM t", "a, ".repeat(3_000_000));
        let e = entry(
            Caller::Panel,
            "prod",
            None,
            "run_query",
            &sql,
            None,
            None,
            None,
        );
        let l = line(&e, SqlDialect::MySql, false);
        assert!(l.len() < 2 * SQL_CHARS, "{}", l.len());
        assert!(l.contains("characters more"), "the cut is said");
        assert!(!rolls_over(ROLL_OVER_BYTES / 2, l.len()));
    }

    /// **An answered call that returned rows says how many** — the count the
    /// agent was shown, never the rows.
    #[test]
    fn an_answer_with_rows_records_their_count() {
        let e = entry(
            Caller::Panel,
            "prod",
            None,
            "run_query",
            "SELECT 1",
            None,
            None,
            Some(3),
        );
        assert_eq!(e.rows, Some(3));
        let none = entry(
            Caller::Panel,
            "prod",
            None,
            "list_schema",
            "",
            None,
            None,
            None,
        );
        assert_eq!(none.rows, None);
    }

    #[test]
    fn the_file_rolls_over_past_the_cap_and_never_when_empty() {
        assert!(!rolls_over(0, 100));
        assert!(!rolls_over(0, (ROLL_OVER_BYTES + 1) as usize));
        assert!(!rolls_over(ROLL_OVER_BYTES - 100, 100));
        assert!(rolls_over(ROLL_OVER_BYTES - 100, 101));
        assert!(rolls_over(ROLL_OVER_BYTES + 1, 1));
    }
}
