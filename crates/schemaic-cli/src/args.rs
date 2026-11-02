//! The command line itself.
//!
//! Parsing is separate from doing, so the whole surface — defaults, aliases,
//! which flags belong to which subcommand — is testable without a database.
//! [`Cli::try_parse_from`] is what the tests drive.

use clap::{Parser, Subcommand};

use crate::format::{Format, Output};

/// How many rows a `query` returns unless asked for more.
///
/// **Deliberately small.** The GUI's cap is about what a grid can hold; this
/// one is about what a caller can sensibly receive down a pipe, and the caller
/// is very often a language model with a context window. A person who wants the
/// whole table says so with `--limit`.
pub const DEFAULT_LIMIT: usize = 200;

/// What the top-level `--help` says after the commands: the formats and the
/// exit codes, which a script's author needs and would otherwise have to find
/// in the README.
///
/// **A literal, checked against the code rather than built from it** — clap
/// takes a `&'static str` here — by `run.rs`'s
/// `the_help_lists_every_exit_code_and_no_other` and
/// `the_help_lists_every_format`, which compare it with `Exit` and
/// `Format::NAMES`.
///
/// **One `\n`-terminated literal per line**, not one literal with its line
/// breaks typed in. The columns are aligned with space runs, and a space run
/// after a real line break is what `source_gate`'s wrapped-literal gate reports
/// — rightly, everywhere but a table like this one. A spelt `\n` is that gate's
/// mark of a block whose spacing is deliberate.
const AFTER_HELP: &str = concat!(
    "Output formats (--format):\n",
    "  table     A Markdown table with a row-count footer (the default)\n",
    "  json      A JSON array of row objects\n",
    "  jsonl     One JSON object per line\n",
    "  csv       RFC 4180 CSV with a header row\n",
    "  vertical  One `name: value` record per row, like the mysql client's \\G\n",
    "--no-header leaves the column names, and the table's footer, out of table and csv.\n",
    "Rows go to stdout; everything else goes to stderr.\n",
    "\n",
    "Exit codes:\n",
    "  0  It ran\n",
    "  2  A usage error, or a connection, file or table that is not there; nothing was sent.\n",
    "     Or a statistics source that is off, or a performance read this engine cannot make\n",
    "  3  A guard refused: not a read, a read-only connection, no --yes, or no CLI access; \
     nothing was sent. Or the connection's settings or session refused it (init SQL, \
     transport, MySQL sql_mode, a read-only PostgreSQL path); your statement never ran\n",
    "  4  The server or the connection failed; retrying may work\n",
    "  5  A write timed out after it was sent and may have been applied; check before retrying\n",
    "  6  query --fail-on-cap: --limit cut the rows short (they were still printed)\n",
    "  7  import: the file has problems (listed on stderr); nothing was written",
);

#[derive(Parser, Debug, PartialEq, Eq)]
#[command(
    name = "schemaic",
    about = "Run SQL against a saved Schemaic connection, without the app.",
    long_about = "Run SQL against a saved Schemaic connection, without the app.\n\n\
                  Connections come from Schemaic's own saved list and their passwords \
                  from the OS keyring, so nothing here takes a credential on the \
                  command line. A connection is reachable only once you have turned on \
                  CLI access for it in Schemaic.",
    version,
    disable_help_subcommand = false,
    after_help = AFTER_HELP
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum Command {
    /// List the saved connections the CLI may use.
    List {
        /// Include connections that have not been granted CLI access, with
        /// their status — so an expected connection that is missing can be
        /// explained rather than just absent.
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// List the databases on a connection — the names `-d` takes.
    Databases {
        #[command(flatten)]
        conn: ConnArgs,
        #[command(flatten)]
        output: OutputArgs,
        /// Seconds before the listing is given up on.
        #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// Check that a connection answers: log in and run `SELECT 1`.
    Ping {
        #[command(flatten)]
        conn: ConnArgs,
        #[command(flatten)]
        output: OutputArgs,
        /// Seconds before the server is given up on.
        #[arg(long, default_value_t = PING_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// List the tables and views in a database.
    Tables {
        #[command(flatten)]
        target: Target,
        #[command(flatten)]
        output: OutputArgs,
        /// Maximum rows to return.
        #[arg(long, default_value_t = DEFAULT_LIMIT, value_parser = at_least_one_row())]
        limit: usize,
        /// Seconds before the listing is cancelled.
        #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// Show a table's or view's columns: type, nullability, default and key.
    Describe {
        /// The table or view. On PostgreSQL it may be schema-qualified, and is
        /// read as a query would read it: folded to lower case unless quoted.
        table: String,
        #[command(flatten)]
        target: Target,
        #[command(flatten)]
        output: OutputArgs,
        /// Seconds before the lookup is cancelled.
        #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// Rank the statements the server has spent its time on.
    ///
    /// Read from pg_stat_statements on PostgreSQL, performance_schema's
    /// statement digests on MySQL and MariaDB, and the database's Query Store
    /// on SQL Server. A source that is off is said on stderr, with the
    /// statement that turns it on — never run — and exits 2.
    Statements {
        #[command(flatten)]
        target: Target,
        #[command(flatten)]
        output: OutputArgs,
        /// total (time across every call), mean (time per call) or calls.
        #[arg(long, default_value = "total", value_parser = parse_statement_order)]
        order: schemaic_core::workload::StatementOrder,
        /// Maximum statements to return.
        #[arg(long, default_value_t = PERF_LIMIT, value_parser = perf_limit(
            schemaic_core::workload::MAX_STATEMENT_LIMIT
        ))]
        limit: usize,
        /// Seconds before the read is given up on.
        #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// List a database's unused, duplicate and prefix-redundant indexes.
    ///
    /// The note on how far the usage counts go back is on stderr. Nothing is
    /// dropped: the output is for reading.
    Indexes {
        #[command(flatten)]
        target: Target,
        #[command(flatten)]
        output: OutputArgs,
        /// List the indexes the optimiser recorded as missing instead, each
        /// with the DDL that would create it — never run. SQL Server only.
        #[arg(long)]
        missing: bool,
        /// Seconds before the read is given up on.
        #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// List a database's largest tables, or with --indexes its largest
    /// indexes.
    Sizes {
        #[command(flatten)]
        target: Target,
        #[command(flatten)]
        output: OutputArgs,
        /// The largest indexes rather than the largest tables.
        #[arg(long)]
        indexes: bool,
        /// Maximum rows to return.
        #[arg(long, default_value_t = PERF_LIMIT, value_parser = perf_limit(MAX_SIZE_ROWS))]
        limit: usize,
        /// Seconds before the read is given up on.
        #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// Show the locks sessions hold and wait for, and who blocks whom.
    Locks {
        #[command(flatten)]
        conn: ConnArgs,
        #[command(flatten)]
        output: OutputArgs,
        /// Seconds before the read is given up on.
        #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// Run a read-only statement.
    Query {
        #[command(flatten)]
        sql: SqlArgs,
        #[command(flatten)]
        params: ParamArgs,
        #[command(flatten)]
        target: Target,
        #[command(flatten)]
        output: OutputArgs,
        /// Maximum rows to return.
        #[arg(long, default_value_t = DEFAULT_LIMIT, value_parser = at_least_one_row())]
        limit: usize,
        /// Exit 6 when --limit capped the result. The rows are still printed;
        /// the exit code is the signal for a caller that does not read stderr.
        #[arg(long)]
        fail_on_cap: bool,
        /// Seconds before the statement is cancelled.
        #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// Show how the server would run a read-only statement, and what in its
    /// plan looks slow.
    ///
    /// The plan's rows go to stdout like a query's; the likely problems —
    /// full scans, sorts, temporary tables, an index the server says it
    /// lacks — go to stderr. Planned, never executed: the figures are the
    /// optimiser's estimates.
    Explain {
        #[command(flatten)]
        sql: SqlArgs,
        #[command(flatten)]
        params: ParamArgs,
        #[command(flatten)]
        target: Target,
        #[command(flatten)]
        output: OutputArgs,
        /// Seconds before the planning is cancelled.
        #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// Run a read-only statement and write its whole result to a file.
    ///
    /// Not capped by --limit: the rows stream from the server into the file.
    /// The file is built beside itself as `<file>.part` and renamed into
    /// place when complete.
    Export {
        #[command(flatten)]
        sql: SqlArgs,
        #[command(flatten)]
        params: ParamArgs,
        #[command(flatten)]
        target: Target,
        /// The file to write.
        #[arg(short = 'o', long = "output")]
        output: std::path::PathBuf,
        /// json, csv, sql, md, html or xlsx. Defaults to what the file's
        /// extension names.
        #[arg(long = "format")]
        format: Option<String>,
        /// Seconds before the export is stopped — an hour by default, since a
        /// large table can take a while.
        #[arg(long, default_value_t = EXPORT_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// Write a database's tables — structure, rows, and its types, sequences
    /// and routines — to a .sql file that recreates them.
    ///
    /// The app's Export ▸ SQL dump: freshly introspected, the rows streamed,
    /// built beside itself as `<file>.part` and renamed into place when
    /// complete.
    Dump {
        #[command(flatten)]
        target: Target,
        /// The .sql file to write.
        #[arg(short = 'o', long = "output")]
        output: std::path::PathBuf,
        /// A table to dump, by name (`orders`, or `sales.orders` outside the
        /// default schema). Repeat for more; every table and view when absent.
        #[arg(short = 't', long = "table")]
        tables: Vec<String>,
        /// The structure only — no rows.
        #[arg(long, conflicts_with = "data_only")]
        schema_only: bool,
        /// The rows only — no `CREATE`s.
        #[arg(long)]
        data_only: bool,
        /// Seconds before the dump is stopped — an hour by default.
        #[arg(long, default_value_t = EXPORT_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// Print the SQL that would make one database's structure match another's.
    ///
    /// The app's Compare with: -c/-d name the database the script would
    /// change, --source-connection/--source-database the one it should come
    /// to match. Nothing is run — the script goes to stdout for you to read,
    /// and what it drops or leaves out goes to stderr. Both sides must be the
    /// same engine.
    Compare {
        /// The saved connection holding the database the script would change.
        #[arg(short = 'c', long, env = "SCHEMAIC_CONNECTION")]
        connection: String,
        /// The database the script would change. Defaults to the
        /// connection's own.
        #[arg(short = 'd', long, env = "SCHEMAIC_DATABASE", value_parser = non_blank)]
        database: Option<String>,
        /// The connection holding the database to match. Defaults to -c's.
        #[arg(long)]
        source_connection: Option<String>,
        /// The database to match. Defaults to the source connection's own.
        #[arg(long, value_parser = non_blank)]
        source_database: Option<String>,
        /// Only this table or view, and the triggers on it (`orders`, or
        /// `sales.orders` outside the default schema). Repeat for more.
        #[arg(short = 't', long = "table")]
        tables: Vec<String>,
        /// Seconds before reading the two databases is given up on.
        #[arg(long, default_value_t = COMPARE_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// Run a statement that writes.
    Exec {
        #[command(flatten)]
        sql: SqlArgs,
        #[command(flatten)]
        params: ParamArgs,
        #[command(flatten)]
        target: Target,
        #[command(flatten)]
        output: OutputArgs,
        /// Answer the guard's question: a DELETE or UPDATE with no WHERE, a
        /// TRUNCATE, or a statement that drops a table, database or schema.
        /// With --script, the question every script is asked. It cannot unlock
        /// a read-only connection.
        #[arg(long)]
        yes: bool,
        /// Run a whole .sql script — a migration, or a `schemaic dump` to
        /// restore — on one connection, in order, stopping at the first
        /// statement the server refuses. Always needs --yes; refused if the
        /// script would switch to another database (`USE`, `\connect`).
        #[arg(long, conflicts_with = "params")]
        script: bool,
        /// Seconds before the statement is stopped — 30 by default, an hour
        /// with --script.
        #[arg(long, value_parser = at_least_one_second())]
        timeout: Option<u64>,
    },
    /// Load a CSV, TSV, JSON or Excel file into an existing table.
    ///
    /// The app's Import: the file's columns are matched to the table's by
    /// header name (by position with --no-header), the whole file is checked
    /// before anything is written, and the rows go in as one transaction. A
    /// file with any problem is refused and nothing is written (exit 7); the
    /// problems go to stderr.
    Import {
        /// The file to load.
        file: std::path::PathBuf,
        /// The table to load into (`orders`, or `sales.orders` outside the
        /// default schema).
        #[arg(short = 't', long = "table")]
        table: String,
        #[command(flatten)]
        target: Target,
        /// csv, json or xlsx. Defaults to what the file's extension names
        /// (.csv, .tsv, .txt, .json, .xlsx).
        #[arg(long = "format")]
        format: Option<String>,
        /// The file has no header row: its columns map to the table's by
        /// position.
        #[arg(long)]
        no_header: bool,
        /// The field separator for CSV — `,`, `;`, `|`, or `tab`. Guessed from
        /// the file when absent.
        #[arg(long, value_parser = parse_delimiter)]
        delimiter: Option<u8>,
        /// Which Excel sheet to read. The first when absent.
        #[arg(long)]
        sheet: Option<String>,
        /// A CSV field that means NULL. Repeat for several; an empty field
        /// is NULL when none is given.
        #[arg(long = "null", value_name = "TEXT")]
        nulls: Vec<String>,
        /// Strip the spaces around every field.
        #[arg(long)]
        trim: bool,
        /// Load a file column into a table column the names don't match:
        /// `"Full Name=name"`. Repeat for each.
        #[arg(long = "map", value_name = "FILE_COLUMN=TABLE_COLUMN", value_parser = crate::import::parse_map)]
        maps: Vec<(String, String)>,
        /// Leave a file column out. Repeat for each.
        #[arg(long = "skip", value_name = "FILE_COLUMN")]
        skips: Vec<String>,
        /// Seconds before the load is stopped and rolled back — an hour by
        /// default.
        #[arg(long, default_value_t = EXPORT_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// Serve a connection to an AI agent over MCP, on stdin/stdout.
    ///
    /// For an MCP client to spawn, not for a terminal: the agent gets
    /// read-only tools over the connections it is given — list_databases,
    /// list_schema, describe_table, search_objects and compare_schemas, and
    /// run_query, explain and sample_rows when a connection's AI data access is
    /// "Let it read data". Runs until the client closes stdin.
    Mcp {
        #[command(flatten)]
        target: McpTarget,
    },
    /// Print the version — the same line as `--version`.
    Version,
    /// Print a shell completion script for bash, zsh, fish, elvish or
    /// powershell.
    ///
    /// Commands and flags only: saved connection names are not completed.
    /// For bash, `schemaic completions bash > ~/.local/share/bash-completion/completions/schemaic`;
    /// for PowerShell, `schemaic completions powershell | Out-String | Invoke-Expression`
    /// in your profile.
    Completions {
        /// The shell to write the script for.
        shell: clap_complete::Shell,
    },
}

/// The completion script for `shell`, generated from the parser itself — so
/// a flag added to [`Cli`] is completed without being listed a second time.
pub fn completion_script(shell: clap_complete::Shell) -> String {
    let mut cmd = <Cli as clap::CommandFactory>::command();
    let mut out = Vec::new();
    clap_complete::generate(shell, &mut cmd, "schemaic", &mut out);
    // clap_complete writes the script as text; it is ASCII in every shell's
    // generator, and the lossy decode is only there so a future change to that
    // can never panic the CLI.
    String::from_utf8_lossy(&out).into_owned()
}

/// [`Target`] for `mcp`: a connection and a database, and **no
/// `--password-stdin`** — stdin is the MCP client's JSON-RPC stream, so a
/// password read from it would be the client's first request.
#[derive(clap::Args, Debug, Default, PartialEq, Eq)]
pub struct McpTarget {
    /// Saved connection, by id or by name — or `#<id>`, which is only ever
    /// the id. Repeat to serve several: every tool then takes a `connection`
    /// argument, and the first is the default.
    #[arg(
        id = "connection",
        short = 'c',
        long = "connection",
        env = "SCHEMAIC_CONNECTION",
        required_unless_present = "url"
    )]
    pub connections: Vec<String>,
    /// A connection given here rather than saved, served after any `-c`s —
    /// see `schemaic query --help`. It may not carry a password: that comes
    /// from SCHEMAIC_PASSWORD, stdin being the protocol's.
    ///
    /// **No `SCHEMAIC_URL` here**, unlike the other commands: `mcp` serves a
    /// `--url` *beside* its `-c`s rather than instead of one, so an exported
    /// variable would quietly add a connection to the agent's server. An MCP
    /// client's config spells its arguments out anyway.
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    /// With --url: what the agent may read — schema-only (the default),
    /// on-request (no rows, as there is nothing to attach them from here) or
    /// full (it may run read-only queries and read sample rows).
    #[arg(long, value_parser = parse_ai_data)]
    pub ai_data: Option<schemaic_core::connection::AiData>,
    /// The database the tools default to — the first connection's. Defaults
    /// to the connection's own; `schemaic databases` lists the names.
    #[arg(short = 'd', long, env = "SCHEMAIC_DATABASE", value_parser = non_blank)]
    pub database: Option<String>,
}

/// `--ai-data`'s parser: the three levels, in words a command line can hold.
fn parse_ai_data(s: &str) -> Result<schemaic_core::connection::AiData, String> {
    use schemaic_core::connection::AiData;
    match s {
        "schema-only" => Ok(AiData::SchemaOnly),
        "on-request" => Ok(AiData::OnRequest),
        "full" => Ok(AiData::Full),
        _ => Err(format!(
            "'{s}' is not a level; use schema-only, on-request or full"
        )),
    }
}

/// What `schemaic version` prints: clap's own `--version` text, so the
/// subcommand and the flag are one answer rather than two that agree today.
pub fn version_text() -> String {
    <Cli as clap::CommandFactory>::command()
        .render_version()
        .to_string()
}

/// [`crate::query::DEFAULT_TIMEOUT`] in the unit `--timeout` takes.
const DEFAULT_TIMEOUT_SECS: u64 = crate::query::DEFAULT_TIMEOUT.as_secs();

/// `export`'s default `--timeout`: an hour, a whole table's worth of time.
const EXPORT_TIMEOUT_SECS: u64 = 3600;

/// `compare`'s default `--timeout`: two whole catalogues are read, which on a
/// large schema takes longer than one statement's thirty seconds.
pub(crate) const COMPARE_TIMEOUT_SECS: u64 = 120;

/// `exec`'s `--timeout`, or its default: a statement's thirty seconds, or for
/// a script [`EXPORT_TIMEOUT_SECS`] — a dump being restored is a whole
/// database's worth of rows, and thirty seconds would stop it a few tables in.
pub fn exec_timeout(script: bool, timeout: Option<u64>) -> u64 {
    timeout.unwrap_or(if script {
        EXPORT_TIMEOUT_SECS
    } else {
        DEFAULT_TIMEOUT_SECS
    })
}

/// `ping`'s default: the app's own answer to "the server is not responding",
/// the five seconds after which its health check says Disconnected — so the
/// two agree on a dead host.
const PING_TIMEOUT_SECS: u64 = schemaic_db::PING_TIMEOUT.as_secs();

/// The statement argument that means "read it from stdin".
pub const SQL_FROM_STDIN: &str = "-";

/// How stdout is written — every subcommand that prints rows takes these.
#[derive(clap::Args, Debug, PartialEq, Eq)]
pub struct OutputArgs {
    /// table, json, jsonl, csv or vertical — `schemaic --help` says what each
    /// is.
    #[arg(long, default_value = "table")]
    pub format: Format,
    /// Leave out the column names — for `table`, the footer too — so the
    /// output is the rows alone. `table` and `csv` only.
    #[arg(long)]
    pub no_header: bool,
}

impl OutputArgs {
    /// The two flags as one [`Output`], or why they do not go together.
    pub fn output(&self) -> Result<Output, String> {
        Output::new(self.format, self.no_header)
    }
}

impl Command {
    /// The output flags, for the subcommands that print rows — so they can be
    /// judged before anything else is read.
    pub fn output_args(&self) -> Option<&OutputArgs> {
        match self {
            Command::List { output, .. }
            | Command::Databases { output, .. }
            | Command::Ping { output, .. }
            | Command::Tables { output, .. }
            | Command::Describe { output, .. }
            | Command::Statements { output, .. }
            | Command::Indexes { output, .. }
            | Command::Sizes { output, .. }
            | Command::Locks { output, .. }
            | Command::Query { output, .. }
            | Command::Explain { output, .. }
            | Command::Exec { output, .. } => Some(output),
            // `export` writes its rows to a file, in its own `--format`.
            // `compare` prints a script, not rows.
            Command::Export { .. }
            | Command::Dump { .. }
            | Command::Compare { .. }
            | Command::Import { .. }
            | Command::Mcp { .. }
            | Command::Version
            | Command::Completions { .. } => None,
        }
    }
}

/// Where the statement comes from: the argument, stdin, or a file — exactly
/// one of them, which the group makes a parse error rather than a precedence
/// rule.
#[derive(clap::Args, Debug, PartialEq, Eq)]
#[group(required = true, multiple = false)]
pub struct SqlArgs {
    /// The SQL to run: one statement. `-` reads it from stdin — the place for
    /// one that carries a password, which on the command line lands in the
    /// process list and the shell's history.
    pub sql: Option<String>,
    /// Read the statement from a file instead (`-` is stdin). Still one
    /// statement, unless `exec --script` was asked for.
    #[arg(short = 'f', long, value_name = "PATH")]
    pub file: Option<std::path::PathBuf>,
}

/// Values for the statement's `:name` placeholders — the app's parameters
/// bar, on the command line.
///
/// **Apart from [`SqlArgs`]**, whose group admits exactly one member: a `-p`
/// inside it would make "a statement and a parameter" a parse error.
#[derive(clap::Args, Debug, Default, PartialEq, Eq)]
pub struct ParamArgs {
    /// Set a `:name` placeholder: `name=text`, `name:number=42`, `name:null`,
    /// or `name:sql=created_at` for SQL put in as written (a column for
    /// `ORDER BY :col`). Repeat for each. Text is quoted for the engine; the
    /// guard reads the statement with the values in it.
    #[arg(
        short = 'p',
        long = "param",
        value_name = "NAME[:KIND]=VALUE",
        value_parser = parse_param
    )]
    pub params: Vec<schemaic_core::params::Binding>,
}

/// One `-p`: `NAME[:KIND]=VALUE`, or `NAME:null`.
///
/// **The kind is spelled before the `=`, never guessed from the value.** `42`
/// is as often a zip code or an id kept as text as it is a number, and a
/// guess that emitted it bare would change what a `WHERE` matches. Text is
/// the default because it is the kind that cannot carry SQL: it is quoted.
///
/// A leading `:` on the name is accepted, since that is how the statement
/// spells it.
pub fn parse_param(s: &str) -> Result<schemaic_core::params::Binding, String> {
    use schemaic_core::params::{Binding, ParamKind, ParamValue};
    use schemaic_core::sql::{is_word_byte, is_word_start};
    let (head, value) = match s.split_once('=') {
        Some((h, v)) => (h, Some(v)),
        None => (s, None),
    };
    let head = head.strip_prefix(':').unwrap_or(head);
    let (name, kind) = match head.split_once(':') {
        Some((n, k)) => (n, Some(k)),
        None => (head, None),
    };
    let named = name.as_bytes().first().is_some_and(|&b| is_word_start(b))
        && name.bytes().all(is_word_byte);
    if !named {
        return Err(format!(
            "'{name}' is not a parameter name; write it as the statement does, \
             without the colon: -p id=42"
        ));
    }
    let kind = match kind {
        None | Some("text") => ParamKind::Text,
        Some("number") => ParamKind::Number,
        Some("null") => ParamKind::Null,
        Some("sql") => ParamKind::Raw,
        Some(other) => {
            return Err(format!(
                "unknown kind '{other}'; use text, number, null or sql"
            ));
        }
    };
    let value = match (kind, value) {
        (ParamKind::Null, None) => ParamValue::Null,
        (ParamKind::Null, Some(_)) => {
            return Err(format!("{name}:null takes no value; drop the '='"));
        }
        (_, None) => {
            return Err(format!(
                "no value for '{name}'; write {name}=<value>, or {name}:null"
            ));
        }
        (kind, Some(v)) => ParamValue::of(kind, v),
    };
    Ok(Binding {
        name: name.to_string(),
        value: Some(value),
    })
}

/// [`SqlArgs`], resolved to the one place it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlSource<'a> {
    Text(&'a str),
    Stdin,
    File(&'a std::path::Path),
}

impl SqlArgs {
    /// Which source was given. `-` means stdin as the argument and as the
    /// file alike.
    pub fn source(&self) -> SqlSource<'_> {
        match (&self.sql, &self.file) {
            (_, Some(f)) if f.as_os_str() == SQL_FROM_STDIN => SqlSource::Stdin,
            (_, Some(f)) => SqlSource::File(f),
            (Some(s), None) if s == SQL_FROM_STDIN => SqlSource::Stdin,
            (Some(s), None) => SqlSource::Text(s),
            // The group requires one; an empty statement is what the guard
            // already refuses, so this is not a second place to decide it.
            (None, None) => SqlSource::Text(""),
        }
    }
}

/// Which connection, and which database on it.
#[derive(clap::Args, Debug, PartialEq, Eq)]
pub struct Target {
    #[command(flatten)]
    pub conn: ConnArgs,
    /// Database to run in. Defaults to the connection's own; `schemaic
    /// databases` lists the names.
    #[arg(short = 'd', long, env = "SCHEMAIC_DATABASE", value_parser = non_blank)]
    pub database: Option<String>,
}

/// Which connection, and how to authenticate to it — [`Target`] without the
/// database, for the one subcommand that has none to run in.
#[derive(clap::Args, Debug, Default, PartialEq, Eq)]
pub struct ConnArgs {
    /// Saved connection, by id or by name — or `#<id>`, which is only ever
    /// the id.
    #[arg(
        short = 'c',
        long,
        env = "SCHEMAIC_CONNECTION",
        required_unless_present = "url",
        conflicts_with = "url"
    )]
    pub connection: Option<String>,
    /// A connection given here rather than saved — `postgresql://user@host/db`,
    /// `mysql://…`, `sqlserver://…` or `sqlite:path` — for a container or CI
    /// job with no Schemaic setup. Never saved. It may not carry a password:
    /// give that with --password-stdin or SCHEMAIC_PASSWORD.
    #[arg(long, env = "SCHEMAIC_URL", value_name = "URL")]
    pub url: Option<String>,
    /// With --url: refuse writes on it, as a saved connection marked read-only
    /// does.
    #[arg(long)]
    pub read_only: bool,
    /// Read the connection's password from stdin instead of the OS keyring.
    ///
    /// **For the headless case the keyring cannot serve.** Linux's Secret
    /// Service needs an unlocked desktop collection, which an SSH session or a
    /// container does not have; without this the CLI would simply be unusable
    /// there.
    #[arg(long)]
    pub password_stdin: bool,
    /// Is stdin the protocol's — `mcp`'s, the MCP client's JSON-RPC stream —
    /// so that a password cannot come from it, and no message may offer
    /// `--password-stdin`. Never a flag: set by the command that builds these
    /// arguments by hand.
    #[arg(skip)]
    pub stdin_is_protocol: bool,
}

/// Which connection [`ConnArgs`] names: a saved one, or a one-off URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnSource<'a> {
    Saved(&'a str),
    Url(&'a str),
}

impl ConnArgs {
    /// The connection asked for, or why the command line names none — or two.
    ///
    /// **Both is refused, not ranked.** Either may arrive from the
    /// environment (`SCHEMAIC_CONNECTION`, `SCHEMAIC_URL`), and an exported
    /// variable quietly beating a typed flag — or the other way round — is a
    /// write landing on a connection the person at the keyboard did not name.
    pub fn source(&self) -> Result<ConnSource<'_>, String> {
        match (self.connection.as_deref(), self.url.as_deref()) {
            // A saved connection's read-only flag is the app's to set; a CLI
            // flag beside it would be a second answer to the same question.
            (Some(_), None) if self.read_only => Err(
                "--read-only is for a --url connection; a saved one is made read-only \
                 in Schemaic"
                    .to_string(),
            ),
            (Some(name), None) => Ok(ConnSource::Saved(name)),
            (None, Some(url)) => Ok(ConnSource::Url(url)),
            (None, None) => Err(
                "no connection: name a saved one with -c (or SCHEMAIC_CONNECTION), \
                 or give one with --url (or SCHEMAIC_URL)"
                    .to_string(),
            ),
            (Some(_), Some(_)) => Err(
                "both a saved connection (-c or SCHEMAIC_CONNECTION) and a --url \
                 (or SCHEMAIC_URL) were given; give one"
                    .to_string(),
            ),
        }
    }
}

/// `--database`'s parser: a name, never a blank.
///
/// A blank is what `-d "$DB"` sends with `DB` unset, and it is not "no flag":
/// taken as a name, PostgreSQL connects to the database named after the user,
/// which is the unscoped landing the exec guard's "no database selected" arm is
/// there to stop — reached without the guard ever seeing it.
///
/// **`SCHEMAIC_DATABASE` set but empty goes through here too**, and is refused
/// the same way rather than read as unset: it is the same `"$DB"` with `DB`
/// unset, one step removed. The message names both sources, since the reader
/// may not have typed a `-d` at all.
fn non_blank(s: &str) -> Result<String, String> {
    if s.trim().is_empty() {
        Err(
            "the database name is empty; leave out -d, and unset SCHEMAIC_DATABASE, \
             to use the connection's own"
                .to_string(),
        )
    } else {
        Ok(s.to_string())
    }
}

/// `import --delimiter`'s parser: one ASCII character, or `tab` — a literal
/// tab is the one separator a shell makes awkward to type.
fn parse_delimiter(s: &str) -> Result<u8, String> {
    match s {
        "tab" | "\\t" | "\t" => Ok(b'\t'),
        _ if s.len() == 1 && s.is_ascii() && s != "\"" => Ok(s.as_bytes()[0]),
        _ => Err(format!(
            "'{s}' is not a delimiter; give one character, or `tab`"
        )),
    }
}

/// `--limit`'s parser: zero rows is a typo, not a request.
fn at_least_one_row() -> clap::builder::RangedU64ValueParser<usize> {
    clap::builder::RangedU64ValueParser::<usize>::new().range(1..)
}

/// How many rows `statements` and `sizes` return unless asked for more — the
/// MCP tools' default, so a person and an agent see the same list.
pub const PERF_LIMIT: usize = 20;

/// The most `sizes` returns: past this the list is the catalogue, which
/// `tables` already is. The MCP `table_sizes` tool's ceiling.
pub const MAX_SIZE_ROWS: usize = 100;

/// A performance command's `--limit`: `1..=max`, **refused above it rather
/// than clamped**, so asking for more than there can be says so.
fn perf_limit(max: usize) -> clap::builder::RangedU64ValueParser<usize> {
    clap::builder::RangedU64ValueParser::<usize>::new().range(1..=max as u64)
}

/// `statements --order`'s parser, over the names core's
/// [`StatementOrder::parse`](schemaic_core::workload::StatementOrder::parse)
/// takes — the MCP tool's `order` argument reads the same ones.
fn parse_statement_order(s: &str) -> Result<schemaic_core::workload::StatementOrder, String> {
    schemaic_core::workload::StatementOrder::parse(s)
        .ok_or_else(|| format!("'{s}' is not an order; use total, mean or calls"))
}

/// `--timeout`'s parser: zero seconds cancels every statement before it runs.
fn at_least_one_second() -> clap::builder::RangedU64ValueParser<u64> {
    clap::builder::RangedU64ValueParser::<u64>::new().range(1..)
}

/// Move SQL that opens with a `--` line comment behind a `--` separator, so
/// the parser takes it as the statement rather than as a long option.
///
/// **A saved snippet very often starts with a comment line**, and clap read
/// `$'-- note\nSELECT 1'` as the option `-- note…` and exited 2 before
/// anything reached a server, while the same text piped on stdin ran. Two
/// things tell them apart, and either is enough: whitespace right after the
/// `--` (no option is spelled that way — `--format`, `--connection=Prod EU` —
/// and a MySQL line comment always is), or a line break anywhere in the token,
/// which no option spelling has either and which catches PostgreSQL's and
/// SQLite's `--TODO` and a `-----` banner. Such a token before any `--` of the
/// user's own goes after one; everything else keeps its place, so a command
/// line with no comment-led token comes back unchanged.
pub fn comment_led_sql_last(argv: Vec<std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    let comment_led = |a: &std::ffi::OsString| {
        a.to_str().is_some_and(|s| {
            s.strip_prefix("--").is_some_and(|rest| {
                rest.chars().next().is_some_and(char::is_whitespace) || rest.contains(['\n', '\r'])
            })
        })
    };
    let separator = argv.iter().position(|a| a == "--");
    let before_separator = |i: usize| separator.is_none_or(|s| i < s);
    let (mut kept, mut moved) = (Vec::new(), Vec::new());
    for (i, a) in argv.into_iter().enumerate() {
        // `i > 0`: the program's own name is never a statement.
        if i > 0 && before_separator(i) && comment_led(&a) {
            moved.push(a);
        } else {
            kept.push(a);
        }
    }
    if !moved.is_empty() {
        if separator.is_none() {
            kept.push("--".into());
        }
        kept.extend(moved);
    }
    kept
}

/// The command line as the CLI reads it: [`comment_led_sql_last`], then
/// clap. `run::main`'s whole front, apart so a test can drive it — a fix that
/// lives in the composition is only pinned by a test of the composition.
pub fn parse_argv(argv: Vec<std::ffi::OsString>) -> Result<Cli, clap::Error> {
    use clap::CommandFactory as _;
    parse_with(Cli::command(), argv)
}

/// [`parse_argv`] over a given `cmd` — the tests' way to point an argument's
/// environment variable at a name no other test reads.
fn parse_with(mut cmd: clap::Command, argv: Vec<std::ffi::OsString>) -> Result<Cli, clap::Error> {
    use clap::FromArgMatches as _;
    let mut matches = cmd.try_get_matches_from_mut(comment_led_sql_last(argv))?;
    refuse_an_exported_connection_beside_a_url(&mut cmd, &matches)?;
    Cli::from_arg_matches_mut(&mut matches).map_err(|e| e.format(&mut cmd))
}

/// **`mcp --url` with `SCHEMAIC_CONNECTION` exported is refused.** `mcp`
/// serves its `-c`s *and* a `--url`, so the variable did not give way to the
/// URL as it does nowhere else ([`ConnArgs::source`] refuses the mix): it
/// became the server's **first, default** connection, `-d` landing on it, and
/// every tool call naming none read the developer's own connection while the
/// person believed the agent was on the URL's. A typed `-c` beside `--url`
/// is still both, as asked.
fn refuse_an_exported_connection_beside_a_url(
    cmd: &mut clap::Command,
    matches: &clap::ArgMatches,
) -> Result<(), clap::Error> {
    let Some(("mcp", m)) = matches.subcommand() else {
        return Ok(());
    };
    let exported = m.value_source("connection") == Some(clap::parser::ValueSource::EnvVariable);
    if exported && m.contains_id("url") && m.get_one::<String>("url").is_some() {
        return Err(cmd.error(
            clap::error::ErrorKind::ArgumentConflict,
            "--url is served beside the connection SCHEMAIC_CONNECTION names, as the \
             default; unset SCHEMAIC_CONNECTION, or name it with -c to serve both",
        ));
    }
    Ok(())
}

/// Does this argv mean the CLI rather than the app?
///
/// **An allowlist of the first argument, never "are there any arguments".**
/// This binary is re-invoked with argv by things that are not the CLI: the
/// Velopack installer and updater (`--veloapp-install`, `--veloapp-updated`,
/// `--veloapp-obsolete`, `--veloapp-uninstall`, `--veloapp-firstrun`), and the
/// AI panel (`--mcp-serve`, `--endpoint-file`). A predicate that routed on "has
/// arguments" would hand those to the argument parser, which would reject them
/// and exit — breaking an install, an update and the assistant, each in a way
/// that looks nothing like a CLI bug.
///
/// So a new flag on the app is safe by default: it has to be added here to
/// reach the CLI.
pub fn wants_cli(argv: &[String]) -> bool {
    matches!(
        argv.get(1).map(String::as_str),
        Some(
            "list"
                | "databases"
                | "ping"
                | "tables"
                | "describe"
                | "statements"
                | "indexes"
                | "sizes"
                | "locks"
                | "query"
                | "explain"
                | "export"
                | "dump"
                | "compare"
                | "exec"
                | "import"
                | "mcp"
                | "version"
                | "completions"
                | "help"
                | "--help"
                | "-h"
                | "--version"
                | "-V"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(args)
    }

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    /// Every subcommand clap knows, read off the parser rather than listed
    /// here — so a new one that [`wants_cli`] was not told about fails this,
    /// instead of reaching the GUI as an unknown flag.
    fn subcommand_names() -> Vec<String> {
        <Cli as clap::CommandFactory>::command()
            .get_subcommands()
            .map(|s| s.get_name().to_string())
            .chain(["help".to_string()])
            .collect()
    }

    #[test]
    fn every_subcommand_routes_to_the_cli() {
        let names = subcommand_names();
        assert!(names.len() > 5, "{names:?}");
        for name in &names {
            assert!(
                wants_cli(&argv(&["schemaic", name])),
                "`{name}` must reach the CLI"
            );
        }
    }

    #[test]
    fn the_help_and_version_flags_route_to_the_cli() {
        for flag in ["--help", "-h", "--version", "-V"] {
            assert!(wants_cli(&argv(&["schemaic", flag])), "{flag}");
        }
    }

    /// **The app's own re-invocations must not be routed.** Each of these is
    /// something else handing this binary argv, and sending any of them to the
    /// argument parser breaks an install, an update or the AI panel.
    #[test]
    fn the_apps_own_flags_are_not_cli_invocations() {
        for flag in [
            "--mcp-serve",
            "--endpoint-file",
            "--veloapp-install",
            "--veloapp-updated",
            "--veloapp-obsolete",
            "--veloapp-uninstall",
            "--veloapp-firstrun",
        ] {
            assert!(
                !wants_cli(&argv(&["schemaic", flag])),
                "`{flag}` belongs to the app, not the CLI"
            );
        }
    }

    fn bound(
        name: &str,
        value: schemaic_core::params::ParamValue,
    ) -> schemaic_core::params::Binding {
        schemaic_core::params::Binding {
            name: name.to_string(),
            value: Some(value),
        }
    }

    #[test]
    fn a_param_is_text_unless_its_kind_says_otherwise() {
        use schemaic_core::params::ParamValue;
        assert_eq!(
            parse_param("id=42"),
            Ok(bound("id", ParamValue::Text("42".into())))
        );
        assert_eq!(
            parse_param("id:text=42"),
            Ok(bound("id", ParamValue::Text("42".into())))
        );
        assert_eq!(
            parse_param("id:number=42"),
            Ok(bound("id", ParamValue::Number("42".into())))
        );
        assert_eq!(
            parse_param("gone:null"),
            Ok(bound("gone", ParamValue::Null))
        );
        assert_eq!(
            parse_param("col:sql=created_at DESC"),
            Ok(bound("col", ParamValue::Raw("created_at DESC".into())))
        );
    }

    /// Only the first `=` splits: the rest is the value, as typed — a URL's
    /// query string, an expression, an empty string.
    #[test]
    fn a_params_value_keeps_every_later_equals_and_may_be_empty() {
        use schemaic_core::params::ParamValue;
        assert_eq!(
            parse_param("q=a=b&c=d"),
            Ok(bound("q", ParamValue::Text("a=b&c=d".into())))
        );
        assert_eq!(
            parse_param("e:sql=x = 1"),
            Ok(bound("e", ParamValue::Raw("x = 1".into())))
        );
        assert_eq!(
            parse_param("s="),
            Ok(bound("s", ParamValue::Text(String::new())))
        );
    }

    #[test]
    fn a_params_name_may_carry_the_statements_colon() {
        use schemaic_core::params::ParamValue;
        assert_eq!(
            parse_param(":id=7"),
            Ok(bound("id", ParamValue::Text("7".into())))
        );
    }

    #[test]
    fn a_malformed_param_is_refused() {
        for bad in [
            "id",         // no value and not null
            "id:number",  // a kind with no value
            "id:null=x",  // null with a value
            "id:float=1", // no such kind
            "=1",         // no name
            "1id=1",      // not a name the statement could spell
            "a b=1",      // nor this
            ":=1",        // a colon and nothing else
        ] {
            assert!(parse_param(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn each_statement_command_takes_repeated_params() {
        use schemaic_core::params::ParamValue;
        for sub in ["query", "explain", "exec", "export"] {
            let mut line = vec![
                "schemaic",
                sub,
                "SELECT :a, :b",
                "-c",
                "x",
                "-p",
                "a=1",
                "--param",
                "b:null",
            ];
            if sub == "export" {
                line.extend(["-o", "out.csv"]);
            }
            let params = match parse(&line)
                .unwrap_or_else(|e| panic!("{sub}: {e}"))
                .command
            {
                Command::Query { params, .. }
                | Command::Explain { params, .. }
                | Command::Exec { params, .. }
                | Command::Export { params, .. } => params.params,
                other => panic!("{other:?}"),
            };
            assert_eq!(
                params,
                vec![
                    bound("a", ParamValue::Text("1".into())),
                    bound("b", ParamValue::Null)
                ],
                "{sub}"
            );
        }
    }

    #[test]
    fn import_takes_a_file_a_table_and_its_reading_flags() {
        let cli = parse(&[
            "schemaic",
            "import",
            "p.csv",
            "-t",
            "people",
            "-c",
            "1",
            "--delimiter",
            "tab",
            "--null",
            "NULL",
            "--null",
            "\\N",
            "--map",
            "Full Name=name",
            "--skip",
            "x",
            "--no-header",
        ])
        .unwrap();
        let Command::Import {
            file,
            table,
            delimiter,
            nulls,
            maps,
            skips,
            no_header,
            timeout,
            ..
        } = cli.command
        else {
            panic!("expected an import");
        };
        assert_eq!(file, std::path::PathBuf::from("p.csv"));
        assert_eq!(table, "people");
        assert_eq!(delimiter, Some(b'\t'));
        assert_eq!(nulls, vec!["NULL".to_string(), "\\N".to_string()]);
        assert_eq!(maps, vec![("Full Name".to_string(), "name".to_string())]);
        assert_eq!(skips, vec!["x".to_string()]);
        assert!(no_header);
        assert_eq!(timeout, EXPORT_TIMEOUT_SECS);
        assert!(
            parse(&["schemaic", "import", "p.csv", "-c", "1"]).is_err(),
            "no table"
        );
    }

    /// `--read-only` belongs to a `--url`: a saved connection's flag is the
    /// app's to set, and a CLI flag that loosened or tightened it would be a
    /// second answer.
    #[test]
    fn read_only_needs_a_url() {
        let saved = parse(&["schemaic", "query", "SELECT 1", "-c", "x", "--read-only"]).unwrap();
        let Command::Query { target, .. } = saved.command else {
            panic!("expected a query");
        };
        assert!(target.conn.source().is_err());
        let cli = parse(&[
            "schemaic",
            "query",
            "SELECT 1",
            "--url",
            "mysql://u@h/d",
            "--read-only",
        ])
        .unwrap();
        let Command::Query { target, .. } = cli.command else {
            panic!("expected a query");
        };
        assert_eq!(target.conn.source(), Ok(ConnSource::Url("mysql://u@h/d")));
        assert!(target.conn.read_only);
    }

    /// `mcp` takes a `--url` in place of, or beside, its `-c`s, and its data
    /// level by name.
    #[test]
    fn mcp_takes_a_url_and_its_data_level() {
        use schemaic_core::connection::AiData;
        let Command::Mcp { target } = parse(&[
            "schemaic",
            "mcp",
            "--url",
            "mysql://u@h/d",
            "--ai-data",
            "full",
        ])
        .unwrap()
        .command
        else {
            panic!("expected mcp");
        };
        assert!(target.connections.is_empty());
        assert_eq!(target.url.as_deref(), Some("mysql://u@h/d"));
        assert_eq!(target.ai_data, Some(AiData::Full));
        assert_eq!(parse_ai_data("schema-only"), Ok(AiData::SchemaOnly));
        assert_eq!(parse_ai_data("on-request"), Ok(AiData::OnRequest));
        assert!(parse_ai_data("everything").is_err());
        assert!(parse(&["schemaic", "mcp"]).is_err(), "neither -c nor --url");
    }

    #[test]
    fn a_delimiter_is_one_character_or_tab() {
        assert_eq!(parse_delimiter(";"), Ok(b';'));
        assert_eq!(parse_delimiter("tab"), Ok(b'\t'));
        assert_eq!(parse_delimiter("\\t"), Ok(b'\t'));
        for bad in ["", ";;", "\"", "é"] {
            assert!(parse_delimiter(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn completions_take_each_shell_by_its_name() {
        for (name, shell) in [
            ("bash", clap_complete::Shell::Bash),
            ("zsh", clap_complete::Shell::Zsh),
            ("fish", clap_complete::Shell::Fish),
            ("elvish", clap_complete::Shell::Elvish),
            ("powershell", clap_complete::Shell::PowerShell),
        ] {
            assert_eq!(
                parse(&["schemaic", "completions", name]).unwrap().command,
                Command::Completions { shell },
                "{name}"
            );
        }
        assert!(parse(&["schemaic", "completions"]).is_err());
        assert!(parse(&["schemaic", "completions", "cmd"]).is_err());
    }

    /// Generated from the parser, so every subcommand is in every script —
    /// a new one cannot be left out of completion the way it could be left
    /// out of a hand-written list.
    #[test]
    fn every_completion_script_names_every_subcommand() {
        let names: Vec<String> = <Cli as clap::CommandFactory>::command()
            .get_subcommands()
            .map(|s| s.get_name().to_string())
            .collect();
        for shell in [
            clap_complete::Shell::Bash,
            clap_complete::Shell::Zsh,
            clap_complete::Shell::Fish,
            clap_complete::Shell::Elvish,
            clap_complete::Shell::PowerShell,
        ] {
            let script = completion_script(shell);
            for name in &names {
                assert!(script.contains(name.as_str()), "{shell}: {name}");
            }
            // Not `--connection`: fish spells a long option `-l connection`.
            assert!(script.contains("connection"), "{shell}");
        }
    }

    /// A bare launch is the GUI, which is the overwhelmingly common case.
    #[test]
    fn a_bare_launch_is_not_a_cli_invocation() {
        assert!(!wants_cli(&argv(&["schemaic"])));
        assert!(!wants_cli(&[]));
    }

    /// A subcommand name has to be *first*. `schemaic --mcp-serve list` is the
    /// app being asked to serve, not a listing.
    #[test]
    fn a_subcommand_further_along_does_not_route() {
        assert!(!wants_cli(&argv(&["schemaic", "--mcp-serve", "list"])));
    }

    fn os(args: &[&str]) -> Vec<std::ffi::OsString> {
        args.iter().map(Into::into).collect()
    }

    fn parse_os(args: Vec<std::ffi::OsString>) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(args)
    }

    /// **SQL that opens with a `--` comment is the statement, not a flag.** A
    /// saved snippet very often starts with a comment line, and clap read
    /// `$'-- note\nSELECT 1'` as an unknown long option and exited 2 before
    /// anything reached a server — while the same text piped on stdin worked.
    ///
    /// Driven through [`parse_argv`], the front `run::main` parses with, so
    /// the composition is what is pinned — the pure half alone was green with
    /// its one call site deleted. And not only `--` plus whitespace: on
    /// PostgreSQL and SQLite `--TODO` is a comment too, and so is a `-----`
    /// banner; a token with a line break in it is no option spelling either.
    #[test]
    fn sql_led_by_a_line_comment_is_the_statement() {
        for sub in ["query", "explain", "exec"] {
            for sql in [
                "-- note\nSELECT 1",
                "--\tnote\nSELECT 1",
                "-- only a comment",
                "--TODO tidy\nSELECT 1",
                "-----\nSELECT 1",
                "--x\r\nSELECT 1",
            ] {
                let argv = os(&["schemaic", sub, sql, "-c", "prod", "--format", "json"]);
                let cli = parse_argv(argv).unwrap_or_else(|e| panic!("{sub} {sql:?}: {e}"));
                let (got, target, output) = match cli.command {
                    Command::Query {
                        sql,
                        target,
                        output,
                        ..
                    }
                    | Command::Explain {
                        sql,
                        target,
                        output,
                        ..
                    }
                    | Command::Exec {
                        sql,
                        target,
                        output,
                        ..
                    } => (sql, target, output),
                    other => panic!("{other:?}"),
                };
                assert_eq!(got.source(), SqlSource::Text(sql));
                assert_eq!(target.conn.connection.as_deref(), Some("prod"));
                assert_eq!(
                    output.format,
                    Format::Json,
                    "the flags after it still apply"
                );
            }
        }
    }

    /// Only a `--` **followed by whitespace** is moved: no flag is spelled that
    /// way, and every other token — a real flag, a `--name=value` whose value
    /// has a space, the bare separator — is the command line's as it was.
    #[test]
    fn nothing_but_a_comment_led_token_is_moved() {
        let argv = os(&[
            "schemaic",
            "query",
            "SELECT 1",
            "--connection=Prod EU",
            "--format",
            "csv",
        ]);
        assert_eq!(comment_led_sql_last(argv.clone()), argv);
        // After an explicit `--` a token is already the statement.
        let argv = os(&["schemaic", "query", "-c", "1", "--", "-- note\nSELECT 1"]);
        assert_eq!(comment_led_sql_last(argv.clone()), argv);
        assert!(parse_os(argv).is_ok());
        // A misspelt flag is still clap's to refuse, not a statement.
        let argv = os(&["schemaic", "query", "SELECT 1", "--formt", "csv"]);
        assert_eq!(comment_led_sql_last(argv.clone()), argv);
    }

    fn sql_of(cli: Cli) -> SqlArgs {
        match cli.command {
            Command::Query { sql, .. }
            | Command::Explain { sql, .. }
            | Command::Exec { sql, .. } => sql,
            other => panic!("{other:?}"),
        }
    }

    /// **`explain` takes its statement the way `query` does** — argument,
    /// `-f` or stdin, in a database, printed in any `--format` — and nothing
    /// a read does not take: no `--yes`, no `--limit`, and no `--analyze`,
    /// since a headless plan never runs the statement.
    #[test]
    fn explain_takes_a_statement_a_target_and_the_output_flags() {
        let cli = parse(&[
            "schemaic", "explain", "SELECT 1", "-c", "prod", "-d", "shop", "--format", "json",
        ])
        .unwrap();
        let Command::Explain {
            sql,
            params: _,
            target,
            output,
            timeout,
        } = cli.command
        else {
            panic!("expected an explain");
        };
        assert_eq!(sql.source(), SqlSource::Text("SELECT 1"));
        assert_eq!(target.conn.connection.as_deref(), Some("prod"));
        assert_eq!(target.database.as_deref(), Some("shop"));
        assert_eq!(output.format, Format::Json);
        assert_eq!(timeout, crate::query::DEFAULT_TIMEOUT.as_secs());
        for flag in ["--yes", "--limit", "--analyze"] {
            let err = parse(&["schemaic", "explain", "SELECT 1", "-c", "1", flag]).unwrap_err();
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::UnknownArgument,
                "{flag}"
            );
        }
        assert!(
            parse(&["schemaic", "explain", "SELECT 1", "-c", "1"])
                .unwrap()
                .command
                .output_args()
                .is_some()
        );
    }

    /// **`-f` is a third place for the statement, beside the argument and
    /// stdin**, on both subcommands that take one — and `-f -` is stdin, the
    /// spelling `psql -f -` taught.
    #[test]
    fn the_statement_can_come_from_a_file() {
        for sub in ["query", "explain", "exec"] {
            for flag in ["-f", "--file"] {
                let sql = sql_of(parse(&["schemaic", sub, flag, "q.sql", "-c", "1"]).unwrap());
                assert_eq!(
                    sql.source(),
                    SqlSource::File(std::path::Path::new("q.sql")),
                    "{sub} {flag}"
                );
            }
            let sql = sql_of(parse(&["schemaic", sub, "-f", "-", "-c", "1"]).unwrap());
            assert_eq!(sql.source(), SqlSource::Stdin);
            let sql = sql_of(parse(&["schemaic", sub, "-", "-c", "1"]).unwrap());
            assert_eq!(sql.source(), SqlSource::Stdin);
            let sql = sql_of(parse(&["schemaic", sub, "SELECT 1", "-c", "1"]).unwrap());
            assert_eq!(sql.source(), SqlSource::Text("SELECT 1"));
        }
    }

    /// **One source, never two** — which of a statement and a file ran would
    /// be a rule nobody could see from the command line.
    #[test]
    fn a_statement_and_a_file_together_are_refused() {
        for sub in ["query", "exec"] {
            let err = parse(&["schemaic", sub, "SELECT 1", "-f", "q.sql", "-c", "1"]).unwrap_err();
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::ArgumentConflict,
                "{sub}"
            );
        }
    }

    /// And none at all is still a usage error, not an empty statement.
    #[test]
    fn a_statement_or_a_file_is_required() {
        for sub in ["query", "exec"] {
            let err = parse(&["schemaic", sub, "-c", "1"]).unwrap_err();
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::MissingRequiredArgument,
                "{sub}"
            );
        }
    }

    /// **`ping` names a connection and nothing to run.** No statement, no
    /// `-d` — it asks whether the server answers — and a timeout of the app's
    /// own reachability check, not a statement's.
    #[test]
    fn ping_takes_a_connection_and_the_reachability_timeout() {
        let cli = parse(&["schemaic", "ping", "-c", "prod"]).unwrap();
        let Command::Ping { conn, timeout, .. } = cli.command else {
            panic!("expected a ping");
        };
        assert_eq!(conn.connection.as_deref(), Some("prod"));
        assert_eq!(timeout, schemaic_db::PING_TIMEOUT.as_secs());
        let err = parse(&["schemaic", "ping", "-c", "1", "-d", "app"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
        let err = parse(&["schemaic", "ping"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
        assert!(parse(&["schemaic", "ping", "-c", "1", "--password-stdin"]).is_ok());
    }

    /// **`tables` runs in a database like `query`**, so it takes `-d` (and
    /// `SCHEMAIC_DATABASE`), and it caps like one: a schema of thousands of
    /// tables is a lot to hand an agent unasked.
    #[test]
    fn tables_takes_a_target_and_a_limit() {
        let cli = parse(&["schemaic", "tables", "-c", "prod", "-d", "shop"]).unwrap();
        let Command::Tables {
            target,
            limit,
            timeout,
            ..
        } = cli.command
        else {
            panic!("expected tables");
        };
        assert_eq!(target.conn.connection.as_deref(), Some("prod"));
        assert_eq!(target.database.as_deref(), Some("shop"));
        assert_eq!(limit, DEFAULT_LIMIT);
        assert_eq!(timeout, crate::query::DEFAULT_TIMEOUT.as_secs());
        assert_eq!(
            env_of("tables", "database").as_deref(),
            Some("SCHEMAIC_DATABASE")
        );
    }

    /// `describe` names one table, and there is nothing to cap.
    #[test]
    fn describe_takes_one_table_and_a_target() {
        let cli = parse(&["schemaic", "describe", "public.orders", "-c", "1"]).unwrap();
        let Command::Describe { table, target, .. } = cli.command else {
            panic!("expected describe");
        };
        assert_eq!(table, "public.orders");
        assert_eq!(target.database, None);
        let err = parse(&["schemaic", "describe", "-c", "1"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
        let err = parse(&["schemaic", "describe", "t", "-c", "1", "--limit", "5"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    /// `statements` ranks by total time and returns the MCP tool's twenty
    /// unless asked otherwise, and takes the tool's three order names.
    #[test]
    fn statements_defaults_to_total_time_and_twenty() {
        use schemaic_core::workload::StatementOrder;
        let cli = parse(&["schemaic", "statements", "-c", "prod", "-d", "shop"]).unwrap();
        let Command::Statements {
            target,
            order,
            limit,
            timeout,
            ..
        } = cli.command
        else {
            panic!("expected statements");
        };
        assert_eq!(target.database.as_deref(), Some("shop"));
        assert_eq!(order, StatementOrder::Total);
        assert_eq!(limit, PERF_LIMIT);
        assert_eq!(timeout, crate::query::DEFAULT_TIMEOUT.as_secs());
        for (name, want) in [
            ("mean", StatementOrder::Mean),
            ("calls", StatementOrder::Calls),
            ("Total", StatementOrder::Total),
        ] {
            let cli = parse(&["schemaic", "statements", "-c", "1", "--order", name]).unwrap();
            let Command::Statements { order, .. } = cli.command else {
                panic!("expected statements");
            };
            assert_eq!(order, want, "{name}");
        }
        let err = parse(&["schemaic", "statements", "-c", "1", "--order", "slowest"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }

    /// **A limit past the ceiling is refused, not clamped** — the MCP tools'
    /// rule, so asking for more than there can be says so.
    #[test]
    fn a_performance_limit_past_its_ceiling_is_refused() {
        let max = schemaic_core::workload::MAX_STATEMENT_LIMIT.to_string();
        let over = (schemaic_core::workload::MAX_STATEMENT_LIMIT + 1).to_string();
        assert!(parse(&["schemaic", "statements", "-c", "1", "--limit", &max]).is_ok());
        for bad in [over.as_str(), "0"] {
            let err = parse(&["schemaic", "statements", "-c", "1", "--limit", bad]).unwrap_err();
            assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation, "{bad}");
        }
        let over = (MAX_SIZE_ROWS + 1).to_string();
        let err = parse(&["schemaic", "sizes", "-c", "1", "--limit", &over]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }

    /// `indexes` and `sizes` each have one switch, off by default; `indexes`
    /// takes no `--limit`.
    #[test]
    fn indexes_and_sizes_each_take_their_switch() {
        let cli = parse(&["schemaic", "indexes", "-c", "1"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Indexes { missing: false, .. }
        ));
        let cli = parse(&["schemaic", "indexes", "-c", "1", "--missing"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Indexes { missing: true, .. }
        ));
        let err = parse(&["schemaic", "indexes", "-c", "1", "--limit", "5"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);

        let cli = parse(&["schemaic", "sizes", "-c", "1"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Sizes {
                indexes: false,
                limit: PERF_LIMIT,
                ..
            }
        ));
        let cli = parse(&["schemaic", "sizes", "-c", "1", "--indexes"]).unwrap();
        assert!(matches!(cli.command, Command::Sizes { indexes: true, .. }));
    }

    /// `locks` is the server's, not one database's: no `-d`.
    #[test]
    fn locks_takes_a_connection_and_no_database() {
        let cli = parse(&["schemaic", "locks", "-c", "prod"]).unwrap();
        let Command::Locks { conn, .. } = cli.command else {
            panic!("expected locks");
        };
        assert_eq!(conn.connection.as_deref(), Some("prod"));
        let err = parse(&["schemaic", "locks", "-c", "1", "-d", "shop"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    /// **`mcp` names a connection and, optionally, a database — nothing else.**
    /// Stdin is the protocol's, so `--password-stdin` would read the client's
    /// first JSON-RPC line as the password; it is not a flag here at all rather
    /// than one that always refuses. And nothing prints rows, so no `--format`.
    #[test]
    fn mcp_takes_a_connection_and_a_database_and_nothing_from_stdin() {
        let cli = parse(&["schemaic", "mcp", "-c", "prod", "-d", "shop"]).unwrap();
        let Command::Mcp { target } = cli.command else {
            panic!("expected mcp");
        };
        assert_eq!(target.connections, vec!["prod".to_string()]);
        assert_eq!(target.database.as_deref(), Some("shop"));
        assert_eq!(
            parse(&["schemaic", "mcp", "-c", "prod"]).unwrap().command,
            Command::Mcp {
                target: McpTarget {
                    connections: vec!["prod".to_string()],
                    ..Default::default()
                }
            }
        );
        // Repeated, it serves several, in the order given.
        let Command::Mcp { target } = parse(&["schemaic", "mcp", "-c", "prod", "-c", "stage"])
            .unwrap()
            .command
        else {
            panic!("expected mcp");
        };
        assert_eq!(
            target.connections,
            vec!["prod".to_string(), "stage".to_string()]
        );
        for extra in [
            &["--password-stdin"][..],
            &["--format", "json"],
            &["--limit", "5"],
        ] {
            let args: Vec<&str> = ["schemaic", "mcp", "-c", "1"]
                .into_iter()
                .chain(extra.iter().copied())
                .collect();
            let err = parse(&args).unwrap_err();
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::UnknownArgument,
                "{extra:?}"
            );
        }
        let err = parse(&["schemaic", "mcp"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
        let err = parse(&["schemaic", "mcp", "-c", "1", "-d", " "]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
        assert_eq!(
            env_of("mcp", "connection").as_deref(),
            Some("SCHEMAIC_CONNECTION")
        );
        assert_eq!(
            env_of("mcp", "database").as_deref(),
            Some("SCHEMAIC_DATABASE")
        );
        assert!(
            parse(&["schemaic", "mcp", "-c", "1"])
                .unwrap()
                .command
                .output_args()
                .is_none()
        );
    }

    /// **`compare` names two databases and prints a script.** `-c`/`-d` are
    /// the side that would change, as they are where every other subcommand
    /// runs; the source defaults to the same connection, so two databases on
    /// one server is the short form. No `--password-stdin` — two connections
    /// would leave it unclear whose password it is — and no `--format`.
    #[test]
    fn compare_takes_a_target_a_source_and_tables() {
        let cli = parse(&[
            "schemaic",
            "compare",
            "-c",
            "prod",
            "-d",
            "shop",
            "--source-database",
            "shop_staging",
            "-t",
            "orders",
            "-t",
            "sales.items",
        ])
        .unwrap();
        let Command::Compare {
            connection,
            database,
            source_connection,
            source_database,
            tables,
            timeout,
        } = cli.command
        else {
            panic!("expected a compare");
        };
        assert_eq!(connection, "prod");
        assert_eq!(database.as_deref(), Some("shop"));
        assert_eq!(source_connection, None);
        assert_eq!(source_database.as_deref(), Some("shop_staging"));
        assert_eq!(tables, ["orders", "sales.items"]);
        assert_eq!(timeout, COMPARE_TIMEOUT_SECS);
        for extra in [&["--password-stdin"][..], &["--format", "json"]] {
            let args: Vec<&str> = ["schemaic", "compare", "-c", "1", "--source-connection", "2"]
                .into_iter()
                .chain(extra.iter().copied())
                .collect();
            let err = parse(&args).unwrap_err();
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::UnknownArgument,
                "{extra:?}"
            );
        }
        let err = parse(&["schemaic", "compare", "-c", "1", "--source-database", ""]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
        assert_eq!(
            env_of("compare", "connection").as_deref(),
            Some("SCHEMAIC_CONNECTION")
        );
        assert!(
            parse(&["schemaic", "compare", "-c", "1", "--source-connection", "2"])
                .unwrap()
                .command
                .output_args()
                .is_none()
        );
    }

    #[test]
    fn a_query_needs_a_connection() {
        let err = parse(&["schemaic", "query", "SELECT 1"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn a_query_carries_its_sql_and_connection() {
        let cli = parse(&["schemaic", "query", "SELECT 1", "-c", "prod"]).unwrap();
        let Command::Query { sql, target, .. } = cli.command else {
            panic!("expected a query");
        };
        assert_eq!(sql.source(), SqlSource::Text("SELECT 1"));
        assert_eq!(target.conn.connection.as_deref(), Some("prod"));
        assert_eq!(target.database, None);
    }

    /// `version` is what someone types before they know the flag spelling,
    /// and it needs nothing else.
    #[test]
    fn version_is_a_subcommand_of_its_own() {
        assert_eq!(
            parse(&["schemaic", "version"]).unwrap().command,
            Command::Version
        );
        let err = parse(&["schemaic", "version", "-c", "1"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    /// **The subcommand and the flag print the same thing** — one text, the
    /// one clap renders for `--version`, so the two cannot drift apart.
    #[test]
    fn version_prints_what_the_flag_prints() {
        let flag = parse(&["schemaic", "--version"]).unwrap_err();
        assert_eq!(flag.kind(), clap::error::ErrorKind::DisplayVersion);
        assert_eq!(version_text(), flag.to_string());
        assert!(
            version_text().contains(env!("CARGO_PKG_VERSION")),
            "{}",
            version_text()
        );
    }

    #[test]
    fn databases_takes_a_connection_and_the_shared_defaults() {
        let cli = parse(&["schemaic", "databases", "-c", "prod"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Databases {
                conn: ConnArgs {
                    connection: Some("prod".to_string()),
                    ..Default::default()
                },
                output: OutputArgs {
                    format: Format::Table,
                    no_header: false,
                },
                timeout: crate::query::DEFAULT_TIMEOUT.as_secs(),
            }
        );
        let err = parse(&["schemaic", "databases"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
    }

    /// **`databases` has no `-d`.** It lists what `-d` may name; accepting one
    /// would suggest the listing was scoped by it.
    #[test]
    fn databases_has_no_database_flag() {
        let err = parse(&["schemaic", "databases", "-c", "1", "-d", "app"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    /// The headless case the keyring cannot serve applies to listing as much
    /// as to querying.
    #[test]
    fn databases_takes_the_password_from_stdin_too() {
        let cli = parse(&["schemaic", "databases", "-c", "1", "--password-stdin"]).unwrap();
        let Command::Databases { conn, .. } = cli.command else {
            panic!("expected a databases listing");
        };
        assert!(conn.password_stdin);
    }

    /// The defaults are the contract: a person who types the shortest possible
    /// command gets the human format and a cap they can live with.
    #[test]
    fn the_defaults_are_table_and_a_small_limit() {
        let cli = parse(&["schemaic", "query", "SELECT 1", "-c", "1"]).unwrap();
        let Command::Query {
            output,
            limit,
            timeout,
            ..
        } = cli.command
        else {
            panic!("expected a query");
        };
        assert_eq!(output.format, Format::Table);
        assert!(!output.no_header, "the header is there unless asked away");
        assert_eq!(limit, DEFAULT_LIMIT);
        assert_eq!(timeout, crate::query::DEFAULT_TIMEOUT.as_secs());
    }

    /// The exec default is the same constant, not a second literal beside it.
    #[test]
    fn exec_defaults_to_the_shared_timeout() {
        let cli = parse(&["schemaic", "exec", "DELETE FROM t", "-c", "1"]).unwrap();
        let Command::Exec {
            timeout, script, ..
        } = cli.command
        else {
            panic!("expected an exec");
        };
        assert!(!script);
        assert_eq!(
            exec_timeout(script, timeout),
            crate::query::DEFAULT_TIMEOUT.as_secs()
        );
    }

    #[test]
    fn a_script_defaults_to_an_hour_and_a_given_timeout_wins() {
        let cli = parse(&[
            "schemaic", "exec", "--script", "-f", "m.sql", "-c", "1", "--yes",
        ])
        .unwrap();
        let Command::Exec {
            timeout,
            script,
            yes,
            ..
        } = cli.command
        else {
            panic!("expected an exec");
        };
        assert!(script && yes);
        assert_eq!(exec_timeout(script, timeout), EXPORT_TIMEOUT_SECS);
        assert_eq!(exec_timeout(true, Some(5)), 5);
        assert_eq!(exec_timeout(false, Some(5)), 5);
    }

    /// A script is run as written; substituting into a whole file is not
    /// something the app does either.
    #[test]
    fn a_script_takes_no_params() {
        assert!(
            parse(&[
                "schemaic", "exec", "--script", "-f", "m.sql", "-c", "1", "-p", "a=1",
            ])
            .is_err()
        );
    }

    #[test]
    fn the_format_flag_is_parsed_through_the_format_type() {
        let cli = parse(&[
            "schemaic", "query", "SELECT 1", "-c", "1", "--format", "jsonl",
        ])
        .unwrap();
        let Command::Query { output, .. } = cli.command else {
            panic!("expected a query");
        };
        assert_eq!(output.format, Format::Jsonl);
    }

    /// **`--no-header` is on every subcommand that prints rows**, and is
    /// judged against the format before anything is read — a pair that does
    /// not go together is a usage error, not a header quietly kept.
    #[test]
    fn no_header_is_on_every_row_printing_subcommand() {
        for args in [
            &["schemaic", "list", "--no-header"][..],
            &["schemaic", "databases", "-c", "1", "--no-header"],
            &["schemaic", "ping", "-c", "1", "--no-header"],
            &["schemaic", "tables", "-c", "1", "--no-header"],
            &["schemaic", "describe", "t", "-c", "1", "--no-header"],
            &["schemaic", "statements", "-c", "1", "--no-header"],
            &["schemaic", "indexes", "-c", "1", "--no-header"],
            &["schemaic", "sizes", "-c", "1", "--no-header"],
            &["schemaic", "locks", "-c", "1", "--no-header"],
            &["schemaic", "query", "SELECT 1", "-c", "1", "--no-header"],
            &["schemaic", "exec", "SELECT 1", "-c", "1", "--no-header"],
        ] {
            let cli = parse(args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
            let output = cli.command.output_args().expect("prints rows");
            assert!(output.no_header, "{args:?}");
            assert_eq!(output.output().map(|o| o.header), Ok(false));
        }
        let cli = parse(&[
            "schemaic",
            "query",
            "SELECT 1",
            "-c",
            "1",
            "--no-header",
            "--format",
            "json",
        ])
        .unwrap();
        assert!(cli.command.output_args().unwrap().output().is_err());
        assert_eq!(
            parse(&["schemaic", "version"])
                .unwrap()
                .command
                .output_args(),
            None
        );
    }

    /// A bad format must fail at parse time, naming the real ones — not reach a
    /// database and fail after the work is done.
    #[test]
    fn an_unknown_format_is_refused_before_anything_connects() {
        let err = parse(&[
            "schemaic", "query", "SELECT 1", "-c", "1", "--format", "yaml",
        ])
        .unwrap_err();
        let text = err.to_string();
        assert!(text.contains("yaml"));
        assert!(
            text.contains("jsonl"),
            "the error must list the real formats"
        );
    }

    /// **`--yes` belongs to `exec` and must not exist on `query`.** A read has
    /// nothing to consent to, and accepting the flag there would teach the
    /// habit of passing it everywhere.
    #[test]
    fn query_has_no_yes_flag() {
        let err = parse(&["schemaic", "query", "SELECT 1", "-c", "1", "--yes"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn exec_takes_yes_and_defaults_it_off() {
        let cli = parse(&["schemaic", "exec", "DELETE FROM t", "-c", "1"]).unwrap();
        let Command::Exec { yes, .. } = cli.command else {
            panic!("expected an exec");
        };
        assert!(!yes, "consent is never the default");

        let cli = parse(&["schemaic", "exec", "DELETE FROM t", "-c", "1", "--yes"]).unwrap();
        let Command::Exec { yes, .. } = cli.command else {
            panic!("expected an exec");
        };
        assert!(yes);
    }

    /// **Failing on a cap is opt-in.** A person at a terminal reads the footer;
    /// only a caller that asks for it gets a non-zero exit for rows that were
    /// read correctly.
    #[test]
    fn fail_on_cap_is_an_opt_in_query_flag() {
        let cli = parse(&["schemaic", "query", "SELECT 1", "-c", "1"]).unwrap();
        let Command::Query { fail_on_cap, .. } = cli.command else {
            panic!("expected a query");
        };
        assert!(!fail_on_cap, "off unless asked for");
        let cli = parse(&["schemaic", "query", "SELECT 1", "-c", "1", "--fail-on-cap"]).unwrap();
        let Command::Query { fail_on_cap, .. } = cli.command else {
            panic!("expected a query");
        };
        assert!(fail_on_cap);
    }

    /// `exec` has no `--limit`, and the cap on what it prints never bounds the
    /// statement — there is nothing for the flag to report on.
    #[test]
    fn exec_has_no_fail_on_cap_flag() {
        let err = parse(&[
            "schemaic",
            "exec",
            "DELETE FROM t",
            "-c",
            "1",
            "--fail-on-cap",
        ])
        .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    /// `--limit` is a read's concern; a write reports rows affected and has
    /// nothing to cap.
    #[test]
    fn exec_has_no_limit_flag() {
        let err = parse(&[
            "schemaic",
            "exec",
            "DELETE FROM t",
            "-c",
            "1",
            "--limit",
            "5",
        ])
        .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn list_needs_no_connection_and_defaults_to_the_permitted_ones() {
        let cli = parse(&["schemaic", "list"]).unwrap();
        assert_eq!(
            cli.command,
            Command::List {
                all: false,
                output: OutputArgs {
                    format: Format::Table,
                    no_header: false,
                },
            }
        );
    }

    #[test]
    fn list_all_is_opt_in() {
        let cli = parse(&["schemaic", "list", "--all"]).unwrap();
        let Command::List { all, .. } = cli.command else {
            panic!("expected a list");
        };
        assert!(all);
    }

    #[test]
    fn the_database_can_be_named_long_or_short() {
        for args in [
            ["schemaic", "query", "SELECT 1", "-c", "1", "-d", "app"],
            [
                "schemaic",
                "query",
                "SELECT 1",
                "-c",
                "1",
                "--database",
                "app",
            ],
        ] {
            let cli = parse(&args).unwrap();
            let Command::Query { target, .. } = cli.command else {
                panic!("expected a query");
            };
            assert_eq!(target.database.as_deref(), Some("app"));
        }
    }

    /// **A blank database is refused, not read as "no flag".** `-d "$DB"` with
    /// `DB` unset arrives as an empty string; taken as a name, PostgreSQL
    /// connects to the database named after the user — exactly the unscoped
    /// landing the exec guard's "no database selected" arm exists to stop.
    #[test]
    fn a_blank_database_is_refused_at_parse_time() {
        for blank in ["", "   "] {
            for sub in ["query", "exec"] {
                let err =
                    parse(&["schemaic", sub, "SELECT 1", "-c", "1", "-d", blank]).unwrap_err();
                assert_eq!(
                    err.kind(),
                    clap::error::ErrorKind::ValueValidation,
                    "{sub} -d {blank:?}"
                );
            }
        }
    }

    /// The `env` attribute of `sub`'s argument `id`, read off the parser
    /// rather than by setting the variable: the process environment is shared
    /// by every test running beside this one.
    fn env_of(sub: &str, id: &str) -> Option<String> {
        let cmd = <Cli as clap::CommandFactory>::command();
        let sub = cmd.find_subcommand(sub).expect("a subcommand");
        let arg = sub
            .get_arguments()
            .find(|a| a.get_id() == id)
            .expect("an argument");
        arg.get_env().map(|e| e.to_string_lossy().into_owned())
    }

    /// **An exported connection does not join `mcp --url`'s server** as its
    /// default; a typed `-c` beside the URL still does. The variable is
    /// renamed to one no other test reads, since the environment is the
    /// process's.
    #[test]
    fn mcp_refuses_an_exported_connection_beside_a_url() {
        use clap::CommandFactory as _;
        const VAR: &str = "SCHEMAIC_TEST_MCP_EXPORTED_CONNECTION";
        // SAFETY: no other test reads or writes this variable.
        unsafe { std::env::set_var(VAR, "prod") };
        let cmd =
            || Cli::command().mut_subcommand("mcp", |s| s.mut_arg("connection", |a| a.env(VAR)));
        let argv = |a: &[&str]| a.iter().map(std::ffi::OsString::from).collect::<Vec<_>>();
        let err =
            parse_with(cmd(), argv(&["schemaic", "mcp", "--url", "mysql://u@h/d"])).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
        assert!(err.to_string().contains("SCHEMAIC_CONNECTION"), "{err}");
        // Typed, it is both; and the variable alone still serves.
        let typed = argv(&["schemaic", "mcp", "-c", "prod", "--url", "mysql://u@h/d"]);
        assert!(parse_with(cmd(), typed).is_ok());
        assert!(parse_with(cmd(), argv(&["schemaic", "mcp"])).is_ok());
    }

    /// **`-d` comes from `SCHEMAIC_DATABASE` the way `-c` comes from
    /// `SCHEMAIC_CONNECTION`**, on both subcommands that run in one — so an
    /// agent's shell is pointed at a connection *and* a database once.
    #[test]
    fn the_database_and_the_connection_can_come_from_the_environment() {
        for sub in ["query", "exec"] {
            assert_eq!(
                env_of(sub, "database").as_deref(),
                Some("SCHEMAIC_DATABASE"),
                "{sub}"
            );
            assert_eq!(
                env_of(sub, "connection").as_deref(),
                Some("SCHEMAIC_CONNECTION"),
                "{sub}"
            );
        }
        for sub in ["databases", "ping"] {
            assert_eq!(
                env_of(sub, "connection").as_deref(),
                Some("SCHEMAIC_CONNECTION"),
                "{sub}"
            );
        }
    }

    /// A zero timeout cancels every statement before it can run, and a zero
    /// limit asks for no rows; both are typos, not requests.
    #[test]
    fn a_zero_timeout_or_limit_is_refused() {
        for args in [
            ["schemaic", "query", "SELECT 1", "-c", "1", "--timeout", "0"],
            ["schemaic", "exec", "SELECT 1", "-c", "1", "--timeout", "0"],
            ["schemaic", "query", "SELECT 1", "-c", "1", "--limit", "0"],
        ] {
            let err = parse(&args).unwrap_err();
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::ValueValidation,
                "{args:?}"
            );
        }
        assert!(parse(&["schemaic", "query", "SELECT 1", "-c", "1", "--limit", "1"]).is_ok());
    }

    /// `help` has to work as a subcommand, because that is what someone
    /// exploring the tool types first.
    #[test]
    fn help_is_a_subcommand_and_reports_itself_as_such() {
        let err = parse(&["schemaic", "help"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    /// The top-level help must name every subcommand, or the tool is not
    /// explorable without reading its source.
    #[test]
    fn the_help_text_names_every_subcommand() {
        let text = parse(&["schemaic", "help"]).unwrap_err().to_string();
        for name in subcommand_names() {
            assert!(text.contains(&name), "help must mention `{name}`");
        }
    }

    /// **`--yes` says every question it answers.** The help called the
    /// missing-WHERE warning the one in practice after a `DROP TABLE` began
    /// asking too — the user-facing text for the one flag that refusal needs.
    #[test]
    fn exec_help_names_every_question_yes_answers() {
        let text = parse(&["schemaic", "exec", "--help"])
            .unwrap_err()
            .to_string();
        let yes = text
            .split("--yes")
            .nth(1)
            .and_then(|t| t.split("--timeout").next())
            .expect("exec --help documents --yes");
        for word in ["WHERE", "TRUNCATE", "drops a table"] {
            assert!(yes.contains(word), "`--yes` help lacks {word}: {yes}");
        }
        assert!(!yes.contains("in practice"), "{yes}");
    }

    /// No subcommand at all is a usage error with the help text, not a silent
    /// success.
    #[test]
    fn a_bare_invocation_is_a_usage_error() {
        let err = parse(&["schemaic"]).unwrap_err();
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }
}
