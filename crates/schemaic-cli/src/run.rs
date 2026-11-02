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
use schemaic_core::params::Binding;
use schemaic_core::secrets::SecretKind;
use schemaic_core::sql::NoDatabaseFailure;
use schemaic_db::Db;

use crate::args::{Command, ConnArgs, McpTarget, OutputArgs, SqlArgs, SqlSource, Target};
use crate::format::{self, Format, Output};
use crate::{catalog, exec, query, select};

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
    /// The command line was wrong, or named a connection, a `-f` file or a
    /// `describe` table that is not there. Nothing was sent to a server — or,
    /// for the table, nothing but the lookup that found it missing.
    ///
    /// **A performance read the server cannot answer comes here too**: a
    /// statement-statistics source that is off or unreadable (`statements`,
    /// which says the statement that turns it on), `indexes --missing` or
    /// `locks` on an engine without them, `sizes --indexes` where indexes are
    /// sized per table. The answer is not there, as the table is not, and
    /// retrying will not bring it — what a script must not read as an empty
    /// list.
    Usage = 2,
    /// A guard refused: not a read, a read-only connection, consent not given,
    /// or a connection the user has not exposed to the CLI. Nothing was sent to
    /// a server — the guard runs before the keyring is read or a tunnel opened
    /// — and retrying unchanged will refuse again.
    ///
    /// **The database layer's standing refusals come here too**
    /// (`NoRows::DbRefused`, `DbRefusal::stands`): init SQL the allowlist will
    /// not run, a sign-in the transport cannot carry, a MySQL session whose
    /// character set or `sql_mode` cannot be made safe, a plan MySQL will not
    /// make read-only, a read-only PostgreSQL session on a path that shadows a
    /// builtin. Some of those connect to find out, so "nothing was sent" is not
    /// theirs to promise — but your statement never ran, and retrying
    /// unchanged refuses again, which is what a caller branches on 3 for, and
    /// why they are not 4.
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
    /// `import`: the file was checked whole and has problems, each listed on
    /// stderr — or could not be read as a file of its format at all, or is
    /// empty; nothing was written. Not 3: no guard refused, and the fix is
    /// in the file — not 2, since the command line was right.
    Rejected = 7,
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
        NoRows::Empty | NoRows::NotARead(_) | NoRows::NotPermitted(_) | NoRows::DbRefused(_) => {
            Exit::Refused
        }
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

/// The stack a headless front end's runtime thread gets — Linux's main-thread
/// default, and what `schemaic mcp` and `--mcp-serve` had there all along.
///
/// Reserved, not committed: the pages are touched only as deep as a call goes.
const RUNTIME_STACK: usize = 8 << 20;

/// Run the future `f` builds to completion on a **current-thread runtime of its
/// own, on a thread with [`RUNTIME_STACK`] of stack** — for the two front ends
/// that never build a window, this CLI's [`main`] and the app's `--mcp-serve`.
///
/// **Not on the calling thread, because that is the process's main thread**,
/// and `block_on` keeps the whole future on its stack. Windows gives the main
/// thread 1 MiB, where Linux gives 8: the MCP server's `list_schema` overflowed
/// it on every engine, the process died mid-call, and the agent reported the
/// tool's server as *connection closed* against a database the editor was
/// querying fine. The GUI never met it — its work runs on tokio's worker
/// threads, with 2 MiB each. A debug build overflowed on `schemaic version`,
/// since [`dispatch`]'s future holds every command's state at once.
///
/// `f` builds the future on the new thread, so the future itself need not be
/// `Send`. A panic there is resumed here, so a bug still ends the process the
/// way it did on the main thread; `Err` is only the thread or the runtime
/// failing to start.
pub fn block_on_own_stack<F, Fut>(f: F) -> std::io::Result<Fut::Output>
where
    F: FnOnce() -> Fut + Send,
    Fut: std::future::Future,
    Fut::Output: Send,
{
    std::thread::scope(|s| {
        let runner = std::thread::Builder::new()
            .name("schemaic-runtime".to_string())
            .stack_size(RUNTIME_STACK)
            .spawn_scoped(s, move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                Ok(rt.block_on(f()))
            })?;
        runner
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

/// Parse, then run. The whole entry point, for both front ends.
pub fn main<I, T>(argv: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = match crate::args::parse_argv(argv.into_iter().map(Into::into).collect()) {
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
    // window — and, like it, not on the main thread: see `block_on_own_stack`.
    match block_on_own_stack(|| dispatch(cli.command)) {
        Ok(exit) => exit.into(),
        Err(e) => {
            warn(&format!("could not start: {e}"));
            Exit::Failed.into()
        }
    }
}

/// Why `command` is refused when this process descends from an agent the AI
/// panel spawned (`under_agent`: [`schemaic_core::AGENT_CHILD_ENV`] is set),
/// or `None`. Every command but `version` and `completions` reaches a saved
/// connection, and the assistant's way to one is the panel's tools, at the
/// level the user chose.
fn agent_refusal(command: &Command, under_agent: bool) -> Option<&'static str> {
    let reaches_a_connection = !matches!(command, Command::Version | Command::Completions { .. });
    (under_agent && reaches_a_connection).then_some(
        "refused: this is running under Schemaic's AI panel, whose assistant reaches \
         the database through the panel's own tools, not the command line",
    )
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
    // A completion script is the parser's own shape: no connection, so it is
    // answered here for the same reason as the version.
    if let Command::Completions { shell } = command {
        return emit_stdout(&crate::args::completion_script(shell))
            .err()
            .unwrap_or(Exit::Ok);
    }
    if let Some(why) = agent_refusal(
        &command,
        std::env::var_os(schemaic_core::AGENT_CHILD_ENV).is_some(),
    ) {
        warn(why);
        return Exit::Refused;
    }
    // A command line that cannot be printed is a usage error before anything
    // is read, the way clap's own refusals are.
    let out = match command.output_args().map(OutputArgs::output).transpose() {
        Ok(out) => out.unwrap_or_else(|| Format::Table.into()),
        Err(e) => {
            warn(&e);
            return Exit::Usage;
        }
    };
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
        Command::Version | Command::Completions { .. } => {
            unreachable!("`version` and `completions` are answered before the file is read")
        }
        Command::List { all, output: _ } => list(&file.connections, all, out),
        Command::Databases {
            conn,
            output: _,
            timeout,
        } => databases(&file.connections, &conn, out, Duration::from_secs(timeout)).await,
        Command::Ping {
            conn,
            output: _,
            timeout,
        } => ping(&file.connections, &conn, out, Duration::from_secs(timeout)).await,
        Command::Tables {
            target,
            output: _,
            limit,
            timeout,
        } => {
            tables(
                &file.connections,
                &target,
                out,
                limit,
                Duration::from_secs(timeout),
            )
            .await
        }
        Command::Describe {
            table,
            target,
            output: _,
            timeout,
        } => {
            describe(
                &file.connections,
                &target,
                &table,
                out,
                Duration::from_secs(timeout),
            )
            .await
        }
        Command::Statements {
            target,
            output: _,
            order,
            limit,
            timeout,
        } => {
            statements(
                &file.connections,
                &target,
                out,
                order,
                limit,
                Duration::from_secs(timeout),
            )
            .await
        }
        Command::Indexes {
            target,
            output: _,
            missing,
            timeout,
        } => {
            indexes(
                &file.connections,
                &target,
                out,
                missing,
                Duration::from_secs(timeout),
            )
            .await
        }
        Command::Sizes {
            target,
            output: _,
            indexes,
            limit,
            timeout,
        } => {
            sizes(
                &file.connections,
                &target,
                out,
                indexes,
                limit,
                Duration::from_secs(timeout),
            )
            .await
        }
        Command::Locks {
            conn,
            output: _,
            timeout,
        } => locks(&file.connections, &conn, out, Duration::from_secs(timeout)).await,
        Command::Query {
            sql,
            params,
            target,
            output: _,
            limit,
            fail_on_cap,
            timeout,
        } => {
            run_query(
                &file.connections,
                &target,
                &sql,
                &params.params,
                out,
                limit,
                fail_on_cap,
                Duration::from_secs(timeout),
            )
            .await
        }
        Command::Explain {
            sql,
            params,
            target,
            output: _,
            timeout,
        } => {
            run_explain(
                &file.connections,
                &target,
                &sql,
                &params.params,
                out,
                Duration::from_secs(timeout),
            )
            .await
        }
        Command::Exec {
            sql,
            params,
            target,
            output: _,
            yes,
            script,
            timeout,
        } => {
            let timeout = Duration::from_secs(crate::args::exec_timeout(script, timeout));
            if script {
                run_script(&file.connections, &target, &sql, yes, timeout).await
            } else {
                run_exec(
                    &file.connections,
                    &target,
                    &sql,
                    &params.params,
                    out,
                    yes,
                    timeout,
                )
                .await
            }
        }
        Command::Export {
            sql,
            params,
            target,
            output,
            format,
            timeout,
        } => {
            run_export(
                &file.connections,
                &target,
                &sql,
                &params.params,
                &output,
                format.as_deref(),
                Duration::from_secs(timeout),
            )
            .await
        }
        Command::Dump {
            target,
            output,
            tables,
            schema_only,
            data_only,
            timeout,
        } => {
            let opts = schemaic_core::dump::DumpOptions {
                structure: !data_only,
                data: !schema_only,
                ..Default::default()
            };
            run_dump(
                &file.connections,
                &target,
                &output,
                tables,
                opts,
                Duration::from_secs(timeout),
            )
            .await
        }
        Command::Compare {
            connection,
            database,
            source_connection,
            source_database,
            tables,
            timeout,
        } => {
            let sides = CompareSides {
                connection: &connection,
                database: database.as_deref(),
                source_connection: source_connection.as_deref().unwrap_or(&connection),
                source_database: source_database.as_deref(),
            };
            run_compare(
                &file.connections,
                sides,
                &tables,
                Duration::from_secs(timeout),
            )
            .await
        }
        Command::Import {
            file: path,
            table,
            target,
            format,
            no_header,
            delimiter,
            sheet,
            nulls,
            trim,
            maps,
            skips,
            timeout,
        } => {
            let reading = crate::import::Reading {
                format,
                no_header,
                delimiter,
                sheet,
                nulls,
                trim,
            };
            let load = ImportLoad {
                path: &path,
                table: &table,
                reading: &reading,
                maps: &maps,
                skips: &skips,
            };
            run_import(
                &file.connections,
                &target,
                load,
                Duration::from_secs(timeout),
            )
            .await
        }
        Command::Mcp { target } => serve_mcp(&file.connections, &target).await,
    }
}

/// The two databases `compare` was named, as typed.
struct CompareSides<'a> {
    connection: &'a str,
    database: Option<&'a str>,
    source_connection: &'a str,
    source_database: Option<&'a str>,
}

/// One side of a `compare`: `database`'s whole schema, bounded by `timeout`,
/// or the line that says why not.
async fn read_schema(
    db: &Db,
    database: &str,
    timeout: Duration,
) -> Result<schemaic_core::schema::DbSchema, String> {
    let token = tokio_util::sync::CancellationToken::new();
    match crate::deadline::with_deadline(db.fetch_schema(database, token.clone()), token, timeout)
        .await
    {
        Some(Ok(s)) => Ok(s),
        Some(Err(e)) => Err(format!("reading {database}: {e}")),
        None => Err(format!(
            "reading {database} exceeded {}s and was stopped",
            timeout.as_secs().max(1)
        )),
    }
}

/// How `compare` names one side on stderr: the database, and the connection
/// too when the two sides are on different ones.
fn side_label(conn: &Connection, database: &str, two_connections: bool) -> String {
    if two_connections {
        format!("{database} on {}", conn.name)
    } else {
        database.to_string()
    }
}

/// `schemaic compare`: the app's Compare with, headless — both schemas read
/// fresh (`Db::fetch_schema`, concurrently), `core::compare` over them, and
/// the plan's script printed. **Nothing is run**, so there is no guard to
/// pass: what reaches the servers is catalogue reads, as `dump`'s are.
///
/// Refused before anything is dialled: a connection without CLI access (on
/// either side), a side with no database, a database compared with itself,
/// and two engines `compare::comparable` will not pair.
async fn run_compare(
    conns: &[Connection],
    sides: CompareSides<'_>,
    tables: &[String],
    timeout: Duration,
) -> Exit {
    // No `--password-stdin` here: with two connections it would be unclear
    // whose password it was, so both come from the keyring.
    let conn_args = |c: &str| ConnArgs {
        connection: Some(c.to_string()),
        ..Default::default()
    };
    let (target_args, source_args) = (
        conn_args(sides.connection),
        conn_args(sides.source_connection),
    );
    let target = match select_conn(conns, &target_args) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let source = match select_conn(conns, &source_args) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let (target, source) = (&*target, &*source);
    let (Some(target_db), Some(source_db)) = (
        default_database(sides.database, target),
        default_database(sides.source_database, source),
    ) else {
        warn(
            "a comparison needs a database on each side: name them with -d and \
             --source-database",
        );
        return Exit::Usage;
    };
    let two_connections = target.id != source.id;
    if !two_connections && target_db == source_db {
        warn(&format!(
            "both sides are {target_db} on {}: name the database to match with \
             --source-database, or another connection with --source-connection",
            target.name
        ));
        return Exit::Usage;
    }
    let dialect = schemaic_core::intel::SqlDialect::from_db_type(&target.db_type);
    let source_dialect = schemaic_core::intel::SqlDialect::from_db_type(&source.db_type);
    if let Err(e) = schemaic_core::compare::comparable(dialect, source_dialect) {
        warn(&e);
        return Exit::Usage;
    }
    let (target_db_handle, _target_tunnel) = match connect(target, &target_args, timeout).await {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    let (source_db_handle, _source_tunnel) = match connect(source, &source_args, timeout).await {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    let (left, right) = tokio::join!(
        read_schema(&target_db_handle, &target_db, timeout),
        read_schema(&source_db_handle, &source_db, timeout)
    );
    let (left, right) = match (left, right) {
        (Ok(l), Ok(r)) => (l, r),
        (Err(e), _) | (_, Err(e)) => {
            warn(&e);
            return Exit::Failed;
        }
    };
    let cmp = schemaic_core::compare::SchemaComparison::of(&left, &right, dialect);
    let report = match crate::compare::report(
        &cmp,
        tables,
        &side_label(target, &target_db, two_connections),
        &side_label(source, &source_db, two_connections),
    ) {
        Ok(r) => r,
        Err(e) => {
            warn(&e);
            return Exit::Usage;
        }
    };
    if let Some(script) = &report.script {
        let script = if script.ends_with('\n') {
            script.clone()
        } else {
            format!("{script}\n")
        };
        if let Err(exit) = emit_stdout(&script) {
            return exit;
        }
    }
    for note in &report.notes {
        warn(note);
    }
    Exit::Ok
}

/// `schemaic dump`: the app's SQL dump (`crate::dump::run`, the one driver
/// both front ends call), against a database the command names or the
/// connection's own. Every table and view when none is named — read from the
/// server's table list, so the dump's own fresh introspection is what decides
/// their order. Progress goes to stderr, a line per table; the result note is
/// the modal's own sentence (`dump::done_note`).
async fn run_dump(
    conns: &[Connection],
    target: &Target,
    output: &std::path::Path,
    tables: Vec<String>,
    opts: schemaic_core::dump::DumpOptions,
    timeout: Duration,
) -> Exit {
    if let Some(why) = output_problem(output, "dump") {
        warn(&why);
        return Exit::Usage;
    }
    let conn = match select_conn(conns, &target.conn) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let conn = &*conn;
    let dialect = schemaic_core::intel::SqlDialect::from_db_type(&conn.db_type);
    let Some(database) = database_for(target, conn) else {
        warn("a dump needs a database: name one with -d");
        return Exit::Usage;
    };
    let (db, _tunnel) = match connect(conn, &target.conn, query::DEFAULT_TIMEOUT).await {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    // The server's list either way: every table when none is named, and the
    // names a `-t` is resolved against when some are.
    let token = tokio_util::sync::CancellationToken::new();
    let list = match crate::deadline::with_deadline(
        db.fetch_table_list(&database),
        token,
        query::DEFAULT_TIMEOUT,
    )
    .await
    {
        Some(Ok(s)) => s,
        Some(Err(e)) => {
            warn(&format!("could not list the tables of {database}: {e}"));
            return Exit::Failed;
        }
        None => {
            warn(&format!("listing the tables of {database} timed out"));
            return Exit::Failed;
        }
    };
    let tables = if tables.is_empty() {
        list.tables
            .iter()
            .map(|t| schemaic_core::schema::display_name(t.schema.as_deref(), &t.name))
            .collect()
    } else {
        match resolve_tables(&list, &tables) {
            Ok(t) => t,
            Err(e) => {
                warn(&e);
                return Exit::Usage;
            }
        }
    };
    let name = output
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let req = schemaic_core::dump::DumpRequest {
        path: output.to_path_buf(),
        conn_id: conn.id,
        database,
        tables,
        opts,
        dialect,
    };
    let progress: crate::dump::Progress = Box::new(|p| {
        warn(&format!("{} of {} — {}", p.index, p.total, p.table));
    });
    let token = tokio_util::sync::CancellationToken::new();
    let outcome = crate::deadline::with_deadline(
        crate::dump::run(
            db,
            req,
            tokio::runtime::Handle::current(),
            token.clone(),
            progress,
            query::EXPORT_CHUNK_ROWS,
        ),
        token,
        timeout,
    )
    .await;
    use schemaic_core::dump::DumpOutcome;
    match outcome {
        Some(DumpOutcome::Done {
            tables,
            tally,
            missing,
            refused,
            left_out,
            rows_left_out,
        }) => {
            warn(&schemaic_core::dump::done_note(
                tables,
                schemaic_core::export::export_note(&tally, &name, true).as_deref(),
                &missing,
                &left_out,
                &rows_left_out,
                &refused,
            ));
            exit_for_dump_done(&missing)
        }
        Some(DumpOutcome::Failed { message, partial }) => {
            warn(&schemaic_core::export::export_failure_note(
                &message.to_string(),
                partial.then_some(name.as_str()),
            ));
            Exit::Failed
        }
        // The run's own answer where it unwound in time; past the grace there
        // is none, and the disk is the only witness left.
        Some(DumpOutcome::Cancelled { partial }) => {
            warn(&dump_timeout_note(timeout, &name, partial));
            Exit::Failed
        }
        None => {
            let partial = crate::dump::part_of(output).exists();
            warn(&dump_timeout_note(timeout, &name, partial));
            Exit::Failed
        }
    }
}

/// Why `-o` cannot be written, before anything is read: a folder that is not
/// there, or a path that is itself a folder — which used to fail only at the
/// final rename, after the whole `what` had run.
fn output_problem(output: &std::path::Path, what: &str) -> Option<String> {
    output_problem_with(output, what, std::path::Path::is_dir)
}

/// [`output_problem`] over `is_dir`, the one filesystem question it asks —
/// a seam, so its tests model the folders rather than stat the machine's.
fn output_problem_with(
    output: &std::path::Path,
    what: &str,
    is_dir: impl Fn(&std::path::Path) -> bool,
) -> Option<String> {
    if is_dir(output) {
        return Some(format!(
            "{} is a folder: name the file to write the {what} to",
            output.display()
        ));
    }
    output
        .parent()
        .filter(|d| !d.as_os_str().is_empty() && !is_dir(d))
        .map(|dir| format!("no folder {} to write the {what} in", dir.display()))
}

/// What a dump stopped by `--timeout` says: the fragment is named only when
/// one was made (`dump::cancel_note`'s `partial`). A dump timed out during its
/// schema read has created nothing, and the note sent the user to look for a
/// `.part` that did not exist.
fn dump_timeout_note(timeout: Duration, name: &str, partial: bool) -> String {
    format!(
        "the dump exceeded {}s and was stopped — {}",
        timeout.as_secs(),
        schemaic_core::dump::cancel_note(name, partial)
    )
}

/// The tables `dump -t` and `import -t` name, spelled as the dump's plan matches them
/// (`schema::display_name`), resolved against the server's `list` the way
/// `compare -t` resolves one: as the tree labels it (`orders`,
/// `sales.orders`), else spelled with its schema (`public.orders`, which a
/// label leaves bare), else by a bare name only one schema has
/// (`orders` for SQL Server's `dbo.orders`). Matched exactly, or ignoring
/// case where the server's names do (`DbSchema::names_ignore_case`).
///
/// A name the list does not have, or one two schemas share, is the command
/// line's to fix — an `Err` naming it, before any file is written. The plan
/// used to take `-t` at its word: SQL Server's documented `-t orders` found
/// nothing (exit 4, "retrying may work"), and one missing name beside a
/// found one wrote a dump a table short with exit 0.
fn resolve_tables(
    list: &schemaic_core::schema::DbSchema,
    names: &[String],
) -> Result<Vec<String>, String> {
    use schemaic_core::schema::display_name;
    let same = |a: &str, b: &str| {
        if list.names_ignore_case {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    let label = |t: &schemaic_core::schema::TableInfo| display_name(t.schema.as_deref(), &t.name);
    names
        .iter()
        .map(|name| {
            if let Some(t) = list.tables.iter().find(|t| same(&label(t), name)) {
                return Ok(label(t));
            }
            if let Some(t) = list.tables.iter().find(|t| {
                t.schema
                    .as_deref()
                    .is_some_and(|s| same(&format!("{s}.{}", t.name), name))
            }) {
                return Ok(label(t));
            }
            let bare: Vec<String> = list
                .tables
                .iter()
                .filter(|t| same(&t.name, name))
                .map(label)
                .collect();
            match bare.as_slice() {
                [one] => Ok(one.clone()),
                [] => Err(format!("no table or view named '{name}'")),
                several => Err(format!(
                    "'{name}' names a table in more than one schema ({}); qualify it",
                    several.join(", ")
                )),
            }
        })
        .collect()
}

/// The exit for a dump that finished: a table it was asked for and did not
/// find (gone between the list and the dump) leaves the file a table short,
/// which a script checking `$?` must not take for a whole dump.
fn exit_for_dump_done(missing: &[String]) -> Exit {
    if missing.is_empty() {
        Exit::Ok
    } else {
        Exit::Failed
    }
}

/// `schemaic export`. **Select, name the format, gate, then connect** — the
/// order [`run_query`] keeps, plus the format, which is the command line's to
/// get right before anything is read: an unknown one is a usage error.
async fn run_export(
    conns: &[Connection],
    target: &Target,
    sql: &SqlArgs,
    params: &[Binding],
    output: &std::path::Path,
    format: Option<&str>,
    timeout: Duration,
) -> Exit {
    use schemaic_core::export::ExportFormat;
    let fmt = match format {
        Some(name) => ExportFormat::from_name(name),
        None => ExportFormat::for_path(output),
    };
    let Some(fmt) = fmt else {
        warn(&match format {
            Some(name) => format!("unknown format {name:?}: use json, csv, sql, md, html or xlsx"),
            None => "the output file's extension names no format: add --format \
                     json, csv, sql, md, html or xlsx"
                .to_string(),
        });
        return Exit::Usage;
    };
    // A folder that is not there is the command line's mistake too — told
    // before anything is read, rather than after a whole query has run.
    if let Some(why) = output_problem(output, "export") {
        warn(&why);
        return Exit::Usage;
    }
    let conn = match select_conn(conns, &target.conn) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let conn = &*conn;
    let sql = match statement(sql, params, target, conn) {
        Ok(s) => s,
        Err(exit) => return exit,
    };
    let dialect = schemaic_core::intel::SqlDialect::from_db_type(&conn.db_type);
    if let Err(e) = query::gate(&sql, dialect) {
        warn(&e.message());
        return exit_for_no_rows(&e);
    }
    // The connect is bounded as `query`'s is; the export by its own `--timeout`.
    let (db, _tunnel) = match connect(conn, &target.conn, query::DEFAULT_TIMEOUT).await {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    let database = database_for(target, conn);
    match query::read_only_export(&db, database.as_deref(), &sql, fmt, output, timeout).await {
        Ok(done) => {
            // What was written, on stderr — stdout is not where the rows went.
            let n = done.tally.rows;
            warn(&format!(
                "wrote {n} {} to {}",
                schemaic_core::text::plural(n as usize, "row", "rows"),
                output.display()
            ));
            Exit::Ok
        }
        Err(e) => {
            warn(&e.message());
            if let Some(how) = no_database_failure(database.as_deref(), &e, dialect) {
                hint(&no_database_hint_for(&target.conn, how));
            }
            exit_for_no_rows(&e)
        }
    }
}

/// `schemaic mcp`.
///
/// **Select, connect, then serve until the client closes stdin.** The same
/// [`select_conn`] as every other subcommand, so a connection without CLI
/// access is refused before the keyring is read — exit 3, and the MCP client
/// shows the server as failed with that line as its log. The tunnel is opened
/// once and held for the server's lifetime; every tool call is still its own
/// connection, since `Db` connects per operation.
///
/// No database connection is made before the first tool call — only the SSH
/// tunnel, if there is one, is opened here: a database that is down
/// right now is an error the model sees on that call and may retry, not a
/// server that never started.
///
/// **stdout is the protocol's**, so what this says goes to stderr, which MCP
/// clients keep as the server's log — and the one line said is what is being
/// served, which is the first question when a tool is missing.
///
/// **Several `-c`s serve several connections** (`mcp::serve_many`), each
/// selected and gated on its own CLI access before any is opened, each with its
/// own data level; the first is the default for a call that names none, and
/// `-d` is the first one's. Every tunnel opens at the start, as one
/// connection's always has — a server that comes up has every connection it
/// lists reachable.
async fn serve_mcp(conns: &[Connection], target: &McpTarget) -> Exit {
    // `--ai-data` is a `--url` connection's level; a saved one's is the app's.
    if target.ai_data.is_some() && target.url.is_none() {
        warn("--ai-data is for a --url connection; a saved one's is set in Schemaic");
        return Exit::Usage;
    }
    let args = mcp_conn_args(target);
    // Every one selected — and its CLI access asked — before any is dialled.
    let mut selected = Vec::with_capacity(args.len());
    for a in &args {
        match select_conn(conns, a) {
            Ok(mut c) => {
                if a.url.is_some() {
                    // **Schema only unless asked**: a level nobody chose is
                    // the narrowest, as a saved connection's never-chosen one
                    // sends no rows either.
                    c.to_mut().ai_data = Some(
                        target
                            .ai_data
                            .unwrap_or(schemaic_core::connection::AiData::SchemaOnly),
                    );
                }
                selected.push(c)
            }
            Err(exit) => return exit,
        }
    }
    // **The same connection twice is refused by what was selected, not by
    // spelling**: `-c 1 -c shop` may name one connection by its id and its
    // name, and would serve it twice under one name. And two under one name —
    // a `--url`'s suggested name beside a saved connection of that name — are
    // refused too: a tool call picks a connection by name.
    if let Some(twice) = selected
        .iter()
        .enumerate()
        .find(|(i, c)| {
            selected[..*i]
                .iter()
                .any(|s| (s.id == c.id && c.id != 0) || s.name == c.name)
        })
        .map(|(_, c)| c)
    {
        warn(&format!(
            "'{}' is given twice — two connections cannot share a name on one server",
            twice.name
        ));
        return Exit::Usage;
    }
    let mut endpoints = Vec::with_capacity(selected.len());
    let mut tunnels = Vec::with_capacity(selected.len());
    for (i, (conn, a)) in selected.iter().zip(&args).enumerate() {
        if let Some(line) = mcp_connecting(conn, selected.len() > 1) {
            eprintln!("{line}");
        }
        let (db, tunnel) = match connect(conn, a, query::DEFAULT_TIMEOUT).await {
            Ok(v) => v,
            Err(exit) => return exit,
        };
        tunnels.push(tunnel);
        let flag = if i == 0 {
            target.database.as_deref()
        } else {
            None
        };
        let endpoint = mcp_endpoint(db, flag, conn);
        eprintln!(
            "{}",
            mcp_notice(
                conn,
                endpoint.samples,
                endpoint.database.as_deref(),
                i == 0,
                selected.len() > 1,
            )
        );
        endpoints.push((conn.name.clone(), endpoint));
    }
    crate::mcp::serve_many(endpoints).await;
    drop(tunnels);
    Exit::Ok
}

/// The connections `schemaic mcp` is asked to serve, one [`ConnArgs`] each:
/// the `-c`s in order, then any `--url`, which with a `-c` is not the default.
///
/// **No `--password-stdin`**, and every one says so: stdin is the protocol's,
/// so a password is `SCHEMAIC_PASSWORD`'s (`connect`), and a refusal that
/// offered the flag would send the user to one `mcp` does not have.
fn mcp_conn_args(target: &McpTarget) -> Vec<ConnArgs> {
    let saved = target.connections.iter().map(|c| ConnArgs {
        connection: Some(c.clone()),
        stdin_is_protocol: true,
        ..Default::default()
    });
    let url = target.url.iter().map(|u| ConnArgs {
        url: Some(u.clone()),
        stdin_is_protocol: true,
        ..Default::default()
    });
    saved.chain(url).collect()
}

/// The endpoint `schemaic mcp` serves `conn` as, over `db`, `-d` being `flag`.
///
/// **A `--url` connection's level is its flag's** — the id 0 `parse_url`
/// leaves is the mark of one, as in [`mcp_notice`] — so its refusals name
/// `--ai-data` rather than a connection setting it does not have.
fn mcp_endpoint(db: Db, flag: Option<&str>, conn: &Connection) -> crate::mcp::Endpoint {
    crate::mcp::Endpoint::standalone(db, default_database(flag, conn), conn.ai_data)
        .audited_as(&conn.name)
        .with_level_from_flag(conn.id == 0)
}

/// What `schemaic mcp` says on stderr as it starts: which connection, and —
/// when they are withheld — why the row-reading tools (`McpTool::reads_row_data`)
/// are missing and where that is changed.
/// A model that finds no query tool reports it as a bug, so the reason has to
/// be in the log beside it.
///
/// **And when there is no default database, it says so with the fix.** On a
/// server engine with neither `-d` nor a saved default, every tool that needs
/// one has to be told, call by call; an agent that does not know that sees
/// `relation does not exist` for a table `list_schema` just showed it.
///
/// `first` is whether this is the first `-c` — the one `-d` sets — and
/// `several` whether the server serves more than one, where a withheld tool is
/// still listed (another connection may offer it) and refused per call.
fn mcp_notice(
    conn: &Connection,
    rows: bool,
    database: Option<&str>,
    first: bool,
    several: bool,
) -> String {
    let mut s = format!(
        "schemaic mcp: serving connection '{}' on stdin/stdout",
        conn.name
    );
    if !rows {
        // A `--url` connection was never saved, so its level is the flag's —
        // the id 0 `parse_url` leaves is the mark of one.
        let fix = if conn.id == 0 {
            "pass --ai-data full to allow them".to_string()
        } else {
            format!(
                "set \"{}\" in Schemaic's connection settings to allow them",
                schemaic_core::connection::AiData::Full.label()
            )
        };
        s.push_str(&format!(
            "; run_query, explain and sample_rows are {} — its AI data access is \"{}\" ({fix})",
            if several { "refused on it" } else { "withheld" },
            conn.ai_data.unwrap_or_default().label(),
        ));
    }
    if database.is_none() {
        // A `--url` connection (id 0) has no name `databases -c` knows and no
        // settings in Schemaic: its default is the URL's own.
        let url = conn.id == 0;
        if first {
            let lists = if url {
                "`schemaic databases --url <the same URL>` lists them".to_string()
            } else {
                format!(
                    "`schemaic databases -c {}` lists them",
                    shell_word(&conn.name)
                )
            };
            s.push_str(&format!(
                "; no default database, so the tools run where the server puts them unless a \
                 call names one — pass -d <database> to set it ({lists})"
            ));
        } else {
            // `-d` is the first connection's; this one is told per call.
            s.push_str(&format!(
                "; no default database, so the tools run where the server puts them unless a \
                 call names one with `database`, or {}",
                if url {
                    "put one in its URL"
                } else {
                    "pick a default for it in Schemaic"
                }
            ));
        }
    }
    s
}

/// Said before each connection of a several-connection `schemaic mcp` is
/// opened, so that what opening it says next — a keyring notice, a tunnel
/// that failed, all worded for a one-connection command — follows a line
/// naming it rather than the previous connection's "serving" line. `None`
/// with one connection, whose subject is never in doubt.
fn mcp_connecting(conn: &Connection, several: bool) -> Option<String> {
    several.then(|| format!("schemaic mcp: connecting '{}'", conn.name))
}

/// A connection name as it can be pasted back after `-c`: quoted when it has a
/// space, since a name is allowed one.
fn shell_word(name: &str) -> String {
    if name.contains(char::is_whitespace) {
        format!("\"{name}\"")
    } else {
        name.to_string()
    }
}

/// `schemaic list`.
///
/// The listing is built as a [`ResultSet`] and handed to the same renderers a
/// query's rows go through, so `--format=json` means the same thing here as it
/// does there and there is no second table-drawing code path to keep in step.
fn list(conns: &[Connection], all: bool, out: Output) -> Exit {
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
        out,
    )) {
        return exit;
    }
    if let Some(note) = list_note(conns.len(), shown.len(), all) {
        warn(&note);
    }
    Exit::Ok
}

/// What `list` says on stderr about what it did not show: `total` saved,
/// `shown` of them listed.
///
/// **A count, not only the empty case.** A listing of three reads as "these
/// are my connections", and the fourth that has no CLI access was simply
/// absent — the reader went looking for a typo. An empty list is still not an
/// error: it is the correct answer to "what may I use", and this is what makes
/// it actionable rather than baffling.
fn list_note(total: usize, shown: usize, all: bool) -> Option<String> {
    let hidden = total - shown;
    if all || (hidden == 0 && total > 0) {
        return None;
    }
    Some(if total == 0 {
        "no saved connections yet; add one in Schemaic".to_string()
    } else if shown == 0 {
        let which = if total == 1 {
            "the one saved connection has no".to_string()
        } else {
            format!("none of the {total} saved connections has")
        };
        format!(
            "{which} CLI access yet; turn it on in Schemaic, or run `schemaic list --all` \
             to see them all"
        )
    } else {
        format!(
            "{hidden} {} without CLI access not shown; `schemaic list --all` lists them",
            if hidden == 1 {
                "connection"
            } else {
                "connections"
            }
        )
    })
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
async fn databases(conns: &[Connection], args: &ConnArgs, out: Output, timeout: Duration) -> Exit {
    let conn = match select_conn(conns, args) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let conn = &*conn;
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
    emit_stdout(&format::render_rows(&rs, out))
        .err()
        .unwrap_or(Exit::Ok)
}

/// `schemaic ping` — log in and run `SELECT 1`, through `Db::ping`, which is
/// the app's own health check.
///
/// **An answer is a row and exit 0; no answer is stderr and exit 4.** There
/// is no "down" row: a script tests the exit code, and a person reads the
/// driver's reason on stderr, where every other failure goes. `--timeout`
/// bounds the tunnel and the check separately, as each is its own wait.
async fn ping(conns: &[Connection], args: &ConnArgs, out: Output, timeout: Duration) -> Exit {
    let conn = match select_conn(conns, args) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let conn = &*conn;
    let (db, _tunnel) = match connect(conn, args, timeout).await {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    let started = std::time::Instant::now();
    // `ping` bounds itself at `timeout` and takes no token, so the deadline
    // here is `databases`' belt over the braces: the crate's gate holds every
    // read to one, and this one never outlives the other.
    let token = tokio_util::sync::CancellationToken::new();
    match crate::deadline::with_deadline(db.ping(timeout), token, timeout).await {
        Some(Ok(())) => {}
        Some(Err(e)) => {
            warn(&e.to_string());
            return Exit::Failed;
        }
        None => {
            warn(&format!("no answer within {}s", timeout.as_secs().max(1)));
            return Exit::Failed;
        }
    }
    emit_stdout(&format::render_rows(
        &ping_rows(conn, started.elapsed()),
        out,
    ))
    .err()
    .unwrap_or(Exit::Ok)
}

/// What a ping that was answered prints: the connection, and the round trip.
fn ping_rows(conn: &Connection, elapsed: Duration) -> ResultSet {
    let columns = ["connection", "engine", "endpoint", "status", "ms"]
        .into_iter()
        .map(text_column)
        .collect();
    let ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    let row = vec![
        Value::Str(conn.name.clone()),
        Value::Str(conn.db_type.clone()),
        Value::Str(conn.endpoint()),
        Value::Str("ok".to_string()),
        Value::UInt(ms),
    ];
    ResultSet::from_rows(columns, vec![row])
}

/// What to say after a failure that [`schemaic_core::sql::no_database_failure`]
/// recognises, or the guard's own no-database refusal: how to name one, and how
/// to find out which there are.
///
/// `connection` is the `-c` the user gave, echoed back so the command can be
/// copied; quoted when it has a space, since a name is allowed one.
fn no_database_hint(connection: &str, how: NoDatabaseFailure) -> String {
    let c = shell_word(connection);
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

/// Where a `--url` connection's password may come from when it is not piped.
/// Read for a `--url` only: a saved connection's password is the keyring's.
use schemaic_core::PASSWORD_ENV;

/// [`no_database_hint`] for the connection `args` names: a saved one echoed
/// back by name, a `--url` told to name its database there or with `-d`.
fn no_database_hint_for(args: &ConnArgs, how: NoDatabaseFailure) -> String {
    match args.source() {
        Ok(crate::args::ConnSource::Url(_)) => {
            let why = match how {
                NoDatabaseFailure::RanElsewhere => {
                    "the URL names no database, so the statement ran in the server's \
                     maintenance database"
                }
                NoDatabaseFailure::Refused => "the URL names no database",
            };
            format!("{why}; pass -d <database>, or end the URL with /<database>")
        }
        Ok(crate::args::ConnSource::Saved(name)) => no_database_hint(name, how),
        Err(_) => no_database_hint("<connection>", how),
    }
}

/// The connection `args` names, or the exit that says why not. No secret is
/// read and nothing is dialled: this is the half of opening a connection the
/// guard can run after, and before anything reaches a server.
///
/// **A saved connection is borrowed; a `--url` is built here**
/// ([`one_off_connection`]) and never saved.
fn select_conn<'a>(
    conns: &'a [Connection],
    args: &ConnArgs,
) -> Result<std::borrow::Cow<'a, Connection>, Exit> {
    match args.source() {
        Err(why) => {
            warn(&why);
            Err(Exit::Usage)
        }
        Ok(crate::args::ConnSource::Saved(name)) => select::select(conns, name)
            .map(std::borrow::Cow::Borrowed)
            .map_err(|e| {
                warn(&e.message());
                exit_for_no_connection(&e)
            }),
        Ok(crate::args::ConnSource::Url(url)) => {
            one_off_connection(url, args.read_only, args.stdin_is_protocol)
                .map(std::borrow::Cow::Owned)
                .map_err(|why| {
                    warn(&why);
                    Exit::Usage
                })
        }
    }
}

/// The connection a `--url` describes, built for this one command.
///
/// **A URL that carries a password is refused**, never used and never
/// repeated: argv is readable by every process on the machine and lands in the
/// shell's history, which is the exposure the rest of this CLI keeps a
/// password out of. `--password-stdin` or `SCHEMAIC_PASSWORD` carries it —
/// only the variable when `stdin_is_protocol` (`mcp`), which has no
/// `--password-stdin` to offer.
///
/// **A sign-in as the user is refused** — Windows sign-in, or an Entra, AWS
/// or Google token — since it would hand their identity to whatever host the
/// URL names, as is an Entra method Schemaic lacks, which would go in as a
/// SQL login. The app's import leaves both rows unticked; here nobody looks.
///
/// **CLI access is implied** — the person typed the connection; the flag
/// exists to keep a *saved* connection out of reach until they say so. It is
/// read-only if the URL asked for a read-only session or `--read-only` did,
/// and its AI data level stays the default, which no CLI command but `mcp`
/// reads.
fn one_off_connection(
    url: &str,
    read_only: bool,
    stdin_is_protocol: bool,
) -> Result<Connection, String> {
    use schemaic_core::conn_import::{ImportNote, read_url};
    use schemaic_core::connection::AuthMode;
    let reading = read_url(url).map_err(|e| format!("--url: {}", e.message()))?;
    let mut conn = reading.connection;
    if reading.carried_secret {
        let carrier = if stdin_is_protocol {
            "SCHEMAIC_PASSWORD"
        } else {
            "--password-stdin or SCHEMAIC_PASSWORD"
        };
        return Err(format!(
            "--url carries a password, which the command line puts in the process \
             list and your shell's history; take it out of the URL and give it with {carrier}"
        ));
    }
    let host = conn.host.clone();
    if conn.auth != AuthMode::Password {
        let who = match conn.auth {
            AuthMode::Windows => "your Windows login",
            AuthMode::AzureCli => "your Microsoft Entra account",
            AuthMode::AwsIam => "your AWS identity",
            AuthMode::GcpIam => "your Google Cloud account",
            AuthMode::Password => unreachable!(),
        };
        return Err(format!(
            "--url signs in with {who}, which would hand your identity to {host}; \
             a URL's host is nobody's promise, so save the connection in Schemaic \
             to sign in that way"
        ));
    }
    if reading.notes.contains(&ImportNote::ExternalLogin) {
        return Err(format!(
            "--url names a sign-in to {host} that Schemaic cannot perform, and would \
             go in as a SQL login instead; give a SQL login's user, or save the \
             connection in Schemaic"
        ));
    }
    conn.cli_access = true;
    // A URL that asks for a read-only session keeps it; `--read-only`
    // can only add one.
    conn.read_only |= read_only;
    Ok(conn)
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

/// [`read_stdin`]'s bytes, undecoded — for a script, whose reader decodes
/// UTF-16 and replaces an invalid byte as it does a file's.
fn read_stdin_bytes(what: &str, example: &str) -> Result<Vec<u8>, Exit> {
    if std::io::stdin().is_terminal() {
        warn(&format!(
            "{what} is read from a pipe, and stdin is a terminal; pipe it in \
             (e.g. `{example}`)"
        ));
        return Err(Exit::Usage);
    }
    let mut bytes = Vec::new();
    if let Err(e) = std::io::stdin().read_to_end(&mut bytes) {
        warn(&format!("could not read {what} from stdin: {e}"));
        return Err(Exit::Usage);
    }
    Ok(bytes)
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

/// The text `source` names: a file through `file`, stdin through `stdin` —
/// each with its byte-order mark taken off ([`sql_text`]) — or the argument as
/// typed. The readers are parameters so a test can reach this without a file
/// or a pipe.
fn read_source(
    source: SqlSource<'_>,
    file: impl FnOnce(&std::path::Path) -> Result<String, Exit>,
    stdin: impl FnOnce() -> Result<String, Exit>,
) -> Result<String, Exit> {
    match source {
        SqlSource::Text(sql) => Ok(sql.to_string()),
        SqlSource::File(path) => file(path).map(sql_text),
        SqlSource::Stdin => stdin().map(sql_text),
    }
}

/// The statement to run: the argument, stdin, or a file.
///
/// Stdin has one reader, so a statement and a password cannot both come from
/// it. An argument that carries a credential is run, but said — it is in the
/// process list for as long as the command runs and in the shell's history
/// after, the exposure the app keeps such statements out of its own history for.
/// Stdin and a file are neither, so they are not warned about — unless a `-p`
/// put the credential back on the command line.
///
/// **What comes back has its `-p` values in it** ([`bind`]), so every gate
/// after this — `query`'s read gate, `exec`'s guard, the write log — reads the
/// statement the engine will be sent, never the template.
fn statement(
    sql: &SqlArgs,
    params: &[Binding],
    target: &Target,
    conn: &Connection,
) -> Result<String, Exit> {
    let source = sql.source();
    let text = match source {
        SqlSource::Stdin if target.conn.password_stdin => {
            warn(
                "the statement and --password-stdin cannot both come from stdin; \
                 leave the password to the OS keyring, or pass the statement as an \
                 argument or with -f",
            );
            return Err(Exit::Usage);
        }
        SqlSource::Text(sql) => sql.to_string(),
        source => read_source(source, read_sql_file, || {
            read_stdin("the statement", "schemaic exec -c … - < change.sql")
        })?,
    };
    let dialect = schemaic_core::intel::SqlDialect::from_db_type(&conn.db_type);
    let (bound, unused) = bind(&text, params, dialect).map_err(|why| {
        warn(&why);
        Exit::Usage
    })?;
    for name in unused {
        warn(&format!(
            "-p {name} is not used: the statement has no :{name}"
        ));
    }
    if let Some(on_argv) = credential_on_argv(&source, &text, &bound, !params.is_empty(), dialect) {
        warn(on_argv.warning());
    }
    Ok(bound)
}

/// Which part of the command line a credential in what runs came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CredentialOnArgv {
    /// The statement itself was the argument.
    Statement,
    /// A `-p` value put it there — wherever the statement came from.
    Param,
}

impl CredentialOnArgv {
    /// The sentence for stderr, with the advice that actually takes it off
    /// the command line.
    fn warning(self) -> &'static str {
        match self {
            CredentialOnArgv::Statement => {
                "this statement carries a password, which the command line puts in the \
                 process list and your shell's history; pass `-` and pipe it on stdin instead"
            }
            CredentialOnArgv::Param => {
                "a -p value puts a password in this statement, and the command line puts it \
                 in the process list and your shell's history; write it into the statement \
                 and pipe that on stdin with `-` instead"
            }
        }
    }
}

/// Is a credential in what runs **on the command line** — and through which
/// part of it? `template` is the text before `-p` values went in, `bound`
/// after.
///
/// **Asked of where the credential came from, not of whether any `-p` was
/// given.** Any `-p` used to make the whole statement "on the command line",
/// so a password in a file or on stdin was blamed on argv, and the advice —
/// pipe it on stdin — left a password given through `-p` exactly where it
/// was. One predicate for every path that takes SQL from argv, `exec
/// --script`'s text among them, which never asked.
///
/// `carries_credential` reads a statement's **shape** (`IDENTIFIED BY`, `WITH
/// PASSWORD`), so `IDENTIFIED BY :pw` carries one in the template too; what
/// tells a `-p` secret from a written one is whether the statement of that
/// shape holds a placeholder.
fn credential_on_argv(
    source: &SqlSource<'_>,
    template: &str,
    bound: &str,
    has_params: bool,
    dialect: schemaic_core::intel::SqlDialect,
) -> Option<CredentialOnArgv> {
    use schemaic_core::sql::{carries_credential, statement_ranges};
    if !carries_credential(bound, dialect) {
        return None;
    }
    let (mut written, mut through_param) = (false, false);
    for (lo, hi) in statement_ranges(template, dialect) {
        let stmt = &template[lo..hi];
        if carries_credential(stmt, dialect) {
            if has_params && !schemaic_core::params::scan(stmt, dialect).is_empty() {
                through_param = true;
            } else {
                written = true;
            }
        }
    }
    // A raw `-p` value can bring the whole shape with it, into a template that
    // had none.
    let shape_from_param = has_params && !written && !through_param;
    if through_param || shape_from_param {
        Some(CredentialOnArgv::Param)
    } else if written && matches!(source, SqlSource::Text(_)) {
        Some(CredentialOnArgv::Statement)
    } else {
        None
    }
}

/// `sql` with its `:name` placeholders replaced by the `-p` values, and the
/// names given that it never asks for — or why it cannot be sent.
///
/// **The app's substitution, not a second one** (`params::substitute`): the
/// same scan decides what is a placeholder, and the same rendering quotes text
/// and refuses a number that is not one.
///
/// **A placeholder with no `-p` is refused**, never sent as written: on
/// MySQL `:id` is a syntax error at best, and on SQL Server a `:` line can be
/// a sqlcmd command. A name given twice is refused too — which of two values
/// "wins" is not something to leave to argument order.
fn bind(
    sql: &str,
    params: &[Binding],
    dialect: schemaic_core::intel::SqlDialect,
) -> Result<(String, Vec<String>), String> {
    use schemaic_core::params::{ParamError, names, substitute};
    for (i, p) in params.iter().enumerate() {
        if params[..i].iter().any(|q| q.name == p.name) {
            return Err(format!("-p {} is given twice", p.name));
        }
    }
    let bound = substitute(sql, params, dialect).map_err(|e| match e {
        ParamError::Missing(missing) => {
            let list = missing
                .iter()
                .map(|n| format!(":{n}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "no value for {list}; pass -p {}=<value> (or {}:null)",
                missing[0], missing[0]
            )
        }
        other => other.to_string(),
    })?;
    let asked = names(sql, dialect);
    let unused = params
        .iter()
        .filter(|p| !asked.contains(&p.name))
        .map(|p| p.name.clone())
        .collect();
    Ok((bound, unused))
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
    // `source()`, the question `select_conn` asked, rather than whether a URL
    // is set: the two must agree on which kind of connection this is.
    if matches!(args.source(), Ok(crate::args::ConnSource::Url(_))) {
        // **A `--url` connection was never saved, so the keyring holds
        // nothing of it** — and its id is the parser's placeholder, which
        // could name another connection's entries there. Its password is the
        // pipe's, or `SCHEMAIC_PASSWORD`'s: an environment variable is not
        // argv, and a CI job's secret store hands one over that way.
        conn.password = piped
            .or_else(|| std::env::var(PASSWORD_ENV).ok())
            .unwrap_or_default();
    } else {
        // The password `--password-stdin` supplies is not asked of the
        // keyring at all, so a locked one has nothing to report about it.
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
    }
    // `uses_tunnel`, the one answer every tunnel site asks: a SQLite
    // connection with a leftover SSH block has no server to tunnel to.
    let tunnel = if conn.uses_tunnel() {
        let opened = tokio::time::timeout(
            timeout,
            schemaic_db::ssh::open_tunnel(&conn.ssh, conn.tcp_target().0, conn.tcp_target().1),
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
fn emit_rows(rs: &ResultSet, out: Output) -> Result<(), Exit> {
    emit_stdout(&format::render_rows(rs, out))?;
    if let Some(w) = format::withheld_warning(rs, out.format) {
        warn(&w);
    }
    Ok(())
}

/// `schemaic query`.
///
/// **Select, gate, then connect.** A refused statement exits 3, which promises
/// nothing reached a server; opening first meant a loop on a refusal made a
/// real SSH login each time round.
#[allow(clippy::too_many_arguments)]
async fn run_query(
    conns: &[Connection],
    target: &Target,
    sql: &SqlArgs,
    params: &[Binding],
    out: Output,
    limit: usize,
    fail_on_cap: bool,
    timeout: Duration,
) -> Exit {
    let conn = match select_conn(conns, &target.conn) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let conn = &*conn;
    let sql = match statement(sql, params, target, conn) {
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
            if let Err(exit) = emit_rows(&rs, out) {
                return exit;
            }
            if let Some(w) = format::truncation_warning(&rs, out) {
                warn(&w);
            }
            exit_for_rows(&rs, fail_on_cap)
        }
        Err(e) => {
            warn(&e.message());
            if let Some(how) = no_database_failure(database.as_deref(), &e, dialect) {
                hint(&no_database_hint_for(&target.conn, how));
            }
            exit_for_no_rows(&e)
        }
    }
}

/// `schemaic explain`: [`query::read_only_explain`]'s plan — the MCP server's
/// `explain`, plain `EXPLAIN` on a read-only session behind `query`'s gate —
/// in [`run_query`]'s order: select, gate, then connect.
async fn run_explain(
    conns: &[Connection],
    target: &Target,
    sql: &SqlArgs,
    params: &[Binding],
    out: Output,
    timeout: Duration,
) -> Exit {
    let conn = match select_conn(conns, &target.conn) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let conn = &*conn;
    let sql = match statement(sql, params, target, conn) {
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
    match query::read_only_explain(&db, database.as_deref(), &sql, timeout).await {
        Ok(plan) => {
            if !plan.rows.is_empty()
                && let Err(exit) = emit_rows(&plan_rows(&plan), out)
            {
                return exit;
            }
            for note in plan_notes(&plan) {
                warn(&note);
            }
            Exit::Ok
        }
        Err(e) => {
            warn(&e.message());
            if let Some(how) = no_database_failure(database.as_deref(), &e, dialect) {
                hint(&no_database_hint_for(&target.conn, how));
            }
            exit_for_no_rows(&e)
        }
    }
}

/// A plan as rows, so `--format` renders it as it renders a query: the
/// server's own columns, each cell as the server wrote it.
///
/// **Never marked truncated**, though the plan may be: the renderers' "capped"
/// wording points at `--limit`, which `explain` has none of. [`plan_notes`]
/// says it instead.
///
/// **A cell that reads exactly `NULL` is a null.** The plan holds display
/// text, which is how MySQL's empty `key` and `ref` arrive, and JSON printed
/// them as the string `"NULL"`; no engine's plan writes that word as a whole
/// cell for anything else.
fn plan_rows(plan: &schemaic_core::plan::QueryPlan) -> ResultSet {
    let columns = plan.columns.iter().map(|c| text_column(c)).collect();
    let rows = plan
        .rows
        .iter()
        .map(|r| {
            r.iter()
                .map(|c| match c.as_str() {
                    "NULL" => Value::Null,
                    _ => Value::Str(c.clone()),
                })
                .collect()
        })
        .collect();
    ResultSet::from_rows(columns, rows)
}

/// What `explain` says on stderr: each finding of `core::plan`'s heuristics,
/// that the plan was cut where the server read cut it, or that there was no
/// plan at all.
fn plan_notes(plan: &schemaic_core::plan::QueryPlan) -> Vec<String> {
    if plan.rows.is_empty() {
        return vec!["the server returned no plan".to_string()];
    }
    let mut notes: Vec<String> = plan
        .warnings
        .iter()
        .map(|w| format!("possible issue: {}", w.message()))
        .collect();
    if plan.truncated {
        notes.push(format!(
            "warning: the plan was cut at its first {} rows; the issues above cover only those",
            plan.rows.len()
        ));
    }
    notes
}

/// Run one of [`catalog`]'s canned reads for `target`, as `what`: the rows, or
/// the exit that has already been explained on stderr.
///
/// **The same order and the same path as [`run_query`]** — select, gate,
/// connect, then `read_only_query` — because it *is* a query, only one the
/// user did not type. A dialect that cannot answer without a database is told
/// so before anything is dialled, with the hint a failed query gets.
async fn run_catalog(
    conns: &[Connection],
    target: &Target,
    what: &str,
    sql: impl FnOnce(schemaic_core::intel::SqlDialect, Option<&str>) -> Option<String>,
    limit: usize,
    timeout: Duration,
) -> Result<ResultSet, Exit> {
    let conn = select_conn(conns, &target.conn)?;
    let conn = &*conn;
    let dialect = schemaic_core::intel::SqlDialect::from_db_type(&conn.db_type);
    let database = database_for(target, conn);
    let Some(sql) = sql(dialect, database.as_deref()) else {
        warn(&format!("`{what}` needs a database to look in"));
        hint(&no_database_hint_for(
            &target.conn,
            NoDatabaseFailure::Refused,
        ));
        return Err(Exit::Usage);
    };
    // Asked here as `run_query` asks it: a canned statement the gate refused
    // would be this crate's bug, but it must still be refused before a login.
    if let Err(e) = query::gate(&sql, dialect) {
        warn(&e.message());
        return Err(exit_for_no_rows(&e));
    }
    let (db, _tunnel) = connect(conn, &target.conn, timeout).await?;
    query::read_only_query(&db, database.as_deref(), &sql, limit, timeout)
        .await
        .map_err(|e| {
            warn(&e.message());
            exit_for_no_rows(&e)
        })
}

/// `schemaic tables`.
async fn tables(
    conns: &[Connection],
    target: &Target,
    out: Output,
    limit: usize,
    timeout: Duration,
) -> Exit {
    let rs = match run_catalog(conns, target, "tables", catalog::tables_sql, limit, timeout).await {
        Ok(rs) => rs,
        Err(exit) => return exit,
    };
    if let Err(exit) = emit_rows(&rs, out) {
        return exit;
    }
    if let Some(w) = format::truncation_warning(&rs, out) {
        warn(&w);
    }
    Exit::Ok
}

/// No column can be more than this; it is a bound on memory, never a cap a
/// real table meets (SQLite's compile-time ceiling is 32 767, and the other
/// two stop far below it).
const DESCRIBE_CAP: usize = 32_768;

/// `schemaic describe <table>`.
///
/// **No rows is "no such table", exit 2** — the command named something that
/// is not there, as a connection that is not there is — said with where it
/// was looked for, since a table in another database is the usual reason.
async fn describe(
    conns: &[Connection],
    target: &Target,
    table: &str,
    out: Output,
    timeout: Duration,
) -> Exit {
    let sql = |d, db: Option<&str>| catalog::describe_sql(d, db, table);
    let rs = match run_catalog(conns, target, "describe", sql, DESCRIBE_CAP, timeout).await {
        Ok(rs) => rs,
        Err(exit) => return exit,
    };
    if rs.row_count() == 0 {
        let conn = select_conn(conns, &target.conn).ok();
        let place = conn
            .and_then(|c| database_for(target, &c))
            .map(|db| format!(" in {db}"))
            .unwrap_or_default();
        warn(&format!("no table or view named '{table}'{place}"));
        return Exit::Usage;
    }
    emit_rows(&rs, out).err().unwrap_or(Exit::Ok)
}

/// How many missing-index suggestions `indexes --missing` asks for: the
/// Performance view's figure, and more than a server keeps worth reading.
const MISSING_INDEX_ROWS: usize = 50;

/// One of the performance reads, under `--timeout`: the value, or the exit
/// already explained on stderr. `what` finishes "reading …".
///
/// **The reads take no token** — the statistics reads have nothing of the
/// user's to `KILL` — so the cancel reaches nothing and the deadline abandons
/// them, `databases`'s trade. `fetch_schema` is the one that listens.
///
/// Named for [`crate::deadline::with_deadline`], which it is with the
/// reporting added: `deadline`'s gate looks for that name around every read.
async fn with_deadline_read<T>(
    what: &str,
    read: impl std::future::Future<Output = Result<T, schemaic_db::DbError>>,
    token: tokio_util::sync::CancellationToken,
    timeout: Duration,
) -> Result<T, Exit> {
    match crate::deadline::with_deadline(read, token, timeout).await {
        Some(Ok(v)) => Ok(v),
        Some(Err(e)) => {
            warn(&format!("reading {what} failed: {e}"));
            Err(Exit::Failed)
        }
        None => {
            warn(&format!(
                "reading {what} exceeded {}s",
                timeout.as_secs().max(1)
            ));
            Err(Exit::Failed)
        }
    }
}

/// The database a per-database performance command reads, or exit 2 with the
/// hint a catalogue read gets — said before anything is dialled.
fn perf_database(target: &Target, conn: &Connection, what: &str) -> Result<String, Exit> {
    database_for(target, conn).ok_or_else(|| {
        warn(&format!("`{what}` needs a database to look in"));
        hint(&no_database_hint_for(
            &target.conn,
            NoDatabaseFailure::Refused,
        ));
        Exit::Usage
    })
}

/// `schemaic statements` — [`Db::fetch_top_statements`], the read behind the
/// MCP `top_statements` tool and the Performance view, as rows.
///
/// **A source that is off is exit 2**, on stderr with the one statement that
/// turns it on (never run): the statistics are not there, as a `describe`
/// table that is not there is, and a script must not read an empty list as
/// "nothing is slow". A source that is on and has counted nothing yet is
/// exit 0 and no rows, with a note.
async fn statements(
    conns: &[Connection],
    target: &Target,
    out: Output,
    order: schemaic_core::workload::StatementOrder,
    limit: usize,
    timeout: Duration,
) -> Exit {
    use schemaic_core::workload::StatementReport;
    let conn = match select_conn(conns, &target.conn) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let conn = &*conn;
    let database = database_for(target, conn);
    let (db, _tunnel) = match connect(conn, &target.conn, timeout).await {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    let token = tokio_util::sync::CancellationToken::new();
    let report = match with_deadline_read(
        "the statement statistics",
        db.fetch_top_statements(database.as_deref(), order, limit),
        token,
        timeout,
    )
    .await
    {
        Ok(r) => r,
        Err(exit) => return exit,
    };
    match report {
        StatementReport::Ready { statements, since } => {
            if let Err(exit) = emit_rows(&crate::perf::statements_result(&statements), out) {
                return exit;
            }
            if statements.is_empty() {
                warn("the statistics source is on but has recorded no statements yet");
            }
            if let Some(since) = since {
                warn(&format!("counted since {since}"));
            }
            Exit::Ok
        }
        StatementReport::Unavailable(u) => {
            warn(&u.reason());
            if let Some(sql) = u.enable_sql() {
                hint(&format!("to turn it on, run: {sql}"));
            }
            Exit::Usage
        }
    }
}

/// `schemaic indexes` — the unused, duplicate and prefix-redundant indexes of
/// one database ([`schemaic_core::workload::index_health`]), or with
/// `--missing` the optimiser's missing-index suggestions, each with the DDL
/// `ddl::diff` would emit for it. Both read what the MCP `index_health` tool
/// reads; nothing is run.
async fn indexes(
    conns: &[Connection],
    target: &Target,
    out: Output,
    missing: bool,
    timeout: Duration,
) -> Exit {
    use schemaic_core::workload as wl;
    let conn = match select_conn(conns, &target.conn) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let conn = &*conn;
    let dialect = schemaic_core::intel::SqlDialect::from_db_type(&conn.db_type);
    let database = match perf_database(target, conn, "indexes") {
        Ok(d) => d,
        Err(exit) => return exit,
    };
    // Asked before dialling: the engine is known from the saved connection.
    if missing && !wl::supports_missing_index_hints(dialect) {
        warn("this engine records no missing-index suggestions; --missing reads SQL Server's");
        return Exit::Usage;
    }
    let (db, _tunnel) = match connect(conn, &target.conn, timeout).await {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    let token = tokio_util::sync::CancellationToken::new();
    let what = format!("the schema of {database}");
    let schema = match with_deadline_read(
        &what,
        db.fetch_schema(&database, token.clone()),
        token,
        timeout,
    )
    .await
    {
        Ok(s) => s,
        Err(exit) => return exit,
    };
    if missing {
        let token = tokio_util::sync::CancellationToken::new();
        let found = match with_deadline_read(
            "the missing-index suggestions",
            db.fetch_missing_indexes(Some(&database), MISSING_INDEX_ROWS),
            token,
            timeout,
        )
        .await
        {
            Ok(m) => m,
            Err(exit) => return exit,
        };
        let s = crate::perf::suggestions(&schema, dialect, found);
        if let Err(exit) = emit_rows(&crate::perf::missing_with_ddl_result(&s), out) {
            return exit;
        }
        warn(if s.is_empty() {
            "the optimiser has recorded no missing indexes since the last restart"
        } else {
            "each is one optimiser's estimate; check it against `schemaic indexes` \
             before creating it"
        });
        return Exit::Ok;
    }
    let token = tokio_util::sync::CancellationToken::new();
    let what = format!("the statistics of {database}");
    let stats =
        match with_deadline_read(&what, db.fetch_table_stats(&database), token, timeout).await {
            Ok(s) => s,
            Err(exit) => return exit,
        };
    let findings = wl::index_health(&schema.tables, &stats);
    if let Err(exit) = emit_rows(&crate::perf::index_health_result(&findings), out) {
        return exit;
    }
    warn(crate::perf::usage_note(&stats));
    if wl::supports_missing_index_hints(dialect) {
        hint("`schemaic indexes --missing` lists the indexes the optimiser recorded as missing");
    }
    Exit::Ok
}

/// `schemaic sizes` — the largest tables of one database, or with `--indexes`
/// its largest indexes ([`schemaic_core::workload::largest`]), as the MCP
/// `table_sizes` tool reads them.
///
/// **An engine that sizes indexes per table only is exit 2 under
/// `--indexes`**, which is a question it cannot answer — told apart from a
/// database that simply has no indexes, which is exit 0 and no rows
/// ([`schemaic_core::workload::index_sizes_per_table_only`]).
async fn sizes(
    conns: &[Connection],
    target: &Target,
    out: Output,
    of_indexes: bool,
    limit: usize,
    timeout: Duration,
) -> Exit {
    use schemaic_core::workload as wl;
    let conn = match select_conn(conns, &target.conn) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let conn = &*conn;
    let database = match perf_database(target, conn, "sizes") {
        Ok(d) => d,
        Err(exit) => return exit,
    };
    let (db, _tunnel) = match connect(conn, &target.conn, timeout).await {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    let token = tokio_util::sync::CancellationToken::new();
    let what = format!("the statistics of {database}");
    let stats =
        match with_deadline_read(&what, db.fetch_table_stats(&database), token, timeout).await {
            Ok(s) => s,
            Err(exit) => return exit,
        };
    let (tables, indexes) = wl::largest(&stats, limit);
    if !of_indexes {
        if let Err(exit) = emit_rows(&crate::perf::table_sizes_result(&tables), out) {
            return exit;
        }
        if tables.is_empty() {
            warn(&format!("{database} reported no table sizes"));
        }
        return Exit::Ok;
    }
    if indexes.is_empty() && wl::index_sizes_per_table_only(&stats) {
        warn("this engine reports index sizes per table, not per index");
        return Exit::Usage;
    }
    if let Err(exit) = emit_rows(&crate::perf::index_sizes_result(&indexes), out) {
        return exit;
    }
    if indexes.is_empty() {
        warn(&format!("{database} has no indexes to size"));
    }
    Exit::Ok
}

/// `schemaic locks` — the locks sessions hold and wait for
/// ([`Db::fetch_locks`], the MCP `current_locks` tool's read). An engine with
/// no server sessions (SQLite) is exit 2, before anything is opened.
async fn locks(conns: &[Connection], args: &ConnArgs, out: Output, timeout: Duration) -> Exit {
    let conn = match select_conn(conns, args) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let conn = &*conn;
    let dialect = schemaic_core::intel::SqlDialect::from_db_type(&conn.db_type);
    if !schemaic_core::activity::supports_activity(dialect) {
        warn("this engine has no server sessions, so there are no locks to show");
        return Exit::Usage;
    }
    let (db, _tunnel) = match connect(conn, args, timeout).await {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    let token = tokio_util::sync::CancellationToken::new();
    let list = match with_deadline_read("the locks", db.fetch_locks(), token, timeout).await {
        Ok(l) => l,
        Err(exit) => return exit,
    };
    if let Err(exit) = emit_rows(&crate::perf::locks_result(&list), out) {
        return exit;
    }
    // The MCP tool's wording, so the two say one thing of one server.
    match (list.locks.is_empty(), list.contended_only) {
        (true, true) => {
            warn("no session is waiting on a lock (this server lists contended locks only)")
        }
        (true, false) => warn("no locks are held or waited for"),
        (false, true) => warn("this server lists contended locks only"),
        (false, false) => {}
    }
    if list.truncated {
        warn("the list was capped; more locks exist");
    }
    Exit::Ok
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
    matches!(
        e,
        exec::NotRun::Blocked(schemaic_core::sql::GuardReason::NoDatabase)
    )
}

/// `schemaic exec`. Select, approve, then connect — [`run_query`]'s order, for
/// its reason.
async fn run_exec(
    conns: &[Connection],
    target: &Target,
    sql: &SqlArgs,
    params: &[Binding],
    out: Output,
    yes: bool,
    timeout: Duration,
) -> Exit {
    let conn = match select_conn(conns, &target.conn) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let conn = &*conn;
    let sql = match statement(sql, params, target, conn) {
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
                hint(&no_database_hint_for(
                    &target.conn,
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
    let logged = log_entry(conn, database.as_deref(), &sql, dialect);
    let ran = exec::run(&db, request, timeout).await;
    if let Some(entry) = logged {
        log_exec(entry, &ran, dialect);
    }
    match ran {
        Ok(rs) => {
            // A write reports what it changed; a statement that happened to
            // return rows through `exec` reports those instead.
            let shown = match rs.affected {
                Some(n) => emit_stdout(&format::render_affected(n, out)),
                None => emit_rows(&rs, out).map(|()| {
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
                hint(&no_database_hint_for(&target.conn, how));
            }
            exit_for_no_rows(&e)
        }
    }
}

/// Where a script's bytes come from: a file, read twice — once for the
/// database switches, once to run, and checked between the two
/// (`script::probe_still_describes`) — or text already in memory, from stdin
/// or the argument, which has nothing to change under it.
enum ScriptInput {
    File(std::path::PathBuf),
    Bytes(Vec<u8>),
}

/// The script `source` names, with the name it is reported under — or the
/// exit that says why not. `pipe` opens a `-f` path and answers `None` for a
/// regular file, or **the whole of anything else** — a pipe, `/dev/stdin`, a
/// `<(…)` — read once here; `stdin` reads stdin's bytes. Both are parameters
/// so a test reaches this without a file or a pipe.
///
/// **A non-regular file is read once, as stdin is.** A regular file is read
/// twice — the database scan, then the run, checked between — and a pipe
/// read twice is empty the second time: the run reported "0 statements ran",
/// exit 0, over a migration it never applied, or "changed on disk" while the
/// producer was still writing. A named FIFO's second open blocked outright.
///
/// **Stdin is bytes, as a file is**, so the reader's UTF-16 and invalid-byte
/// handling reaches it too: a `- < dump.sql` used to be refused as "not valid
/// UTF-8" when `-f dump.sql` ran. And **a script given as the argument is
/// warned about** when it carries a credential, as any statement is.
fn script_input(
    source: SqlSource<'_>,
    password_stdin: bool,
    dialect: schemaic_core::intel::SqlDialect,
    pipe: impl FnOnce(&std::path::Path) -> std::io::Result<Option<Vec<u8>>>,
    stdin: impl FnOnce() -> Result<Vec<u8>, Exit>,
) -> Result<(String, ScriptInput), Exit> {
    Ok(match source {
        SqlSource::File(path) => {
            let name = path.display().to_string();
            match pipe(path) {
                Ok(None) => (name, ScriptInput::File(path.into())),
                Ok(Some(bytes)) => (name, ScriptInput::Bytes(bytes)),
                Err(e) => {
                    warn(&format!("could not read the script {name}: {e}"));
                    return Err(Exit::Usage);
                }
            }
        }
        SqlSource::Stdin if password_stdin => {
            warn(
                "the script and --password-stdin cannot both come from stdin; \
                 leave the password to the OS keyring, or pass the script with -f",
            );
            return Err(Exit::Usage);
        }
        SqlSource::Stdin => ("stdin".to_string(), ScriptInput::Bytes(stdin()?)),
        SqlSource::Text(text) => {
            if let Some(on_argv) = credential_on_argv(&source, text, text, false, dialect) {
                warn(on_argv.warning());
            }
            (
                "the command line".to_string(),
                ScriptInput::Bytes(text.as_bytes().to_vec()),
            )
        }
    })
}

/// `schemaic exec --script`: a whole `.sql` file on one connection, through
/// [`exec::ScriptRequest`] — `sql::script_verdict`, the app's gate for a file
/// — and `Db::run_script`, the app's runner.
///
/// **Select, approve, scan, then connect.** The scan is
/// `script::restore_refusal`: a `USE other` or `\connect other` takes every
/// statement after it off the database the guard judged, so a script that
/// would leave it is refused before anything is dialled, its lines named.
async fn run_script(
    conns: &[Connection],
    target: &Target,
    sql: &SqlArgs,
    yes: bool,
    timeout: Duration,
) -> Exit {
    use schemaic_core::script::{FileStamp, ReadEnd, database_switches, restore_refusal};
    let conn = match select_conn(conns, &target.conn) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let conn = &*conn;
    let dialect = schemaic_core::intel::SqlDialect::from_db_type(&conn.db_type);
    let (name, input) = match script_input(
        sql.source(),
        target.conn.password_stdin,
        dialect,
        |path| {
            let mut file = std::fs::File::open(path)?;
            if file.metadata()?.is_file() {
                Ok(None)
            } else {
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(&mut file, &mut bytes)?;
                Ok(Some(bytes))
            }
        },
        || {
            read_stdin_bytes(
                "the script",
                "schemaic exec --script -c … --yes - < migration.sql",
            )
        },
    ) {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    let database = database_for(target, conn);
    let request = match exec::ScriptRequest::approved(conn, database.as_deref(), &name, yes) {
        Ok(r) => r,
        Err(e) => {
            warn(&e.message());
            if refused_for_no_database(&e) {
                hint(&no_database_hint_for(
                    &target.conn,
                    NoDatabaseFailure::Refused,
                ));
            }
            return Exit::Refused;
        }
    };
    // The whole input, not a prefix: a `--databases` dump writes `USE b`
    // half-way down.
    let (switches, stamp) = match &input {
        ScriptInput::File(path) => {
            let file = match std::fs::File::open(path) {
                Ok(f) => f,
                Err(e) => {
                    warn(&format!("could not read the script {name}: {e}"));
                    return Exit::Usage;
                }
            };
            let stamp = FileStamp::of(&file);
            (
                database_switches(std::io::BufReader::new(file), dialect),
                stamp,
            )
        }
        ScriptInput::Bytes(b) => (database_switches(&b[..], dialect), FileStamp::default()),
    };
    let switches = match switches {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
            warn(&format!(
                "{name} holds no statement terminator, so it is not a SQL script; \
                 check that it is the file you meant"
            ));
            return Exit::Usage;
        }
        Err(e) => {
            warn(&format!("could not read the script {name}: {e}"));
            return Exit::Usage;
        }
    };
    if let Some(refusal) = restore_refusal(&name, request.database(), &switches) {
        warn(&refusal.to_string());
        return Exit::Refused;
    }
    // The connect is bounded as a statement's is; the run by `--timeout`.
    let (db, _tunnel) =
        match connect(request.connection(), &target.conn, query::DEFAULT_TIMEOUT).await {
            Ok(v) => v,
            Err(exit) => return exit,
        };
    let entry = schemaic_core::write_log::Entry::now(
        schemaic_core::write_log::Source::Cli,
        &conn.name,
        Some(request.database()),
        "",
    );

    let token = tokio_util::sync::CancellationToken::new();
    let (tx, rx) = tokio::sync::mpsc::channel(schemaic_db::SCRIPT_QUEUE);
    let reader = {
        let token = token.clone();
        tokio::task::spawn_blocking(move || match input {
            ScriptInput::File(path) => {
                let file = match std::fs::File::open(&path) {
                    Ok(f) => f,
                    Err(e) => return ReadEnd::Failed(e.to_string()),
                };
                // The scan above read this path; what runs must be what it read.
                if !schemaic_core::script::probe_still_describes(stamp, FileStamp::of(&file)) {
                    return ReadEnd::Changed;
                }
                let file =
                    std::io::BufReader::with_capacity(schemaic_core::script::READ_BLOCK, file);
                schemaic_db::feed_script(file, dialect, tx, token, |_| {})
            }
            ScriptInput::Bytes(b) => {
                schemaic_db::feed_script(std::io::Cursor::new(b), dialect, tx, token, |_| {})
            }
        })
    };
    // The deadline is the only thing that cancels the run, so a cancelled
    // outcome below is a timeout. Its unwound answer is kept: the count of
    // what ran before it is the one thing the caller needs.
    use crate::deadline::{Deadlined, with_deadline_unwound};
    let ran = with_deadline_unwound(
        db.run_script(request.database(), rx, token.clone()),
        token.clone(),
        timeout,
    )
    .await;
    let (exec_end, ran) = match ran {
        Deadlined::InTime(r) | Deadlined::Late(Some(r)) => r,
        Deadlined::Late(None) => {
            // Abandoned mid-statement: nothing can say how far it got.
            token.cancel();
            let entry = entry
                .with_detail(format!("file {name}, stopped after {}s", timeout.as_secs()))
                .ended_as(schemaic_core::write_log::Outcome::Unknown, None);
            append_to_write_log(&entry, dialect);
            warn(&format!(
                "stopped after {}s and the server did not answer the cancel; \
                 some of the script may have been applied",
                timeout.as_secs()
            ));
            return Exit::Unknown;
        }
    };
    let read_end = reader
        .await
        .unwrap_or_else(|e| ReadEnd::ReaderStopped(e.to_string()));
    let outcome = schemaic_core::script::run_outcome(read_end, exec_end, ran);
    append_to_write_log(&entry.ended_script(&name, &outcome), dialect);
    for line in script_report(&outcome, timeout, dialect) {
        warn(&line);
    }
    script_exit(&outcome)
}

/// Append `entry` to the write log, with values kept or masked as the user
/// set it in the app; a failure to write is said, never fatal.
fn append_to_write_log(
    entry: &schemaic_core::write_log::Entry,
    dialect: schemaic_core::intel::SqlDialect,
) {
    let keep =
        schemaic_core::persist::load_json::<schemaic_core::persist::UiState>("ui_state.json")
            .write_log_values;
    if let Err(e) = schemaic_core::write_log::append(entry, dialect, keep) {
        warn(&format!("couldn't append to the write log: {e}"));
    }
}

/// What `exec --script` says on stderr when it ends — stdout stays empty,
/// since a script prints no rows.
///
/// **Every ending says how many statements ran**, the failures most of all: a
/// script is not transactional unless the file said so, and that count is the
/// answer to "how much of it happened?".
///
/// **On SQL Server the count is of batches**, which is what the runner sends
/// one at a time — a script with no `GO` is one — and a failed batch of
/// several statements is said as one: SQL Server carries on past a
/// statement's error inside a batch, so some of it may have been applied, on
/// either side of the error. "0 statements ran before it" over a table the
/// batch had created sent the reader back to re-run what had landed.
fn script_report(
    outcome: &schemaic_core::script::RunOutcome,
    timeout: Duration,
    dialect: schemaic_core::intel::SqlDialect,
) -> Vec<String> {
    use schemaic_core::script::{RunOutcome, piece_is_batch, pieces_are_batches};
    let (one, many) = if pieces_are_batches(dialect) {
        ("batch", "batches")
    } else {
        ("statement", "statements")
    };
    let ran = |n: usize| format!("{n} {} ran", schemaic_core::text::plural(n, one, many));
    match outcome {
        RunOutcome::Done { ran: n } => vec![ran(*n)],
        RunOutcome::Cancelled { ran: n } => vec![format!(
            "stopped after {}s: {} before it, and the rest did not run; \
             unless the script holds its own transaction, those are applied",
            timeout.as_secs(),
            ran(*n)
        )],
        RunOutcome::Failed {
            failure,
            ran: n,
            at,
        } => {
            let mut lines = vec![match failure {
                // Core's sentence is the app modal's, which has a summary
                // above it to choose again; a command line has neither.
                schemaic_core::script::ScriptFailure::Changed => {
                    "the file changed on disk between the database check and the run, \
                     so none of it was run; run it again"
                        .to_string()
                }
                failure => failure.message(),
            }];
            if let Some((sql, line)) = at {
                let head: String = sql.chars().take(200).collect();
                let cut = if head.len() < sql.len() { "…" } else { "" };
                if piece_is_batch(sql, dialect) {
                    lines.push(format!(
                        "the batch starting on line {line} failed: {head}{cut}"
                    ));
                    lines.push(
                        "SQL Server carries on past an error inside a batch, so statements \
                         of this one, before and after the error, may have been applied"
                            .to_string(),
                    );
                } else {
                    lines.push(format!("the {one} on line {line} failed: {head}{cut}"));
                }
            }
            // Nothing to say about "before it" when nothing ran and no
            // statement failed — a connect that failed, a file not readable.
            if *n > 0 || at.is_some() {
                lines.push(format!(
                    "{} before it; unless the script holds its own transaction, those are applied",
                    ran(*n)
                ));
            }
            lines
        }
    }
}

/// How `exec --script` exits: a failure is 4, as a failed statement is; a run
/// the deadline stopped is 5, since the statements before it may have been
/// applied and running it again would repeat them.
fn script_exit(outcome: &schemaic_core::script::RunOutcome) -> Exit {
    use schemaic_core::script::RunOutcome;
    match outcome {
        RunOutcome::Done { .. } => Exit::Ok,
        // A refusal that stands is 3 here as on every other command:
        // retrying unchanged refuses again.
        RunOutcome::Failed {
            failure: schemaic_core::script::ScriptFailure::Refused(_),
            ..
        } => Exit::Refused,
        RunOutcome::Failed { .. } => Exit::Failed,
        RunOutcome::Cancelled { .. } => Exit::Unknown,
    }
}

/// What `schemaic import` loads, and how — the command's own flags, apart
/// from where it goes.
struct ImportLoad<'a> {
    path: &'a std::path::Path,
    table: &'a str,
    reading: &'a crate::import::Reading,
    maps: &'a [(String, String)],
    skips: &'a [String],
}

/// The end of an import whose file could not be read as one — unparseable,
/// not objects, empty: [`Exit::Rejected`], since the command line was right
/// and the fix is in the file, with the sentence a caller branching on 7
/// reads. Nothing was written.
fn file_rejected(name: &str) -> Exit {
    warn(&format!("{name} was not imported; nothing was written"));
    Exit::Rejected
}

/// How many problems the check collects before it stops counting, as the
/// app's import does, and how many of them are printed.
const IMPORT_MAX_ISSUES: usize = 200;
const IMPORT_SHOWN_ISSUES: usize = 20;

/// `schemaic import`: the app's two passes over a file — check it whole, then
/// load it in one transaction (`Db::import_rows`) — with the app's own
/// mapping (`import::auto_map`) corrected by `--map`/`--skip`.
///
/// **Select, refuse, read the file's columns, connect, check, load.** A
/// read-only connection is refused before the file is opened; a file with any
/// problem is refused after the whole of it has been read and before a row is
/// written — [`Exit::Rejected`], every problem on stderr, as the app's modal
/// lists them rather than failing one row at a time.
async fn run_import(
    conns: &[Connection],
    target: &Target,
    load: ImportLoad<'_>,
    timeout: Duration,
) -> Exit {
    use schemaic_core::import as core_import;
    let conn = match select_conn(conns, &target.conn) {
        Ok(c) => c,
        Err(exit) => return exit,
    };
    let conn = &*conn;
    // **An import is a write**, and the app offers none on a read-only
    // connection; there is no `--yes` for it, as there is none for an INSERT.
    if conn.read_only {
        warn(&format!(
            "refused: {}",
            schemaic_core::sql::GuardReason::ReadOnly.message()
        ));
        return Exit::Refused;
    }
    let Some(database) = database_for(target, conn) else {
        warn("`import` needs a database to load into");
        hint(&no_database_hint_for(
            &target.conn,
            NoDatabaseFailure::Refused,
        ));
        return Exit::Refused;
    };
    let path = load.path.to_path_buf();
    let name = path.display().to_string();
    let open = {
        let path = path.clone();
        move || std::fs::File::open(&path).map(std::io::BufReader::new)
    };
    // The head is for the CSV sniffer only; a short or empty read is still a
    // head, and an unreadable file is said here, before anything is dialled.
    let head = {
        use std::io::Read as _;
        let mut head = Vec::new();
        match std::fs::File::open(&path) {
            Ok(f) => {
                let _ = f.take(64 * 1024).read_to_end(&mut head);
            }
            Err(e) => {
                warn(&format!("could not read {name}: {e}"));
                return Exit::Usage;
            }
        }
        head
    };
    let (format, cfg) = match crate::import::read_config(&name, load.reading, Some(&head)) {
        Ok(v) => v,
        Err(why) => {
            warn(&why);
            return Exit::Usage;
        }
    };
    let file_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if let Some(too_big) = core_import::xlsx_size_refusal(format, file_bytes) {
        warn(&too_big.message());
        return Exit::Usage;
    }
    if let Some(w) = core_import::memory_warning(format, file_bytes) {
        warn(&w.message());
    }
    // The file's columns as the load will read them — for JSON, every
    // record's keys — and nothing else: the check below reads every row.
    let columns = match crate::import::file_columns(&open, format, &cfg) {
        Ok(columns) => columns,
        Err(problem) => {
            warn(&problem.message(&name));
            return file_rejected(&name);
        }
    };

    let (db, _tunnel) = match connect(conn, &target.conn, query::DEFAULT_TIMEOUT).await {
        Ok(v) => v,
        Err(exit) => return exit,
    };
    let token = tokio_util::sync::CancellationToken::new();
    let schema = match crate::deadline::with_deadline(
        db.fetch_schema(&database, token.clone()),
        token,
        query::DEFAULT_TIMEOUT,
    )
    .await
    {
        Some(Ok(s)) => s,
        Some(Err(e)) => {
            warn(&format!("could not read {database}'s tables: {e}"));
            return Exit::Failed;
        }
        None => {
            warn(&format!(
                "reading {database}'s tables timed out after {}s",
                query::DEFAULT_TIMEOUT.as_secs()
            ));
            return Exit::Failed;
        }
    };
    // **`dump -t`'s resolver, not `describe`'s lenient lookup**: a bare name
    // two schemas share is refused here, since this one writes, and loading
    // into whichever schema happened to come first is a wrong table in silence.
    let found = resolve_tables(&schema, &[load.table.to_string()])
        .map(|labels| labels.into_iter().next())
        .map(|label| label.and_then(|l| schema.find_by_display(&l).cloned()));
    let table = match found {
        Ok(Some(t)) => t,
        Ok(None) => {
            warn(&format!("no table named '{}' in {database}", load.table));
            return Exit::Usage;
        }
        Err(why) => {
            warn(&why);
            return Exit::Usage;
        }
    };
    if table.is_view {
        warn(&format!(
            "{} is a view; import loads into a table",
            table.name
        ));
        return Exit::Usage;
    }
    let plan =
        match crate::import::plan_mapping(&columns, format, &cfg, &table, load.maps, load.skips) {
            Ok(p) => p,
            Err(why) => {
                warn(&why);
                return Exit::Usage;
            }
        };
    for note in &plan.notes {
        warn(note);
    }
    let mapping = plan.mapping;
    let missing = mapping.missing_required(&table);
    if !missing.is_empty() {
        warn(&format!(
            "nothing in the file fills {}, which can't be NULL and has no default; \
             the load will likely fail",
            missing.join(", ")
        ));
    }
    let dialect = db.engine().dialect();

    // Pass 1 — the whole file, nothing written.
    let checked = {
        let (open, cfg, table, mapping) =
            (open.clone(), cfg.clone(), table.clone(), mapping.clone());
        tokio::task::spawn_blocking(move || {
            let f = open().map_err(|e| e.to_string())?;
            core_import::validate(
                f,
                format,
                &cfg,
                &table,
                &mapping,
                dialect,
                IMPORT_MAX_ISSUES,
            )
            .map_err(|e| e.to_string())
        })
        .await
    };
    let validation = match checked {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            warn(&format!("{name}: {e}"));
            return file_rejected(&name);
        }
        Err(e) => {
            warn(&format!("the file check stopped unexpectedly: {e}"));
            return Exit::Failed;
        }
    };
    if !validation.issues.is_empty() {
        for issue in validation.issues.iter().take(IMPORT_SHOWN_ISSUES) {
            warn(&crate::import::issue_line(issue));
        }
        let shown = validation.issues.len().min(IMPORT_SHOWN_ISSUES);
        if let Some(tail) =
            core_import::issue_tail(shown, validation.issues.len(), validation.more_issues)
        {
            warn(&tail);
        }
        warn(&format!("{name} was not imported; nothing was written"));
        return Exit::Rejected;
    }

    // Pass 2 — the load, in one transaction.
    let built = {
        let (open, cfg, table, mapping) =
            (open.clone(), cfg.clone(), table.clone(), mapping.clone());
        tokio::task::spawn_blocking(move || {
            let f = open().map_err(|e| e.to_string())?;
            core_import::row_iter(f, format, &cfg, &table, &mapping, dialect)
                .map_err(|e| e.to_string())
        })
        .await
    };
    let mut rows = match built {
        Ok(Ok(it)) => it,
        Ok(Err(e)) => {
            warn(&format!("{name}: {e}"));
            return file_rejected(&name);
        }
        Err(e) => {
            warn(&format!("the file reader stopped unexpectedly: {e}"));
            return Exit::Failed;
        }
    };
    let insert: Vec<String> = core_import::insert_columns(&mapping, &table)
        .iter()
        .map(|&i| table.columns[i].name.clone())
        .collect();
    let qualified = [
        Some(database.as_str()),
        table.schema.as_deref(),
        Some(table.name.as_str()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(".");
    let entry = schemaic_core::write_log::Entry::now(
        schemaic_core::write_log::Source::Import,
        &conn.name,
        Some(&database),
        "",
    )
    .with_detail(format!("import {name} into {qualified}"));
    let token = tokio_util::sync::CancellationToken::new();
    use crate::deadline::{Deadlined, with_deadline_unwound};
    let loaded = with_deadline_unwound(
        db.import_rows(
            schemaic_db::ImportTarget {
                database: &database,
                schema: table.schema.as_deref(),
                table: &table.name,
                columns: &insert,
                ..Default::default()
            },
            &mut rows,
            token.clone(),
        ),
        token,
        timeout,
    )
    .await;
    use schemaic_core::write_log::Outcome;
    let (entry, exit) = match loaded {
        // A load that committed while the cancel was on its way did import:
        // the report never claims less than the engine delivered either.
        Deadlined::InTime(Ok(n)) | Deadlined::Late(Some(Ok(n))) => {
            warn(&format!(
                "imported {n} {} into {qualified}",
                schemaic_core::text::plural(n as usize, "row", "rows")
            ));
            (entry.ended(Ok(Some(n))), Exit::Ok)
        }
        Deadlined::InTime(Err(e)) => {
            let message = e.to_string();
            warn(&message);
            (entry.ended(Err(message)), Exit::Failed)
        }
        // Cancelled by the deadline, and the transaction rolled back as the
        // engine allows — `import_rows`' error says when it could not.
        Deadlined::Late(Some(Err(e))) => {
            let said = match e {
                schemaic_db::DbError::Cancelled => format!(
                    "stopped after {}s and rolled back; nothing was imported",
                    timeout.as_secs()
                ),
                e => e.to_string(),
            };
            warn(&said);
            (entry.ended_as(Outcome::Cancelled, Some(said)), Exit::Failed)
        }
        Deadlined::Late(None) => {
            let said = format!(
                "stopped after {}s and the server did not answer the cancel; \
                 check the table before importing again",
                timeout.as_secs()
            );
            warn(&said);
            (entry.ended_as(Outcome::Unknown, Some(said)), Exit::Unknown)
        }
    };
    append_to_write_log(&entry, dialect);
    exit
}

/// The write log's entry for an `exec` of `sql`, stamped before it runs — or
/// `None` for a statement that does not write (`exec` runs reads too), which
/// the log has no business holding. `write_log::is_logged` errs toward a write.
fn log_entry(
    conn: &Connection,
    database: Option<&str>,
    sql: &str,
    dialect: schemaic_core::intel::SqlDialect,
) -> Option<schemaic_core::write_log::Entry> {
    schemaic_core::write_log::is_logged(sql, dialect).then(|| {
        schemaic_core::write_log::Entry::now(
            schemaic_core::write_log::Source::Cli,
            &conn.name,
            database,
            sql,
        )
    })
}

/// Append `entry` to the write log with how `ran` ended — the GUI's masking
/// setting honoured, since the log is one file — and say so on stderr if it
/// could not be, without changing the exit status: the write it records has
/// happened either way.
fn log_exec(
    entry: schemaic_core::write_log::Entry,
    ran: &Result<schemaic_core::model::ResultSet, crate::query::NoRows>,
    dialect: schemaic_core::intel::SqlDialect,
) {
    // `exec` holds no transaction across runs, so nothing it runs is inside
    // an aborted one.
    let Some(entry) = entry.landed(exec_ran(ran), false, dialect) else {
        return;
    };
    append_to_write_log(&entry, dialect);
}

/// How an `exec` ended, in the write log's terms: a timeout after the
/// statement was sent (`TimedOut`, `Indeterminate`) is `Unknown` — it may
/// have written — never `Failed`.
fn exec_ran(
    ran: &Result<schemaic_core::model::ResultSet, crate::query::NoRows>,
) -> schemaic_core::write_log::Ran {
    use crate::query::NoRows;
    use schemaic_core::write_log::Ran;
    match ran {
        Ok(rs) => Ran::Rows(rs.affected),
        Err(e @ (NoRows::Indeterminate(_) | NoRows::TimedOut(_))) => {
            Ran::Unknown(Some(e.message()))
        }
        Err(e) => Ran::Failed(e.message()),
    }
}

/// Which database to run in — [`default_database`]'s rule over the target's
/// `-d`: the engine's implied one, else the flag, else the connection's own.
fn database_for(target: &Target, conn: &Connection) -> Option<String> {
    default_database(target.database.as_deref(), conn)
}

/// [`database_for`]'s rule, for a `-d` that is not in a [`Target`].
///
/// **The engine's implied database comes first**
/// ([`schemaic_db::Engine::implied_database`], `main` on SQLite) — the rule
/// `mcp::Endpoint::standalone` keeps, for its reason: a file has exactly one
/// database, so a `-d` there names nothing else. Then the flag, then the
/// connection's own through [`Connection::default_database`], which reads a
/// blank name as none. Reading `conn.database` raw made `dump` and `compare`
/// demand a `-d` of a SQLite file that any value satisfied.
fn default_database(flag: Option<&str>, conn: &Connection) -> Option<String> {
    schemaic_db::Engine::from_db_type(&conn.db_type)
        .implied_database()
        .or(flag)
        .or_else(|| conn.default_database())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::Cli;
    use schemaic_core::intel::SqlDialect;

    /// **Under the AI panel's agent, the command line reaches no connection**
    /// (security review S2). A harness that keeps a shell could run `schemaic
    /// exec` — a write the panel's MCP tools never offer — or `schemaic query`
    /// past the panel's data level. Only `version` and `completions` answer
    /// there — neither reaches a connection.
    #[test]
    fn under_the_panels_agent_only_version_and_completions_answer() {
        let parse = |line: &[&str]| {
            crate::args::parse_argv(
                std::iter::once(&"schemaic")
                    .chain(line)
                    .map(std::ffi::OsString::from)
                    .collect(),
            )
            .unwrap_or_else(|e| panic!("{line:?}: {e}"))
            .command
        };
        for line in [
            &["exec", "UPDATE t SET a = 1 WHERE id = 1", "-c", "prod"][..],
            &["query", "SELECT 1", "-c", "prod"],
            &["list"],
            &["mcp", "-c", "prod"],
        ] {
            assert!(agent_refusal(&parse(line), true).is_some(), "{line:?}");
            assert!(agent_refusal(&parse(line), false).is_none(), "{line:?}");
        }
        assert!(agent_refusal(&Command::Version, true).is_none());
        assert!(agent_refusal(&parse(&["completions", "bash"]), true).is_none());
    }

    fn listed(tables: &[(Option<&str>, &str)]) -> schemaic_core::schema::DbSchema {
        schemaic_core::schema::DbSchema {
            tables: tables
                .iter()
                .map(|(s, n)| schemaic_core::schema::TableInfo {
                    schema: s.map(str::to_string),
                    name: n.to_string(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// `-t orders` is the help's own example, and on SQL Server the dump's
    /// plan spells that table `dbo.orders`, as PostgreSQL's `-t public.orders`
    /// is `orders` there — so each `-t` is resolved against the server's list
    /// as `compare -t` resolves one: by label, by `schema.name`, or by a bare
    /// name only one schema has.
    #[test]
    fn a_dump_table_is_found_by_label_qualified_or_bare_name() {
        let mssql = listed(&[(Some("dbo"), "orders"), (Some("sales"), "customers")]);
        assert_eq!(
            resolve_tables(&mssql, &names(&["orders", "sales.customers"])),
            Ok(names(&["dbo.orders", "sales.customers"]))
        );
        assert_eq!(
            resolve_tables(&mssql, &names(&["dbo.orders", "customers"])),
            Ok(names(&["dbo.orders", "sales.customers"]))
        );
        let pg = listed(&[(Some("public"), "orders")]);
        assert_eq!(
            resolve_tables(&pg, &names(&["public.orders"])),
            Ok(names(&["orders"]))
        );
        let mysql = listed(&[(None, "orders")]);
        assert_eq!(
            resolve_tables(&mysql, &names(&["orders"])),
            Ok(names(&["orders"]))
        );
        // Where the server's names ignore case (a `_CI_` SQL Server database,
        // which the table list now says), so does the match — and only there.
        let ci = schemaic_core::schema::DbSchema {
            names_ignore_case: true,
            ..listed(&[(Some("dbo"), "orders")])
        };
        assert_eq!(
            resolve_tables(&ci, &names(&["Orders", "DBO.ORDERS"])),
            Ok(names(&["dbo.orders", "dbo.orders"]))
        );
        assert!(resolve_tables(&mssql, &names(&["Orders"])).is_err());
    }

    /// A name the server does not have, or one two schemas share, is the
    /// command line's to fix — refused before a file is written, rather than
    /// a dump a table short that exits 0.
    #[test]
    fn a_dump_table_that_is_not_there_or_is_ambiguous_is_refused() {
        let list = listed(&[(Some("dbo"), "orders"), (Some("sales"), "orders")]);
        let missing = resolve_tables(&list, &names(&["dbo.orders", "nope"])).unwrap_err();
        assert!(missing.contains("'nope'"), "{missing}");
        let ambiguous = resolve_tables(&list, &names(&["orders"])).unwrap_err();
        assert!(
            ambiguous.contains("dbo.orders") && ambiguous.contains("sales.orders"),
            "{ambiguous}"
        );
    }

    /// A table that went missing between the list and the dump leaves a file
    /// a table short: not a success a backup script can trust.
    #[test]
    fn a_dump_that_left_a_named_table_out_does_not_exit_ok() {
        assert_eq!(exit_for_dump_done(&[]), Exit::Ok);
        assert_eq!(exit_for_dump_done(&names(&["orders"])), Exit::Failed);
    }

    /// **A future larger than the calling thread's stack still runs.** The
    /// headless front ends used to `block_on` on the main thread, which on
    /// Windows has 1 MiB: `list_schema` overflowed it and the MCP server died
    /// mid-call. The test thread here has 2 MiB, and the future holds 1.5 MiB
    /// across an `.await`, so it lives in the future's own state — which
    /// `block_on` keeps on the stack of whichever thread runs it, and which a
    /// debug build copies more than once on the way there.
    #[test]
    fn a_future_larger_than_the_callers_stack_runs_on_its_own_thread() {
        const LEN: usize = 3 << 19;
        let n = block_on_own_stack(|| async {
            let buf = [7u8; LEN];
            tokio::task::yield_now().await;
            std::hint::black_box(&buf)
                .iter()
                .map(|&b| b as usize)
                .sum::<usize>()
        })
        .expect("the runtime starts");
        assert_eq!(n, 7 * LEN);
    }

    /// **A panic on that thread is the caller's panic**, not a silent `Err`:
    /// the front ends' behaviour on a bug stays what it was on the main thread.
    #[test]
    #[should_panic(expected = "boom")]
    fn a_panic_on_the_runtime_thread_reaches_the_caller() {
        let _ = block_on_own_stack(|| async { panic!("boom") });
    }

    /// **A ping answers with which connection answered, and how fast** — a
    /// row, so `--format` means what it means everywhere; whole milliseconds,
    /// since the fraction is noise at a network's scale.
    #[test]
    fn a_ping_reports_the_connection_and_its_round_trip() {
        let mut c = conn("");
        c.name = "Prod EU".to_string();
        let rs = ping_rows(&c, Duration::from_micros(12_700));
        let names: Vec<&str> = rs.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["connection", "engine", "endpoint", "status", "ms"]);
        let out = format::render_rows(&rs, Format::Jsonl);
        let row: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(row["connection"], "Prod EU");
        assert_eq!(row["endpoint"], "h:3306");
        assert_eq!(row["status"], "ok");
        assert_eq!(row["ms"], 12);
    }

    /// **`list` says how many it left out**, so a connection that is missing
    /// from it reads as "not exposed" rather than "not there".
    #[test]
    fn list_counts_what_it_hid() {
        let n = list_note(5, 3, false).expect("two were hidden");
        assert!(n.contains("2 connections"), "{n}");
        assert!(n.contains("--all"), "{n}");
        let n = list_note(4, 3, false).expect("one was hidden");
        assert!(n.contains("1 connection "), "{n}");
    }

    /// Nothing hidden, or `--all`, which hides nothing: nothing to say.
    #[test]
    fn list_is_quiet_when_it_hid_nothing() {
        assert_eq!(list_note(3, 3, false), None);
        assert_eq!(list_note(5, 5, true), None);
        assert_eq!(list_note(0, 0, true), None);
    }

    /// An empty listing is explained either way — but "turn CLI access on"
    /// is no answer when nothing has been saved at all.
    #[test]
    fn an_empty_list_says_whether_anything_is_saved() {
        let n = list_note(2, 0, false).expect("all hidden");
        assert!(n.contains("none of the 2"), "{n}");
        assert!(n.contains("--all"), "{n}");
        let n = list_note(0, 0, false).expect("nothing saved");
        assert!(n.contains("no saved connections"), "{n}");
        assert!(!n.contains("--all"), "{n}");
    }

    /// Every outcome, spelled so that a new variant fails to compile here
    /// until it is added to the list — and so to the help's check below.
    fn every_exit() -> [Exit; 7] {
        fn _exhaustive(e: Exit) {
            match e {
                Exit::Ok
                | Exit::Usage
                | Exit::Refused
                | Exit::Failed
                | Exit::Unknown
                | Exit::Capped
                | Exit::Rejected => {}
            }
        }
        [
            Exit::Ok,
            Exit::Usage,
            Exit::Refused,
            Exit::Failed,
            Exit::Unknown,
            Exit::Capped,
            Exit::Rejected,
        ]
    }

    fn top_level_help() -> String {
        <Cli as clap::Parser>::try_parse_from(["schemaic", "--help"])
            .unwrap_err()
            .to_string()
    }

    /// **`--help` lists every exit code**, since a script's author reads the
    /// help and not this file, and the codes are the contract they retry on.
    /// Each on a line of its own, and only the ones there are.
    #[test]
    fn the_help_lists_every_exit_code_and_no_other() {
        let help = top_level_help();
        let section = help
            .split("Exit codes:")
            .nth(1)
            .expect("the help has an exit-code section");
        let listed: Vec<u8> = section
            .lines()
            .filter_map(|l| l.split_whitespace().next()?.parse().ok())
            .collect();
        let real: Vec<u8> = every_exit().iter().map(|e| *e as u8).collect();
        assert_eq!(listed, real, "{section}");
    }

    /// And every `--format`, from the same list the parser accepts.
    #[test]
    fn the_help_lists_every_format() {
        let help = top_level_help();
        let section = help
            .split("Output formats")
            .nth(1)
            .and_then(|s| s.split("Exit codes:").next())
            .expect("the help has a format section");
        let listed: Vec<&str> = section
            .lines()
            .filter(|l| l.starts_with("  "))
            .filter_map(|l| l.split_whitespace().next())
            .collect();
        assert_eq!(listed, Format::NAMES, "{section}");
        // A subcommand's own help names them where `--format` is.
        let sub = <Cli as clap::Parser>::try_parse_from(["schemaic", "query", "--help"])
            .unwrap_err()
            .to_string();
        for name in Format::NAMES {
            assert!(sub.contains(name), "`query --help` must name {name}");
        }
    }

    #[test]
    fn the_exit_codes_are_the_documented_ones() {
        assert_eq!(Exit::Ok as u8, 0);
        assert_eq!(Exit::Usage as u8, 2);
        assert_eq!(Exit::Refused as u8, 3);
        assert_eq!(Exit::Failed as u8, 4);
        assert_eq!(Exit::Unknown as u8, 5);
        assert_eq!(Exit::Capped as u8, 6);
        assert_eq!(Exit::Rejected as u8, 7);
    }

    fn plan(rows: usize, truncated: bool) -> schemaic_core::plan::QueryPlan {
        use schemaic_core::plan::{PlanNote, PlanWarning, PlanWarningKind, QueryPlan};
        QueryPlan {
            wording: Default::default(),
            columns: vec!["id".into(), "detail".into(), "key".into()],
            rows: (0..rows)
                .map(|i| vec![i.to_string(), format!("SCAN t{i}"), "NULL".into()])
                .collect(),
            warnings: vec![PlanWarning {
                row: 0,
                kind: PlanWarningKind::FullScan,
                note: PlanNote::MyFullScan {
                    table: Some("t0".into()),
                    keyed: false,
                },
            }],
            truncated,
        }
    }

    /// **A plan prints as rows**, so `--format` means what it means for a
    /// query: the server's own columns, every cell as the server wrote it.
    #[test]
    fn a_plan_prints_as_rows_in_any_format() {
        let rs = plan_rows(&plan(2, false));
        let out = format::render_rows(&rs, Format::Jsonl);
        let rows: Vec<serde_json::Value> = out
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1]["id"], "1");
        assert_eq!(rows[1]["detail"], "SCAN t1");
        assert!(rows[1]["key"].is_null(), "MySQL's empty key: {}", rows[1]);
        assert!(!rs.truncated);
    }

    /// **The heuristics' findings are not rows** — stdout is data — so they
    /// go to stderr, one line each, and a cut plan says it was cut, with no
    /// `--limit` to point at: the cap is the server read's own.
    #[test]
    fn a_plans_findings_and_its_cut_go_to_stderr() {
        let notes = plan_notes(&plan(2, false));
        assert_eq!(
            notes,
            ["possible issue: Full table scan on `t0` (no index used, type = ALL)"]
        );
        let notes = plan_notes(&plan(3, true));
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes[1].contains("first 3 rows"), "{notes:?}");
        assert!(!notes[1].contains("--limit"), "{notes:?}");
        let quiet = schemaic_core::plan::QueryPlan {
            warnings: Vec::new(),
            ..plan(1, false)
        };
        assert!(plan_notes(&quiet).is_empty());
    }

    /// No plan at all is said, not printed as an empty table.
    #[test]
    fn an_empty_plan_is_said() {
        let notes = plan_notes(&schemaic_core::plan::QueryPlan {
            warnings: Vec::new(),
            ..plan(0, false)
        });
        assert_eq!(notes, ["the server returned no plan"]);
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
        assert_eq!(
            exit_for_no_rows(&NoRows::NotPermitted("x".into())),
            Exit::Refused
        );
        // Refused by the database layer before the statement ran, and refused
        // again on a retry: a guard's 3, not a server's 4.
        assert_eq!(
            exit_for_no_rows(&NoRows::DbRefused("x".into())),
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

    /// The write log's reading of an `exec`: a timeout after sending may
    /// have written, so it is `Unknown`; the server's refusal is `Failed`.
    #[test]
    fn an_indeterminate_exec_is_unknown() {
        use query::NoRows;
        use schemaic_core::write_log::Ran;
        let d = Duration::from_secs(1);
        assert!(matches!(
            exec_ran(&Err(NoRows::Indeterminate(d))),
            Ran::Unknown(Some(_))
        ));
        assert!(matches!(
            exec_ran(&Err(NoRows::TimedOut(d))),
            Ran::Unknown(Some(_))
        ));
        assert_eq!(
            exec_ran(&Err(NoRows::Failed("dup".into()))),
            Ran::Failed("dup".into())
        );
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

    /// **And the source readers deliver it without the mark** — the
    /// composition, which `sql_text` alone could not pin: either arm's call
    /// could be deleted with the test above still green.
    #[test]
    fn a_file_or_pipe_source_reads_without_its_bom() {
        let bom = || Ok("\u{FEFF}SELECT 1".to_string());
        let path = std::path::Path::new("q.sql");
        assert_eq!(
            read_source(SqlSource::File(path), |_| bom(), || unreachable!()),
            Ok("SELECT 1".to_string())
        );
        assert_eq!(
            read_source(SqlSource::Stdin, |_| unreachable!(), bom),
            Ok("SELECT 1".to_string())
        );
        assert_eq!(
            read_source(
                SqlSource::Text("\u{FEFF}x"),
                |_| unreachable!(),
                || unreachable!()
            ),
            Ok("\u{FEFF}x".to_string()),
            "an argument is taken as typed"
        );
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

    /// A failed script is a 4 like a failed statement; one the deadline
    /// stopped is a 5, since what ran before it may be applied and a retry
    /// would run it twice.
    #[test]
    fn a_scripts_exit_says_whether_retrying_is_safe() {
        use schemaic_core::script::{RunOutcome, ScriptFailure};
        assert_eq!(script_exit(&RunOutcome::Done { ran: 3 }), Exit::Ok);
        assert_eq!(
            script_exit(&RunOutcome::Failed {
                failure: ScriptFailure::Said("x".into()),
                ran: 1,
                at: None,
            }),
            Exit::Failed
        );
        assert_eq!(
            script_exit(&RunOutcome::Cancelled { ran: 2 }),
            Exit::Unknown
        );
    }

    /// **Every ending names how many statements ran**, and a failure names
    /// the statement and its line.
    #[test]
    fn a_scripts_report_counts_what_ran_and_names_the_failure() {
        use schemaic_core::script::{RunOutcome, ScriptFailure};
        let t = Duration::from_secs(60);
        assert_eq!(
            script_report(&RunOutcome::Done { ran: 1 }, t, LITE),
            vec!["1 statement ran"]
        );
        let failed = script_report(
            &RunOutcome::Failed {
                failure: ScriptFailure::Said("no such table: nope".into()),
                ran: 2,
                at: Some(("INSERT INTO nope VALUES (2)".into(), 3)),
            },
            t,
            LITE,
        );
        assert_eq!(failed[0], "no such table: nope");
        assert!(failed[1].contains("line 3") && failed[1].contains("INSERT INTO nope"));
        assert!(failed[2].starts_with("2 statements ran"), "{failed:?}");
        let stopped = script_report(&RunOutcome::Cancelled { ran: 4 }, t, LITE);
        assert!(stopped[0].contains("60s") && stopped[0].contains("4 statements"));
    }

    const LITE: schemaic_core::intel::SqlDialect = schemaic_core::intel::SqlDialect::Sqlite;

    /// **On SQL Server a failed batch of several statements is said as a
    /// batch**, some of which may have been applied — not "0 statements ran
    /// before it" over a table the batch created. The count is of batches.
    #[test]
    fn a_failed_tsql_batch_is_said_as_a_batch() {
        use schemaic_core::script::{RunOutcome, ScriptFailure};
        let ms = schemaic_core::intel::SqlDialect::MsSql;
        let batch = "CREATE TABLE a (id int PRIMARY KEY);\nINSERT INTO a VALUES (1);\n\
                     INSERT INTO a VALUES (1);\nINSERT INTO a VALUES (2);";
        let lines = script_report(
            &RunOutcome::Failed {
                failure: ScriptFailure::Said("Violation of PRIMARY KEY".into()),
                ran: 0,
                at: Some((batch.into(), 1)),
            },
            Duration::from_secs(60),
            ms,
        );
        assert!(
            lines[1].starts_with("the batch starting on line 1"),
            "{lines:?}"
        );
        assert!(lines[2].contains("may have been applied"), "{lines:?}");
        assert!(lines[3].starts_with("0 batches ran"), "{lines:?}");
        // One statement is no batch to explain.
        let one = script_report(
            &RunOutcome::Failed {
                failure: ScriptFailure::Said("x".into()),
                ran: 1,
                at: Some(("INSERT INTO a VALUES (1)".into(), 4)),
            },
            Duration::from_secs(60),
            ms,
        );
        assert!(one[1].starts_with("the batch on line 4"), "{one:?}");
        assert!(one.iter().all(|l| !l.contains("may have been applied")));
    }

    /// **A changed file is said in the command line's words** — there is no
    /// summary above it and nothing to choose.
    #[test]
    fn a_changed_script_names_no_summary() {
        use schemaic_core::script::{RunOutcome, ScriptFailure};
        let lines = script_report(
            &RunOutcome::Failed {
                failure: ScriptFailure::Changed,
                ran: 0,
                at: None,
            },
            Duration::from_secs(60),
            LITE,
        );
        assert!(
            !lines[0].contains("summary") && !lines[0].contains("Choose"),
            "{lines:?}"
        );
    }

    /// **A refusal that stands exits 3 on `exec --script`**, as it does on
    /// every other command; a connect that failed stays 4.
    #[test]
    fn a_standing_refusal_exits_refused_on_a_script() {
        use schemaic_core::script::{ExecEnd, ReadEnd, run_outcome};
        let refused = run_outcome(ReadEnd::Eof, ExecEnd::Refused("init SQL".into()), 0);
        assert_eq!(script_exit(&refused), Exit::Refused);
        let failed = run_outcome(ReadEnd::Eof, ExecEnd::Connect("no route".into()), 0);
        assert_eq!(script_exit(&failed), Exit::Failed);
    }

    /// **Argv is blamed only for what came from argv**: a password in a file
    /// or on stdin is not, an unrelated `-p` changes nothing, and a password
    /// given through `-p` gets the advice that actually takes it off the
    /// command line.
    #[test]
    fn a_credential_is_blamed_on_the_part_of_argv_it_came_from() {
        let my = schemaic_core::intel::SqlDialect::MySql;
        let file = std::path::Path::new("f.sql");
        let literal = "CREATE USER u IDENTIFIED BY 'pw'";
        let on = |source: SqlSource<'_>, template: &str, bound: &str, params: bool| {
            credential_on_argv(&source, template, bound, params, my)
        };
        assert_eq!(
            on(SqlSource::Text(literal), literal, literal, false),
            Some(CredentialOnArgv::Statement)
        );
        assert_eq!(on(SqlSource::File(file), literal, literal, false), None);
        assert_eq!(on(SqlSource::File(file), literal, literal, true), None);
        assert_eq!(on(SqlSource::Stdin, literal, literal, true), None);
        let template = "CREATE USER u IDENTIFIED BY :pw";
        let bound = "CREATE USER u IDENTIFIED BY 's3cret'";
        for source in [
            SqlSource::Text(template),
            SqlSource::Stdin,
            SqlSource::File(file),
        ] {
            assert_eq!(
                on(source, template, bound, true),
                Some(CredentialOnArgv::Param)
            );
        }
        assert!(CredentialOnArgv::Param.warning().contains("-p"));
        assert_eq!(
            on(SqlSource::Text("SELECT 1"), "SELECT 1", "SELECT 1", false),
            None
        );
    }

    /// **`exec --script` reads its input once where once is all there is**:
    /// a `-f` that is not a regular file comes back as bytes, a regular one
    /// as a path to read twice; stdin's bytes are kept undecoded, so a UTF-16
    /// script reads as `-f` reads it; and the argument is warned about like
    /// any statement.
    #[test]
    fn a_scripts_input_is_read_once_unless_it_is_a_regular_file() {
        let my = schemaic_core::intel::SqlDialect::MySql;
        let path = std::path::Path::new("/dev/stdin");
        let no_stdin = || -> Result<Vec<u8>, Exit> { unreachable!() };
        let (name, input) = script_input(
            SqlSource::File(path),
            false,
            my,
            |_| Ok(Some(b"select 1;".to_vec())),
            no_stdin,
        )
        .unwrap();
        assert_eq!(name, path.display().to_string());
        assert!(matches!(input, ScriptInput::Bytes(ref b) if b == b"select 1;"));
        let (_, input) =
            script_input(SqlSource::File(path), false, my, |_| Ok(None), no_stdin).unwrap();
        assert!(matches!(input, ScriptInput::File(_)));
        assert_eq!(
            script_input(
                SqlSource::File(path),
                false,
                my,
                |_| Err(std::io::Error::other("gone")),
                no_stdin
            )
            .err(),
            Some(Exit::Usage)
        );
        // UTF-16LE with its BOM, as Windows PowerShell 5.1's `>` writes it.
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("USE other;\n".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        let (_, input) = script_input(
            SqlSource::Stdin,
            false,
            my,
            |_| unreachable!(),
            || Ok(utf16.clone()),
        )
        .unwrap();
        let ScriptInput::Bytes(bytes) = input else {
            panic!("stdin is bytes")
        };
        let switches = schemaic_core::script::database_switches(&bytes[..], my).unwrap();
        assert_eq!(switches.len(), 1, "the reader decoded UTF-16");
        assert_eq!(
            script_input(SqlSource::Stdin, true, my, |_| unreachable!(), no_stdin).err(),
            Some(Exit::Usage)
        );
    }

    /// **The guards run before the connection is dialled**, on both new
    /// write paths — each case here would fail differently (a connect error,
    /// a missing file) if the guard came later. The host does not resolve.
    #[tokio::test]
    async fn the_write_paths_refuse_before_they_connect_or_read() {
        let url = |u: &str, read_only: bool| Target {
            conn: ConnArgs {
                url: Some(u.to_string()),
                read_only,
                ..Default::default()
            },
            database: None,
        };
        let script = |text: &str| SqlArgs {
            sql: Some(text.to_string()),
            file: None,
        };
        let t = Duration::from_secs(5);
        let host = "mysql://u@no-such-host.invalid/shop";
        // A script that leaves the database it was judged on.
        let (writable, read_only) = (url(host, false), url(host, true));
        let (leave, delete) = (
            script("USE other; DELETE FROM t;"),
            script("DELETE FROM t;"),
        );
        assert_eq!(
            run_script(&[], &writable, &leave, true, t).await,
            Exit::Refused
        );
        // A read-only connection, and a script without --yes.
        assert_eq!(
            run_script(&[], &read_only, &delete, true, t).await,
            Exit::Refused
        );
        assert_eq!(
            run_script(&[], &writable, &delete, false, t).await,
            Exit::Refused
        );
        // An import into a read-only connection, or with no database: refused
        // before the file — which does not exist — is opened.
        let reading = crate::import::Reading::default();
        let load = || ImportLoad {
            path: std::path::Path::new("no-such-file-7c1d.csv"),
            table: "t",
            reading: &reading,
            maps: &[],
            skips: &[],
        };
        assert_eq!(run_import(&[], &read_only, load(), t).await, Exit::Refused);
        let no_db = url("mysql://u@no-such-host.invalid", false);
        assert_eq!(run_import(&[], &no_db, load(), t).await, Exit::Refused);
    }

    /// **A URL that carries a password is refused**, and never repeated —
    /// the message names the flags, not the URL.
    #[test]
    fn a_url_with_a_password_is_refused() {
        let url = "postgresql://app:s3cret@pg.example/shop";
        let why = one_off_connection(url, false, false).unwrap_err();
        assert!(!why.contains("s3cret"), "{why}");
        assert!(
            why.contains("SCHEMAIC_PASSWORD") && why.contains("--password-stdin"),
            "{why}"
        );
        // A JDBC query string is a password too.
        assert!(
            one_off_connection(
                "jdbc:mysql://pg.example/shop?user=app&password=x",
                false,
                false
            )
            .is_err()
        );
        // Where stdin is the protocol's, the flag it has no use for is not
        // offered.
        let why = one_off_connection(url, false, true).unwrap_err();
        assert!(
            why.contains("SCHEMAIC_PASSWORD") && !why.contains("--password-stdin"),
            "{why}"
        );
    }

    /// **A sign-in that sends the user's own identity is refused** — Windows
    /// sign-in's NTLM exchange or an Entra token, which would go to whatever
    /// host the URL names. The app's import leaves the same row unticked.
    #[test]
    fn a_url_signing_in_as_the_user_is_refused() {
        let url = "sqlserver://sql.attacker.example;authentication=ActiveDirectoryDefault";
        let why = one_off_connection(url, false, false).unwrap_err();
        assert!(why.contains("sql.attacker.example"), "{why}");
        let ado =
            "Server=tcp:sql.attacker.example;Database=d;Authentication=ActiveDirectoryDefault";
        assert!(one_off_connection(ado, false, false).is_err());
        #[cfg(windows)]
        assert!(
            one_off_connection(
                "Server=tcp:h;Database=d;Integrated Security=true",
                false,
                false
            )
            .is_err()
        );
        // An Entra method Schemaic does not have would sign in as a SQL login
        // with that name: refused too, though `auth` stays a password.
        let interactive = "Server=tcp:h;Database=d;Authentication=ActiveDirectoryInteractive;User Id=me@corp.example";
        assert!(one_off_connection(interactive, false, false).is_err());
    }

    /// **The refusal asks what the URL carried, not what survived parsing** —
    /// an Entra password the parser clears, and the secret keys it never
    /// reads, are passwords on the command line all the same.
    #[test]
    fn a_url_carrying_any_secret_is_refused() {
        for url in [
            "Server=tcp:h;Database=d;Authentication=ActiveDirectoryPassword;User Id=u;Password=Entra-Pw",
            "postgresql://u@h/d?sslpassword=k3y",
            "mysql://u@h/d?passwd=pw",
            "jdbc:sqlserver://h;databaseName=d;trustStorePassword=ts",
        ] {
            let why = one_off_connection(url, false, false).unwrap_err();
            assert!(why.contains("SCHEMAIC_PASSWORD"), "{url}: {why}");
        }
    }

    /// **A URL asking for a read-only session gets one** — the URL is the
    /// whole of a `--url` connection's configuration, so the word that says
    /// "don't write" is the person's only statement of it.
    #[test]
    fn a_url_asking_for_read_only_is_read_only() {
        for url in [
            "sqlite:///srv/app.db?mode=ro",
            "file:/srv/app.db?mode=ro",
            "sqlite:///srv/app.db?immutable=1",
            "postgresql://report@db/shop?options=-c%20default_transaction_read_only%3Don",
            "postgresql://report@db/shop?target_session_attrs=read-only",
            "Server=tcp:h;Database=d;User Id=u;ApplicationIntent=ReadOnly",
            "sqlserver://h;databaseName=d;applicationIntent=ReadOnly",
        ] {
            let c = one_off_connection(url, false, false).unwrap();
            assert!(c.read_only, "{url}");
        }
        assert!(
            !one_off_connection("sqlite:///srv/app.db?mode=rw", false, false)
                .unwrap()
                .read_only
        );
    }

    /// **Every connection `mcp` serves knows stdin is the protocol's** — the
    /// composition that brings the wording above to `mcp --url`'s refusal.
    #[test]
    fn mcp_marks_every_connection_as_having_no_stdin() {
        let target = McpTarget {
            connections: vec!["a".to_string(), "b".to_string()],
            url: Some("mysql://u@h/d".to_string()),
            ..Default::default()
        };
        let args = mcp_conn_args(&target);
        let named: Vec<_> = args
            .iter()
            .map(|a| (a.connection.as_deref(), a.url.as_deref()))
            .collect();
        assert_eq!(
            named,
            [
                (Some("a"), None),
                (Some("b"), None),
                (None, Some("mysql://u@h/d"))
            ]
        );
        assert!(args.iter().all(|a| a.stdin_is_protocol));
        // And `select_conn` hands it on: a password-carrying `--url` is
        // still refused there.
        let pw = McpTarget {
            url: Some("mysql://u:pw@h/d".to_string()),
            ..Default::default()
        };
        assert_eq!(
            select_conn(&[], &mcp_conn_args(&pw)[0]).unwrap_err(),
            Exit::Usage
        );
    }

    /// The person typed it, so CLI access is theirs to give; `--read-only`
    /// makes it read-only, and nothing else is invented.
    #[test]
    fn a_url_connection_has_cli_access_and_the_read_only_asked_for() {
        let c = one_off_connection("postgresql://app@pg.example:5433/shop", true, false).unwrap();
        assert!(c.cli_access && c.read_only);
        assert_eq!(
            (c.host.as_str(), c.port, c.user.as_str()),
            ("pg.example", 5433, "app")
        );
        assert_eq!(c.default_database(), Some("shop"));
        assert!(
            !one_off_connection("mysql://app@h/db", false, false)
                .unwrap()
                .read_only
        );
        assert!(one_off_connection("not a url", false, false).is_err());
    }

    /// **The composition**: `select_conn` builds a `--url` connection and
    /// never looks in the saved list for it, and refuses both sources at once.
    #[test]
    fn select_conn_takes_a_url_or_a_name_but_not_both() {
        let saved = [conn("shop")];
        let url = ConnArgs {
            url: Some("mysql://app@h/other".to_string()),
            ..Default::default()
        };
        let c = select_conn(&saved, &url).unwrap();
        assert!(matches!(c, std::borrow::Cow::Owned(_)));
        assert_eq!(c.default_database(), Some("other"));
        let both = ConnArgs {
            connection: Some("1".to_string()),
            url: Some("mysql://app@h/other".to_string()),
            ..Default::default()
        };
        assert_eq!(select_conn(&saved, &both).unwrap_err(), Exit::Usage);
        assert_eq!(
            select_conn(&saved, &ConnArgs::default()).unwrap_err(),
            Exit::Usage
        );
    }

    fn param(s: &str) -> Binding {
        crate::args::parse_param(s).unwrap()
    }

    #[test]
    fn bind_substitutes_each_kind_as_the_app_does() {
        let (sql, unused) = bind(
            "SELECT * FROM t WHERE a = :a AND b = :b AND c IS :c ORDER BY :col",
            &[
                param("a=it's"),
                param("b:number=42"),
                param("c:null"),
                param("col:sql=created_at"),
            ],
            SqlDialect::MySql,
        )
        .unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM t WHERE a = 'it''s' AND b = 42 AND c IS NULL ORDER BY created_at"
        );
        assert!(unused.is_empty());
    }

    #[test]
    fn bind_leaves_a_statement_without_placeholders_alone() {
        assert_eq!(
            bind("SELECT a::text, @x := 1", &[], SqlDialect::Postgres),
            Ok(("SELECT a::text, @x := 1".to_string(), vec![]))
        );
    }

    /// **Never sent with the placeholder in it.** Every name missing is named,
    /// with the flag that would supply it.
    #[test]
    fn bind_refuses_a_placeholder_with_no_value() {
        let why = bind("SELECT :a, :b", &[param("a=1")], SqlDialect::MySql).unwrap_err();
        assert!(why.contains(":b") && !why.contains(":a,"), "{why}");
        assert!(why.contains("-p b="), "{why}");
    }

    #[test]
    fn bind_refuses_a_number_that_is_not_one() {
        let why = bind(
            "SELECT :n",
            &[param("n:number=1 OR 1=1")],
            SqlDialect::MySql,
        )
        .unwrap_err();
        assert!(why.contains(":n"), "{why}");
    }

    #[test]
    fn bind_refuses_a_name_given_twice() {
        let why = bind(
            "SELECT :a",
            &[param("a=1"), param("a=2")],
            SqlDialect::MySql,
        )
        .unwrap_err();
        assert!(why.contains("twice"), "{why}");
    }

    #[test]
    fn bind_reports_a_param_the_statement_never_asks_for() {
        assert_eq!(
            bind(
                "SELECT :a",
                &[param("a=1"), param("typo=2")],
                SqlDialect::MySql
            ),
            Ok(("SELECT '1'".to_string(), vec!["typo".to_string()]))
        );
    }

    /// **The guard reads the bound statement** — the composition `bind` alone
    /// cannot pin. A raw value that turns a read into a write must reach
    /// `query`'s gate as the write it is; with `statement` handing back the
    /// template, the gate would pass `SELECT :x` and the engine would run a
    /// `DELETE`.
    #[test]
    fn the_statement_the_gates_read_has_the_values_in_it() {
        let sql = SqlArgs {
            sql: Some("SELECT 1; :x".to_string()),
            file: None,
        };
        let bound = statement(
            &sql,
            &[param("x:sql=DELETE FROM t")],
            &target(None),
            &conn("shop"),
        )
        .unwrap();
        assert_eq!(bound, "SELECT 1; DELETE FROM t");
        assert!(query::gate(&bound, SqlDialect::MySql).is_err());
    }

    #[test]
    fn a_statement_missing_a_param_is_a_usage_error() {
        let sql = SqlArgs {
            sql: Some("SELECT :x".to_string()),
            file: None,
        };
        assert_eq!(
            statement(&sql, &[], &target(None), &conn("shop")),
            Err(Exit::Usage)
        );
    }

    fn target(database: Option<&str>) -> Target {
        Target {
            conn: ConnArgs {
                connection: Some("1".to_string()),
                ..Default::default()
            },
            database: database.map(str::to_string),
        }
    }

    /// **`mcp` is gated on CLI access like every other subcommand**, and
    /// refused before anything is read or served: an agent pointed at a
    /// connection the user never exposed gets exit 3 and a server that failed
    /// to start, not a server with a tool list.
    #[tokio::test]
    async fn mcp_refuses_a_connection_without_cli_access() {
        let c = conn("shop");
        assert!(!c.cli_access, "the fixture is unexposed");
        let target = McpTarget {
            connections: vec!["1".to_string()],
            ..Default::default()
        };
        assert_eq!(
            serve_mcp(std::slice::from_ref(&c), &target).await,
            Exit::Refused
        );
        let target = McpTarget {
            connections: vec!["nope".to_string()],
            ..Default::default()
        };
        assert_eq!(serve_mcp(&[], &target).await, Exit::Usage);
        // **Every connection is gated before any is opened**: an exposed one
        // first and an unexposed one after is refused without dialling the
        // first — whose host here would not answer.
        let mut open = conn("shop");
        open.id = 2;
        open.name = "open".to_string();
        open.cli_access = true;
        let target = McpTarget {
            connections: vec!["open".to_string(), "1".to_string()],
            ..Default::default()
        };
        assert_eq!(serve_mcp(&[open.clone(), c], &target).await, Exit::Refused);
        // One connection named by its name and by its id is still twice.
        let target = McpTarget {
            connections: vec!["open".to_string(), "#2".to_string()],
            ..Default::default()
        };
        assert_eq!(serve_mcp(&[open], &target).await, Exit::Usage);
    }

    /// **`--ai-data` belongs to a `--url`**, and a `--url` whose name a saved
    /// connection already has is refused — a tool call picks by name — both
    /// before anything is dialled.
    #[tokio::test]
    async fn mcp_refuses_a_stray_level_and_a_url_that_shares_a_name() {
        let target = McpTarget {
            connections: vec!["1".to_string()],
            ai_data: Some(schemaic_core::connection::AiData::Full),
            ..Default::default()
        };
        assert_eq!(serve_mcp(&[conn("shop")], &target).await, Exit::Usage);

        let url = "mysql://app@unreachable.invalid/shop";
        let mut saved = conn("shop");
        saved.name = one_off_connection(url, false, true).unwrap().name;
        saved.cli_access = true;
        let target = McpTarget {
            connections: vec![saved.name.clone()],
            url: Some(url.to_string()),
            ..Default::default()
        };
        assert_eq!(serve_mcp(&[saved], &target).await, Exit::Usage);
    }

    /// **A `--url` endpoint's level is its flag's, a saved one's the app's** —
    /// so a withheld tool's refusal names `--ai-data` on the one and the
    /// connection settings on the other.
    #[test]
    fn an_mcp_endpoint_knows_where_its_level_is_set() {
        use crate::mcp::McpTool;
        let db = || {
            Db::from_parts(
                schemaic_db::Engine::MySql,
                "h".to_string(),
                3306,
                "u".to_string(),
                String::new(),
                String::new(),
            )
        };
        let url = one_off_connection("mysql://u@h/d", false, true).unwrap();
        let e = mcp_endpoint(db(), None, &url);
        let why = crate::mcp::refusal_on(&e, McpTool::RunQuery).expect("withheld");
        assert!(
            why.contains("--ai-data full") && !why.contains("connection settings"),
            "{why}"
        );
        let e = mcp_endpoint(db(), None, &conn("shop"));
        let why = crate::mcp::refusal_on(&e, McpTool::RunQuery).expect("withheld");
        assert!(why.contains("connection settings"), "{why}");
    }

    /// On a server engine `-d` wins, else the connection's own, else none —
    /// `query`'s rule (SQLite's implied `main` comes first:
    /// `a_sqlite_file_defaults_to_main`).
    #[test]
    fn mcp_defaults_to_the_connections_database() {
        assert_eq!(
            default_database(Some("a"), &conn("b")).as_deref(),
            Some("a")
        );
        assert_eq!(default_database(None, &conn("b")).as_deref(), Some("b"));
        assert_eq!(default_database(None, &conn("")), None);
    }

    /// **The start-up line says why `run_query` is missing**, naming the level
    /// the connection is at and the one that allows it — and says nothing
    /// about it when it is there.
    #[test]
    fn the_mcp_notice_explains_a_withheld_query_tool() {
        use schemaic_core::connection::AiData;
        let mut c = conn("");
        c.name = "Prod EU".to_string();
        let n = mcp_notice(&c, false, Some("shop"), true, false);
        assert!(n.contains("'Prod EU'"), "{n}");
        assert!(
            n.contains(AiData::OnRequest.label()),
            "a never-chosen level is the default: {n}"
        );
        assert!(n.contains(AiData::Full.label()), "{n}");
        assert!(n.contains("withheld"), "{n}");
        c.ai_data = Some(AiData::SchemaOnly);
        assert!(
            mcp_notice(&c, false, Some("shop"), true, false).contains(AiData::SchemaOnly.label())
        );
        let n = mcp_notice(&c, true, Some("shop"), true, false);
        assert!(!n.contains("run_query"), "{n}");
        assert!(!n.contains("-d"), "a default was given: {n}");
        // Beside other connections the tool is still listed, and refused here.
        let n = mcp_notice(&c, false, Some("shop"), true, true);
        // A `--url` connection's level is its flag's, not a setting in the app.
        let url = one_off_connection("mysql://u@h/d", false, true).unwrap();
        let u = mcp_notice(&url, false, Some("d"), true, false);
        assert!(
            u.contains("--ai-data full") && !u.contains("connection settings"),
            "{u}"
        );
        assert!(
            n.contains("refused on it") && !n.contains("withheld"),
            "{n}"
        );
    }

    /// **No default database is said at start-up**, with the fix. Both real
    /// saved connections the release check ran against had none, and every
    /// tool that needed one misfired quietly — `describe_table` refused each
    /// call without a `database`, `run_query` ran wherever the driver landed.
    #[test]
    fn the_mcp_notice_says_when_there_is_no_default_database() {
        let mut c = conn("");
        c.name = "pg".to_string();
        let n = mcp_notice(&c, true, None, true, false);
        assert!(n.contains("no default database"), "{n}");
        assert!(n.contains("-d"), "{n}");
        assert!(n.contains("schemaic databases -c pg"), "{n}");
        // `-d` is the first connection's: a later one is told per call.
        let n = mcp_notice(&c, true, None, false, true);
        assert!(
            n.contains("no default database") && !n.contains("-d"),
            "{n}"
        );
        // **A `--url` connection has neither a name `-c` knows nor settings
        // in Schemaic**: its advice is the URL's.
        let url = one_off_connection("mysql://u@h", false, true).unwrap();
        let n = mcp_notice(&url, true, None, true, false);
        assert!(n.contains("databases --url") && !n.contains("-c "), "{n}");
        let n = mcp_notice(&url, true, None, false, true);
        assert!(
            n.contains("in its URL") && !n.contains("in Schemaic"),
            "{n}"
        );
        // And with several, each connect is announced by name first.
        assert_eq!(mcp_connecting(&c, false), None);
        assert_eq!(
            mcp_connecting(&c, true).as_deref(),
            Some("schemaic mcp: connecting 'pg'")
        );
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
            NotRun::Blocked(schemaic_core::sql::GuardReason::ReadOnly),
            NotRun::NeedsConsent(schemaic_core::sql::GuardReason::NoDatabase),
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

    /// **SQLite's file is its database, so it always has one** — `main`, the
    /// name the tree shows. `dump` and `compare` demanded a `-d` that any value
    /// satisfied.
    #[test]
    fn a_sqlite_file_defaults_to_main() {
        let mut lite = conn("");
        lite.db_type = "sqlite".to_string();
        assert_eq!(database_for(&target(None), &lite).as_deref(), Some("main"));
        // A name left over from an engine switch is not the file's database.
        lite.database = "shop".to_string();
        assert_eq!(database_for(&target(None), &lite).as_deref(), Some("main"));
        // Nor is a flag: nothing else is ever attached (`mcp`'s rule too).
        assert_eq!(
            database_for(&target(Some("other")), &lite).as_deref(),
            Some("main")
        );
    }

    /// The folders a test's machine holds — `output_problem_with`'s seam, so
    /// no test stats the real filesystem.
    fn folders(dirs: &'static [&'static str]) -> impl Fn(&std::path::Path) -> bool {
        move |p| dirs.iter().any(|d| std::path::Path::new(d) == p)
    }

    /// **`-o` naming a folder is refused before anything is read**, not at
    /// the rename after a whole dump.
    #[test]
    fn an_output_that_is_a_folder_is_the_command_lines_mistake() {
        let out = std::path::Path::new("backups");
        let why = output_problem_with(out, "dump", folders(&["backups"])).expect("refused");
        assert!(why.contains("is a folder"), "{why}");
        assert!(why.contains("dump"), "{why}");
    }

    #[test]
    fn an_output_in_a_missing_folder_is_refused() {
        let out = std::path::Path::new("backups")
            .join("no-such-folder")
            .join("x.sql");
        let why = output_problem_with(&out, "export", folders(&["backups"])).expect("refused");
        assert!(why.starts_with("no folder"), "{why}");
    }

    #[test]
    fn a_new_file_in_an_existing_folder_or_a_bare_name_passes() {
        let out = std::path::Path::new("backups").join("not-there.sql");
        assert_eq!(
            output_problem_with(&out, "dump", folders(&["backups"])),
            None
        );
        assert_eq!(
            output_problem_with(std::path::Path::new("not-there.sql"), "dump", folders(&[])),
            None
        );
    }

    /// **A stopped dump names a `.part` only where one was made.** It said
    /// "what had been written is in x.sql.part" for a dump timed out during
    /// its schema read, before any file existed.
    #[test]
    fn a_timed_out_dump_names_a_fragment_only_when_there_is_one() {
        let t = Duration::from_secs(5);
        let none = dump_timeout_note(t, "x.sql", false);
        // Through `part_path`: the crate's source gate refuses the suffix spelt.
        let fragment = schemaic_core::export::part_path("x.sql");
        assert!(!none.contains(&fragment), "{none}");
        assert!(none.contains("5s"), "{none}");
        let some = dump_timeout_note(t, "x.sql", true);
        assert!(some.contains(&fragment), "{some}");
    }

    /// The saved name is read the way the connection reads it: a blank one
    /// is none, not a database named by whitespace.
    #[test]
    fn a_blank_saved_database_is_none() {
        assert_eq!(database_for(&target(None), &conn("  ")), None);
    }
}
