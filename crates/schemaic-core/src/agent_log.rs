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

/// One call, as the log records it.
///
/// `sql` is the statement the tool was given — `run_query`'s, `explain`'s —
/// and empty for a tool that takes none. `subject` is what else the call
/// named: a table, a search, a database. `refused_or_failed` carries the
/// tool's own answer when it was an error, which is Schemaic's sentence or
/// the engine's, never rows.
pub fn entry(
    caller: Caller,
    connection: &str,
    database: Option<&str>,
    tool: &str,
    sql: &str,
    subject: Option<&str>,
    refused_or_failed: Option<&str>,
) -> Entry {
    let detail = match subject {
        Some(s) if !s.is_empty() => format!("{tool} {s} ({})", caller.word()),
        _ => format!("{tool} ({})", caller.word()),
    };
    let e = Entry::now(Source::Agent, connection, database, sql).with_detail(detail);
    match refused_or_failed {
        None => e.ended(Ok(None)),
        Some(why) => e.ended_as(Outcome::Failed, Some(why.to_string())),
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
    let mut text = crate::write_log::line(entry, dialect, keep_values);
    text.push('\n');
    let path = dir.join(FILE);
    let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if rolls_over(len, text.len()) {
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
        );
        let line = crate::write_log::line(&e, SqlDialect::MySql, false);
        assert!(!line.contains("ann@example.com"), "{line}");
        assert!(line.contains("\"source\":\"agent\""), "{line}");
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
