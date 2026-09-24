//! Doing what the command line asked for.
//!
//! **stdout is data; stderr is everything else.** A caller that pipes stdout
//! gets rows and nothing else — no banners, no warnings, no progress. That is
//! what makes `--format=json` safe to parse and `--format=csv` safe to
//! redirect, and it is why the truncation warning goes to stderr for every
//! format that cannot carry it in its own syntax.
//!
//! The exit code is the other half of that contract: see [`Exit`].

use std::io::{IsTerminal, Read, Write};
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
/// **Five outcomes, not two.** A caller that can only tell success from failure
/// retries the refusal that will never succeed and gives up on the timeout that
/// would have. These are stable: scripts depend on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Exit {
    /// It ran. Also the exit when the reader of stdout went away first (`| head`)
    /// — it had what it wanted.
    Ok = 0,
    /// The command line was wrong, or named a connection that is not there.
    /// Nothing was sent to a server.
    Usage = 2,
    /// A guard refused: not a read, a read-only connection, consent not given,
    /// or a connection the user has not exposed to the CLI. Nothing was sent to
    /// a server — the guard runs before the keyring is read or a tunnel opened
    /// — and retrying unchanged will refuse again.
    Refused = 3,
    /// The server or the connection failed. Retrying may work.
    Failed = 4,
    /// A **write** timed out after it was sent. It may have been applied in
    /// whole or in part — a cancel does not undo a non-transactional engine's
    /// changes — so retrying may apply it twice. Check first.
    Unknown = 5,
}

/// The exit for a connection `select` could not hand back. One that exists but
/// is not exposed is a *refusal*; one that does not exist is a usage error —
/// only one of them is worth retrying after a change in Schemaic rather than a
/// change in the command.
fn exit_for_no_connection(e: &select::NoConnection) -> Exit {
    match e {
        select::NoConnection::NotExposed(_) => Exit::Refused,
        _ => Exit::Usage,
    }
}

/// The exit for a statement that produced no result.
fn exit_for_no_rows(e: &query::NoRows) -> Exit {
    use query::NoRows;
    match e {
        NoRows::Empty | NoRows::NotARead(_) => Exit::Refused,
        NoRows::Failed(_) | NoRows::TimedOut(_) => Exit::Failed,
        NoRows::Indeterminate(_) => Exit::Unknown,
    }
}

/// A password as `--password-stdin` received it, made into the password.
///
/// **One line ending off the end, and a byte-order mark off the front.**
/// Windows PowerShell 5.1 with a UTF-8 `$OutputEncoding` pipes `EF BB BF`
/// ahead of the text, and the server was sent U+FEFF as the password's first
/// character — "Access denied" for the right password. Only *one* line ending:
/// the newline `echo` adds is not the password's, but a second one might be.
fn piped_password(raw: &str) -> String {
    let s = raw.strip_prefix('\u{FEFF}').unwrap_or(raw);
    let s = s
        .strip_suffix("\r\n")
        .or_else(|| s.strip_suffix('\n'))
        .unwrap_or(s);
    s.to_string()
}

/// Write data to stdout's handle, `out`.
///
/// **A reader that went away is a quiet end, not a crash.** `print!` panics
/// when the write fails, so `schemaic list | head -3` exited 101 — no code
/// the contract names — with a panic message on the stderr an agent reads as
/// the diagnosis. A closed pipe means the reader had what it wanted: `Ok`,
/// and nothing more is written. Any other failure is reported.
fn emit(out: &mut impl Write, s: &str) -> Result<(), Exit> {
    match out.write_all(s.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Err(Exit::Ok),
        Err(e) => {
            warn(&format!("could not write the output: {e}"));
            Err(Exit::Failed)
        }
    }
}

/// [`emit`] to the process's stdout.
fn emit_stdout(s: &str) -> Result<(), Exit> {
    emit(&mut std::io::stdout().lock(), s)
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
    // Unhydrated: `list` shows no secret, and `connect` fills in only the one
    // connection a command runs against, once its guard has said yes.
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
    if let Err(exit) = emit_stdout(&format::render_rows(
        &ResultSet::from_rows(columns, rows),
        format,
    )) {
        return exit;
    }
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

/// The saved connection `target` names, or the exit that says why not. No
/// secret is read and nothing is dialled: this is the half of opening a
/// connection the guard can run after, and before anything reaches a server.
fn select_conn<'a>(conns: &'a [Connection], target: &Target) -> Result<&'a Connection, Exit> {
    select::select(conns, &target.connection).map_err(|e| {
        warn(&e.message());
        exit_for_no_connection(&e)
    })
}

/// All of stdin, for `what` — refusing a terminal rather than waiting on it.
///
/// **A terminal is refused, not prompted.** `read_to_string` on one waits for
/// an EOF nobody knows to type, so the command just hung — and a prompt would
/// echo a password onto the screen. Stdin here is for a pipe; say so.
fn read_stdin(what: &str, example: &str) -> Result<String, Exit> {
    if std::io::stdin().is_terminal() {
        warn(&format!(
            "{what} is read from a pipe, and stdin is a terminal; pipe it in \
             (e.g. `{example}`)"
        ));
        return Err(Exit::Usage);
    }
    let mut text = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut text) {
        warn(&format!("could not read {what} from stdin: {e}"));
        return Err(Exit::Usage);
    }
    Ok(text)
}

/// The statement to run: the argument, or stdin when it is `-`.
///
/// Stdin has one reader, so a statement and a password cannot both come from
/// it. An argument that carries a credential is run, but said — it is in the
/// process list for as long as the command runs and in the shell's history
/// after, the exposure the app keeps such statements out of its own history for.
fn statement(sql: &str, target: &Target, conn: &Connection) -> Result<String, Exit> {
    if sql == crate::args::SQL_FROM_STDIN {
        if target.password_stdin {
            warn(
                "the statement and --password-stdin cannot both come from stdin; \
                 leave the password to the OS keyring, or pass the statement as an argument",
            );
            return Err(Exit::Usage);
        }
        return read_stdin("the statement", "schemaic exec -c … - < change.sql");
    }
    let dialect = schemaic_core::intel::SqlDialect::from_db_type(&conn.db_type);
    if schemaic_core::sql::carries_credential(sql, dialect) {
        warn(
            "this statement carries a password, which the command line puts in the \
             process list and your shell's history; pass `-` and pipe it on stdin instead",
        );
    }
    Ok(sql.to_string())
}

/// Hydrate `conn` and open it, tunnel included — **only after the guard has
/// said yes**, since this is the step that reads the keyring and logs in over
/// SSH.
///
/// The `TunnelHandle` is returned alongside the `Db` because dropping it closes
/// the tunnel — holding it for exactly as long as the connection is used is the
/// whole of its lifetime management.
///
/// **The tunnel is bounded by `timeout`**, which the statement's deadline never
/// covered: an SSH endpoint that accepts TCP and never sends a banner held the
/// command for as long as nobody killed it.
async fn connect(
    conn: &Connection,
    target: &Target,
    timeout: Duration,
) -> Result<(Db, Option<schemaic_db::ssh::TunnelHandle>), Exit> {
    // Read before the keyring is touched, so a refused terminal is refused
    // before anything else is said about the connection.
    let piped = if target.password_stdin {
        let raw = read_stdin(
            "--password-stdin's password",
            "printf '%s' \"$PASSWORD\" | schemaic …",
        )?;
        Some(piped_password(&raw))
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
        let opened = tokio::time::timeout(
            timeout,
            schemaic_db::ssh::open_tunnel(&conn.ssh, &conn.host, conn.port),
        )
        .await;
        match opened {
            Ok(Ok(t)) => Some(t),
            Ok(Err(e)) => {
                warn(&format!("ssh tunnel failed: {e}"));
                return Err(Exit::Failed);
            }
            Err(_) => {
                warn(&format!(
                    "ssh tunnel failed: no answer within {}s",
                    timeout.as_secs().max(1)
                ));
                return Err(Exit::Failed);
            }
        }
    } else {
        None
    };
    let db = Db::connect(&conn, tunnel.as_ref().map(|t| t.port()));
    Ok((db, tunnel))
}

/// The rows, then what could not be said in them — on stderr.
fn emit_rows(rs: &ResultSet, format: Format) -> Result<(), Exit> {
    emit_stdout(&format::render_rows(rs, format))?;
    if let Some(w) = format::withheld_warning(rs, format) {
        warn(&w);
    }
    Ok(())
}

/// `schemaic query`.
///
/// **Select, gate, then connect.** A refused statement exits 3, which promises
/// nothing reached a server; opening first meant a loop on a refusal made a
/// real SSH login each time round.
async fn run_query(
    conns: &[Connection],
    target: &Target,
    sql: &str,
    format: Format,
    limit: usize,
    timeout: Duration,
) -> Exit {
    let conn = match select_conn(conns, target) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let sql = match statement(sql, target, conn) {
        Ok(s) => s,
        Err(exit) => return exit,
    };
    let dialect = schemaic_core::intel::SqlDialect::from_db_type(&conn.db_type);
    if let Err(e) = query::gate(&sql, dialect) {
        warn(&e.message());
        return exit_for_no_rows(&e);
    }
    let (db, _tunnel) = match connect(conn, target, timeout).await {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    let database = database_for(target, conn);
    match query::read_only_query(&db, database.as_deref(), &sql, limit, timeout).await {
        Ok(rs) => {
            if let Err(exit) = emit_rows(&rs, format) {
                return exit;
            }
            if let Some(w) = format::truncation_warning(&rs, format) {
                warn(&w);
            }
            Exit::Ok
        }
        Err(e) => {
            warn(&e.message());
            exit_for_no_rows(&e)
        }
    }
}

/// `schemaic exec`. Select, approve, then connect — [`run_query`]'s order, for
/// its reason.
async fn run_exec(
    conns: &[Connection],
    target: &Target,
    sql: &str,
    format: Format,
    yes: bool,
    timeout: Duration,
) -> Exit {
    let conn = match select_conn(conns, target) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let sql = match statement(sql, target, conn) {
        Ok(s) => s,
        Err(exit) => return exit,
    };
    let database = database_for(target, conn);
    // The guard, and the only way to build what `exec::run` takes. Its subject
    // is the *saved* connection — what the user configured.
    let request = match exec::ExecRequest::approved(conn, database.as_deref(), &sql, yes) {
        Ok(r) => r,
        Err(e) => {
            warn(&e.message());
            return Exit::Refused;
        }
    };
    let (db, _tunnel) = match connect(conn, target, timeout).await {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    match exec::run(&db, database.as_deref(), request, timeout).await {
        Ok(rs) => {
            // A write reports what it changed; a statement that happened to
            // return rows through `exec` reports those instead.
            let shown = match rs.affected {
                Some(n) => emit_stdout(&format::render_affected(n, format)),
                None => emit_rows(&rs, format).map(|()| {
                    if let Some(w) = format::exec_truncation_warning(&rs) {
                        warn(&w);
                    }
                }),
            };
            shown.err().unwrap_or(Exit::Ok)
        }
        Err(e) => {
            warn(&e.message());
            exit_for_no_rows(&e)
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
        assert_eq!(Exit::Unknown as u8, 5);
    }

    /// Which outcome gets which code is the contract scripts retry on.
    #[test]
    fn a_connection_not_exposed_is_a_refusal_and_a_missing_one_is_usage() {
        use select::NoConnection;
        assert_eq!(
            exit_for_no_connection(&NoConnection::NotExposed("prod".into())),
            Exit::Refused
        );
        assert_eq!(
            exit_for_no_connection(&NoConnection::Unknown("prod".into())),
            Exit::Usage
        );
    }

    #[test]
    fn a_guard_refusal_a_server_failure_and_an_unknown_write_exit_differently() {
        use query::NoRows;
        let d = Duration::from_secs(1);
        assert_eq!(exit_for_no_rows(&NoRows::Empty), Exit::Refused);
        assert_eq!(
            exit_for_no_rows(&NoRows::NotARead("x".into())),
            Exit::Refused
        );
        assert_eq!(exit_for_no_rows(&NoRows::Failed("x".into())), Exit::Failed);
        assert_eq!(exit_for_no_rows(&NoRows::TimedOut(d)), Exit::Failed);
        // **Not retry-safe.** A timed-out write that reported "cancelled" and
        // exit 4 — "retrying may work" — had updated nine of forty rows.
        assert_eq!(exit_for_no_rows(&NoRows::Indeterminate(d)), Exit::Unknown);
        let m = NoRows::Indeterminate(d).message();
        assert!(!m.contains("cancelled"), "{m}");
        assert!(m.contains("may have been applied"), "{m}");
    }

    /// **PowerShell 5.1's byte-order mark is not part of the password.**
    #[test]
    fn a_piped_password_loses_a_bom_and_one_line_ending() {
        assert_eq!(piped_password("\u{FEFF}pw\r\n"), "pw");
        assert_eq!(piped_password("pw\n"), "pw");
        assert_eq!(piped_password("pw"), "pw");
        // Only one line ending is the pipe's; a second is the password's.
        assert_eq!(piped_password("pw\n\n"), "pw\n");
        // A BOM anywhere but the front is data.
        assert_eq!(piped_password("p\u{FEFF}w"), "p\u{FEFF}w");
    }

    struct Broken(std::io::ErrorKind);

    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// **A closed pipe is a quiet end, not a panic.** `list | head` exited 101.
    #[test]
    fn a_closed_stdout_ends_quietly_and_any_other_write_error_fails() {
        assert_eq!(
            emit(&mut Broken(std::io::ErrorKind::BrokenPipe), "rows"),
            Err(Exit::Ok)
        );
        assert_eq!(
            emit(&mut Broken(std::io::ErrorKind::Other), "rows"),
            Err(Exit::Failed)
        );
        let mut buf = Vec::new();
        assert_eq!(emit(&mut buf, "rows"), Ok(()));
        assert_eq!(buf, b"rows");
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
