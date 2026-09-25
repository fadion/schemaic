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
const AFTER_HELP: &str = "\
Output formats (--format):
  table     A Markdown table with a row-count footer (the default)
  json      A JSON array of row objects
  jsonl     One JSON object per line
  csv       RFC 4180 CSV with a header row
  vertical  One `name: value` record per row, like the mysql client's \\G
--no-header leaves the column names, and the table's footer, out of table and csv.
Rows go to stdout; everything else goes to stderr.

Exit codes:
  0  It ran
  2  A usage error, or a connection, file or table that is not there; nothing was sent
  3  A guard refused: not a read, a read-only connection, no --yes, or no CLI access; \
nothing was sent
  4  The server or the connection failed; retrying may work
  5  A write timed out after it was sent and may have been applied; check before retrying
  6  query --fail-on-cap: --limit cut the rows short (they were still printed)";

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
    /// Run a read-only statement.
    Query {
        #[command(flatten)]
        sql: SqlArgs,
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
    /// Run a statement that writes.
    Exec {
        #[command(flatten)]
        sql: SqlArgs,
        #[command(flatten)]
        target: Target,
        #[command(flatten)]
        output: OutputArgs,
        /// Answer the guard's question: a DELETE or UPDATE with no WHERE, a
        /// TRUNCATE, or a statement that drops a table, database or schema.
        /// It cannot unlock a read-only connection.
        #[arg(long)]
        yes: bool,
        /// Seconds before the statement is stopped.
        #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// Print the version — the same line as `--version`.
    Version,
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
            | Command::Query { output, .. }
            | Command::Exec { output, .. } => Some(output),
            Command::Version => None,
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
    /// statement: a whole script is not what this runs.
    #[arg(short = 'f', long, value_name = "PATH")]
    pub file: Option<std::path::PathBuf>,
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
#[derive(clap::Args, Debug, PartialEq, Eq)]
pub struct ConnArgs {
    /// Saved connection, by id or by name — or `#<id>`, which is only ever
    /// the id.
    #[arg(short = 'c', long, env = "SCHEMAIC_CONNECTION")]
    pub connection: String,
    /// Read the connection's password from stdin instead of the OS keyring.
    ///
    /// **For the headless case the keyring cannot serve.** Linux's Secret
    /// Service needs an unlocked desktop collection, which an SSH session or a
    /// container does not have; without this the CLI would simply be unusable
    /// there.
    #[arg(long)]
    pub password_stdin: bool,
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

/// `--limit`'s parser: zero rows is a typo, not a request.
fn at_least_one_row() -> clap::builder::RangedU64ValueParser<usize> {
    clap::builder::RangedU64ValueParser::<usize>::new().range(1..)
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
    Cli::try_parse_from(comment_led_sql_last(argv))
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
                | "query"
                | "exec"
                | "version"
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
        for sub in ["query", "exec"] {
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
                    | Command::Exec {
                        sql,
                        target,
                        output,
                        ..
                    } => (sql, target, output),
                    other => panic!("{other:?}"),
                };
                assert_eq!(got.source(), SqlSource::Text(sql));
                assert_eq!(target.conn.connection, "prod");
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
            Command::Query { sql, .. } | Command::Exec { sql, .. } => sql,
            other => panic!("{other:?}"),
        }
    }

    /// **`-f` is a third place for the statement, beside the argument and
    /// stdin**, on both subcommands that take one — and `-f -` is stdin, the
    /// spelling `psql -f -` taught.
    #[test]
    fn the_statement_can_come_from_a_file() {
        for sub in ["query", "exec"] {
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
        assert_eq!(conn.connection, "prod");
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
        assert_eq!(target.conn.connection, "prod");
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
        assert_eq!(target.conn.connection, "prod");
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
                    connection: "prod".to_string(),
                    password_stdin: false,
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
        let Command::Exec { timeout, .. } = cli.command else {
            panic!("expected an exec");
        };
        assert_eq!(timeout, crate::query::DEFAULT_TIMEOUT.as_secs());
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
