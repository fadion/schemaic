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
use schemaic_core::sql::NoDatabaseFailure;
use schemaic_db::Db;

use crate::args::{Cli, Command, ConnArgs, SqlArgs, SqlSource, Target};
use crate::format::{self, Format};
use crate::{exec, query, select};

/// What the process exits with.
///
/// **Six outcomes, not two.** A caller that can only tell success from failure
/// retries the refusal that will never succeed and gives up on the timeout that
/// would have. These are stable: scripts depend on them — a new outcome gets a
/// new number, never an old one.
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
    /// A read ran and its rows were printed, but `--limit` cut them short —
    /// only under `query --fail-on-cap`, for a caller that reads stdout and not
    /// stderr. Not 4: nothing failed, and retrying returns the same cap.
    Capped = 6,
}

/// The exit for a read that returned rows: [`Exit::Capped`] if the cap bit and
/// the caller asked to hear about it with a code, else success.
fn exit_for_rows(rs: &ResultSet, fail_on_cap: bool) -> Exit {
    if rs.truncated && fail_on_cap {
        Exit::Capped
    } else {
        Exit::Ok
    }
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

/// What to do about the [`warn`] just before it, on a line of its own so the
/// error above stays the driver's words alone.
fn hint(message: &str) {
    eprintln!("hint: {message}");
}

/// Parse, then run. The whole entry point, for both front ends.
pub fn main<I, T>(argv: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let argv = crate::args::comment_led_sql_last(argv.into_iter().map(Into::into).collect());
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
    // **Before the saved connections are read**: a version needs none of them,
    // and a damaged file must not stop anyone learning which build they have —
    // the first thing to ask when reporting that it is damaged.
    if command == Command::Version {
        return emit_stdout(&crate::args::version_text())
            .err()
            .unwrap_or(Exit::Ok);
    }
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
        Command::Version => unreachable!("`version` is answered before the file is read"),
        Command::List { all, format } => list(&file.connections, all, format),
        Command::Databases {
            conn,
            format,
            timeout,
        } => {
            databases(
                &file.connections,
                &conn,
                format,
                Duration::from_secs(timeout),
            )
            .await
        }
        Command::Query {
            sql,
            target,
            format,
            limit,
            fail_on_cap,
            timeout,
        } => {
            run_query(
                &file.connections,
                &target,
                &sql,
                format,
                limit,
                fail_on_cap,
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

/// `schemaic databases` — the names `-d` takes on this connection.
///
/// The same list the app's schema tree shows (`Db::fetch_databases`, system
/// schemas left out), rendered like any other rows so `--format` means what it
/// means everywhere. It reaches a server, so it is a CLI-access connection like
/// the rest; there is no statement for a guard to judge.
async fn databases(
    conns: &[Connection],
    args: &ConnArgs,
    format: Format,
    timeout: Duration,
) -> Exit {
    let conn = match select_conn(conns, args) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let (db, _tunnel) = match connect(conn, args, timeout).await {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    // `fetch_databases` takes no token — there is no statement of the user's
    // to `KILL` — and gives up by itself at `PING_TIMEOUT`, so the cancel here
    // reaches nothing: a `--timeout` past five seconds never fires on the
    // listing, and a shorter one fails a late answer without returning sooner.
    // It is wrapped because `deadline`'s gate holds every read in this crate to
    // it; what `--timeout` really bounds for this command is the SSH tunnel.
    let token = tokio_util::sync::CancellationToken::new();
    let names = match crate::deadline::with_deadline(db.fetch_databases(), token, timeout).await {
        Some(Ok(names)) => names,
        Some(Err(e)) => {
            warn(&e.to_string());
            return Exit::Failed;
        }
        None => {
            warn(&format!(
                "listing the databases exceeded {}s",
                timeout.as_secs().max(1)
            ));
            return Exit::Failed;
        }
    };
    let rows = names.into_iter().map(|n| vec![Value::Str(n)]).collect();
    let rs = ResultSet::from_rows(vec![text_column("database")], rows);
    emit_stdout(&format::render_rows(&rs, format))
        .err()
        .unwrap_or(Exit::Ok)
}

/// What to say after a failure that [`schemaic_core::sql::no_database_failure`]
/// recognises, or the guard's own no-database refusal: how to name one, and how
/// to find out which there are.
///
/// `connection` is the `-c` the user gave, echoed back so the command can be
/// copied; quoted when it has a space, since a name is allowed one.
fn no_database_hint(connection: &str, how: NoDatabaseFailure) -> String {
    let c = if connection.contains(char::is_whitespace) {
        format!("\"{connection}\"")
    } else {
        connection.to_string()
    };
    // A statement that ran somewhere has to be told where, or "does not
    // exist" reads as a wrong table name. One that was refused did not run,
    // and saying it did is worse than saying nothing.
    let why = match how {
        NoDatabaseFailure::RanElsewhere => {
            "this connection has no default database, so the statement ran in the \
             server's maintenance database"
        }
        NoDatabaseFailure::Refused => "this connection has no default database",
    };
    format!(
        "{why}; pass -d <database>, or pick a default for the connection in \
         Schemaic. `schemaic databases -c {c}` lists them"
    )
}

/// The saved connection `args` names, or the exit that says why not. No
/// secret is read and nothing is dialled: this is the half of opening a
/// connection the guard can run after, and before anything reaches a server.
fn select_conn<'a>(conns: &'a [Connection], args: &ConnArgs) -> Result<&'a Connection, Exit> {
    select::select(conns, &args.connection).map_err(|e| {
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

/// Statement text read from a pipe or a file, with a byte-order mark taken
/// off the front.
///
/// **An editor's BOM is not SQL.** Notepad and Windows PowerShell 5.1 both put
/// `EF BB BF` ahead of UTF-8 text, and U+FEFF before `SELECT` is not a
/// statement head the read gate knows: `query -f q.sql` was refused as "not a
/// read" for a file that said nothing else. [`piped_password`] strips the same
/// mark for the same reason.
fn sql_text(raw: String) -> String {
    match raw.strip_prefix('\u{FEFF}') {
        Some(rest) => rest.to_string(),
        None => raw,
    }
}

/// The statement in the file at `path`, or the usage exit that says why not
/// — nothing has been sent anywhere yet, and the fix is in the command line.
fn read_sql_file(path: &std::path::Path) -> Result<String, Exit> {
    let bytes = std::fs::read(path).map_err(|e| {
        warn(&format!(
            "could not read the statement from {}: {e}",
            path.display()
        ));
        Exit::Usage
    })?;
    String::from_utf8(bytes).map_err(|_| {
        warn(&format!(
            "{} is not UTF-8 text; save it as UTF-8 and try again",
            path.display()
        ));
        Exit::Usage
    })
}

/// The statement to run: the argument, stdin, or a file.
///
/// Stdin has one reader, so a statement and a password cannot both come from
/// it. An argument that carries a credential is run, but said — it is in the
/// process list for as long as the command runs and in the shell's history
/// after, the exposure the app keeps such statements out of its own history for.
/// Stdin and a file are neither, so they are not warned about.
fn statement(sql: &SqlArgs, target: &Target, conn: &Connection) -> Result<String, Exit> {
    let sql = match sql.source() {
        SqlSource::Stdin => {
            if target.conn.password_stdin {
                warn(
                    "the statement and --password-stdin cannot both come from stdin; \
                     leave the password to the OS keyring, or pass the statement as an \
                     argument or with -f",
                );
                return Err(Exit::Usage);
            }
            return read_stdin("the statement", "schemaic exec -c … - < change.sql").map(sql_text);
        }
        SqlSource::File(path) => return read_sql_file(path).map(sql_text),
        SqlSource::Text(sql) => sql,
    };
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
    args: &ConnArgs,
    timeout: Duration,
) -> Result<(Db, Option<schemaic_db::ssh::TunnelHandle>), Exit> {
    // Read before the keyring is touched, so a refused terminal is refused
    // before anything else is said about the connection.
    let piped = if args.password_stdin {
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
    sql: &SqlArgs,
    format: Format,
    limit: usize,
    fail_on_cap: bool,
    timeout: Duration,
) -> Exit {
    let conn = match select_conn(conns, &target.conn) {
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
    let (db, _tunnel) = match connect(conn, &target.conn, timeout).await {
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
            exit_for_rows(&rs, fail_on_cap)
        }
        Err(e) => {
            warn(&e.message());
            if let Some(how) = no_database_failure(database.as_deref(), &e, dialect) {
                hint(&no_database_hint(&target.conn.connection, how));
            }
            exit_for_no_rows(&e)
        }
    }
}

/// Did this statement fail because it ran with no database — so the
/// [`no_database_hint`] is the next thing to say?
///
/// Both halves, because each alone is wrong: the error text of a missing table
/// on a *scoped* run matches on PostgreSQL, and an unscoped run fails for every
/// other reason too.
fn no_database_failure(
    database: Option<&str>,
    e: &query::NoRows,
    dialect: schemaic_core::intel::SqlDialect,
) -> Option<NoDatabaseFailure> {
    match e {
        query::NoRows::Failed(m) if database.is_none() => {
            schemaic_core::sql::no_database_failure(m, dialect)
        }
        _ => None,
    }
}

/// Did the write guard refuse for want of a database? It says so on PostgreSQL,
/// where the server would not — see `sql::needs_database`.
fn refused_for_no_database(e: &exec::NotRun) -> bool {
    matches!(e, exec::NotRun::Blocked(why) if why == schemaic_core::sql::NO_DATABASE_SELECTED)
}

/// `schemaic exec`. Select, approve, then connect — [`run_query`]'s order, for
/// its reason.
async fn run_exec(
    conns: &[Connection],
    target: &Target,
    sql: &SqlArgs,
    format: Format,
    yes: bool,
    timeout: Duration,
) -> Exit {
    let conn = match select_conn(conns, &target.conn) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let sql = match statement(sql, target, conn) {
        Ok(s) => s,
        Err(exit) => return exit,
    };
    let dialect = schemaic_core::intel::SqlDialect::from_db_type(&conn.db_type);
    let database = database_for(target, conn);
    // The guard, and the only way to build what `exec::run` takes. Its subject
    // is the *saved* connection — what the user configured.
    let request = match exec::ExecRequest::approved(conn, database.as_deref(), &sql, yes) {
        Ok(r) => r,
        Err(e) => {
            warn(&e.message());
            if refused_for_no_database(&e) {
                hint(&no_database_hint(
                    &target.conn.connection,
                    NoDatabaseFailure::Refused,
                ));
            }
            return Exit::Refused;
        }
    };
    // Connected through the request, so the statement runs on the connection
    // its verdict judged — not on whatever a second caller had to hand.
    let (db, _tunnel) = match connect(request.connection(), &target.conn, timeout).await {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    match exec::run(&db, request, timeout).await {
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
            if let Some(how) = no_database_failure(database.as_deref(), &e, dialect) {
                hint(&no_database_hint(&target.conn.connection, how));
            }
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
        assert_eq!(Exit::Capped as u8, 6);
    }

    fn two_rows(truncated: bool) -> ResultSet {
        let mut rs = ResultSet::from_rows(
            vec![text_column("id")],
            vec![vec![Value::Int(1)], vec![Value::Int(2)]],
        );
        rs.truncated = truncated;
        rs
    }

    /// **Issue #3: a capped read exits 6 when the caller asked for it**, and
    /// its own code rather than 4 — the rows it printed are right, there are
    /// only more of them, and "retrying may work" is not what a script should
    /// do about a cap.
    #[test]
    fn a_capped_read_exits_6_only_when_asked_to() {
        assert_eq!(exit_for_rows(&two_rows(true), true), Exit::Capped);
        assert_eq!(exit_for_rows(&two_rows(true), false), Exit::Ok);
    }

    /// A complete result is a success with or without the flag.
    #[test]
    fn a_complete_read_exits_0_whatever_the_flag_says() {
        assert_eq!(exit_for_rows(&two_rows(false), true), Exit::Ok);
        assert_eq!(exit_for_rows(&two_rows(false), false), Exit::Ok);
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

    /// **An editor's byte-order mark is not part of the statement.** The read
    /// gate refuses U+FEFF ahead of `SELECT`, so a Notepad-saved `-f q.sql`
    /// (or PowerShell 5.1 piping one) came back "not a read".
    #[test]
    fn a_statement_from_a_file_or_pipe_loses_its_bom() {
        use schemaic_core::intel::SqlDialect;
        let raw = "\u{FEFF}SELECT 1\n".to_string();
        assert!(
            query::gate(&raw, SqlDialect::MySql).is_err(),
            "the bug: the gate refuses a BOM-led read"
        );
        let sql = sql_text(raw);
        assert_eq!(sql, "SELECT 1\n");
        assert_eq!(query::gate(&sql, SqlDialect::MySql), Ok("SELECT 1"));
        // Only a leading mark is the file's; anywhere else it is data.
        assert_eq!(sql_text("SELECT '\u{FEFF}'".into()), "SELECT '\u{FEFF}'");
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
            conn: ConnArgs {
                connection: "1".to_string(),
                password_stdin: false,
            },
            database: database.map(str::to_string),
        }
    }

    const MYSQL_1046: &str =
        "query failed: Server error: `ERROR 1046 (3D000): No database selected'";

    /// **The case issue #2 reported**: a MySQL connection with no database,
    /// no `-d`, and the server's 1046. The hint names the flag and the command
    /// that lists what to give it, for the connection the user typed.
    #[test]
    fn an_unscoped_1046_gets_the_hint_naming_d_and_databases() {
        use schemaic_core::intel::SqlDialect;
        let e = query::NoRows::Failed(MYSQL_1046.into());
        let how = no_database_failure(None, &e, SqlDialect::MySql)
            .expect("an unscoped 1046 is the no-database failure");
        let h = no_database_hint("AEU", how);
        assert!(h.contains("-d <database>"), "{h}");
        assert!(h.contains("schemaic databases -c AEU"), "{h}");
    }

    /// **A scoped run is never told to pass `-d`** — on PostgreSQL its missing
    /// table reads exactly like the unscoped one, and it already has a database.
    #[test]
    fn a_scoped_failure_gets_no_hint() {
        use schemaic_core::intel::SqlDialect;
        let pg = query::NoRows::Failed("query failed: relation \"company\" does not exist".into());
        assert_eq!(
            no_database_failure(None, &pg, SqlDialect::Postgres),
            Some(NoDatabaseFailure::RanElsewhere)
        );
        assert_eq!(
            no_database_failure(Some("app"), &pg, SqlDialect::Postgres),
            None
        );
        let my = query::NoRows::Failed(MYSQL_1046.into());
        assert_eq!(
            no_database_failure(Some("app"), &my, SqlDialect::MySql),
            None
        );
    }

    /// Only a server failure carries the server's words; a timeout or a
    /// refusal on an unscoped run is not this.
    #[test]
    fn only_a_server_failure_can_be_the_no_database_one() {
        use schemaic_core::intel::SqlDialect;
        for e in [
            query::NoRows::Empty,
            query::NoRows::NotARead(MYSQL_1046.into()),
            query::NoRows::TimedOut(Duration::from_secs(1)),
            query::NoRows::Indeterminate(Duration::from_secs(1)),
        ] {
            assert_eq!(
                no_database_failure(None, &e, SqlDialect::MySql),
                None,
                "{e:?}"
            );
        }
    }

    /// A statement that ran somewhere is told where — or "does not exist"
    /// reads as a typo in the table name. **One that was refused is not**:
    /// the first cut said "ran in the maintenance database" after `exec`'s
    /// guard on PostgreSQL had refused it and nothing had run at all.
    #[test]
    fn only_a_statement_that_ran_is_told_where_it_ran() {
        assert!(
            no_database_hint("pg", NoDatabaseFailure::RanElsewhere)
                .contains("maintenance database")
        );
        assert!(!no_database_hint("pg", NoDatabaseFailure::Refused).contains("maintenance"));
    }

    /// A connection name may have a space; the echoed command must still be
    /// one argument when pasted.
    #[test]
    fn a_connection_name_with_a_space_is_quoted_in_the_hint() {
        let h = no_database_hint("Prod EU", NoDatabaseFailure::Refused);
        assert!(h.contains("schemaic databases -c \"Prod EU\""), "{h}");
    }

    /// `exec`'s guard refuses an unscoped PostgreSQL write itself, before the
    /// server can; that refusal gets the same hint, and no other refusal does.
    #[test]
    fn the_guards_no_database_refusal_is_recognised_and_nothing_else_is() {
        use exec::NotRun;
        let mut pg = conn("");
        pg.db_type = "PostgreSQL".to_string();
        pg.cli_access = true;
        let e = exec::ExecRequest::approved(&pg, None, "CREATE TABLE t (id int)", false)
            .expect_err("an unscoped CREATE TABLE on PostgreSQL is refused");
        assert!(refused_for_no_database(&e), "{e:?}");
        for other in [
            NotRun::Empty,
            NotRun::Several,
            NotRun::Blocked("Read-only connection.".into()),
            NotRun::NeedsConsent(schemaic_core::sql::NO_DATABASE_SELECTED.into()),
        ] {
            assert!(!refused_for_no_database(&other), "{other:?}");
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
