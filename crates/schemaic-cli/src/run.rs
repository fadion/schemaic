//! Doing what the command line asked for.
//!
//! **stdout is data; stderr is everything else.** A caller that pipes stdout
//! gets rows and nothing else — no banners, no warnings, no progress. That is
//! what makes `--format=json` safe to parse and `--format=csv` safe to
//! redirect, and it is why the truncation warning goes to stderr for every
//! format that cannot carry it in its own syntax.
//!
//! The exit code is the other half of that contract: see [`Exit`].

use std::io::{IsTerminal, Read};
use std::process::ExitCode;
use std::time::Duration;

use schemaic_core::connection::Connection;
use schemaic_core::model::{Column, ResultSet, Value};
use schemaic_core::secrets::SecretKind;
use schemaic_db::Db;

use crate::args::{Cli, Command, Target};
use crate::format::{self, Format};
use crate::{exec, query, select};

/// What the process exits with.
///
/// **Four outcomes, not two.** A caller that can only tell success from failure
/// retries the refusal that will never succeed and gives up on the timeout that
/// would have. These are stable: scripts depend on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Exit {
    /// It ran.
    Ok = 0,
    /// The command line was wrong, or named a connection that is not there.
    /// Nothing was sent to a server.
    Usage = 2,
    /// A guard refused: not a read, a read-only connection, consent not given,
    /// or a connection the user has not exposed to the CLI. Nothing was sent to
    /// a server, and retrying unchanged will refuse again.
    Refused = 3,
    /// The server or the connection failed. Retrying may work.
    Failed = 4,
}

impl From<Exit> for ExitCode {
    fn from(e: Exit) -> ExitCode {
        ExitCode::from(e as u8)
    }
}

/// Everything the CLI says that is not data.
fn warn(message: &str) {
    eprintln!("schemaic: {message}");
}

/// Parse, then run. The whole entry point, for both front ends.
pub fn main<I, T>(argv: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = match <Cli as clap::Parser>::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(e) => {
            // clap writes help and `--version` to stdout and real errors to
            // stderr, and it knows which is which; `print` honours that.
            let _ = e.print();
            return if e.use_stderr() {
                Exit::Usage.into()
            } else {
                Exit::Ok.into()
            };
        }
    };
    // **Current-thread, built here.** A CLI invocation is one statement on one
    // connection; a multi-thread pool would cost startup for nothing. This
    // mirrors `--mcp-serve`, which is the other front end that never builds a
    // window.
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            warn(&format!("could not start: {e}"));
            return Exit::Failed.into();
        }
    };
    rt.block_on(dispatch(cli.command)).into()
}

async fn dispatch(command: Command) -> Exit {
    // Unhydrated: `list` shows no secret, and `open` fills in only the one
    // connection a command runs against.
    let file = match schemaic_conn::secrets::load_connections_readonly() {
        Ok(v) => v,
        Err(e) => {
            // Reported, never repaired: moving the file aside is the app's
            // decision to make, with its recovery modal in front of the user.
            warn(&format!(
                "could not read the saved connections: {e}. Open Schemaic to recover them."
            ));
            return Exit::Failed;
        }
    };
    match command {
        Command::List { all, format } => list(&file.connections, all, format),
        Command::Query {
            sql,
            target,
            format,
            limit,
            timeout,
        } => {
            run_query(
                &file.connections,
                &target,
                &sql,
                format,
                limit,
                Duration::from_secs(timeout),
            )
            .await
        }
        Command::Exec {
            sql,
            target,
            format,
            yes,
            timeout,
        } => {
            run_exec(
                &file.connections,
                &target,
                &sql,
                format,
                yes,
                Duration::from_secs(timeout),
            )
            .await
        }
    }
}

/// `schemaic list`.
///
/// The listing is built as a [`ResultSet`] and handed to the same renderers a
/// query's rows go through, so `--format=json` means the same thing here as it
/// does there and there is no second table-drawing code path to keep in step.
fn list(conns: &[Connection], all: bool, format: Format) -> Exit {
    let shown: Vec<&Connection> = if all {
        conns.iter().collect()
    } else {
        select::listed(conns)
    };
    let rows: Vec<Vec<Value>> = shown
        .iter()
        .map(|c| {
            let mut row = vec![
                Value::UInt(c.id),
                Value::Str(c.name.clone()),
                Value::Str(c.db_type.clone()),
                Value::Str(c.endpoint()),
                Value::Str(
                    if c.read_only {
                        "read-only"
                    } else {
                        "read/write"
                    }
                    .to_string(),
                ),
            ];
            if all {
                row.push(Value::Str(
                    if c.cli_access { "yes" } else { "no" }.to_string(),
                ));
            }
            row
        })
        .collect();
    let mut names = vec!["id", "name", "engine", "endpoint", "writes"];
    if all {
        names.push("cli access");
    }
    let columns = names.into_iter().map(text_column).collect();
    print!(
        "{}",
        format::render_rows(&ResultSet::from_rows(columns, rows), format)
    );
    // An empty list is not an error — it is the correct answer to "what may I
    // use". The hint is what makes it actionable rather than baffling.
    if shown.is_empty() && !all {
        warn(
            "no connection has CLI access yet; turn it on for one in Schemaic, \
             or run `schemaic list --all` to see them all",
        );
    }
    Exit::Ok
}

/// A listing column. The listing is not a query result, so no column of it has
/// a real database type or origin.
fn text_column(name: &str) -> Column {
    Column {
        name: name.to_string(),
        type_name: "TEXT".to_string(),
        origin: None,
    }
}

/// Resolve the connection and open it, tunnel included.
///
/// The `TunnelHandle` is returned alongside the `Db` because dropping it closes
/// the tunnel — holding it for exactly as long as the connection is used is the
/// whole of its lifetime management.
async fn open<'a>(
    conns: &'a [Connection],
    target: &Target,
) -> Result<(Db, Option<schemaic_db::ssh::TunnelHandle>, &'a Connection), Exit> {
    let conn = match select::select(conns, &target.connection) {
        Ok(c) => c,
        Err(e) => {
            warn(&e.message());
            // A connection that exists but is not exposed is a *refusal*; one
            // that does not exist is a usage error. Same message channel,
            // different exit, because only one of them is worth retrying after
            // a change in Schemaic rather than a change in the command.
            return Err(match e {
                select::NoConnection::NotExposed(_) => Exit::Refused,
                _ => Exit::Usage,
            });
        }
    };
    // Read before the keyring is touched, so a refused terminal is refused
    // before anything else is said about the connection.
    let piped = if target.password_stdin {
        // **A terminal is refused, not prompted.** `read_to_string` on one
        // waits for an EOF nobody knows to type, so the command just hung — and
        // a prompt would echo the password onto the screen. The flag is for a
        // pipe; say so.
        if std::io::stdin().is_terminal() {
            warn(
                "--password-stdin reads the password from a pipe, and stdin is a terminal; \
                 pipe it in (e.g. `printf '%s' \"$PASSWORD\" | schemaic …`), or leave the \
                 flag out to use the OS keyring",
            );
            return Err(Exit::Usage);
        }
        let mut password = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut password) {
            warn(&format!("could not read the password from stdin: {e}"));
            return Err(Exit::Usage);
        }
        // A password typed or piped in arrives with the newline that ended it.
        Some(password.trim_end_matches(['\r', '\n']).to_string())
    } else {
        None
    };
    let mut conn = conn.clone();
    // The password `--password-stdin` supplies is not asked of the keyring at
    // all, so a locked one has nothing to report about it.
    let supplied: &[SecretKind] = if piped.is_some() {
        &[SecretKind::DbPassword]
    } else {
        &[]
    };
    for notice in schemaic_conn::secrets::hydrate_for_cli(&mut conn, supplied) {
        warn(&notice);
    }
    if let Some(password) = piped {
        conn.password = password;
    }
    let tunnel = if conn.ssh.enabled {
        match schemaic_db::ssh::open_tunnel(&conn.ssh, &conn.host, conn.port).await {
            Ok(t) => Some(t),
            Err(e) => {
                warn(&format!("ssh tunnel failed: {e}"));
                return Err(Exit::Failed);
            }
        }
    } else {
        None
    };
    let db = Db::connect(&conn, tunnel.as_ref().map(|t| t.port()));
    // The connection is cloned above (for `--password-stdin`), so hand back a
    // borrow of the *saved* one for the guard to read: the guard's subject is
    // what the user configured, not what this invocation patched.
    let saved = conns
        .iter()
        .find(|c| c.id == conn.id)
        .expect("the selected connection is in the list it came from");
    Ok((db, tunnel, saved))
}

/// `schemaic query`.
async fn run_query(
    conns: &[Connection],
    target: &Target,
    sql: &str,
    format: Format,
    limit: usize,
    timeout: Duration,
) -> Exit {
    let (db, _tunnel, conn) = match open(conns, target).await {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    let database = database_for(target, conn);
    match query::read_only_query(&db, database.as_deref(), sql, limit, timeout).await {
        Ok(rs) => {
            print!("{}", format::render_rows(&rs, format));
            if let Some(w) = format::truncation_warning(&rs, format) {
                warn(&w);
            }
            Exit::Ok
        }
        Err(e) => {
            warn(&e.message());
            if e.is_refusal() {
                Exit::Refused
            } else {
                Exit::Failed
            }
        }
    }
}

/// `schemaic exec`.
async fn run_exec(
    conns: &[Connection],
    target: &Target,
    sql: &str,
    format: Format,
    yes: bool,
    timeout: Duration,
) -> Exit {
    let (db, _tunnel, conn) = match open(conns, target).await {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    let database = database_for(target, conn);
    // The guard, and the only way to build what `exec::run` takes.
    let request = match exec::ExecRequest::approved(conn, database.as_deref(), sql, yes) {
        Ok(r) => r,
        Err(e) => {
            warn(&e.message());
            return Exit::Refused;
        }
    };
    match exec::run(&db, database.as_deref(), request, timeout).await {
        Ok(rs) => {
            // A write reports what it changed; a statement that happened to
            // return rows through `exec` reports those instead.
            match rs.affected {
                Some(n) => print!("{}", format::render_affected(n, format)),
                None => {
                    print!("{}", format::render_rows(&rs, format));
                    if let Some(w) = format::exec_truncation_warning(&rs) {
                        warn(&w);
                    }
                }
            }
            Exit::Ok
        }
        Err(e) => {
            warn(&e.message());
            Exit::Failed
        }
    }
}

/// Which database to run in: the flag if given, else the connection's own.
fn database_for(target: &Target, conn: &Connection) -> Option<String> {
    target
        .database
        .clone()
        .or_else(|| (!conn.database.is_empty()).then(|| conn.database.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exit_codes_are_the_documented_ones() {
        assert_eq!(Exit::Ok as u8, 0);
        assert_eq!(Exit::Usage as u8, 2);
        assert_eq!(Exit::Refused as u8, 3);
        assert_eq!(Exit::Failed as u8, 4);
    }

    fn conn(database: &str) -> Connection {
        let mut c: Connection = serde_json::from_str(
            r#"{"id":1,"name":"c","host":"h","port":3306,"user":"u","password":""}"#,
        )
        .unwrap();
        c.database = database.to_string();
        c
    }

    fn target(database: Option<&str>) -> Target {
        Target {
            connection: "1".to_string(),
            database: database.map(str::to_string),
            password_stdin: false,
        }
    }

    #[test]
    fn the_flag_wins_over_the_connections_own_database() {
        assert_eq!(
            database_for(&target(Some("other")), &conn("saved")),
            Some("other".to_string())
        );
    }

    #[test]
    fn the_connections_database_is_the_fallback() {
        assert_eq!(
            database_for(&target(None), &conn("saved")),
            Some("saved".to_string())
        );
    }

    /// A connection with no saved database and no flag runs unscoped, which is
    /// what the guard's `no_database` arm is there to notice — so it must be
    /// `None` rather than an empty string that reads as a real name.
    #[test]
    fn no_database_anywhere_is_none_not_an_empty_name() {
        assert_eq!(database_for(&target(None), &conn("")), None);
    }
}
