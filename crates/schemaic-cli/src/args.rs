//! The command line itself.
//!
//! Parsing is separate from doing, so the whole surface — defaults, aliases,
//! which flags belong to which subcommand — is testable without a database.
//! [`Cli::try_parse_from`] is what the tests drive.

use clap::{Parser, Subcommand};

use crate::format::Format;

/// How many rows a `query` returns unless asked for more.
///
/// **Deliberately small.** The GUI's cap is about what a grid can hold; this
/// one is about what a caller can sensibly receive down a pipe, and the caller
/// is very often a language model with a context window. A person who wants the
/// whole table says so with `--limit`.
pub const DEFAULT_LIMIT: usize = 200;

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
    disable_help_subcommand = false
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
        #[arg(long, default_value = "table")]
        format: Format,
    },
    /// List the databases on a connection — the names `-d` takes.
    Databases {
        #[command(flatten)]
        conn: ConnArgs,
        #[arg(long, default_value = "table")]
        format: Format,
        /// Seconds before the listing is given up on.
        #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// Run a read-only statement.
    Query {
        /// The SQL to run. One statement; `-` reads it from stdin.
        sql: String,
        #[command(flatten)]
        target: Target,
        #[arg(long, default_value = "table")]
        format: Format,
        /// Maximum rows to return.
        #[arg(long, default_value_t = DEFAULT_LIMIT, value_parser = at_least_one_row())]
        limit: usize,
        /// Seconds before the statement is cancelled.
        #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS, value_parser = at_least_one_second())]
        timeout: u64,
    },
    /// Run a statement that writes.
    Exec {
        /// The SQL to run. One statement; `-` reads it from stdin — the place
        /// for one that carries a password, which on the command line lands in
        /// the process list and the shell's history.
        sql: String,
        #[command(flatten)]
        target: Target,
        #[arg(long, default_value = "table")]
        format: Format,
        /// Answer the guard's question — in practice, "this statement has no
        /// WHERE clause". It cannot unlock a read-only connection.
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

/// The statement argument that means "read it from stdin".
pub const SQL_FROM_STDIN: &str = "-";

/// Which connection, and which database on it.
#[derive(clap::Args, Debug, PartialEq, Eq)]
pub struct Target {
    #[command(flatten)]
    pub conn: ConnArgs,
    /// Database to run in. Defaults to the connection's own; `schemaic
    /// databases` lists the names.
    #[arg(short = 'd', long, value_parser = non_blank)]
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
fn non_blank(s: &str) -> Result<String, String> {
    if s.trim().is_empty() {
        Err("the database name is empty; leave out -d to use the connection's own".to_string())
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

    #[test]
    fn every_subcommand_routes_to_the_cli() {
        for name in ["list", "databases", "query", "exec", "version", "help"] {
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
        assert_eq!(sql, "SELECT 1");
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
                format: Format::Table,
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
            format,
            limit,
            timeout,
            ..
        } = cli.command
        else {
            panic!("expected a query");
        };
        assert_eq!(format, Format::Table);
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
        let Command::Query { format, .. } = cli.command else {
            panic!("expected a query");
        };
        assert_eq!(format, Format::Jsonl);
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
                format: Format::Table
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
        for name in ["list", "databases", "query", "exec", "version"] {
            assert!(text.contains(name), "help must mention `{name}`");
        }
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
